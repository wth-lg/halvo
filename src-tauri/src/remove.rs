use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use halvo_control::model::APP_BUNDLE_ID;
use halvo_control::paths::{
    one_line, AccountLookup, DataDir, RemovedBy, ENTRIES, INTENT_FILE, KEYCHAIN_ITEMS,
    MACOS_DATA_DIR, MACOS_LAUNCH_AGENTS_DIR, STATE_FILE,
};

use crate::commands::capture::{run_offline, Before, Offline, WithoutTheChild};
use crate::poller::Views;
use crate::quit::Children;
use crate::view_state::{
    ActionError, DeleteView, Detail, Hold, Outcome, OutcomeError, Removal, Step, StepResult, STEPS,
};

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
    Link,
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
    Fixed(&'static str),
}

struct Row {
    step: Step,
    parents: &'static [&'static str],
    last: Last,
    kind: Kind,
    at_start: bool,
}

const LIBRARY: &str = "Library";

const APPLICATION_SUPPORT: &str = "Application Support";

const OLD_CONSENT_FILE: &str = "consent.json";

const DATA_FOLDER_NAME: &str = MACOS_DATA_DIR[2];

const ROWS: [Row; 7] = [
    Row {
        step: Step::LoginItem,
        parents: &MACOS_LAUNCH_AGENTS_DIR,
        last: Last::IdThen(".plist"),
        kind: Kind::File,
        at_start: false,
    },
    Row {
        step: Step::AppFiles,
        parents: &[LIBRARY, "Caches"],
        last: Last::Id,
        kind: Kind::Tree,
        at_start: false,
    },
    Row {
        step: Step::AppFiles,
        parents: &[LIBRARY, "WebKit"],
        last: Last::Id,
        kind: Kind::Tree,
        at_start: false,
    },
    Row {
        step: Step::AppFiles,
        parents: &[LIBRARY, "Saved Application State"],
        last: Last::IdThen(".savedState"),
        kind: Kind::Tree,
        at_start: false,
    },
    Row {
        step: Step::AppFiles,
        parents: &[LIBRARY, APPLICATION_SUPPORT],
        last: Last::InId(OLD_CONSENT_FILE),
        kind: Kind::File,
        at_start: true,
    },
    Row {
        step: Step::AppFiles,
        parents: &[LIBRARY, APPLICATION_SUPPORT],
        last: Last::Id,
        kind: Kind::EmptyFolder,
        at_start: true,
    },
    Row {
        step: Step::AppFiles,
        parents: &[LIBRARY, "Caches"],
        last: Last::Fixed(DATA_FOLDER_NAME),
        kind: Kind::Tree,
        at_start: false,
    },
];

const KEPT: [&[&str]; 6] = [
    &[LIBRARY],
    &[LIBRARY, "Caches"],
    &[LIBRARY, "WebKit"],
    &[LIBRARY, "Saved Application State"],
    &[LIBRARY, APPLICATION_SUPPORT],
    &MACOS_LAUNCH_AGENTS_DIR,
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct DataFolder {
    name: PathBuf,
    resolved: Option<PathBuf>,
    entries: Vec<Place>,
    folder: Option<Place>,
    link: Option<Place>,
}

/// Every place that a removal may take, each made from the home, the bundle id and a fixed last
/// part, or from the home, the default data folder's name and a fixed last part (the cache place
/// of the agents' browser), or from the data folder and a name of halvo-control's table, and each
/// checked by one check before any removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Places {
    login_item: Option<Place>,
    app_files: Option<Vec<Place>>,
    at_start: Vec<Place>,
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
        let outside = |take: &dyn Fn(&Row) -> bool| -> Result<Vec<Place>, Refused> {
            ROWS.iter()
                .filter(|row| take(row))
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
        let login_item = outside(&|row| row.step == Step::LoginItem)?.pop();
        let app_files = outside(&|row| row.step == Step::AppFiles)?;
        let at_start = outside(&|row| row.at_start)?;
        Ok(Places {
            login_item: (!given).then_some(login_item).flatten(),
            app_files: (!given).then_some(app_files),
            at_start,
            data,
        })
    }

    /// Removes the old consent.json of 685d846 and then its folder where that is empty, at a
    /// start (H-09); the file is never read.
    pub fn remove_at_start(&self) {
        for place in &self.at_start {
            if let Err(e) = remove(place) {
                eprintln!("halvo: the old settings file could not be removed: {e}");
            }
        }
    }
}

