use std::path::{Component, Path, PathBuf};

use halvo_control::paths::{DataDir, ENTRIES, INTENT_FILE, MACOS_LAUNCH_AGENTS_DIR, STATE_FILE};
use serde::Serialize;

use crate::commands::capture::{run_offline, Before, Offline, WithoutTheChild};
use crate::launch::Exit;
use crate::quit::Children;
use crate::view_state::{ActionError, DeleteView, OutcomeError};

/// A step of Remove Halvo (contracts.md C10.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Account,
    DeleteAll,
    Keychain,
    LoginItem,
    AppFiles,
    DataDir,
}

/// The steps of Remove Halvo in their order; a failed step ends the list.
pub const STEPS: [Step; 6] = [
    Step::Account,
    Step::DeleteAll,
    Step::Keychain,
    Step::LoginItem,
    Step::AppFiles,
    Step::DataDir,
];

/// How a step ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Done,
    Skipped,
    Failed,
}

/// What a step tells beside its outcome: the folder that it names, a count, and the cause of a
/// skip or a failure.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Detail {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

/// One step of the answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StepResult {
    pub step: Step,
    pub result: Outcome,
    pub detail: Detail,
}

/// The answer of `remove_halvo`: the steps that ran, in their order, up to a failed one; none
/// where the list did not start.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Removal {
    pub steps: Vec<StepResult>,
}

impl Removal {
    /// Whether the list ran to its end: every step, and none failed.
    pub fn whole(&self) -> bool {
        self.steps.len() == STEPS.len()
            && self.steps.iter().all(|step| step.result != Outcome::Failed)
    }

    /// The exit of the app after this answer: 0 after a whole list, none after one that
    /// stopped.
    pub fn exit(&self) -> Option<i32> {
        self.whole().then(|| Exit::Quit.code())
    }
}

/// The supervisor of the app's child as Remove Halvo holds it: the offline delete's hold, and the
/// end of a quit.
pub trait TheChild: WithoutTheChild + Children {}

impl<T: WithoutTheChild + Children> TheChild for T {}

/// Why the places of a removal were refused before any removal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    NoHome,
    NoDataFolder,
    BundleId,
    LastPart,
    NotBelowTheHome,
    Kept,
    HoldsTheDataFolder,
    DataFolder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    File,
    Tree,
    EmptyFolder,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Place {
    path: PathBuf,
    kind: Kind,
}

