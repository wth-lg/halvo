//! Proof lines of the floor's boundaries, of the daemon's port and of the
//! store's boundary, built only by
//! scripts/r1/ci.d/30-core-boundary-proofs.sh. Each proof sits under its own
//! `halvo_proof` cfg and must fail to build with the error code that the script
//! names; its twin, under the same name with `_twin`, must build. This module is
//! outside every guarded module, as a source file of the crate is. Its one test
//! is the test of the two debug doors, which the check lets stand here alone
//! beside daemon/prefill.rs.

#![allow(unexpected_cfgs, dead_code)]

#[cfg(halvo_proof = "clearance_made_outside_secure_input")]
fn clearance_made_outside_secure_input() -> crate::secure_input::KeystrokeClearance {
    crate::secure_input::KeystrokeClearance::issue(None)
}

#[cfg(halvo_proof = "clearance_made_outside_secure_input_twin")]
fn clearance_made_outside_secure_input_twin(
    probe: &dyn crate::secure_input::SecureInputProbe,
) -> Option<crate::secure_input::KeystrokeClearance> {
    crate::secure_input::check(probe, None).ok()
}

#[cfg(halvo_proof = "probe_implemented_outside_secure_input")]
mod probe_implemented_outside_secure_input {
    use crate::secure_input::{SecureInputProbe, SecureInputReading};
    struct Elsewhere;
    impl crate::secure_input::seal::Sealed for Elsewhere {}
    impl SecureInputProbe for Elsewhere {
        fn read(&self) -> SecureInputReading {
            SecureInputReading::Inactive
        }
    }
}

#[cfg(halvo_proof = "probe_implemented_outside_secure_input_twin")]
mod probe_implemented_outside_secure_input_twin {
    use crate::secure_input::SecureInputReading;
    struct Elsewhere;
    impl Elsewhere {
        fn read(&self) -> SecureInputReading {
            SecureInputReading::Inactive
        }
    }
}

#[cfg(halvo_proof = "record_from_an_old_path_event")]
fn record_from_an_old_path_event<P: super::text::TextFilterPoint>(
    event: &crate::event::CaptureStreamEvent,
    point: &P,
) {
    let _ = super::record::encode(event, point);
}

#[cfg(halvo_proof = "record_from_an_old_path_event_twin")]
fn record_from_an_old_path_event_twin<P: super::text::TextFilterPoint>(
    received: &crate::sink::FloorReceived,
    point: &P,
) {
    let _ = super::record::encode(received, point);
}

#[cfg(halvo_proof = "received_value_made_outside_the_sink")]
fn received_value_made_outside_the_sink(
    event: crate::event::FloorEvent,
    number: crate::sink::ArrivalNumber,
) -> crate::sink::FloorReceived {
    crate::sink::FloorReceived { event, number }
}

#[cfg(halvo_proof = "received_value_made_outside_the_sink_twin")]
fn received_value_made_outside_the_sink_twin(
    received: crate::sink::Received,
) -> Option<crate::sink::FloorReceived> {
    received.into_floor().ok()
}

#[cfg(halvo_proof = "emitted_value_without_the_keep_verdict")]
fn emitted_value_without_the_keep_verdict(
    subject: super::exclude::Subject,
) -> crate::event::FloorEvent {
    let keep = super::exclude::Keep::<super::kind::ForegroundApp>::issue(subject, false);
    crate::event::FloorEvent::foreground_app(keep, None, crate::event::EventClock::system())
}

#[cfg(halvo_proof = "emitted_value_without_the_keep_verdict_twin")]
fn emitted_value_without_the_keep_verdict_twin(
    floor: &super::exclude::FloorHandle,
    subject: super::exclude::Subject,
    probe: &dyn crate::secure_input::SecureInputProbe,
) -> Option<crate::event::FloorEvent> {
    use super::exclude::{PrivateWindow, Verdict};
    match floor.decide::<super::kind::ForegroundApp>(subject, PrivateWindow::Unknown, probe) {
        Verdict::Keep(keep) => Some(crate::event::FloorEvent::foreground_app(
            keep,
            None,
            crate::event::EventClock::system(),
        )),
        Verdict::Drop(_) => None,
    }
}

