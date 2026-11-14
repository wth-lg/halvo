mod common;

use std::collections::HashMap;
use std::ffi::{c_char, c_int, CStr};
use std::sync::{Mutex, OnceLock};

use common::{
    episode, open_entries, own_note_body, position, raw_request, store_id, Grouping, OwnNote,
    Scratch, Seq, TestClock, OWN,
};
use halvo_derived::{Error, Handler, HandlerCx, Host, MoveTo, Store, Takes};
use halvo_schema_core::ids::{AgentId, RequestId, WorkflowId};
use halvo_schema_core::record::Workflow;
use halvo_schema_core::request::{ErrorName, InterfaceDecision, InterfaceRole, RawRequest};
use halvo_sqlcipher::rusqlite::ffi;
use serde_json::value::RawValue;

const DAY: u32 = 20_767;

/// The flag of a sync call that asks the system to flush the drive: `F_FULLFSYNC` on macOS.
const SYNC_FULL: c_int = 0x03;

/// The methods of the system's layer of each open file of the counting layer, by the address of the file, with the
/// path that the file was opened under.
fn opened() -> &'static Mutex<HashMap<usize, (usize, String)>> {
    static OPENED: OnceLock<Mutex<HashMap<usize, (usize, String)>>> = OnceLock::new();
    OPENED.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The sync calls that each path took, as (calls with the full flag, other calls).
fn syncs() -> &'static Mutex<HashMap<String, (u64, u64)>> {
    static SYNCS: OnceLock<Mutex<HashMap<String, (u64, u64)>>> = OnceLock::new();
    SYNCS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The methods of the counting layer, by the address of the system's methods that they wrap.
fn tables() -> &'static Mutex<HashMap<usize, usize>> {
    static TABLES: OnceLock<Mutex<HashMap<usize, usize>>> = OnceLock::new();
    TABLES.get_or_init(|| Mutex::new(HashMap::new()))
}

static SYSTEM: OnceLock<usize> = OnceLock::new();

unsafe extern "C" fn count_sync(file: *mut ffi::sqlite3_file, flags: c_int) -> c_int {
    let (methods, path) = opened().lock().unwrap()[&(file as usize)].clone();
    {
        let mut syncs = syncs().lock().unwrap();
        let entry = syncs.entry(path).or_insert((0, 0));
        if flags & 0x0f == SYNC_FULL {
            entry.0 += 1;
        } else {
            entry.1 += 1;
        }
    }
    let methods = methods as *const ffi::sqlite3_io_methods;
    ((*methods).xSync.unwrap())(file, flags)
}

unsafe extern "C" fn count_close(file: *mut ffi::sqlite3_file) -> c_int {
    let (methods, _) = opened().lock().unwrap().remove(&(file as usize)).unwrap();
    let methods = methods as *const ffi::sqlite3_io_methods;
    (*file).pMethods = methods;
    ((*methods).xClose.unwrap())(file)
}

unsafe extern "C" fn count_open(
    _vfs: *mut ffi::sqlite3_vfs,
    name: *const c_char,
    file: *mut ffi::sqlite3_file,
    flags: c_int,
    out: *mut c_int,
) -> c_int {
    let system = *SYSTEM.get().unwrap() as *mut ffi::sqlite3_vfs;
    let rc = ((*system).xOpen.unwrap())(system, name, file, flags, out);
    if rc != ffi::SQLITE_OK || (*file).pMethods.is_null() {
        return rc;
    }
    let methods = (*file).pMethods;
    let table = *tables()
        .lock()
        .unwrap()
        .entry(methods as usize)
        .or_insert_with(|| {
            let mut table = *methods;
            table.xSync = Some(count_sync);
            table.xClose = Some(count_close);
            Box::leak(Box::new(table)) as *const ffi::sqlite3_io_methods as usize
        });
    let path = if name.is_null() {
        String::new()
    } else {
        CStr::from_ptr(name).to_string_lossy().into_owned()
    };
    opened()
        .lock()
        .unwrap()
        .insert(file as usize, (methods as usize, path));
    (*file).pMethods = table as *const ffi::sqlite3_io_methods;
    rc
}

/// Makes a layer over the system's file layer of the database the default of the process, once: it passes every
/// call through and counts the sync calls of each file by its path and by whether they carry the full flag.
fn install() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        halvo_derived::test_host::start_sqlite();
        unsafe {
            let system = ffi::sqlite3_vfs_find(std::ptr::null());
            assert!(!system.is_null());
            SYSTEM.set(system as usize).unwrap();
            let mut layer = *system;
            layer.zName = c"halvo_sync_count".as_ptr();
            layer.pNext = std::ptr::null_mut();
            layer.xOpen = Some(count_open);
            let layer = Box::leak(Box::new(layer));
            assert_eq!(ffi::sqlite3_vfs_register(layer, 1), ffi::SQLITE_OK);
        }
    });
}

/// The sync calls with the full flag that the database file and the log of the store under `scratch` took so far,
/// and every sync call of the log.
fn full_syncs(scratch: &Scratch) -> (u64, u64, u64) {
    let leaf = scratch
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let syncs = syncs().lock().unwrap();
    let of = |end: &str| -> (u64, u64) {
        syncs
            .iter()
            .filter(|(path, _)| path.contains(&leaf) && path.ends_with(end))
            .fold((0, 0), |(f, a), (_, (full, other))| {
                (f + full, a + full + other)
            })
    };
    let ((db, _), (wal, any)) = (of("derived.db"), of("derived.db-wal"));
    (db, wal, any)
}