#[derive(Debug, Clone, Copy)]
enum Last {
    Id,
    IdThen(&'static str),
    InId(&'static str),
}

struct Row {
    step: Step,
    parents: &'static [&'static str],
    last: Last,
    kind: Kind,
}

const LIBRARY: &str = "Library";

const APPLICATION_SUPPORT: &str = "Application Support";

/// The name of the settings file that 685d846 wrote under the app's config folder.
pub const OLD_CONSENT_FILE: &str = "consent.json";

const ROWS: [Row; 5] = [
    Row {
        step: Step::LoginItem,
        parents: &MACOS_LAUNCH_AGENTS_DIR,
        last: Last::IdThen(".plist"),
        kind: Kind::File,
    },
    Row {
        step: Step::AppFiles,
        parents: &[LIBRARY, "Caches"],
        last: Last::Id,
        kind: Kind::Tree,
    },
    Row {
        step: Step::AppFiles,
        parents: &[LIBRARY, "WebKit"],
        last: Last::Id,
        kind: Kind::Tree,
    },
    Row {
        step: Step::AppFiles,
        parents: &[LIBRARY, APPLICATION_SUPPORT],
        last: Last::InId(OLD_CONSENT_FILE),
        kind: Kind::File,
    },
    Row {
        step: Step::AppFiles,
        parents: &[LIBRARY, APPLICATION_SUPPORT],
        last: Last::Id,
        kind: Kind::EmptyFolder,
    },
];

const KEPT: [&[&str]; 5] = [
    &[LIBRARY],
    &[LIBRARY, "Caches"],
    &[LIBRARY, "WebKit"],
    &[LIBRARY, APPLICATION_SUPPORT],
    &MACOS_LAUNCH_AGENTS_DIR,
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct DataFolder {
    name: PathBuf,
    link: bool,
    resolved: Option<PathBuf>,
    entries: Vec<Place>,
}

/// Every place that a removal may take, each made from the home, the bundle id and a fixed last
/// part, or from the data folder and a name of halvo-control's table, and each checked by one
/// check before any removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Places {
    login_item: Option<Place>,
    app_files: Option<Vec<Place>>,
    data: DataFolder,
}

impl Places {
    /// The places of `bundle_id` under `home` and those of `data_dir`; with `given`, a run with a
    /// data folder of its own, none outside the data folder. It refuses an empty or a compound
    /// bundle id or last part, a place that is not strictly below the home, the home's Library and
    /// its fixed folders, a place that is or holds the data folder, a data folder whose name is
    /// not strictly below the home, and one that resolves to the home, a folder that holds it or
    /// one of those fixed folders.
    pub fn new(
        home: &Path,
        bundle_id: &str,
        data_dir: &DataDir,
        given: bool,
    ) -> Result<Places, Refused> {
        if !home.is_absolute() || home.parent().is_none() || !plain(home) {
            return Err(Refused::NoHome);
        }
        let real_home = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
        let data = data_folder(home, &real_home, data_dir)?;
        let mut held = vec![data.name.as_path()];
        held.extend(data.resolved.as_deref());
        let mut outside = |step: Step| -> Result<Vec<Place>, Refused> {
            ROWS.iter()
                .filter(|row| row.step == step)
                .map(|row| {
                    let path = path_of(home, bundle_id, row)?;
                    check(home, &path, &held)?;
                    Ok(Place {
                        path,
                        kind: row.kind,
                    })
                })
                .collect()
        };
        let login_item = outside(Step::LoginItem)?.pop();
        let app_files = outside(Step::AppFiles)?;
        Ok(Places {
            login_item: (!given).then_some(login_item).flatten(),
            app_files: (!given).then_some(app_files),
            data,
        })
    }
}

fn one_name(text: &str) -> bool {
    !text.is_empty() && text != "." && text != ".." && !text.contains(['/', '\0'])
}

fn plain(path: &Path) -> bool {
    path.components()
        .all(|part| matches!(part, Component::RootDir | Component::Normal(_)))
}

fn path_of(home: &Path, bundle_id: &str, row: &Row) -> Result<PathBuf, Refused> {
    if !one_name(bundle_id) {
        return Err(Refused::BundleId);
    }
    if !row.parents.iter().all(|part| one_name(part)) {
        return Err(Refused::LastPart);
    }
    let mut path = home.to_path_buf();
    path.extend(row.parents);
    match row.last {
        Last::Id => path.push(bundle_id),
        Last::IdThen(suffix) if !suffix.is_empty() && one_name(suffix) => {
            path.push(format!("{bundle_id}{suffix}"));
        }
        Last::InId(name) if one_name(name) => {
            path.push(bundle_id);
            path.push(name);
        }
        Last::IdThen(_) | Last::InId(_) => return Err(Refused::LastPart),
    }
    Ok(path)
}

fn below(home: &Path, path: &Path) -> bool {
    plain(path) && path.starts_with(home) && path != home
}

fn kept(home: &Path, path: &Path) -> bool {
    KEPT.iter().any(|parts| {
        let mut folder = home.to_path_buf();
        folder.extend(*parts);
        path == folder
    })
}

fn check(home: &Path, path: &Path, held: &[&Path]) -> Result<(), Refused> {
    if !below(home, path) {
        return Err(Refused::NotBelowTheHome);
    }
    if kept(home, path) {
        return Err(Refused::Kept);
    }
    if held.iter().any(|data| data.starts_with(path)) {
        return Err(Refused::HoldsTheDataFolder);
    }
    Ok(())
}

fn table_names() -> impl Iterator<Item = &'static str> {
    ENTRIES
        .iter()
        .map(|entry| entry.name())
        .chain([STATE_FILE, INTENT_FILE])
}

fn data_folder(home: &Path, real_home: &Path, data_dir: &DataDir) -> Result<DataFolder, Refused> {
    let name = data_dir.path().to_path_buf();
    if !below(home, &name) || kept(home, &name) {
        return Err(Refused::DataFolder);
    }
    let link = std::fs::symlink_metadata(&name).is_ok_and(|meta| meta.file_type().is_symlink());
    let resolved = std::fs::canonicalize(&name).ok();
    if let Some(folder) = &resolved {
        let at_home = [home, real_home]
            .iter()
            .any(|home| home.starts_with(folder) || kept(home, folder));
        if at_home || !plain(folder) {
            return Err(Refused::DataFolder);
        }
    }
    let entries = match &resolved {
        Some(folder) => table_names()
            .map(|entry| {
                if !one_name(entry) {
                    return Err(Refused::DataFolder);
                }
                Ok(Place {
                    path: folder.join(entry),
                    kind: Kind::Tree,
                })
            })
            .collect::<Result<_, _>>()?,
        None => Vec::new(),
    };
    Ok(DataFolder {
        name,
        link,
        resolved,
        entries,
    })
}

/// The account step in this version: the hook that a later run fills, which removes nothing.
pub fn account_hook() -> (Outcome, Detail) {
    (
        Outcome::Skipped,
        Detail {
            cause: Some("no_account".to_owned()),
            ..Detail::default()
        },
    )
}

