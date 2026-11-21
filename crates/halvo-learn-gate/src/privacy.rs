use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::ErrorKind;
use std::path::Path;

use halvo_derived::Report as StoreReport;
use halvo_schema::{RetentionClass, WorkflowId};

use crate::case::Case;
use crate::cli::{Cause, Flags, Refusal, Report};
use crate::delete::{seed, seed_cases, seed_derived, Cascade, MARKERS};
use crate::derived::DerivedHost;
use crate::host::PlayHost;
use crate::store::{Family, MemoryStore};
use halvo_schema::kinds::registry;

/// The six checks of contracts.md L11.4, in the order of their lines. `Listed`, `Scrub` and `Expired` run on the
/// derived store, opened with its key (`derived_outcomes`); `Scrub` reads rows, not bytes. No byte search of this unit
/// shows the scrub. Every open of the store sets secure_delete; the store's own page check reads purges of evidence
/// rows today, and a unit of the workflows run widens it to every removal: until then no check reads the file's pages
/// after a delete of learned rows. `DeleteAll` is `not-run` in this build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    Listed,
    Deleted,
    Scrub,
    DeleteAll,
    Outside,
    Expired,
}

impl Check {
    pub const ALL: [Check; 6] = [
        Check::Listed,
        Check::Deleted,
        Check::Scrub,
        Check::DeleteAll,
        Check::Outside,
        Check::Expired,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Check::Listed => "learn_listed",
            Check::Deleted => "learn_deleted",
            Check::Scrub => "learn_scrub",
            Check::DeleteAll => "learn_delete_all",
            Check::Outside => "learn_outside",
            Check::Expired => "learn_expired",
        }
    }
}

/// The result of one check: `not-run` for a check that this build cannot make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Miss,
    NotRun,
}

impl Outcome {
    pub fn holds(holds: bool) -> Outcome {
        if holds {
            Outcome::Ok
        } else {
            Outcome::Miss
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Miss => "miss",
            Outcome::NotRun => "not-run",
        }
    }
}

type Row = (&'static str, String);

/// The rows among `rows` that name the workflow `id` by any of the three readings of learn_deleted (its kind's
/// workflow key, its id as a JSON string, its marker), in byte order.
fn naming_in<'r>(
    rows: impl Iterator<Item = (&'static str, &'r str)>,
    id: &WorkflowId,
    marker: &str,
) -> Vec<Row> {
    let quoted = format!("\"{}\"", id.as_str());
    let mut found: Vec<Row> = rows
        .filter(|(kind, json)| {
            key_names(kind, json, id) || json.contains(&quoted) || json.contains(marker)
        })
        .map(|(kind, json)| (kind, json.to_owned()))
        .collect();
    found.sort();
    found
}

fn naming(store: &MemoryStore, id: &WorkflowId, marker: &str) -> Vec<Row> {
    naming_in(store.rows(Family::Learn).all(), id, marker)
}

fn key_names(kind: &str, json: &str, id: &WorkflowId) -> bool {
    let Some(key) = registry()
        .find(|e| e.name == kind)
        .map_or(Some("workflow"), |e| e.workflow_key)
    else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| Some(WorkflowId::parse(v.as_object()?.get(key)?.as_str()?).ok()? == *id))
        .unwrap_or(false)
}

fn keyed(store: &MemoryStore, id: &WorkflowId) -> Vec<Row> {
    let mut rows: Vec<Row> = store
        .rows(Family::Learn)
        .all()
        .filter(|(kind, json)| key_names(kind, json, id))
        .map(|(kind, json)| (kind, json.to_owned()))
        .collect();
    rows.sort();
    rows
}

fn deleted(
    first_before: &[Row],
    stored: bool,
    first_after: &[Row],
    kept: &[Row],
    second_after: &[Row],
) -> bool {
    !first_before.is_empty()
        && stored
        && first_after.is_empty()
        && !kept.is_empty()
        && second_after == kept
}

fn markers_seeded(first: &[Row], second: &[Row]) -> bool {
    let in_state = |rows: &[Row], marker: &str, state: &str| {
        rows.iter().any(|(kind, json)| {
            *kind == "learn_edit"
                && json.contains(marker)
                && serde_json::from_str::<serde_json::Value>(json)
                    .is_ok_and(|v| v["state"] == state)
        })
    };
    [(first, MARKERS[0]), (second, MARKERS[1])]
        .iter()
        .all(|(rows, marker)| {
            in_state(rows, marker, "active") && in_state(rows, marker, "reverted")
        })
}

fn has_marker(bytes: &[u8]) -> bool {
    MARKERS
        .iter()
        .any(|m| bytes.windows(m.len()).any(|w| w == m.as_bytes()))
}

fn entries(dir: &Path) -> std::io::Result<BTreeSet<OsString>> {
    let mut names = BTreeSet::new();
    for entry in std::fs::read_dir(dir)? {
        names.insert(entry?.file_name());
    }
    Ok(names)
}

fn gone_or_held(error: &std::io::Error) -> bool {
    error.kind() != ErrorKind::NotFound
}

fn holds_marker(path: &Path) -> bool {
    if path
        .file_name()
        .is_some_and(|name| has_marker(name.as_encoded_bytes()))
    {
        return true;
    }
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) => return gone_or_held(&e),
    };
    let bytes = if meta.is_symlink() {
        std::fs::read_link(path).map(|target| target.into_os_string().into_encoded_bytes())
    } else if meta.is_dir() {
        return match entries(path) {
            Ok(names) => names.iter().any(|name| holds_marker(&path.join(name))),
            Err(e) => gone_or_held(&e),
        };
    } else if meta.is_file() {
        std::fs::read(path)
    } else {
        return false;
    };
    match bytes {
        Ok(bytes) => has_marker(&bytes),
        Err(e) => gone_or_held(&e),
    }
}

fn clean_under(dir: &Path, made_after: Option<&BTreeSet<OsString>>) -> bool {
    let Ok(names) = entries(dir) else {
        return false;
    };
    names
        .iter()
        .filter(|name| made_after.is_none_or(|before| !before.contains(*name)))
        .all(|name| !holds_marker(&dir.join(name)))
}