/// A handler that makes the state call that its body names, with the agent `4...4`.
struct SetActive;

impl Handler for SetActive {
    fn handle(
        &self,
        request: &RawRequest,
        cx: &mut HandlerCx<'_>,
    ) -> Result<Box<RawValue>, ErrorName> {
        let body: serde_json::Value = serde_json::from_str(request.body().get()).unwrap();
        let workflow = WorkflowId::parse(body["workflow"].as_str().unwrap()).unwrap();
        let agent = AgentId::parse(&"4".repeat(32)).unwrap();
        match cx.state(&workflow, MoveTo::Active, Some(agent)) {
            Ok(()) => Ok(RawValue::from_string("null".to_owned()).unwrap()),
            Err(Error::Refused(name)) => Err(name),
            Err(Error::Fault(_)) => Err(ErrorName::Internal),
        }
    }
}

fn host(clock: &TestClock) -> Host {
    Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 1 }))
        .handler("test", Takes::Runner, Box::new(OwnNote))
        .handler(
            "test",
            Takes::Coordinator(&["set_active"]),
            Box::new(SetActive),
        )
}

fn decision(n: u8, kind: &str, workflow: &WorkflowId) -> InterfaceDecision {
    let role = InterfaceRole::claim();
    let id = RequestId::from_random([n; 16]);
    match kind {
        "approve" => InterfaceDecision::approve(&role, id, workflow.clone()),
        "dismiss" => InterfaceDecision::dismiss(&role, id, workflow.clone()),
        _ => InterfaceDecision::delete(&role, id, workflow.clone()),
    }
}

/// C25.5 and the coordinator's ruling S-78, item 3 (U5.1d, part 6): each answered commit asks the system to flush the
/// drive. A handler's typed commit, each decision (approve, dismiss and delete), the state call and a checkpointed call
/// (the expiry call) each make one or more sync calls of the log with the full flag, and each call that ends with the
/// truncating checkpoint (the dismissal, the delete and the expiry call) one or more of the database file with it,
/// counted through a layer under the database that counts the sync calls of each file and passes them on.
#[test]
fn each_answered_commit_and_each_checkpoint_syncs_with_the_full_flag() {
    install();
    let scratch = Scratch::new("sync-full");
    let clock = TestClock::at_day(DAY);
    let mut store = open_entries(&scratch, host(&clock), &[&OWN]).unwrap();
    let mut other = episode(2, DAY, None);
    other.app = halvo_schema_core::site::AppId::parse("com.example.other").unwrap();
    store
        .commit_batch(
            &store_id(1),
            &[
                episode(1, DAY, None),
                other,
                episode(3, DAY, common::domain("northwind-power.test")),
            ],
            position(1, 0),
            &mut Seq(100),
        )
        .unwrap();
    let ids: Vec<WorkflowId> = common::listed(&store)
        .iter()
        .map(|item| item.workflow.id().clone())
        .collect();
    assert_eq!(ids.len(), 3);
    let mut seen = Vec::new();
    let mut step = |name: &str, store: &mut Store, act: &dyn Fn(&mut Store)| {
        let (db, wal, any) = full_syncs(&scratch);
        act(store);
        let (db_after, wal_after, any_after) = full_syncs(&scratch);
        assert!(
            any_after > any,
            "{name}: the layer sees a sync call of the log"
        );
        println!(
            "{name}: full syncs of the log {}, of the database file {}",
            wal_after - wal,
            db_after - db
        );
        seen.push((name.to_owned(), wal_after > wal, db_after > db));
    };
    step("approve", &mut store, &|s| {
        s.decide(&decision(1, "approve", &ids[0])).unwrap();
    });
    step("state", &mut store, &|s| {
        let body = format!(r#"{{"op":"set_active","workflow":"{}"}}"#, ids[0].as_str());
        s.handle(&raw_request("coordinator_write", &body)).unwrap();
    });
    step("typed commit", &mut store, &|s| {
        let body = own_note_body(ids[0].as_str(), "Call the landlord");
        s.handle(&raw_request("runner", &body)).unwrap();
    });
    step("dismiss", &mut store, &|s| {
        s.decide(&decision(2, "dismiss", &ids[1])).unwrap();
    });
    step("delete", &mut store, &|s| {
        s.decide(&decision(3, "delete", &ids[2])).unwrap();
    });
    clock.set_day(DAY + 93);
    step("expiry call", &mut store, &|s| {
        assert!(s.expire().unwrap() > 0);
    });
    drop(store);
    let stored: Vec<Workflow> = common::bodies(&scratch, "workflow")
        .iter()
        .map(|b| serde_json::from_str(b).unwrap())
        .collect();
    assert_eq!(stored.len(), 2, "the delete took its workflow");
    let want: Vec<(String, bool, bool)> = [
        ("approve", false),
        ("state", false),
        ("typed commit", false),
        ("dismiss", true),
        ("delete", true),
        ("expiry call", true),
    ]
    .iter()
    .map(|(name, checkpointed)| ((*name).to_owned(), true, *checkpointed))
    .collect();
    assert_eq!(
        seen, want,
        "(call, a sync of the log with the full flag, a sync of the database file with the full flag)"
    );
}