/// The home of `account`, none where it gives none.
pub fn home_of(account: &dyn AccountLookup) -> Option<PathBuf> {
    account.base_dir().ok().flatten().map(PathBuf::from)
}

/// At a start with the account's own data folder: the places of this app's bundle id and the
/// removal of the old consent.json through them.
pub fn old_consent_at_start(account: &dyn AccountLookup, data_dir: &DataDir) {
    if let Some(home) = home_of(account) {
        if let Ok(places) = Places::new(&home, APP_BUNDLE_ID, data_dir, false) {
            places.remove_at_start();
        }
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
        Last::Fixed(name) if one_name(name) => path.push(name),
        Last::IdThen(_) | Last::InId(_) | Last::Fixed(_) => return Err(Refused::LastPart),
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
    let real = std::fs::canonicalize(path).ok();
    let holds = |data: &&Path| {
        data.starts_with(path) || real.as_deref().is_some_and(|real| data.starts_with(real))
    };
    if held.iter().any(holds) {
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
    let resolved = std::fs::canonicalize(&name)
        .ok()
        .filter(|folder| std::fs::metadata(folder).is_ok_and(|meta| meta.is_dir()));
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
    let folder = resolved.clone().map(|path| Place {
        path,
        kind: Kind::EmptyFolder,
    });
    let link = link.then(|| Place {
        path: name.clone(),
        kind: Kind::Link,
    });
    Ok(DataFolder {
        name,
        resolved,
        entries,
        folder,
        link,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gone {
    Removed,
    Absent,
    Left,
}

fn remove(place: &Place) -> std::io::Result<Gone> {
    let meta = match std::fs::symlink_metadata(&place.path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Gone::Absent),
        Err(e) => return Err(e),
    };
    let folder = meta.is_dir();
    let link = meta.file_type().is_symlink();
    let removed = match place.kind {
        Kind::Tree if folder => std::fs::remove_dir_all(&place.path),
        Kind::EmptyFolder if folder => match std::fs::remove_dir(&place.path) {
            Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => return Ok(Gone::Left),
            other => other,
        },
        Kind::Link if link => std::fs::remove_file(&place.path),
        Kind::File if folder => return Ok(Gone::Left),
        Kind::EmptyFolder | Kind::Link => return Ok(Gone::Left),
        Kind::Tree | Kind::File => std::fs::remove_file(&place.path),
    };
    removed.map(|()| Gone::Removed)
}

fn failed(cause: String) -> (Outcome, Detail) {
    (
        Outcome::Failed,
        Detail {
            cause: Some(cause),
            ..Detail::default()
        },
    )
}

fn own_folder() -> (Outcome, Detail) {
    (
        Outcome::Skipped,
        Detail {
            cause: Some("own_data_folder".to_owned()),
            ..Detail::default()
        },
    )
}

fn counted(count: usize) -> Option<u64> {
    Some(u64::try_from(count).unwrap_or(u64::MAX))
}

fn keychain() -> (Outcome, Detail) {
    let ours = KEYCHAIN_ITEMS
        .iter()
        .filter(|item| item.removed_by == RemovedBy::RemoveHalvo)
        .count();
    if ours == 0 {
        return (
            Outcome::Done,
            Detail {
                count: counted(0),
                ..Detail::default()
            },
        );
    }
    (
        Outcome::Failed,
        Detail {
            count: counted(ours),
            cause: Some("no_removal_in_this_build".to_owned()),
            ..Detail::default()
        },
    )
}

fn login_item(place: Option<&Place>) -> (Outcome, Detail) {
    let Some(place) = place else {
        return own_folder();
    };
    match remove(place) {
        Ok(Gone::Removed) => (
            Outcome::Done,
            Detail {
                count: counted(1),
                ..Detail::default()
            },
        ),
        Ok(Gone::Absent) => (
            Outcome::Done,
            Detail {
                count: counted(0),
                ..Detail::default()
            },
        ),
        Ok(Gone::Left) => failed("not_a_file".to_owned()),
        Err(e) => failed(e.to_string()),
    }
}

fn app_files(places: Option<&[Place]>) -> (Outcome, Detail) {
    let Some(places) = places else {
        return own_folder();
    };
    let mut removed = 0;
    let mut first_error = None;
    for place in places {
        match remove(place) {
            Ok(Gone::Removed) => removed += 1,
            Ok(Gone::Absent | Gone::Left) => {}
            Err(e) => {
                first_error.get_or_insert_with(|| format!("{}: {e}", place.path.display()));
            }
        }
    }
    match first_error {
        Some(cause) => failed(cause),
        None => (
            Outcome::Done,
            Detail {
                count: counted(removed),
                ..Detail::default()
            },
        ),
    }
}

fn unknown_in(folder: &Path) -> usize {
    std::fs::read_dir(folder).map_or(0, |entries| {
        entries
            .filter_map(Result::ok)
            .filter(|entry| {
                !entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| table_names().any(|known| known == name))
            })
            .count()
    })
}

fn data_dir(data: &DataFolder) -> (Outcome, Detail) {
    let mut left = 0;
    for entry in &data.entries {
        if !matches!(remove(entry), Ok(Gone::Removed | Gone::Absent)) {
            left += 1;
        }
    }
    let unknown = data.resolved.as_deref().map_or(0, unknown_in);
    let folder_left = match &data.folder {
        Some(folder) if left == 0 && unknown == 0 => {
            !matches!(remove(folder), Ok(Gone::Removed | Gone::Absent))
        }
        Some(_) => true,
        None => false,
    };
    let link_left = data
        .link
        .as_ref()
        .is_some_and(|link| !matches!(remove(link), Ok(Gone::Removed | Gone::Absent)));
    let no_folder_stands = data.resolved.is_none()
        && data.link.is_none()
        && std::fs::symlink_metadata(&data.name).is_ok();
    let shown = data.resolved.as_deref().unwrap_or(&data.name);
    let named = Some(one_line(&shown.to_string_lossy()));
    if left > 0 || link_left || no_folder_stands || (folder_left && unknown == 0) {
        return (
            Outcome::Failed,
            Detail {
                folder: named,
                count: counted(unknown),
                cause: Some(
                    if no_folder_stands {
                        "not_a_folder"
                    } else {
                        "own_files_left"
                    }
                    .to_owned(),
                ),
            },
        );
    }
    (
        Outcome::Done,
        Detail {
            folder: (unknown > 0).then_some(named).flatten(),
            count: counted(unknown),
            cause: None,
        },
    )
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
/// app's child is ended first, with no request, and every start is held to the end of the list:
/// the child's by `WithoutTheChild`, and from the child's end on, before the account step, the
/// agents' runs by a guard of the list, which also sets the mark of a removal on `hold`, so that
/// no press writes the login item or asks for a child while the list runs. The guard is made
/// before its hold begins, as the delete's own is, and its drop ends the list's delete in the
/// runs' hold, which stands on until the view of a poll made after that end is drawn
/// (`agents::RunsHold`), and takes the mark off: after a list that stopped, and where a step
/// unwinds. After a whole list the list keeps the guard (`keep` marks it kept, and its drop then
/// ends nothing), so the runs stay held and the mark stands to the end of the app, and the
/// child's supervisor ends as at a quit, so that no child starts after it; after a list that
/// stopped, or one that unwound, no child starts by itself until a person's press on Start
/// behind the list or the next launch of the app (`Supervisor::without_the_child`). The account
/// step is `account`; the delete is the offline delete of `offline` after `before`, and it is
/// done only where it deleted and left nothing of the store, or found no store.
pub fn remove_halvo(
    places: Result<Places, Refused>,
    children: Option<&dyn TheChild>,
    hold: &Hold,
    account: &dyn Fn() -> (Outcome, Detail),
    offline: Option<&Offline>,
    before: &Before,
) -> Removal {
    let places = match places {
        Ok(places) => places,
        Err(refused) => {
            eprintln!("halvo: Remove Halvo removes nothing, because its list refused a place: {refused:?}");
            return Removal::default();
        }
    };
    let mut removal = Removal::default();
    let mut run = || {
        let held = HeldForTheList::take(before, hold);
        removal.steps = steps_of(&places, account, offline, before);
        if removal.whole() {
            held.keep();
            if let Some(children) = children {
                children.end();
            }
        }
    };
    match children {
        Some(children) => children.without_the_child(&mut run),
        None => run(),
    }
    removal
}

struct HeldForTheList<'a> {
    before: &'a Before,
    hold: &'a Hold,
    kept: bool,
}

impl<'a> HeldForTheList<'a> {
    fn take(before: &'a Before, hold: &'a Hold) -> Self {
        let held = HeldForTheList {
            before,
            hold,
            kept: false,
        };
        hold.removal(true);
        before.runs.delete_begins(before.core.as_deref());
        held
    }

    fn keep(mut self) {
        self.kept = true;
    }
}

impl Drop for HeldForTheList<'_> {
    fn drop(&mut self) {
        if !self.kept {
            self.hold.removal(false);
            self.before.runs.delete_ends(self.before.core.as_deref());
        }
    }
}

fn steps_of(
    places: &Places,
    account: &dyn Fn() -> (Outcome, Detail),
    offline: Option<&Offline>,
    before: &Before,
) -> Vec<StepResult> {
    let mut steps = Vec::new();
    for step in STEPS {
        let (result, detail) = match step {
            Step::Account => account(),
            Step::DeleteAll => delete_all(offline, before),
            Step::Keychain => keychain(),
            Step::LoginItem => login_item(places.login_item.as_ref()),
            Step::AppFiles => app_files(places.app_files.as_deref()),
            Step::DataDir => data_dir(&places.data),
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
    steps
}

/// What main gives Remove Halvo: the account whose home it takes, whether the app runs with a
/// data folder of its own (`--data-dir`), and the supervisor of the app's child.
#[derive(Clone)]
pub struct Wiring {
    pub account: Arc<dyn AccountLookup + Send + Sync>,
    pub given: bool,
    pub children: Option<Arc<dyn TheChild>>,
    pub views: Arc<Views>,
}

/// Remove Halvo as the app runs it: the places of the app's bundle id under the account's home
/// and of `data_dir`, the account hook of this version, and the offline delete of `offline`.
pub fn run(
    wiring: &Wiring,
    data_dir: Option<&DataDir>,
    offline: Option<&Offline>,
    before: &Before,
) -> Removal {
    let places = match (home_of(wiring.account.as_ref()), data_dir) {
        (None, _) => Err(Refused::NoHome),
        (_, None) => Err(Refused::NoDataFolder),
        (Some(home), Some(dir)) => Places::new(&home, APP_BUNDLE_ID, dir, wiring.given),
    };
    remove_halvo(
        places,
        wiring.children.as_deref(),
        wiring.views.hold(),
        &account_hook,
        offline,
        before,
    )
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
    use std::time::{Duration, Instant};

    use halvo_control::paths::{DarwinUserTempDir, PathError};

    use super::*;
    use halvo_coord::items::ConfirmAnswer;
    use halvo_coord::types::{AgentId, Cause, ItemId, RunId};

    use crate::commands::agents::NoService;
    use crate::commands::capture::consent;
    use crate::commands::control::{ChildPid, Gated, Unresolved};
    use crate::daemon_child::stand_in::{note, Record, StandInChild, StandInClock, StandInSpawner};
    use crate::daemon_child::Supervisor;
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
            &library(
                home,
                &[
                    "Saved Application State",
                    &format!("{ID}.savedState"),
                    "windows.plist",
                ],
            ),
            "state",
        );
        write(
            &library(home, &[APPLICATION_SUPPORT, ID, OLD_CONSENT_FILE]),
            "{}",
        );
        write(
            &library(
                home,
                &[
                    "Caches", "Halvo", "agents", "profiles", "run-1", "profile", "Default",
                    "Cache", "data_0",
                ],
            ),
            "a page that an agent's browser loaded",
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
                &Hold::default(),
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

    fn neighbours(home: &Path) -> Vec<PathBuf> {
        let other = "com.other.app";
        let placed = [
            library(home, &["Caches", other, "cached"]),
            library(home, &["WebKit", other, "WebsiteData"]),
            library(
                home,
                &[
                    "Saved Application State",
                    &format!("{other}.savedState"),
                    "windows.plist",
                ],
            ),
            library(home, &["LaunchAgents", &format!("{other}.plist")]),
            library(home, &[APPLICATION_SUPPORT, "notes of the person.txt"]),
            library(home, &[APPLICATION_SUPPORT, other, "settings.json"]),
            library(home, &[APPLICATION_SUPPORT, ID, "kept by the person.txt"]),
            library(home, &["Caches", "Halvo beside", "kept"]),
        ];
        for path in &placed {
            write(path, "the person's");
        }
        placed.to_vec()
    }

    fn halvos_names(home: &Path, data: &DataDir) -> Vec<PathBuf> {
        let mut names = vec![
            library(home, &["LaunchAgents", &format!("{ID}.plist")]),
            library(home, &["Caches", ID]),
            library(home, &["WebKit", ID]),
            library(
                home,
                &["Saved Application State", &format!("{ID}.savedState")],
            ),
            library(home, &[APPLICATION_SUPPORT, ID, OLD_CONSENT_FILE]),
            library(home, &["Caches", "Halvo"]),
            data.state_file(),
            data.intent_file(),
        ];
        names.extend(ENTRIES.iter().map(|entry| data.entry(entry)));
        names
    }

    fn whole(offline: &Offline, places: Result<Places, Refused>) -> Removal {
        remove_halvo(
            places,
            None,
            &Hold::default(),
            &account_hook,
            Some(offline),
            &before(),
        )
    }

    #[test]
    fn remove_halvo_takes_what_halvo_wrote_and_leaves_what_it_does_not_know() {
        let every_step: Vec<(Step, Outcome)> = STEPS
            .iter()
            .map(|step| {
                let result = if *step == Step::Account {
                    Outcome::Skipped
                } else {
                    Outcome::Done
                };
                (*step, result)
            })
            .collect();
        let delete_of = |data: &DataDir, record: &Record| {
            let looked_in = data.path().display().to_string();
            let end = OfflineEnd::Exited {
                code: 0,
                out: deleted(&looked_in, "yes", "-", 0),
            };
            (
                offline(data, record, end),
                format!("run /stand-in/halvo-capture --delete-all --data-dir {looked_in}"),
            )
        };

        let home = scratch_dir("remove-whole").join("home");
        let data = halvos_data(&home);
        halvos_places(&home);
        let theirs = neighbours(&home);
        let record = Record::default();
        let (stand_in, ran) = delete_of(&data, &record);
        let removal = whole(&stand_in, Places::new(&home, ID, &data, false));
        assert_eq!(
            ended_at(&removal),
            every_step,
            "a whole removal: {removal:?}"
        );
        assert_eq!(removal.exit(), Some(0));
        for name in halvos_names(&home, &data) {
            assert!(
                std::fs::symlink_metadata(&name).is_err(),
                "{} is gone",
                name.display()
            );
        }
        assert!(!data.path().exists(), "the data folder is gone");
        for path in &theirs {
            assert_eq!(
                std::fs::read_to_string(path).ok().as_deref(),
                Some("the person's"),
                "{} stands",
                path.display()
            );
        }
        assert_eq!(
            *record.lock().unwrap(),
            vec![ran],
            "the runner records the delete and no other program"
        );

        let home = scratch_dir("remove-unknown").join("home");
        let data = halvos_data(&home);
        halvos_places(&home);
        let theirs = neighbours(&home);
        let unknown = data.path().join("a file of an unknown name");
        write(&unknown, "the person's");
        let record = Record::default();
        let (stand_in, _) = delete_of(&data, &record);
        let removal = whole(&stand_in, Places::new(&home, ID, &data, false));
        assert_eq!(ended_at(&removal), every_step);
        let last = removal.steps.last().expect("a last step");
        assert_eq!(last.detail.count, Some(1), "the unknown entry is counted");
        assert_eq!(
            last.detail.folder.as_deref(),
            Some(data.path().to_string_lossy().as_ref()),
            "and its folder named"
        );
        assert_eq!(
            tree(data.path()).into_keys().collect::<Vec<_>>(),
            vec![PathBuf::from("a file of an unknown name")],
            "it stands, and the folder stays"
        );
        for path in &theirs {
            assert!(path.exists(), "{} stands", path.display());
        }

        let scratch = scratch_dir("remove-link");
        let home = scratch.join("home");
        let elsewhere = scratch.join("elsewhere").join("Halvo");
        let real = halvos_data(&elsewhere.join("home"));
        let resolved = scratch.join("elsewhere").join("data");
        std::fs::rename(real.path(), &resolved).expect("the data folder moves");
        write(&resolved.join("a file of an unknown name"), "the person's");
        write(&scratch.join("elsewhere").join("beside"), "the person's");
        let data = DataDir::for_account(&Home(home.clone())).expect("a data folder");
        std::fs::create_dir_all(data.path().parent().expect("a parent"))
            .expect("the parent is made");
        std::os::unix::fs::symlink(&resolved, data.path()).expect("the link is made");
        let record = Record::default();
        let (stand_in, _) = delete_of(&data, &record);
        let removal = whole(&stand_in, Places::new(&home, ID, &data, false));
        assert_eq!(ended_at(&removal), every_step);
        assert!(
            std::fs::symlink_metadata(data.path()).is_err(),
            "the link goes as the link that it is"
        );
        assert_eq!(
            tree(&resolved).into_keys().collect::<Vec<_>>(),
            vec![PathBuf::from("a file of an unknown name")],
            "the resolved folder's entries go"
        );
        let named = std::fs::canonicalize(&resolved).expect("the resolved folder stands");
        assert_eq!(
            removal
                .steps
                .last()
                .and_then(|step| step.detail.folder.clone()),
            Some(named.to_string_lossy().into_owned()),
            "the report names the resolved folder"
        );
        assert!(scratch.join("elsewhere").join("beside").exists());

        let home = scratch_dir("remove-no-bundle-id").join("home");
        let data = halvos_data(&home);
        halvos_places(&home);
        neighbours(&home);
        let as_before = tree(&home);
        let record = Record::default();
        let (stand_in, _) = delete_of(&data, &record);
        let removal = whole(&stand_in, Places::new(&home, "", &data, false));
        assert_eq!(
            removal,
            Removal::default(),
            "an empty bundle id runs no step"
        );
        assert_eq!(tree(&home), as_before, "and removes nothing");
        assert!(
            record.lock().unwrap().is_empty(),
            "and the runner records nothing"
        );
    }

    struct HoldingCore(Record);

    impl halvo_coord_app::Core for HoldingCore {
        fn call(&self, command: &str, args: Option<&serde_json::Value>) -> halvo_coord_app::Reply {
            NoService.call(command, args)
        }

        fn shown(&self) {}

        fn hidden(&self) {}

        fn destroyed(&self) {}

        fn tick(&self) {}

        fn quit(&self, budget: Option<Duration>) -> Vec<(AgentId, RunId)> {
            NoService.quit(budget)
        }

        fn hold_runs(&self, held: bool) -> Vec<(AgentId, RunId)> {
            note(
                &self.0,
                if held {
                    "hold the runs"
                } else {
                    "lift the runs"
                },
            );
            Vec::new()
        }

        fn on_screen(&self, _item: &ItemId) {}

        fn answer_native(&self, item: &ItemId, answer: ConfirmAnswer) -> Result<(), Cause> {
            NoService.answer_native(item, answer)
        }
    }

    fn noted(record: &Record) -> Vec<String> {
        record.lock().unwrap().clone()
    }

    #[test]
    fn after_remove_halvo_no_child_starts_and_the_app_ends_with_0() {
        for whole in [true, false] {
            let home = scratch_dir(&format!("remove-child-{whole}")).join("home");
            let data = halvos_data(&home);
            halvos_places(&home);
            let record = Record::default();
            let clock = Arc::new(StandInClock::default());
            let pid = Arc::<ChildPid>::default();
            let spawner = StandInSpawner {
                children: [7, 8]
                    .into_iter()
                    .map(|pid| StandInChild::new(pid, &clock, &record))
                    .collect(),
                record: Arc::clone(&record),
            };
            let views = Arc::<Views>::default();
            let supervisor = Supervisor::start_holding(
                spawner,
                Arc::clone(&pid),
                Arc::clone(&clock),
                Arc::clone(&views),
            );
            let until = Instant::now() + Duration::from_secs(10);
            while pid.get() != Some(7) {
                assert!(Instant::now() < until, "the stand-in child starts");
                std::thread::sleep(Duration::from_millis(5));
            }
            record.lock().unwrap().clear();
            let looked_in = data.path().display().to_string();
            let (root, root_id, left) = if whole {
                ("yes", "-", 0)
            } else {
                ("no", "0123456789abcdef0123456789abcdef", 1)
            };
            let stand_in = offline(
                &data,
                &record,
                OfflineEnd::Exited {
                    code: 0,
                    out: deleted(&looked_in, root, root_id, left),
                },
            );
            let held = Before {
                core: Some(Arc::new(HoldingCore(Arc::clone(&record)))),
                ..before()
            };
            let written = || {
                note(&record, "login item written");
                true
            };
            let pressed_while_it_runs = || {
                let _ = consent(
                    &Gated::new(Unresolved),
                    &views,
                    Some(&written),
                    Duration::ZERO,
                );
                if views.hold().ask_start() {
                    note(&record, "start asked");
                }
                account_hook()
            };
            let removal = remove_halvo(
                Places::new(&home, ID, &data, false),
                Some(&supervisor),
                views.hold(),
                &pressed_while_it_runs,
                Some(&stand_in),
                &held,
            );
            if let Some(code) = removal.exit() {
                note(&record, format!("exit {code}"));
                let clicked = consent(
                    &Gated::new(Unresolved),
                    &views,
                    Some(&written),
                    Duration::ZERO,
                );
                assert!(
                    !noted(&record).contains(&"login item written".to_owned()),
                    "a press on Start while the list runs or after a whole removal writes no login item: {:?}",
                    noted(&record)
                );
                assert!(clicked.is_ok(), "and does nothing: {clicked:?}");
            }
            let ran = format!("run /stand-in/halvo-capture --delete-all --data-dir {looked_in}");
            let mut expected = vec![
                "closed descriptor".to_owned(),
                "kill after 20 s".to_owned(),
                "hold the runs".to_owned(),
                ran,
            ];
            if whole {
                expected.push("exit 0".to_owned());
                std::thread::sleep(Duration::from_millis(300));
                assert_eq!(
                    noted(&record),
                    expected,
                    "the child ended first, no start after it, and exit 0"
                );
            } else {
                use crate::daemon_child::Clock;
                let behind = clock.now();
                let until = Instant::now() + Duration::from_secs(10);
                while clock.now().saturating_duration_since(behind)
                    <= crate::daemon_child::BACKOFF_MAX
                {
                    assert!(
                        Instant::now() < until,
                        "behind a list that stopped the supervisor's clock passes the longest backoff: {:?}",
                        noted(&record)
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
                assert_eq!(
                    noted(&record),
                    expected,
                    "the child ended first, a press while the list ran did nothing, and a list that stopped has no exit, no start by itself and no lift at its end"
                );
                let calls = Arc::<crate::commands::control::Places>::default();
                held.runs.drawn(held.core.as_deref(), &views, &calls);
                views.take_poll(
                    calls.make().number(),
                    &Err(halvo_control::client::ClientError::Unsupported),
                );
                held.runs.drawn(held.core.as_deref(), &views, &calls);
                expected.push("lift the runs".to_owned());
                assert_eq!(
                    noted(&record),
                    expected,
                    "the lift comes at the first poll read behind the list"
                );
                let _ = consent(
                    &Gated::new(Unresolved),
                    &views,
                    Some(&written),
                    Duration::ZERO,
                );
                assert_eq!(
                    noted(&record).last().map(String::as_str),
                    Some("login item written"),
                    "after a list that stopped a click works again"
                );
                assert!(
                    views.hold().ask_start(),
                    "behind a list that stopped a press on Start asks for a child"
                );
                let until = Instant::now() + Duration::from_secs(10);
                while !noted(&record).contains(&"start 8".to_owned()) {
                    assert!(
                        Instant::now() < until,
                        "the press on Start brings one child: {:?}",
                        noted(&record)
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            supervisor.end();
        }
    }

    #[test]
    fn an_unwind_in_a_step_of_remove_halvo_lifts_the_hold_of_the_runs_once() {
        let home = scratch_dir("remove-unwind").join("home");
        let data = halvos_data(&home);
        halvos_places(&home);
        let record = Record::default();
        let views = Arc::<Views>::default();
        let held = Before {
            core: Some(Arc::new(HoldingCore(Arc::clone(&record)))),
            ..before()
        };
        let stand_in = offline(&data, &record, OfflineEnd::NoEnd);
        let unwinds = || -> (Outcome, Detail) { panic!("a step of the list unwinds") };
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            remove_halvo(
                Places::new(&home, ID, &data, false),
                None,
                views.hold(),
                &unwinds,
                Some(&stand_in),
                &held,
            )
        }));
        assert!(unwound.is_err(), "the step unwinds through the list");
        let calls = Arc::<crate::commands::control::Places>::default();
        held.runs.drawn(held.core.as_deref(), &views, &calls);
        views.take_poll(
            calls.make().number(),
            &Err(halvo_control::client::ClientError::Unsupported),
        );
        held.runs.drawn(held.core.as_deref(), &views, &calls);
        assert_eq!(
            noted(&record),
            vec!["hold the runs".to_owned(), "lift the runs".to_owned()],
            "the core was told to hold once and to lift once, at the latest at the first poll read behind the unwind"
        );
        assert!(
            views.hold().begin_consent().is_some(),
            "and after the unwind a consent click begins again"
        );
    }
}