#[cfg(halvo_proof = "keep_verdict_of_another_kind")]
fn keep_verdict_of_another_kind(
    keep: super::exclude::Keep<super::kind::Pointer>,
) -> crate::event::FloorEvent {
    crate::event::FloorEvent::screen_frame(keep, None, crate::event::EventClock::system())
}

#[cfg(halvo_proof = "keep_verdict_of_another_kind_twin")]
fn keep_verdict_of_another_kind_twin(
    keep: super::exclude::Keep<super::kind::ScreenFrame>,
) -> crate::event::FloorEvent {
    crate::event::FloorEvent::screen_frame(keep, None, crate::event::EventClock::system())
}

#[cfg(halvo_proof = "decoded_record_emitted_into_the_sink")]
fn decoded_record_emitted_into_the_sink(
    sink: &crate::sink::SinkHandle,
    record: super::record::DecodedRecord,
) {
    let _ = sink.emit_event(record);
}

#[cfg(halvo_proof = "decoded_record_emitted_into_the_sink_twin")]
fn decoded_record_emitted_into_the_sink_twin(
    sink: &crate::sink::SinkHandle,
    event: crate::event::FloorEvent,
) {
    let _ = sink.emit_event(event);
}

#[cfg(halvo_proof = "fold_record_through_the_blocking_emit")]
fn fold_record_through_the_blocking_emit(
    sink: &crate::sink::SinkHandle,
    fold: crate::event::FoldEvent,
) {
    let _ = sink.emit_event(fold);
}

#[cfg(halvo_proof = "fold_record_through_the_blocking_emit_twin")]
fn fold_record_through_the_blocking_emit_twin(
    sink: &crate::sink::SinkHandle,
    fold: crate::event::FoldEvent,
) {
    let _ = sink.emit_continuous(fold);
}

#[cfg(halvo_proof = "key_event_with_a_character")]
fn key_event_with_a_character(
    keep: super::exclude::Keep<super::kind::Keystroke>,
    clearance: crate::secure_input::KeystrokeClearance,
) -> crate::event::FloorEvent {
    let clock = crate::event::EventClock::system();
    crate::event::FloorEvent::keystroke(keep, clearance, 'a', clock)
}

#[cfg(halvo_proof = "key_event_with_a_character_twin")]
fn key_event_with_a_character_twin(
    keep: super::exclude::Keep<super::kind::Keystroke>,
    clearance: crate::secure_input::KeystrokeClearance,
) -> crate::event::FloorEvent {
    let clock = crate::event::EventClock::system();
    let class = super::payload::KeyClass::Printable;
    crate::event::FloorEvent::keystroke(keep, clearance, class, clock)
}

#[cfg(halvo_proof = "pointer_event_with_a_coordinate")]
fn pointer_event_with_a_coordinate(
    keep: super::exclude::Keep<super::kind::Pointer>,
) -> crate::event::FloorEvent {
    let clock = crate::event::EventClock::system();
    let button = super::payload::Button::Primary;
    crate::event::FloorEvent::click(keep, button, (640.0, 480.0), None, clock)
}

#[cfg(halvo_proof = "pointer_event_with_a_coordinate_twin")]
fn pointer_event_with_a_coordinate_twin(
    keep: super::exclude::Keep<super::kind::Pointer>,
) -> crate::event::FloorEvent {
    let clock = crate::event::EventClock::system();
    let button = super::payload::Button::Primary;
    crate::event::FloorEvent::click(keep, button, None, clock)
}

#[cfg(halvo_proof = "fold_count_from_an_integer")]
fn fold_count_from_an_integer() -> super::payload::FoldCount {
    super::payload::FoldCount::from(12_u64)
}

