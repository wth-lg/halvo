use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::ErrorKind;
use std::path::Path;

use halvo_schema::WorkflowId;

use crate::cli::{Cause, Flags, Refusal, Report};
use crate::delete::{seed, seed_cases, Cascade, MARKERS};
use crate::store::{Family, MemoryStore};

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

fn naming(store: &MemoryStore, workflow: &str) -> Vec<Row> {
    let quoted = format!("\"{workflow}\"");
    let mut rows: Vec<Row> = store
        .rows(Family::Learn)
        .all()
        .filter(|(_, json)| json.contains(&quoted))
        .map(|(kind, json)| (kind, json.to_owned()))
        .collect();
    rows.sort();
    rows
}

fn markers_seeded(first: &[Row], second: &[Row]) -> bool {
    [(first, MARKERS[0]), (second, MARKERS[1])]
        .iter()
        .all(|(rows, marker)| rows.iter().any(|(_, json)| json.contains(marker)))
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

/// The checks on the memory store (contracts.md L11.4; the coordinator's ruling LM-54 E5), each with its result.
///
/// It seeds the two workflows of `learn-gate delete` (`delete::seed`), whose learned rows hold the markers, reads
/// the rows of the family `learn` that hold each workflow's id as a JSON string, each by kind and bytes, and calls
/// `cascade` with the first workflow's id; then it calls `after` with `data_dir` and `temp`. `learn_deleted` is ok
/// when the first workflow had rows, the cascade answered that it was stored, no row holds its id after the
/// cascade, and the second workflow's rows are the same rows as before it, by kind and bytes, and more than none.
/// `learn_outside` is ok when each marker was in a row of its own workflow, so that the search can find one, and
/// neither marker's bytes are in a name, the bytes of a regular file or the target of a symbolic link, which is not
/// followed, at any depth under `data_dir`, or under an entry of `temp` whose name was not there before the seed;
/// a directory that cannot be listed or an entry that cannot be read counts as holding a marker, and an entry that
/// is gone as holding none. The memory store keeps no file, so all of `data_dir` is outside the store. The other
/// four checks are `not-run`: `learn_listed`, `learn_scrub` and `learn_expired` need the derived store and
/// `learn_delete_all` the capture run's `halvo-capture --delete-all`, which this build does not hold.
pub fn outcomes(
    data_dir: &Path,
    temp: &Path,
    cascade: Cascade<'_>,
    after: &mut dyn FnMut(&Path, &Path),
) -> Result<[(Check, Outcome); 6], String> {
    let before = entries(temp).ok();
    let cases = seed_cases();
    let (mut play, first) = seed(&cases)?;
    let id = WorkflowId::parse(&first).map_err(|_| "the first workflow's id".to_owned())?;
    let second = play.workflow().to_owned();
    let store = &mut play.host.store;
    let first_rows = naming(store, &first);
    let kept = naming(store, &second);
    let seeded = markers_seeded(&first_rows, &kept);
    let stored = cascade(store, &id);
    let deleted = !first_rows.is_empty()
        && stored
        && naming(store, &first).is_empty()
        && !kept.is_empty()
        && naming(store, &second) == kept;
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

    use super::{holds_marker, markers_seeded, outcomes, report, Check, Outcome};
    use crate::cli::Cause;
    use crate::delete::MARKERS;
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
        outcomes(data, temp, cascade, after)
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
    fn learn_outside_misses_a_link_to_a_marker_and_a_file_that_cannot_be_read() {
        use std::os::unix::fs::PermissionsExt;
        let (data, temp) = (scratch("link-data"), scratch("link-temp"));
        std::os::unix::fs::symlink(MARKERS[0], data.join("link")).expect("a link");
        assert_eq!(outside_with(&data, &temp, &mut |_, _| {}), Outcome::Miss);
        let (data, temp) = (scratch("closed-data"), scratch("closed-temp"));
        let closed = data.join("closed");
        std::fs::write(&closed, "x").expect("a file");
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o000)).expect("a mode");
        let readable = std::fs::read(&closed).is_ok();
        let got = outside_with(&data, &temp, &mut |_, _| {});
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o600)).expect("a mode");
        if !readable {
            assert_eq!(got, Outcome::Miss);
        }
    }

    #[test]
    fn an_entry_that_is_gone_holds_no_marker_and_a_marked_name_holds_one() {
        let dir = scratch("gone");
        assert!(!holds_marker(&dir.join("never-made")));
        assert!(holds_marker(&dir.join(MARKERS[1])));
    }

    #[test]
    fn learn_outside_needs_each_marker_in_a_row_of_its_own_workflow() {
        let row = |text: &str| ("learn_edit", text.to_owned());
        let a = row(&format!("{{\"name\":\"{}\"}}", MARKERS[0]));
        let b = row(&format!("{{\"name\":\"{}\"}}", MARKERS[1]));
        let none = row("{\"name\":\"Pay now\"}");
        let one = |r: &(&'static str, String)| vec![r.clone()];
        assert!(markers_seeded(&[none.clone(), a.clone()], &one(&b)));
        assert!(!markers_seeded(&one(&none), &one(&b)));
        assert!(!markers_seeded(&one(&a), &one(&none)));
        assert!(!markers_seeded(&one(&b), &one(&a)));
        assert!(!markers_seeded(&[], &[]));
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
}
