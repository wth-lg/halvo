//! The mask of stored text, which no feature switches off: a window title is
//! stored as `halvo_redact::mask` gives it, and the app id and the site's
//! domain are stored as captured.

use halvo_redact::mask;

use crate::floor::text::{TextField, TEXT_MAX_BYTES};

/// The longest title that the filter masks. The floor's text entry cuts a
/// longer title to [`TEXT_MAX_BYTES`] bytes on a character boundary, which
/// keeps at least `TEXT_MAX_BYTES - 3` bytes, so a title of at most this
/// length is whole. The filter is not told of a cut, so it treats every longer
/// title as one that the entry may have cut inside an identifier.
const TITLE_MASKED_MAX_BYTES: usize = TEXT_MAX_BYTES - 4;

/// The filter that `TextFilters::r1` registers ahead of the byte cap, in every
/// build.
pub(super) fn text_filter() -> impl Fn(TextField, &str) -> String + Send + Sync {
    stored_text
}

/// A title of at most [`TITLE_MASKED_MAX_BYTES`] bytes masked whole, a longer
/// title as the empty title, and the text of every other field unchanged.
fn stored_text(field: TextField, text: &str) -> String {
    match field {
        TextField::Title if text.len() > TITLE_MASKED_MAX_BYTES => String::new(),
        TextField::Title => mask(text),
        TextField::BundleId | TextField::ExecutablePath | TextField::Site => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    use crate::daemon::model_store::{noon, ManualClocks, ModelStore, PID};
    use crate::daemon::scripted::{ScriptedRun, ScriptedSource};
    use crate::daemon::{run_timed, SecureInput, SourceName, SwitchFlags, TimedRun};
    use crate::event::{AppIdentity, EventClock, FloorEvent};
    use crate::floor::cases::keep;
    use crate::floor::kind;
    use crate::floor::payload::{Title, TitleWhy};
    use crate::floor::record::{DecodedPayload, DecodedRecord};
    use crate::floor::site::Site;
    use crate::floor::Switch;

    /// A title event built as `title_of` builds it, of `app`, `title` and `site`.
    fn title_event(app: &str, title: String, site: Option<Site>) -> FloorEvent {
        FloorEvent::window_title(
            keep::<kind::WindowTitle>(AppIdentity::bundle_id(app.to_owned()), PID),
            Title::new(title, TitleWhy::TitleChange, site),
            None,
            EventClock::system(),
        )
    }

    /// The one record that a timed run with the site switch on stores of
    /// `event`, which the run's foreground source emits.
    fn stored(event: FloorEvent) -> DecodedRecord {
        let mut scripted = ScriptedRun::new(vec![(
            SourceName::Foreground,
            ScriptedSource::emitting(vec![event]),
        )]);
        let run = TimedRun {
            window: Duration::from_secs(5),
            secure_input: SecureInput::DoNotHold,
            switches: SwitchFlags(vec![(Switch::SiteRead, true)]),
            store_dir: PathBuf::from("/"),
        };
        let store = ModelStore::new();
        let stop = AtomicBool::new(false);
        run_timed(
            &run,
            &mut scripted,
            store.clone(),
            ManualClocks::at(noon()),
            &stop,
            Duration::from_secs(2),
        )
        .unwrap();
        let mut records = store.records();
        assert_eq!(records.len(), 1, "the run did not store one record");
        records.remove(0)
    }

    fn title_and_cut(record: &DecodedRecord) -> (&str, bool) {
        match record.payload() {
            DecodedPayload::WindowTitle { text, cut, .. } => (text.as_str(), *cut),
            _ => panic!("the stored record is no title"),
        }
    }

    #[test]
    fn planted_values_are_stored_masked() {
        let key = format!("AKIA{}", "Q".repeat(16));
        let card = "4242 4242 4242 4242";
        let iban = "DE87123456781234567890";
        let ssn = "123-45-6789";
        let title = format!("pay {card} to {iban} ssn {ssn} key {key} now");
        let record = stored(title_event("com.example.editor", title, None));
        let (text, _) = title_and_cut(&record);
        for token in ["[card]", "[iban]", "[ssn]", "[key]"] {
            assert!(text.contains(token), "{token} is not in {text:?}");
        }
        for value in [card, iban, ssn, key.as_str()] {
            assert!(!text.contains(value), "{value} is stored in {text:?}");
        }
    }

    #[test]
    fn app_id_and_site_are_stored_unchanged() {
        let app = "com.example.4242424242424242";
        let site = Site::cut("https://4242424242424242.com/".to_owned());
        assert!(site.is_some(), "the cut made no site");
        let record = stored(title_event(app, "a title".to_owned(), site));
        assert_eq!(record.app(), Some(app));
        match record.payload() {
            DecodedPayload::WindowTitle {
                site: Some(site), ..
            } => assert_eq!(site.as_str(), "4242424242424242.com"),
            _ => panic!("the stored record holds no site"),
        }
    }

    #[test]
    fn a_cut_title_is_stored_empty() {
        let title = format!("{} 4242 4242 4242 4242", "a".repeat(1010));
        assert_eq!(title.len(), 1030);
        let record = stored(title_event("com.example.editor", title, None));
        assert_eq!(title_and_cut(&record), ("", true));
    }
}