#[cfg(halvo_proof = "fold_count_from_an_integer_twin")]
fn fold_count_from_an_integer_twin() -> super::payload::FoldCount {
    let mut count = super::payload::FoldCount::one();
    count.add_one();
    count
}

#[cfg(halvo_proof = "frame_event_with_bytes")]
fn frame_event_with_bytes(
    keep: super::exclude::Keep<super::kind::ScreenFrame>,
    pixels: Vec<u8>,
) -> crate::event::FloorEvent {
    let clock = crate::event::EventClock::system();
    crate::event::FloorEvent::screen_frame(keep, pixels, None, clock)
}

#[cfg(halvo_proof = "frame_event_with_bytes_twin")]
fn frame_event_with_bytes_twin(
    keep: super::exclude::Keep<super::kind::ScreenFrame>,
) -> crate::event::FloorEvent {
    let clock = crate::event::EventClock::system();
    crate::event::FloorEvent::screen_frame(keep, None, clock)
}

#[cfg(halvo_proof = "camera_event")]
fn camera_event(
    floor: &super::exclude::FloorHandle,
    subject: super::exclude::Subject,
    probe: &dyn crate::secure_input::SecureInputProbe,
) {
    let unknown = super::exclude::PrivateWindow::Unknown;
    let _ = floor.decide::<super::kind::Camera>(subject, unknown, probe);
}

#[cfg(halvo_proof = "camera_event_twin")]
fn camera_event_twin(
    floor: &super::exclude::FloorHandle,
    subject: super::exclude::Subject,
    probe: &dyn crate::secure_input::SecureInputProbe,
) {
    let unknown = super::exclude::PrivateWindow::Unknown;
    let _ = floor.decide::<super::kind::ScreenFrame>(subject, unknown, probe);
}

#[cfg(halvo_proof = "captured_text_read_outside_floor")]
fn captured_text_read_outside_floor(text: &super::text::CapturedText) -> &str {
    &text.bytes
}

#[cfg(halvo_proof = "captured_text_read_outside_floor_twin")]
fn captured_text_read_outside_floor_twin<P: super::text::TextFilterPoint>(
    received: &crate::sink::FloorReceived,
    point: &P,
) -> bool {
    super::record::encode(received, point).is_ok()
}

#[cfg(halvo_proof = "captured_text_from_a_string")]
fn captured_text_from_a_string() -> super::text::CapturedText {
    super::text::CapturedText::from(String::from("Inbox"))
}

#[cfg(halvo_proof = "captured_text_from_a_string_twin")]
fn captured_text_from_a_string_twin() -> super::text::CapturedText {
    super::text::CapturedText::enter(super::text::TextField::Title, String::from("Inbox"))
}

#[cfg(halvo_proof = "app_identity_from_a_string")]
fn app_identity_from_a_string() -> crate::event::AppIdentity {
    crate::event::AppIdentity::from(String::from("com.apple.Terminal"))
}

#[cfg(halvo_proof = "app_identity_from_a_string_twin")]
fn app_identity_from_a_string_twin() -> crate::event::AppIdentity {
    crate::event::AppIdentity::bundle_id(String::from("com.apple.Terminal"))
}

#[cfg(halvo_proof = "site_from_a_string")]
fn site_from_a_string() -> super::site::Site {
    super::site::Site::from(String::from("example.com"))
}

#[cfg(halvo_proof = "site_from_a_string_twin")]
fn site_from_a_string_twin() -> Option<super::site::Site> {
    super::site::Site::cut(String::from("https://www.example.com/"))
}

#[cfg(halvo_proof = "event_id_made_outside_its_maker")]
fn event_id_made_outside_its_maker() -> crate::event::EventId {
    crate::event::EventId(uuid::Uuid::nil())
}