/// Remove Halvo (contracts.md C10.7 and item 8 of C17): the steps of `STEPS` in their order,
/// each answered, up to the first that fails. Where `places` was refused it runs nothing. The
/// account step is `account`; the delete is the offline delete of `offline` after `before`, and
/// it is done only where it deleted and left nothing of the store, or found no store.
pub fn remove_halvo(
    places: Result<Places, Refused>,
    children: Option<&dyn TheChild>,
    account: &dyn Fn() -> (Outcome, Detail),
    offline: Option<&Offline>,
    before: &Before,
) -> Removal {
    let _places = match places {
        Ok(places) => places,
        Err(refused) => {
            eprintln!("halvo: Remove Halvo removes nothing, because its list refused a place: {refused:?}");
            return Removal::default();
        }
    };
    let _ = children;
    let mut steps = Vec::new();
    for step in STEPS {
        let (result, detail) = match step {
            Step::Account => account(),
            Step::DeleteAll => delete_all(offline, before),
            Step::Keychain | Step::LoginItem | Step::AppFiles | Step::DataDir => (
                Outcome::Skipped,
                Detail {
                    cause: Some("not_built".to_owned()),
                    ..Detail::default()
                },
            ),
        };
        steps.push(StepResult {
            step,
            result,
            detail,
        });
        if result == Outcome::Failed {
            break;
        }
    }
    Removal { steps }
}