/// The days on which the person is told that a learned row of a bounded kind is kept (contracts.md L11.4): a row of
/// day `d` stands on the days `d` to `d + 92` and is gone from the day `d + 93` on.
pub const KEPT_DAYS: u32 = 93;

/// The delete of a workflow on the derived store, and whether it was stored before it.
pub type DerivedCascade<'a> =
    &'a mut dyn FnMut(&mut DerivedHost, &WorkflowId) -> Result<bool, String>;

/// The rows of the family `learn` that the gate's keyed read hands out on the host's clock, each as (kind, bytes).
fn learned(host: &mut DerivedHost) -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    for entry in halvo_schema::learning::KINDS {
        for json in host.learn_rows(entry.name)? {
            rows.push((entry.name, json));
        }
    }
    Ok(rows)
}

fn refs(rows: &[Row]) -> impl Iterator<Item = (&'static str, &str)> {
    rows.iter().map(|(kind, json)| (*kind, json.as_str()))
}

fn revision(report: Option<&StoreReport>, family: &str) -> u64 {
    report
        .and_then(|r| r.revisions.iter().find(|(f, _)| *f == family))
        .map_or(0, |(_, n)| *n)
}

/// A count of rows by retention class.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ByClass {
    window: i128,
    bounded: i128,
    kept: i128,
}

impl ByClass {
    /// The store's counts in `report`, none before the first report.
    fn of(report: Option<&StoreReport>) -> ByClass {
        report.map_or_else(ByClass::default, |r| ByClass {
            window: r.counts.window.into(),
            bounded: r.counts.bounded.into(),
            kept: r.counts.kept.into(),
        })
    }

    fn add_change(&mut self, from: ByClass, to: ByClass) {
        self.window += to.window - from.window;
        self.bounded += to.bounded - from.bounded;
        self.kept += to.kept - from.kept;
    }
}

/// learn_listed on the derived store (contracts.md L11.4): the store's counts per retention class, which its report
/// to its host carries after each committed transaction, count every learned row. It reads that report and not the
/// app's data view, so whether the Inventory shows those counts is not shown here. The report sums the rows of every
/// family, so the check takes the change of the counts across each transaction whose report raises the revision of
/// the family `learn` and of no other family, from the first report on; the sum of those changes, by class, must
/// equal the gate's own count of the learned rows that its keyed read hands out, by the class of each row's kind
/// (`learn_run` and `learn_routine` bounded, `learn_edit` kept), with a bounded and a kept row among them and no
/// window row. A report that raises `learn` together with another family makes the check miss, because its change
/// cannot be given to the learned rows alone.
pub(crate) fn listed(host: &mut DerivedHost) -> Result<bool, String> {
    let reports = host.reports();
    let mut changed = ByClass::default();
    let (mut alone, mut mixed) = (0usize, false);
    let mut before: Option<&StoreReport> = None;
    for report in &reports {
        let rose: Vec<&str> = report
            .revisions
            .iter()
            .filter(|(family, n)| *n > revision(before, family))
            .map(|(family, _)| *family)
            .collect();
        if rose == ["learn"] {
            changed.add_change(ByClass::of(before), ByClass::of(Some(report)));
            alone += 1;
        } else if rose.contains(&"learn") {
            mixed = true;
        }
        before = Some(report);
    }
    let mut counted = ByClass::default();
    for (kind, _) in learned(host)? {
        let class = halvo_schema::learning::KINDS
            .iter()
            .find(|entry| entry.name == kind)
            .map(|entry| entry.class);
        match class {
            Some(RetentionClass::Window) => counted.window += 1,
            Some(RetentionClass::Bounded { .. }) => counted.bounded += 1,
            Some(RetentionClass::Kept) => counted.kept += 1,
            None => return Ok(false),
        }
    }
    Ok(!mixed
        && alone > 0
        && changed == counted
        && counted.bounded > 0
        && counted.kept > 0
        && counted.window == 0)
}

/// learn_scrub on the derived store, by the coordinator's word of 2026-11-21 (way C): after the delete of the first
/// workflow no learned row of it stands, read through the store's own calls with the key, while the second
/// workflow's rows stand, and the removal went through halvo-derived's own delete call and through no statement of
/// the family's own. It reads the rows of the family `learn` right before and right after one call of `cascade`
/// with the first workflow's id, and takes the host's tallies on both sides of that call. It is ok when: the first
/// workflow had rows by any of the three readings of learn_deleted before and has none after; `cascade` answered
/// that it was stored; the second workflow's rows are the same rows, by kind and bytes, and more than none; and
/// across the call the store committed exactly one transaction, whose report raises the revision of `learn`, while
/// no request reached `LearnHandlers`, no batch ran (so no step) and the store was not opened again (so neither the
/// open's reconcile nor its expiry ran). The writer of a typed commit has no call that removes a row; the one removal
/// under a plain write is of a support row that a new proposal drops, inside a batch, which the tally of batches
/// counts. So a removal by another path is a further transaction, a request, a batch, an open, or a removal before or
/// after the call, and each makes the check miss. It reads rows, not bytes. No byte search of this unit shows the
/// scrub. Every open of the store sets secure_delete; the store's own page check reads purges of evidence rows today,
/// and a unit of the workflows run widens it to every removal: until then no check reads the file's pages after a
/// delete of learned rows.
pub(crate) fn scrub(
    host: &mut DerivedHost,
    first: &WorkflowId,
    second: &WorkflowId,
    cascade: DerivedCascade<'_>,
) -> Result<bool, String> {
    let rows = learned(host)?;
    let first_before = naming_in(refs(&rows), first, MARKERS[0]);
    let kept = naming_in(refs(&rows), second, MARKERS[1]);
    let before = host.tallies();
    let stored = cascade(host, first)?;
    let after = host.tallies();
    let reports = host.reports();
    let last_before = before.reports.checked_sub(1).and_then(|at| reports.get(at));
    let one_delete = after.reports == before.reports.saturating_add(1)
        && revision(reports.get(before.reports), "learn") > revision(last_before, "learn")
        && after.learn_requests == before.learn_requests
        && after.batches == before.batches
        && after.opens == before.opens;
    let rows = learned(host)?;
    Ok(one_delete
        && deleted(
            &first_before,
            stored,
            &naming_in(refs(&rows), first, MARKERS[0]),
            &kept,
            &naming_in(refs(&rows), second, MARKERS[1]),
        ))
}

