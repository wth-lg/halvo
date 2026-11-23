//! U5.4w, sentence 1: with a data directory the app's Core is the hub over the product's ports, on
//! Windows as on a Mac.

use std::sync::{Arc, Mutex, PoisonError};

use halvo_control::client::{ClientError, DeleteOutcome};
use halvo_control::model::{StorageCap, StoreId};
use halvo_control::onboarding::{Permission, RequestOutcome};
use halvo_control::paths::DataDir;
use halvo_control::proto::{CapSet, DaemonStatus, InventoryReply};
use halvo_control::r2::{OwnerStreamEnd, RunEdge, RunMarked};
use halvo_coord::words;
use halvo_coord_app::Reply;

use super::scratch_dir;
use crate::commands::agents;
use crate::commands::control::{Control, Made, Places};
use crate::view_state::Hold;

/// A control that keeps the name of each call and refuses every one, as when no service listens.
#[derive(Default)]
struct Refusing {
    places: Arc<Places>,
    calls: Mutex<Vec<&'static str>>,
}

impl Refusing {
    fn refuse<T>(&self, call: &'static str) -> Result<T, ClientError> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(call);
        Err(ClientError::Unreachable {
            endpoint: "refusing".to_owned(),
        })
    }

    fn calls(&self) -> Vec<&'static str> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Control for Refusing {
    fn places(&self) -> &Arc<Places> {
        &self.places
    }

    fn status(&self, _: Made, _: bool) -> Result<DaemonStatus, ClientError> {
        self.refuse("status")
    }

    fn status_of_a_pass(&self, _: Made, _: bool, _: &Hold) -> Result<DaemonStatus, ClientError> {
        self.refuse("status_of_a_pass")
    }

    fn inventory(&self, _: Made) -> Result<InventoryReply, ClientError> {
        self.refuse("inventory")
    }

    fn pause(&self, _: Made) -> Result<DaemonStatus, ClientError> {
        self.refuse("pause")
    }

    fn resume(&self, _: Made) -> Result<DaemonStatus, ClientError> {
        self.refuse("resume")
    }

    fn delete_all(&self, _: Made, _: Option<StoreId>) -> Result<DeleteOutcome, ClientError> {
        self.refuse("delete_all")
    }

    fn set_storage_cap(&self, _: Made, _: StorageCap) -> Result<CapSet, ClientError> {
        self.refuse("set_storage_cap")
    }

    fn request_permission(&self, _: Made, _: Permission) -> Result<RequestOutcome, ClientError> {
        self.refuse("request_permission")
    }

    fn relay(&self, _: Made, _: &[u8]) -> Result<Vec<u8>, ClientError> {
        self.refuse("relay")
    }

    fn acknowledge_lost_permission(
        &self,
        _: Made,
        _: Permission,
    ) -> Result<DaemonStatus, ClientError> {
        self.refuse("acknowledge_lost_permission")
    }

    fn reset_settings(&self, _: Made) -> Result<DaemonStatus, ClientError> {
        self.refuse("reset_settings")
    }

    fn set_typing(&self, _: Made, _: bool) -> Result<DaemonStatus, ClientError> {
        self.refuse("set_typing")
    }

    fn owner_stream(&self, _: Made) -> Result<OwnerStreamEnd, ClientError> {
        self.refuse("owner_stream")
    }

    fn run_mark(&self, _: Made, _: RunEdge) -> Result<RunMarked, ClientError> {
        self.refuse("run_mark")
    }

    fn defer(&self, _: Made, _: i64) -> Result<Option<i64>, ClientError> {
        self.refuse("defer")
    }
}

/// With a data directory of the test's own, the app's Core is the hub over the product's ports, on
/// Windows as on a Mac: after the start's steps the control has relayed a request, which
/// `NoService` never sends; with every call refused the page reads no row and the note of
/// `note.no_service`; and the runner is the file halvo-runner with the system's suffix. The hub's
/// one thread of its own ends once the test drops the Core, its last owner.
#[test]
fn with_a_data_directory_the_core_is_the_hub_over_the_products_ports() {
    let app = tauri::test::mock_app();
    let dir = DataDir::from_flag(scratch_dir("u54w-hub")).expect("a data directory");
    let refusing = Arc::new(Refusing::default());
    let control: Arc<dyn Control> = refusing.clone();
    let (core, start) = agents::core(app.handle(), Box::new(|_| {}), &control, Some(&dir));
    start();
    assert!(
        refusing.calls().contains(&"relay"),
        "the start's steps relayed nothing; the calls: {:?}",
        refusing.calls()
    );
    match core.0.call("agents_snapshot", None) {
        Reply::Snapshot(snapshot) => {
            assert!(snapshot.rows.is_empty(), "rows: {:?}", snapshot.rows);
            assert_eq!(snapshot.note, words::text("note.no_service", &[]).ok());
        }
        other => panic!("agents_snapshot answered {other:?}"),
    }
    let runner = agents::runner(&dir);
    assert_eq!(
        runner.file_name().and_then(|name| name.to_str()),
        Some(if cfg!(windows) {
            "halvo-runner.exe"
        } else {
            "halvo-runner"
        })
    );
    drop(core);
}