#[cfg(halvo_proof = "event_id_made_outside_its_maker_twin")]
fn event_id_made_outside_its_maker_twin(event: &crate::event::FloorEvent) -> crate::event::EventId {
    event.id()
}

#[cfg(halvo_proof = "segment_write_filled_outside_the_daemon")]
fn segment_write_filled_outside_the_daemon(
    segment_id: &crate::daemon::port::SegmentId,
    day: halvo_control::model::UtcDay,
) {
    let _ = crate::daemon::port::SegmentWrite {
        segment_id,
        day,
        first_capture: 0,
        last_capture: 0,
        entries: &[],
        payload: b"{}",
    };
}

#[cfg(halvo_proof = "segment_write_filled_outside_the_daemon_twin")]
fn segment_write_filled_outside_the_daemon_twin(
    write: &crate::daemon::port::SegmentWrite<'_>,
) -> usize {
    write.payload().len()
}

#[cfg(halvo_proof = "batch_filled_outside_the_daemon")]
fn batch_filled_outside_the_daemon(batch: &mut crate::daemon::batch::Batch) {
    batch.push(
        crate::daemon::port::EntryKind::of(crate::event::Modality::Keystroke),
        0,
        b"{}",
        std::time::Duration::ZERO,
    );
}

#[cfg(halvo_proof = "batch_filled_outside_the_daemon_twin")]
fn batch_filled_outside_the_daemon_twin(batch: &crate::daemon::batch::Batch) -> usize {
    batch.events()
}

#[cfg(halvo_proof = "store_database_named_by_its_path")]
fn store_database_named_by_its_path() -> &'static str {
    std::any::type_name::<crate::store::db::Db>()
}

#[cfg(halvo_proof = "store_database_named_by_its_path_twin")]
fn store_database_named_by_its_path_twin() -> &'static str {
    std::any::type_name::<crate::store::ShredReport>()
}

#[cfg(halvo_proof = "store_key_file_named_by_its_path")]
fn store_key_file_named_by_its_path() -> &'static str {
    std::any::type_name::<crate::store::db::KeyFile>()
}

#[cfg(halvo_proof = "store_key_file_named_by_its_path_twin")]
fn store_key_file_named_by_its_path_twin() -> &'static str {
    std::any::type_name::<crate::store::db::EncryptedStore>()
}

#[cfg(halvo_proof = "store_segment_files_named_by_their_module")]
mod store_segment_files_named_by_their_module {
    pub(crate) use crate::store::segment as files;
}

#[cfg(halvo_proof = "store_segment_files_named_by_their_module_twin")]
mod store_segment_files_named_by_their_module_twin {
    pub(crate) use crate::store::db as files;
}

#[cfg(halvo_proof = "store_database_named_after_a_glob_of_the_crate_root")]
#[rustfmt::skip]
mod store_database_named_after_a_glob_of_the_crate_root {
    use super::super::super::*;
    use self::store as backing;

    fn name() -> &'static str {
        std::any::type_name::<backing::db::Db>()
    }
}

#[cfg(halvo_proof = "store_database_named_after_a_glob_of_the_crate_root_twin")]
#[rustfmt::skip]
mod store_database_named_after_a_glob_of_the_crate_root_twin {
    use super::super::super::*;
    use self::store as backing;

    fn name() -> &'static str {
        std::any::type_name::<backing::ShredReport>()
    }
}

#[cfg(halvo_proof = "store_database_named_through_a_glob_of_the_store")]
#[rustfmt::skip]
mod store_database_named_through_a_glob_of_the_store {
    use crate::{{*}};
    use self::store::*;

    fn name() -> &'static str {
        std::any::type_name::<db::Db>()
    }
}

#[cfg(halvo_proof = "store_database_named_through_a_glob_of_the_store_twin")]
#[rustfmt::skip]
mod store_database_named_through_a_glob_of_the_store_twin {
    use crate::{{*}};
    use self::store::*;