/// The `learn_run` and `learn_routine` rows that the gate's keyed read hands out on the host's clock, each with its
/// day, `None` for a row whose day cannot be read.
fn bounded_rows(host: &mut DerivedHost) -> Result<Vec<(Row, Option<u32>)>, String> {
    let mut rows = Vec::new();
    for kind in ["learn_run", "learn_routine"] {
        for json in host.learn_rows(kind)? {
            let day = serde_json::from_str::<serde_json::Value>(&json)
                .ok()
                .and_then(|v| v["day"].as_u64())
                .and_then(|day| u32::try_from(day).ok());
            rows.push(((kind, json), day));
        }
    }
    Ok(rows)
}

fn rows_now(host: &mut DerivedHost) -> Result<BTreeSet<Row>, String> {
    Ok(bounded_rows(host)?
        .into_iter()
        .map(|(row, _)| row)
        .collect())
}

/// learn_expired on the derived store (contracts.md L11.4): a learned row of a bounded kind is kept no longer than
/// `KEPT_DAYS`. It takes the `learn_run` and `learn_routine` rows of the earliest day `d` among them, with one of each
/// kind, and the rows of later days. On a clock at `d + 92`, the last of the 93 days, it opens the store again and
/// reads that every one of those rows stands. On a clock one day past, `d + 93`, it opens the store again, which
/// runs the store's expiry, and then sets the clock back to `d + 92` and reads: a read hands out a bounded row until
/// its day of going (DS-4), so on that clock it hands out every row of day `d` that is still stored, and the check
/// counts the stored rows through the gate's handler, never through a read on a clock at which the row is due. It is
/// ok when none of the rows of day `d` is read then and every row of a later day is. It leaves the clock at `d + 93`.
pub(crate) fn expired(host: &mut DerivedHost) -> Result<bool, String> {
    let Some(rows) = bounded_rows(host)?
        .into_iter()
        .map(|(row, day)| day.map(|day| (row, day)))
        .collect::<Option<Vec<(Row, u32)>>>()
    else {
        return Ok(false);
    };
    let Some(first) = rows.iter().map(|(_, day)| *day).min() else {
        return Ok(false);
    };
    let (Some(last), Some(gone)) = (
        first.checked_add(KEPT_DAYS - 1),
        first.checked_add(KEPT_DAYS),
    ) else {
        return Ok(false);
    };
    let of_first: BTreeSet<Row> = rows
        .iter()
        .filter(|(_, day)| *day == first)
        .map(|(row, _)| row.clone())
        .collect();
    let later: BTreeSet<Row> = rows
        .iter()
        .filter(|(_, day)| *day > first)
        .map(|(row, _)| row.clone())
        .collect();
    let both = ["learn_run", "learn_routine"]
        .iter()
        .all(|kind| of_first.iter().any(|(k, _)| k == kind));
    host.set_day(last);
    host.reopen()?;
    let standing = rows_now(host)?;
    host.set_day(gone);
    host.reopen()?;
    host.set_day(last);
    let left = rows_now(host)?;
    host.set_day(gone);
    Ok(both
        && of_first.is_subset(&standing)
        && later.is_subset(&standing)
        && of_first.is_disjoint(&left)
        && later.is_subset(&left))
}

/// learn_listed, learn_scrub and learn_expired on the derived store under `data_dir`, in that order: the play of
/// `seed_derived`, which opens the store there with a key that this process makes and leaves both seeded workflows in
/// it; then `listed`, `scrub` with `cascade` on the first workflow, and `expired` on the rows that are left, which
/// moves the clock past the bound and expires every bounded row of the store. The store stays under `data_dir`.
pub fn derived_outcomes(
    cases: &[Case; 2],
    data_dir: &Path,
    cascade: DerivedCascade<'_>,
) -> Result<[Outcome; 3], String> {
    let (mut play, first) = seed_derived(cases, data_dir)?;
    let first = WorkflowId::parse(&first).map_err(|_| "the first workflow's id".to_owned())?;
    let second =
        WorkflowId::parse(play.workflow()).map_err(|_| "the second workflow's id".to_owned())?;
    let host = &mut play.host;
    let listed = listed(host)?;
    let scrub = scrub(host, &first, &second, cascade)?;
    let expired = expired(host)?;
    Ok([listed, scrub, expired].map(Outcome::holds))
}