fn delete_all(offline: Option<&Offline>, before: &Before) -> (Outcome, Detail) {
    let answer = match offline {
        Some(offline) => run_offline(
            &Offline {
                children: None,
                ..offline.clone()
            },
            before,
        ),
        None => Err(OutcomeError {
            error: ActionError::DeleteNotStarted,
            outcome_not_known: false,
        }),
    };
    match answer {
        Ok(view) => {
            let looked_in = match &view {
                DeleteView::Deleted { looked_in, .. } => looked_in.clone(),
                DeleteView::NoStore { looked_in } => Some(looked_in.clone()),
                DeleteView::AlreadyDeleted { .. } | DeleteView::DeleteRefused => None,
            };
            let done = view.finished() || matches!(view, DeleteView::NoStore { .. });
            (
                if done { Outcome::Done } else { Outcome::Failed },
                Detail {
                    folder: looked_in,
                    cause: (!done).then(|| "something_left".to_owned()),
                    ..Detail::default()
                },
            )
        }
        Err(error) => {
            let read = serde_json::to_value(&error.error).unwrap_or_default();
            let text = |key: &str| {
                read.get(key)
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            };
            (
                Outcome::Failed,
                Detail {
                    folder: text("looked_in"),
                    cause: text("error"),
                    ..Detail::default()
                },
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::sync::Arc;

    use halvo_control::paths::{AccountLookup, DarwinUserTempDir, PathError};

    use super::*;
    use crate::daemon_child::stand_in::{note, Record};
    use crate::tests::scratch_dir;
    use crate::view_state::OfflineEnd;

    const ID: &str = halvo_control::model::APP_BUNDLE_ID;

    struct Home(PathBuf);

    impl AccountLookup for Home {
        fn base_dir(&self) -> Result<Option<OsString>, PathError> {
            Ok(Some(self.0.clone().into_os_string()))
        }

        fn user_temp_dir(&self) -> Result<Option<DarwinUserTempDir>, PathError> {
            Ok(None)
        }
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("a file has a folder"))
            .expect("the folder is made");
        std::fs::write(path, text).expect("the file is written");
    }

    fn library(home: &Path, parts: &[&str]) -> PathBuf {
        let mut path = home.join(LIBRARY);
        path.extend(parts);
        path
    }

    fn halvos_data(home: &Path) -> DataDir {
        let data = DataDir::for_account(&Home(home.to_path_buf())).expect("a data folder");
        for entry in ENTRIES {
            write(&data.entry(&entry).join("kept"), entry.name());
        }
        write(&data.state_file(), "{}");
        write(&data.intent_file(), "");
        data
    }

    fn halvos_places(home: &Path) {
        write(
            &library(home, &["LaunchAgents", &format!("{ID}.plist")]),
            "plist",
        );
        write(
            &library(home, &["Caches", ID, "engine-rules", "rules"]),
            "rules",
        );
        write(&library(home, &["WebKit", ID, "WebsiteData", "x"]), "web");
        write(
            &library(home, &[APPLICATION_SUPPORT, ID, OLD_CONSENT_FILE]),
            "{}",
        );
    }

    fn tree(dir: &Path) -> BTreeMap<PathBuf, String> {
        let mut seen = BTreeMap::new();
        let mut left = vec![dir.to_path_buf()];
        while let Some(folder) = left.pop() {
            for entry in std::fs::read_dir(&folder).expect("the folder reads") {
                let path = entry.expect("an entry").path();
                let meta = std::fs::symlink_metadata(&path).expect("the entry reads");
                let text = if meta.file_type().is_symlink() {
                    format!(
                        "link {}",
                        std::fs::read_link(&path).expect("a link").display()
                    )
                } else if meta.is_dir() {
                    left.push(path.clone());
                    "folder".to_owned()
                } else {
                    std::fs::read_to_string(&path).unwrap_or_default()
                };
                seen.insert(path.strip_prefix(dir).expect("below").to_path_buf(), text);
            }
        }
        seen
    }

    fn before() -> Before {
        Before {
            modules: Vec::new(),
            runs: Arc::default(),
            core: None,
        }
    }

    fn offline(data: &DataDir, record: &Record, end: OfflineEnd) -> Offline {
        let record = Arc::clone(record);
        Offline {
            program: PathBuf::from("/stand-in/halvo-capture"),
            data_dir: data.clone(),
            children: None,
            run: Arc::new(move |program, args| {
                let args: Vec<String> = args
                    .iter()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect();
                note(
                    &record,
                    format!("run {} {}", program.display(), args.join(" ")),
                );
                end.clone()
            }),
        }
    }

    fn deleted(looked_in: &str, root: &str, root_id: &str, left: u64) -> String {
        format!(
            "data_dir={looked_in}\ndeleted segments_shredded=unknown keys_destroyed=unknown root_destroyed={root} root_id={root_id} files_left={left} database_left=no not_followed=0 entries_removed=3 entries_left={left} unknown_entries=0 intent=written\n"
        )
    }

    fn failing_account() -> (Outcome, Detail) {
        (
            Outcome::Failed,
            Detail {
                cause: Some("the account service refused".to_owned()),
                ..Detail::default()
            },
        )
    }

    fn ended_at(removal: &Removal) -> Vec<(Step, Outcome)> {
        removal
            .steps
            .iter()
            .map(|step| (step.step, step.result))
            .collect()
    }

    #[test]
    fn remove_halvo_removes_nothing_where_the_delete_did_not_finish() {
        let root_id = "0123456789abcdef0123456789abcdef";
        let cases: [(&str, Option<OfflineEnd>, &str); 5] = [
            (
                "a delete that answers an error",
                Some(OfflineEnd::Exited {
                    code: 75,
                    out: "data_dir={looked_in}\n".to_owned(),
                }),
                "delete_held",
            ),
            (
                "a delete that never answers",
                Some(OfflineEnd::NoEnd),
                "delete_not_known",
            ),
            (
                "an offline end with 1",
                Some(OfflineEnd::Exited {
                    code: 1,
                    out: deleted("{looked_in}", "yes", "-", 0),
                }),
                "something_left",
            ),
            (
                "a delete that answers deleted with something left",
                Some(OfflineEnd::Exited {
                    code: 0,
                    out: deleted("{looked_in}", "no", root_id, 1),
                }),
                "something_left",
            ),
            (
                "a failing account step",
                None,
                "the account service refused",
            ),
        ];
        for (n, (name, end, cause)) in cases.into_iter().enumerate() {
            let home = scratch_dir(&format!("remove-not-finished-{n}")).join("home");
            let data = halvos_data(&home);
            halvos_places(&home);
            let looked_in = data.path().display().to_string();
            let as_before = tree(&home);
            let record = Record::default();
            let end = end.map(|end| match end {
                OfflineEnd::Exited { code, out } => OfflineEnd::Exited {
                    code,
                    out: out.replace("{looked_in}", &looked_in),
                },
                other => other,
            });
            let account_fails = end.is_none();
            let stand_in = offline(&data, &record, end.unwrap_or(OfflineEnd::NoEnd));
            let account: &dyn Fn() -> (Outcome, Detail) = if account_fails {
                &failing_account
            } else {
                &account_hook
            };
            let removal = remove_halvo(
                Places::new(&home, ID, &data, false),
                None,
                account,
                Some(&stand_in),
                &before(),
            );
            assert_eq!(
                tree(&home),
                as_before,
                "{name}: the scratch home reads as before"
            );
            let (expected, ran) = if account_fails {
                (vec![(Step::Account, Outcome::Failed)], Vec::<String>::new())
            } else {
                (
                    vec![
                        (Step::Account, Outcome::Skipped),
                        (Step::DeleteAll, Outcome::Failed),
                    ],
                    vec![format!(
                        "run /stand-in/halvo-capture --delete-all --data-dir {looked_in}"
                    )],
                )
            };
            assert_eq!(
                ended_at(&removal),
                expected,
                "{name}: the list ends at the failed step"
            );
            assert_eq!(
                removal
                    .steps
                    .last()
                    .and_then(|step| step.detail.cause.as_deref()),
                Some(cause),
                "{name}"
            );
            assert_eq!(
                *record.lock().unwrap(),
                ran,
                "{name}: the runner records the delete alone"
            );
            assert_eq!(removal.exit(), None, "{name}: the app does not end");
        }
    }
}