    fn name() -> &'static str {
        std::any::type_name::<ShredReport>()
    }
}

#[cfg(halvo_proof = "store_database_named_by_a_macro_of_path_segments")]
mod store_database_named_by_a_macro_of_path_segments {
    macro_rules! joined {
        ($($seg:tt),+) => { $($seg)::+ };
    }

    type Handle = joined!(crate, store, db, Db);

    fn name() -> &'static str {
        std::any::type_name::<Handle>()
    }
}

#[cfg(halvo_proof = "store_database_named_by_a_macro_of_path_segments_twin")]
mod store_database_named_by_a_macro_of_path_segments_twin {
    macro_rules! joined {
        ($($seg:tt),+) => { $($seg)::+ };
    }

    type Handle = joined!(crate, store, ShredReport);

    fn name() -> &'static str {
        std::any::type_name::<Handle>()
    }
}

#[cfg(halvo_proof = "store_database_named_by_an_alias_in_a_brace_list")]
#[rustfmt::skip]
mod store_database_named_by_an_alias_in_a_brace_list {
    use crate::/* x */*;
    use {sink, store as backing};

    fn name() -> &'static str {
        let _ = sink::channel;
        std::any::type_name::<backing::db::Db>()
    }
}

#[cfg(halvo_proof = "store_database_named_by_an_alias_in_a_brace_list_twin")]
#[rustfmt::skip]
mod store_database_named_by_an_alias_in_a_brace_list_twin {
    use crate::/* x */*;
    use {sink, store as backing};

    fn name() -> &'static str {
        let _ = sink::channel;
        std::any::type_name::<backing::ShredReport>()
    }
}

#[cfg(halvo_proof = "store_database_re_exported_under_a_new_name")]
mod store_database_re_exported_under_a_new_name {
    pub(crate) use crate::store::db::Db as Backing;
}

#[cfg(halvo_proof = "store_database_re_exported_under_a_new_name_twin")]
mod store_database_re_exported_under_a_new_name_twin {
    pub(crate) use crate::store::ShredReport as Backing;
}

#[cfg(halvo_proof = "synthetic_clearance_in_a_release_build")]
fn synthetic_clearance_in_a_release_build() -> crate::secure_input::KeystrokeClearance {
    crate::secure_input::synthetic_clearance(None)
}

#[cfg(halvo_proof = "synthetic_clearance_in_a_release_build_twin")]
fn synthetic_clearance_in_a_release_build_twin() -> crate::secure_input::KeystrokeClearance {
    crate::secure_input::synthetic_clearance(None)
}

#[cfg(halvo_proof = "synthetic_keep_in_a_release_build")]
fn synthetic_keep_in_a_release_build(
    subject: super::exclude::Subject,
) -> super::exclude::Keep<super::kind::Keystroke> {
    super::exclude::synthetic_keep(subject)
}

#[cfg(halvo_proof = "synthetic_keep_in_a_release_build_twin")]
fn synthetic_keep_in_a_release_build_twin(
    subject: super::exclude::Subject,
) -> super::exclude::Keep<super::kind::Keystroke> {
    super::exclude::synthetic_keep(subject)
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::super::exclude::{synthetic_keep, ProcessId, Subject};
    use super::super::kind;
    use crate::event::AppIdentity;
    use crate::secure_input::synthetic_clearance;
    use std::time::Duration;

    #[test]
    fn the_debug_doors_give_a_site_and_the_age_that_they_are_given() {
        let subject = Subject::new(
            AppIdentity::bundle_id("com.apple.Safari".to_string()),
            ProcessId::new(7),
        );
        let (_, site_read) = synthetic_keep::<kind::WindowTitle>(subject).into_parts();
        assert!(site_read, "the synthetic keep reads the site");
        let age = Some(Duration::from_millis(1_500));
        assert_eq!(synthetic_clearance(age).key_age(), age);
        assert_eq!(synthetic_clearance(None).key_age(), None);
    }
}
