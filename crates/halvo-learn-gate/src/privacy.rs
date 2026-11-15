use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::ErrorKind;
use std::path::Path;

use halvo_schema::WorkflowId;

use crate::case::Case;
use crate::cli::{Cause, Flags, Refusal, Report};
use crate::delete::{seed, seed_cases, Cascade, MARKERS};
use crate::store::{Family, MemoryStore};
use halvo_schema::kinds::registry;

/// The six checks of contracts.md L11.4, in the order of their lines.
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

fn naming(store: &MemoryStore, id: &WorkflowId, marker: &str) -> Vec<Row> {
    let quoted = format!("\"{}\"", id.as_str());
    let mut rows: Vec<Row> = store
        .rows(Family::Learn)
        .all()
        .filter(|(kind, json)| {
            key_names(kind, json, id) || json.contains(&quoted) || json.contains(marker)
        })
        .map(|(kind, json)| (kind, json.to_owned()))
        .collect();
    rows.sort();
    rows
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

/// The checks on the memory store (contracts.md L11.4; the coordinator's rulings LM-54 E5 and LM-82), each with its
/// result. It plays the two seeded cases as `learn-gate delete` does (`delete::seed`; the command's are
/// `delete::seed_cases`, whose learned rows hold the markers), reads the rows of the family `learn` that name each
/// workflow, each by kind and bytes, and calls `cascade` with the first workflow's id; then it calls `after` with
/// `data_dir` and `temp`. A row names a workflow when the value under its kind's workflow key parses to that
/// workflow's id, when its bytes hold the id as a JSON string, or when its bytes hold that workflow's marker. In this
/// pull request `learn_deleted` checks the delete of the gate's own test store in memory (its own cascade), not the
/// app's delete, and reads no file: after that cascade no learned row names the first seeded workflow by its kind's
/// workflow key, holds its id as a JSON string or holds its marker, and the second workflow's rows are the same rows,
/// by kind and bytes, with none added. `learn_deleted` is ok when the first workflow had rows, the cascade answered
/// that it was stored, no row names it after the cascade, and the rows that name the second workflow are the same
/// rows as before it, by kind and bytes, and more than none. `learn_outside` searches only the directory that the
/// command is given, to which nothing of the command writes in this build, and, in the process's temporary directory,
/// each entry whose name was not at its top before the seed, at every depth, and nothing else on the computer; the
/// search looks for the markers' bytes in names, in regular files and in the target texts of links, follows no link
/// below those places, reads no extended attribute (on Windows, no alternate data stream) and no other encoding, and
/// counts an entry that cannot be read as holding a marker. `learn_outside` is ok when, besides, each marker was in
/// an `active` and in a `reverted` `learn_edit` row of its own workflow, read by the kind's workflow key alone, so
/// that the search can find one; a directory that cannot be listed counts as holding a marker, and an entry that is
/// gone as holding none. The memory store keeps no file, so all of `data_dir` is outside the store. The other four
/// checks are `not-run`: `learn_listed`, `learn_scrub` and `learn_expired` need the derived store and
/// `learn_delete_all` the capture run's `halvo-capture --delete-all`, which this build does not hold.
pub fn outcomes(
    cases: &[Case; 2],
    data_dir: &Path,
    temp: &Path,
    cascade: Cascade<'_>,
    after: &mut dyn FnMut(&Path, &Path),
) -> Result<[(Check, Outcome); 6], String> {
    let before = entries(temp).ok();
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
            Check::Deleted => Outcome::holds(deleted),
            Check::Outside => Outcome::holds(outside),
            Check::Listed | Check::Scrub | Check::DeleteAll | Check::Expired => Outcome::NotRun,
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

/// `learn-gate privacy --data-dir DIR` (contracts.md L10.4, L11.4): the checks of `outcomes` on the memory store with
/// the store's cascade, `DIR` as the data directory and the process's temporary directory, one line each. `DIR`
/// must be an existing directory; the refusal `usage` otherwise, and `store` when the seed fails.
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
        &mut |store, id| store.delete_workflow(id),
        &mut |_, _| {},
    )
    .map_err(|detail| Refusal::new(Cause::Store, detail))?;
    Ok(report(&outcomes))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    use halvo_schema::WorkflowId;

    use super::{deleted, holds_marker, markers_seeded, outcomes, report, Check, Outcome, Row};
    use crate::cli::Cause;
    use crate::delete::{seed_case, seed_cases, MARKERS, SEED_DAY};
    use crate::store::MemoryStore;
    use crate::stub::id;

    fn scratch(name: &str) -> PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let exe = std::env::current_exe().expect("the test binary");
        let root = exe
            .ancestors()
            .nth(3)
            .expect("the target directory")
            .join("tmp")
            .join("r5-privacy");
        std::fs::create_dir_all(&root).expect("the tests' directory");
        loop {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = root.join(format!("{name}-{}-{n}", std::process::id()));
            match std::fs::create_dir(&dir) {
                Ok(()) => return dir,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("a test directory: {e}"),
            }
        }
    }

    fn store_cascade(store: &mut MemoryStore, id: &WorkflowId) -> bool {
        store.delete_workflow(id)
    }

    fn checks_with(
        data: &Path,
        temp: &Path,
        cascade: &mut dyn FnMut(&mut MemoryStore, &WorkflowId) -> bool,
        after: &mut dyn FnMut(&Path, &Path),
    ) -> Vec<(Check, Outcome)> {
        outcomes(&seed_cases(), data, temp, cascade, after)
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
            .position(|r| keep_out(r))
            .expect("a row to take");
        rows.remove(at);
    }

    fn put_back(store: &mut MemoryStore, kind: &str, row: String) {
        store
            .learn_rows_mut()
            .entry(kind_name(kind))
            .or_default()
            .push(row);
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
                "privacy check=learn_listed result=not-run",
                "privacy check=learn_deleted result=ok",
                "privacy check=learn_scrub result=not-run",
                "privacy check=learn_delete_all result=not-run",
                "privacy check=learn_outside result=ok",
                "privacy check=learn_expired result=not-run",
            ]
        );
        assert!(got.ok);
        assert_eq!(
            std::fs::read_dir(&data)
                .expect("the data directory")
                .count(),
            0
        );
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
                    .find(|r| names(r, &first))
                    .expect("a row of the first workflow")
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
                .find(|r| names(r, &first))
                .expect("an edit row of the first workflow")
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
                .find(|r| names(r, &second) && r.contains("\"state\":\"active\""))
                .expect("the active row of the second workflow");
            *row = row.replacen("Zq7", "Zq8", 1);
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
        let got = outcomes(&cases, &data, &temp, &mut store_cascade, &mut |_, _| {})
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
                .find(|r| names(r, &first) && r.contains(holds))
                .expect("a row of the first workflow")
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
                .position(|r| names(r, &second))
                .expect("a run row of the second");
            let row = rows.remove(at);
            put_back(store, "learn_routine", row);
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
                .find(|r| names(r, &second))
                .expect("a run row of the second")
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
}