/// The six checks (contracts.md L11.4; the coordinator's rulings LM-54 E5 and LM-82, and the coordinator's word of
/// 2026-11-21 on the derived store, way C), each with its result. It lists the top of `temp`, then calls `derived` with
/// `data_dir`, which answers `learn_listed`, `learn_scrub` and `learn_expired` (the command's is
/// `derived_outcomes`, on the derived store under `data_dir`, opened with its key). Then it plays the two seeded
/// cases on the memory store as `learn-gate delete` does (`delete::seed`; the command's are `delete::seed_cases`,
/// whose learned rows hold the markers), reads the rows of the family `learn` that name each workflow, each by kind
/// and bytes, and calls `cascade` with the first workflow's id; then it calls `after` with `data_dir` and `temp`. A
/// row names a workflow when the value under its kind's workflow key parses to that workflow's id, when its bytes
/// hold the id as a JSON string, or when its bytes hold that workflow's marker. `learn_deleted` checks the delete of
/// the gate's own test store in memory (its own cascade), not the app's delete, and reads no file: after that
/// cascade no learned row names the first seeded workflow by its kind's workflow key, holds its id as a JSON string
/// or holds its marker, and the second workflow's rows are the same rows, by kind and bytes, with none added.
/// `learn_deleted` is ok when the first workflow had rows, the cascade answered that it was stored, no row names it
/// after the cascade, and the rows that name the second workflow are the same rows as before it, by kind and bytes,
/// and more than none. `learn_outside` searches only the directory that the command is given, which holds the
/// derived store's directory `derived/` after `derived` (its files included), and, in the process's temporary
/// directory, each entry whose name was not at its top before `derived` ran, at every depth, and nothing else on the
/// computer; the search looks for the markers' bytes in names, in regular files and in the target texts of links,
/// follows no link below those places, reads no extended attribute (on Windows, no alternate data stream) and no
/// other encoding, and counts an entry that cannot be read as holding a marker. `learn_outside` is ok when, besides,
/// each marker was in an `active` and in a `reverted` `learn_edit` row of its own workflow on the memory store, read
/// by the kind's workflow key alone, so that the search can find one; a directory that cannot be listed counts as
/// holding a marker, and an entry that is gone as holding none. `learn_delete_all` is `not-run`: it needs the
/// capture run's `halvo-capture --delete-all`, which does not reach `derived/` in this build.
pub fn outcomes(
    cases: &[Case; 2],
    data_dir: &Path,
    temp: &Path,
    derived: &mut dyn FnMut(&Path) -> Result<[Outcome; 3], String>,
    cascade: Cascade<'_>,
    after: &mut dyn FnMut(&Path, &Path),
) -> Result<[(Check, Outcome); 6], String> {
    let before = entries(temp).ok();
    let [listed, scrub, expired] = derived(data_dir)?;
    let (mut play, first) = seed(cases)?;
    let id = WorkflowId::parse(&first).map_err(|_| "the first workflow's id".to_owned())?;
    let second = play.workflow().to_owned();
    let store = &mut play.host.store;
    let second_id =
        WorkflowId::parse(&second).map_err(|_| "the second workflow's id".to_owned())?;
    let first_rows = naming(store, &id, MARKERS[0]);
    let kept = naming(store, &second_id, MARKERS[1]);
    let seeded = markers_seeded(&keyed(store, &id), &keyed(store, &second_id));
    let stored = cascade(store, &id);
    let deleted = deleted(
        &first_rows,
        stored,
        &naming(store, &id, MARKERS[0]),
        &kept,
        &naming(store, &second_id, MARKERS[1]),
    );
    after(data_dir, temp);
    let outside = seeded
        && clean_under(data_dir, None)
        && before.is_some_and(|before| clean_under(temp, Some(&before)));
    Ok(Check::ALL.map(|check| {
        let outcome = match check {
            Check::Listed => listed,
            Check::Deleted => Outcome::holds(deleted),
            Check::Scrub => scrub,
            Check::DeleteAll => Outcome::NotRun,
            Check::Outside => Outcome::holds(outside),
            Check::Expired => expired,
        };
        (check, outcome)
    }))
}

/// One line `privacy check=<name> result=<ok|miss|not-run>` for each check, in order; the report is ok when no
/// check misses.
pub fn report(outcomes: &[(Check, Outcome)]) -> Report {
    Report {
        lines: outcomes
            .iter()
            .map(|(check, outcome)| {
                format!("privacy check={} result={}", check.name(), outcome.as_str())
            })
            .collect(),
        ok: outcomes
            .iter()
            .all(|(_, outcome)| *outcome != Outcome::Miss),
    }
}

/// `learn-gate privacy --data-dir DIR` (contracts.md L10.4, L11.4): the checks of `outcomes`, `DIR` as the data
/// directory and the process's temporary directory, one line each: `derived_outcomes` on the derived store under
/// `DIR` with the decision `delete` of the store as its cascade, which leaves the store under `DIR`, and the memory
/// store with its own cascade. `DIR` must be an existing directory; the refusal `usage` otherwise, and `store` when a
/// seed fails or the derived store refuses (for one, a `derived/` under `DIR` that another key made).
pub fn run(args: &[String]) -> Result<Report, Refusal> {
    let mut flags = Flags::new(args);
    let dir = flags
        .value("--data-dir")?
        .ok_or_else(|| Refusal::usage("privacy takes --data-dir DIR"))?;
    flags.done()?;
    let dir = Path::new(dir);
    if !dir.is_dir() {
        return Err(Refusal::usage("--data-dir takes an existing directory"));
    }
    let temp = std::env::temp_dir();
    let outcomes = outcomes(
        &seed_cases(),
        dir,
        &temp,
        &mut |data| {
            derived_outcomes(&seed_cases(), data, &mut |host, id| {
                host.delete_workflow(id)
            })
        },
        &mut |store, id| store.delete_workflow(id),
        &mut |_, _| {},
    )
    .map_err(|detail| Refusal::new(Cause::Store, detail))?;
    Ok(report(&outcomes))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use halvo_schema::WorkflowId;

    use super::{deleted, holds_marker, markers_seeded, outcomes, report, Check, Outcome, Row};
    use crate::cli::Cause;
    use crate::delete::{seed_case, seed_cases, MARKERS, SEED_DAY};
    use crate::store::{Held, MemoryStore};
    use crate::stub::id;

    fn scratch(name: &str) -> PathBuf {
        let exe = std::env::current_exe().expect("the test binary");
        let root = exe
            .ancestors()
            .nth(3)
            .expect("the target directory")
            .join("tmp")
            .join("r5-privacy");
        halvo_test_dir::make(&root, name).expect("a test directory")
    }

    fn store_cascade(store: &mut MemoryStore, id: &WorkflowId) -> bool {
        store.delete_workflow(id)
    }

    fn no_derived(_: &Path) -> Result<[Outcome; 3], String> {
        Ok([Outcome::NotRun; 3])
    }

    fn checks_with(
        data: &Path,
        temp: &Path,
        cascade: &mut dyn FnMut(&mut MemoryStore, &WorkflowId) -> bool,
        after: &mut dyn FnMut(&Path, &Path),
    ) -> Vec<(Check, Outcome)> {
        outcomes(&seed_cases(), data, temp, &mut no_derived, cascade, after)
            .expect("the seeded store")
            .to_vec()
    }

    fn result_of(outcomes: &[(Check, Outcome)], check: Check) -> Outcome {
        outcomes
            .iter()
            .find(|(c, _)| *c == check)
            .map(|(_, o)| *o)
            .expect("a line of the check")
    }

    fn deleted_with(cascade: &mut dyn FnMut(&mut MemoryStore, &WorkflowId) -> bool) -> Outcome {
        let (data, temp) = (scratch("deleted-data"), scratch("deleted-temp"));
        let got = checks_with(&data, &temp, cascade, &mut |_, _| {});
        assert_eq!(result_of(&got, Check::Outside), Outcome::Ok);
        result_of(&got, Check::Deleted)
    }

    fn outside_with(data: &Path, temp: &Path, after: &mut dyn FnMut(&Path, &Path)) -> Outcome {
        let got = checks_with(data, temp, &mut store_cascade, after);
        assert_eq!(result_of(&got, Check::Deleted), Outcome::Ok);
        result_of(&got, Check::Outside)
    }

    fn names(json: &str, workflow: &str) -> bool {
        serde_json::from_str::<serde_json::Value>(json).is_ok_and(|v| v["workflow"] == workflow)
    }

    fn kind_name(kind: &str) -> &'static str {
        ["learn_run", "learn_edit", "learn_routine"]
            .into_iter()
            .find(|k| *k == kind)
            .expect("a kind of the family learn")
    }

    fn remove_first(store: &mut MemoryStore, kind: &str, keep_out: impl Fn(&str) -> bool) {
        let rows = store
            .learn_rows_mut()
            .get_mut(kind)
            .expect("rows of the kind");
        let at = rows
            .iter()
            .position(|r| keep_out(&r.json))
            .expect("a row to take");
        rows.remove(at);
    }

    fn put_back(store: &mut MemoryStore, kind: &str, row: String) {
        store
            .learn_rows_mut()
            .entry(kind_name(kind))
            .or_default()
            .push(Held::of(kind, row));
    }

    #[test]
    fn privacy_prints_the_six_lines_of_l11_4_with_learn_deleted_and_learn_outside_ok() {
        let data = scratch("command");
        let args = [
            "privacy".to_owned(),
            "--data-dir".to_owned(),
            data.to_str().expect("a path").to_owned(),
        ];
        let got = crate::cli::run(&args).expect("the privacy lines");
        assert_eq!(
            got.lines,
            [
                "privacy check=learn_listed result=ok",
                "privacy check=learn_deleted result=ok",
                "privacy check=learn_scrub result=ok",
                "privacy check=learn_delete_all result=not-run",
                "privacy check=learn_outside result=ok",
                "privacy check=learn_expired result=ok",
            ]
        );
        assert!(got.ok);
        let left: Vec<_> = std::fs::read_dir(&data)
            .expect("the data directory")
            .map(|e| e.expect("an entry").file_name())
            .collect();
        assert_eq!(left, ["derived"]);
    }

    #[test]
    fn privacy_reads_ok_on_its_own_directories_with_the_store_s_cascade() {
        let (data, temp) = (scratch("ok-data"), scratch("ok-temp"));
        let got = checks_with(&data, &temp, &mut store_cascade, &mut |_, _| {});
        let want: Vec<(Check, Outcome)> = Check::ALL
            .into_iter()
            .map(|c| {
                let made = matches!(c, Check::Deleted | Check::Outside);
                (c, if made { Outcome::Ok } else { Outcome::NotRun })
            })
            .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn learn_deleted_misses_a_cascade_that_keeps_a_learn_row_of_the_first_workflow() {
        let first = id('3', 1);
        for kind in ["learn_run", "learn_edit", "learn_routine"] {
            let got = deleted_with(&mut |store, id| {
                let row = store.learn_rows_mut()[kind]
                    .iter()
                    .find(|r| names(&r.json, &first))
                    .expect("a row of the first workflow")
                    .json
                    .clone();
                let stored = store.delete_workflow(id);
                put_back(store, kind, row);
                stored
            });
            assert_eq!(got, Outcome::Miss, "{kind}");
        }
    }

    #[test]
    fn learn_deleted_misses_a_kept_row_of_the_first_workflow_that_cannot_be_read() {
        let first = id('3', 1);
        let got = deleted_with(&mut |store, id| {
            let row = store.learn_rows_mut()["learn_edit"]
                .iter()
                .find(|r| names(&r.json, &first))
                .expect("an edit row of the first workflow")
                .json
                .clone();
            let stored = store.delete_workflow(id);
            put_back(store, "learn_edit", row[..row.len() - 1].to_owned());
            stored
        });
        assert_eq!(got, Outcome::Miss);
    }

    #[test]
    fn learn_deleted_misses_a_cascade_that_takes_a_row_of_the_second_workflow() {
        let second = id('3', 2);
        let cases: [(&str, &str); 4] = [
            ("learn_run", ""),
            ("learn_edit", "\"state\":\"reverted\""),
            ("learn_edit", "\"state\":\"active\""),
            ("learn_routine", ""),
        ];
        for (kind, holds) in cases {
            let got = deleted_with(&mut |store, id| {
                let stored = store.delete_workflow(id);
                remove_first(store, kind, |r| names(r, &second) && r.contains(holds));
                stored
            });
            assert_eq!(got, Outcome::Miss, "{kind} {holds}");
        }
    }

    #[test]
    fn learn_deleted_misses_a_cascade_that_rewrites_a_row_of_the_second_workflow() {
        let second = id('3', 2);
        let got = deleted_with(&mut |store, id| {
            let stored = store.delete_workflow(id);
            let rows = store
                .learn_rows_mut()
                .get_mut("learn_edit")
                .expect("edit rows");
            let row = rows
                .iter_mut()
                .find(|r| names(&r.json, &second) && r.json.contains("\"state\":\"active\""))
                .expect("the active row of the second workflow");
            row.json = row.json.replacen("Zq7", "Zq8", 1);
            stored
        });
        assert_eq!(got, Outcome::Miss);
    }

    #[test]
    fn learn_deleted_misses_a_cascade_that_takes_nothing_answers_not_stored_or_takes_another_id() {
        assert_eq!(deleted_with(&mut |_, _| true), Outcome::Miss, "nothing");
        assert_eq!(
            deleted_with(&mut |store, id| {
                store.delete_workflow(id);
                false
            }),
            Outcome::Miss,
            "not stored"
        );
        let elsewhere = WorkflowId::parse(&id('3', 9)).expect("an id");
        assert_eq!(
            deleted_with(&mut |store, _| store.delete_workflow(&elsewhere)),
            Outcome::Miss,
            "another id"
        );
    }

    #[test]
    fn learn_outside_misses_a_marker_in_a_file_or_a_name_under_the_data_directory() {
        for marker in MARKERS {
            let (data, temp) = (scratch("file-data"), scratch("file-temp"));
            std::fs::write(data.join("notes.txt"), format!("x {marker} y")).expect("a file");
            assert_eq!(outside_with(&data, &temp, &mut |_, _| {}), Outcome::Miss);
        }
        let (data, temp) = (scratch("deep-data"), scratch("deep-temp"));
        std::fs::create_dir_all(data.join("a").join("b")).expect("a nested directory");
        std::fs::write(data.join("a").join("b").join("log"), MARKERS[1]).expect("a file");
        assert_eq!(outside_with(&data, &temp, &mut |_, _| {}), Outcome::Miss);
        let (data, temp) = (scratch("name-data"), scratch("name-temp"));
        std::fs::write(data.join(format!("{}.txt", MARKERS[0])), "x").expect("a file");
        assert_eq!(outside_with(&data, &temp, &mut |_, _| {}), Outcome::Miss);
        let (data, temp) = (scratch("dir-data"), scratch("dir-temp"));
        std::fs::create_dir(data.join(MARKERS[1])).expect("a directory");
        assert_eq!(outside_with(&data, &temp, &mut |_, _| {}), Outcome::Miss);
    }

    #[test]
    fn learn_outside_misses_a_marker_that_the_command_writes_into_its_temporary_directory() {
        let (data, temp) = (scratch("leak-data"), scratch("leak-temp"));
        let got = outside_with(&data, &temp, &mut |_, temp| {
            std::fs::write(temp.join("cache.bin"), format!("\0{}\0", MARKERS[0])).expect("a file");
        });
        assert_eq!(got, Outcome::Miss);
        let (data, temp) = (scratch("leak-deep-data"), scratch("leak-deep-temp"));
        let got = outside_with(&data, &temp, &mut |_, temp| {
            std::fs::create_dir_all(temp.join("run").join("1")).expect("a directory");
            std::fs::write(temp.join("run").join("1").join("x"), MARKERS[1]).expect("a file");
        });
        assert_eq!(got, Outcome::Miss);
        let (data, temp) = (scratch("leak-data-dir"), scratch("leak-data-dir-temp"));
        let got = outside_with(&data, &temp, &mut |data, _| {
            std::fs::write(data.join("store.log"), MARKERS[0]).expect("a file");
        });
        assert_eq!(got, Outcome::Miss);
    }

    #[test]
    fn learn_outside_reads_ok_beside_files_without_a_marker_and_a_marked_entry_made_before_the_command(
    ) {
        let (data, temp) = (scratch("clean-data"), scratch("clean-temp"));
        std::fs::write(data.join("notes.txt"), "Zq7-halvo-marker-learn-c").expect("a file");
        std::fs::write(temp.join(MARKERS[0]), MARKERS[1]).expect("a file made before");
        let got = outside_with(&data, &temp, &mut |data, temp| {
            std::fs::write(temp.join("made.txt"), "nothing learned").expect("a file");
            std::fs::create_dir(data.join("empty")).expect("a directory");
        });
        assert_eq!(got, Outcome::Ok);
    }

    #[test]
    fn learn_outside_misses_when_a_directory_cannot_be_read() {
        let (data, temp) = (scratch("gone-data"), scratch("gone-temp"));
        assert_eq!(
            outside_with(&data.join("missing"), &temp, &mut |_, _| {}),
            Outcome::Miss
        );
        assert_eq!(
            outside_with(&data, &temp.join("missing"), &mut |_, _| {}),
            Outcome::Miss
        );
    }

    #[cfg(unix)]
    #[test]
    fn learn_outside_misses_a_link_whose_target_text_holds_a_marker() {
        let (data, temp) = (scratch("link-data"), scratch("link-temp"));
        std::os::unix::fs::symlink(MARKERS[0], data.join("link")).expect("a link");
        assert_eq!(outside_with(&data, &temp, &mut |_, _| {}), Outcome::Miss);
    }

    #[cfg(unix)]
    #[test]
    fn learn_outside_misses_a_file_that_cannot_be_read_and_a_directory_that_cannot_be_listed() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path, bits: u32| {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits)).expect("a mode");
        };
        let probe = scratch("probe").join("probe");
        std::fs::write(&probe, "x").expect("a probe file");
        mode(&probe, 0o000);
        let refused = std::fs::read(&probe).is_err();
        mode(&probe, 0o600);
        assert!(
            refused,
            "mode 000 did not refuse a read of a probe file (a user that reads every file, such as root?), so this \
             test cannot hold its rule here"
        );
        let (data, temp) = (scratch("closed-data"), scratch("closed-temp"));
        let closed = data.join("closed");
        std::fs::write(&closed, "x").expect("a file");
        mode(&closed, 0o000);
        let got = outside_with(&data, &temp, &mut |_, _| {});
        mode(&closed, 0o600);
        assert_eq!(got, Outcome::Miss, "a file that cannot be read");
        let (data, temp) = (scratch("shut-data"), scratch("shut-temp"));
        let shut = data.join("a").join("shut");
        std::fs::create_dir_all(&shut).expect("a nested directory");
        mode(&shut, 0o000);
        let got = outside_with(&data, &temp, &mut |_, _| {});
        mode(&shut, 0o700);
        assert_eq!(got, Outcome::Miss, "a directory that cannot be listed");
    }

    #[test]
    fn an_entry_that_is_gone_holds_no_marker_and_a_marked_name_holds_one() {
        let dir = scratch("gone");
        assert!(!holds_marker(&dir.join("never-made")));
        assert!(holds_marker(&dir.join(MARKERS[1])));
    }

    #[test]
    fn the_report_is_ok_with_not_run_lines_and_not_ok_with_a_miss() {
        let all = |o: Outcome| Check::ALL.map(|c| (c, o));
        assert!(report(&all(Outcome::NotRun)).ok);
        assert!(report(&all(Outcome::Ok)).ok);
        let mut one = all(Outcome::Ok);
        one[4].1 = Outcome::Miss;
        let got = report(&one);
        assert!(!got.ok);
        assert_eq!(got.lines[4], "privacy check=learn_outside result=miss");
        assert_eq!(got.lines[0], "privacy check=learn_listed result=ok");
    }

    #[test]
    fn privacy_refuses_a_call_without_an_existing_data_directory_with_usage_and_writes_nothing() {
        let data = scratch("usage");
        let file = data.join("a-file");
        std::fs::write(&file, "x").expect("a file");
        let path = |p: &Path| p.to_str().expect("a path").to_owned();
        let calls: [Vec<String>; 5] = [
            vec!["privacy".into()],
            vec!["privacy".into(), "--data-dir".into()],
            vec![
                "privacy".into(),
                "--data-dir".into(),
                path(&data.join("missing")),
            ],
            vec!["privacy".into(), "--data-dir".into(), path(&file)],
            vec![
                "privacy".into(),
                "--data-dir".into(),
                path(&data),
                "--store".into(),
                "memory".into(),
            ],
        ];
        for call in calls {
            let got = crate::cli::run(&call).expect_err("a refusal");
            assert_eq!(got.cause, Cause::Usage, "{call:?}");
        }
        let left: Vec<_> = std::fs::read_dir(&data)
            .expect("the directory")
            .map(|e| e.expect("an entry").file_name())
            .collect();
        assert_eq!(left, ["a-file"]);
    }

    #[test]
    fn learn_deleted_needs_rows_before_a_stored_answer_none_left_and_the_same_other_rows() {
        let row = |text: &str| -> Row { ("learn_run", text.to_owned()) };
        let (a, b, none) = (vec![row("a")], vec![row("b")], Vec::<Row>::new());
        assert!(deleted(&a, true, &none, &b, &b));
        assert!(!deleted(&none, true, &none, &b, &b), "no row before");
        assert!(!deleted(&a, false, &none, &b, &b), "not stored");
        assert!(!deleted(&a, true, &a, &b, &b), "a row left");
        assert!(!deleted(&a, true, &none, &none, &none), "no other row");
        assert!(!deleted(&a, true, &none, &b, &none), "the other row lost");
        assert!(
            !deleted(&a, true, &none, &b, &[row("c")]),
            "the other row changed"
        );
    }

    #[test]
    fn learn_outside_misses_when_the_seeded_rows_hold_no_marker() {
        let first = seed_case("Pay later", SEED_DAY);
        let last = first.occurrences[first.occurrences.len() - 1].day;
        let cases = [first, seed_case("Pay soon", last)];
        let (data, temp) = (scratch("unmarked-data"), scratch("unmarked-temp"));
        let got = outcomes(
            &cases,
            &data,
            &temp,
            &mut no_derived,
            &mut store_cascade,
            &mut |_, _| {},
        )
        .expect("the seeded store");
        assert_eq!(result_of(&got, Check::Deleted), Outcome::Ok);
        assert_eq!(result_of(&got, Check::Outside), Outcome::Miss);
    }

    #[cfg(unix)]
    #[test]
    fn learn_outside_reads_a_pipe_by_its_name_alone() {
        let (data, temp) = (scratch("pipe-data"), scratch("pipe-temp"));
        let made = std::process::Command::new("mkfifo")
            .arg(data.join("pipe"))
            .status()
            .expect("mkfifo runs");
        assert!(made.success());
        assert_eq!(outside_with(&data, &temp, &mut |_, _| {}), Outcome::Ok);
        let (data, temp) = (scratch("pipe-named-data"), scratch("pipe-named-temp"));
        let made = std::process::Command::new("mkfifo")
            .arg(data.join(MARKERS[0]))
            .status()
            .expect("mkfifo runs");
        assert!(made.success());
        assert_eq!(outside_with(&data, &temp, &mut |_, _| {}), Outcome::Miss);
    }

    #[test]
    fn privacy_usage_refusals_name_privacy_among_the_commands() {
        let none = crate::cli::run(&[]).expect_err("a refusal with no argument");
        assert_eq!(none.cause, Cause::Usage);
        assert_eq!(
            none.line(),
            "REFUSED usage: no command; commands: drift, delete, routine, privacy"
        );
        let other = crate::cli::run(&["nosuch".to_owned()]).expect_err("a refusal of nosuch");
        assert_eq!(other.cause, Cause::Usage);
        assert_eq!(
            other.line(),
            "REFUSED usage: an unknown command; commands: drift, delete, routine, privacy"
        );
    }

    #[test]
    fn learn_outside_needs_each_marker_in_an_active_and_a_reverted_row_of_its_own_workflow() {
        let edit = |state: &str, name: &str| -> Row {
            (
                "learn_edit",
                format!("{{\"state\":\"{state}\",\"target\":{{\"desc\":\"{name}\"}}}}"),
            )
        };
        let [a, b] = MARKERS;
        let first = vec![edit("active", a), edit("reverted", &format!("{a} second"))];
        let second = vec![edit("reverted", &format!("{b} second")), edit("active", b)];
        assert!(markers_seeded(&first, &second));
        assert!(
            !markers_seeded(&first[..1], &second),
            "no reverted row of the first"
        );
        assert!(
            !markers_seeded(&first, &second[..1]),
            "no active row of the second"
        );
        assert!(
            !markers_seeded(&second, &first),
            "each marker in the other workflow"
        );
        let withdrawn = vec![edit("active", a), edit("withdrawn", a)];
        assert!(
            !markers_seeded(&withdrawn, &second),
            "withdrawn in place of reverted"
        );
        let runs = vec![
            (
                "learn_run",
                format!("{{\"state\":\"active\",\"x\":\"{a}\"}}"),
            ),
            first[1].clone(),
        ];
        assert!(
            !markers_seeded(&runs, &second),
            "the active marker in a run row"
        );
        let plain = vec![edit("active", "Pay now"), first[1].clone()];
        assert!(
            !markers_seeded(&plain, &second),
            "an active row without the marker"
        );
        assert!(!markers_seeded(&[], &[]));
    }

    type Rewrite<'a> = &'a dyn Fn(&str, &str) -> String;

    fn kept_first_row(kind: &'static str, holds: &str, rewrite: Rewrite<'_>) -> Outcome {
        let first = id('3', 1);
        deleted_with(&mut |store, wid| {
            let row = store.learn_rows_mut()[kind]
                .iter()
                .find(|r| names(&r.json, &first) && r.json.contains(holds))
                .expect("a row of the first workflow")
                .json
                .clone();
            let stored = store.delete_workflow(wid);
            put_back(store, kind, rewrite(&row, &first));
            stored
        })
    }

    #[test]
    fn learn_deleted_misses_a_kept_row_whose_json_writes_the_first_id_with_an_escape() {
        let escaped = |row: &str, first: &str| {
            let escape = format!("\"\\u{:04x}{}\"", first.as_bytes()[0], &first[1..]);
            let new = row.replace(&format!("\"{first}\""), &escape);
            assert!(names(&new, first) && !new.contains(&format!("\"{first}\"")));
            new
        };
        for (kind, holds) in [
            ("learn_run", ""),
            ("learn_routine", ""),
            ("learn_edit", "\"state\":\"active\""),
            ("learn_edit", "\"state\":\"reverted\""),
        ] {
            assert_eq!(
                kept_first_row(kind, holds, &escaped),
                Outcome::Miss,
                "{kind} {holds}"
            );
        }
    }

    #[test]
    fn learn_deleted_misses_a_kept_edit_row_with_the_marker_whose_workflow_key_is_null_missing_or_no_id(
    ) {
        let forms: [(&str, Rewrite<'_>); 3] = [
            ("null", &|row, first| {
                row.replace(&format!("\"{first}\""), "null")
            }),
            ("missing", &|row, first| {
                row.replace(&format!("\"workflow\":\"{first}\","), "")
            }),
            ("no id", &|row, first| {
                row.replace(&format!("\"{first}\""), "\"x\"")
            }),
        ];
        for (form, rewrite) in forms {
            let checked = |row: &str, first: &str| {
                let new = rewrite(row, first);
                assert!(
                    new != row && new.contains(MARKERS[0]) && !new.contains(first),
                    "{form}"
                );
                new
            };
            assert_eq!(
                kept_first_row("learn_edit", "\"state\":\"active\"", &checked),
                Outcome::Miss,
                "{form}"
            );
        }
    }

    #[test]
    fn learn_deleted_misses_a_cascade_that_moves_a_row_of_the_second_workflow_to_another_kind() {
        let second = id('3', 2);
        let got = deleted_with(&mut |store, wid| {
            let stored = store.delete_workflow(wid);
            let rows = store
                .learn_rows_mut()
                .get_mut("learn_run")
                .expect("run rows");
            let at = rows
                .iter()
                .position(|r| names(&r.json, &second))
                .expect("a run row of the second");
            let row = rows.remove(at);
            put_back(store, "learn_routine", row.json);
            stored
        });
        assert_eq!(got, Outcome::Miss);
    }

    #[test]
    fn learn_deleted_misses_a_cascade_that_adds_a_copy_of_a_row_of_the_second_workflow() {
        let second = id('3', 2);
        let got = deleted_with(&mut |store, wid| {
            let stored = store.delete_workflow(wid);
            let row = store.learn_rows_mut()["learn_run"]
                .iter()
                .find(|r| names(&r.json, &second))
                .expect("a run row of the second")
                .json
                .clone();
            put_back(store, "learn_run", row);
            stored
        });
        assert_eq!(got, Outcome::Miss);
    }

    #[test]
    fn learn_outside_misses_a_marker_in_a_dot_entry_below_the_top_of_the_data_directory() {
        let (data, temp) = (scratch("dot-data"), scratch("dot-temp"));
        std::fs::create_dir_all(data.join("a").join(".cache")).expect("a dot directory");
        std::fs::write(data.join("a").join(".cache").join("log"), MARKERS[0]).expect("a file");
        assert_eq!(outside_with(&data, &temp, &mut |_, _| {}), Outcome::Miss);
        let (data, temp) = (scratch("dot-file-data"), scratch("dot-file-temp"));
        std::fs::create_dir(data.join("b")).expect("a directory");
        std::fs::write(data.join("b").join(".hidden"), MARKERS[1]).expect("a dot file");
        assert_eq!(outside_with(&data, &temp, &mut |_, _| {}), Outcome::Miss);
    }

    #[test]
    fn learn_outside_misses_a_marker_beyond_the_first_4096_bytes_of_a_file() {
        let (data, temp) = (scratch("far-data"), scratch("far-temp"));
        let mut bytes = vec![b'x'; 4096];
        bytes.extend_from_slice(MARKERS[0].as_bytes());
        std::fs::write(data.join("far.bin"), &bytes).expect("a file");
        assert_eq!(outside_with(&data, &temp, &mut |_, _| {}), Outcome::Miss);
    }

    #[test]
    fn learn_deleted_misses_a_kept_row_that_cannot_be_parsed_and_holds_no_marker_by_its_quoted_id()
    {
        let cut = |row: &str, first: &str| {
            let cut = row[..row.len() - 1].to_owned();
            assert!(serde_json::from_str::<serde_json::Value>(&cut).is_err());
            assert!(!cut.contains(MARKERS[0]));
            assert!(cut.contains(&format!("\"{first}\"")));
            cut
        };
        for kind in ["learn_run", "learn_routine"] {
            assert_eq!(kept_first_row(kind, "", &cut), Outcome::Miss, "{kind}");
        }
    }

    #[test]
    fn learn_outside_misses_when_each_seed_holds_the_other_workflow_s_marker() {
        let first = seed_case(MARKERS[1], SEED_DAY);
        let last = first.occurrences[first.occurrences.len() - 1].day;
        let cases = [first, seed_case(MARKERS[0], last)];
        let (data, temp) = (scratch("swapped-data"), scratch("swapped-temp"));
        let got = outcomes(
            &cases,
            &data,
            &temp,
            &mut no_derived,
            &mut store_cascade,
            &mut |_, _| {},
        )
        .expect("the seeded store");
        assert_eq!(result_of(&got, Check::Outside), Outcome::Miss);
    }
}
