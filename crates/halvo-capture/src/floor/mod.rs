//! The content floor: what a capture source may emit and what the store keeps.
//! No value of the floor holds a typed character, a key code, a coordinate or
//! a pixel.

pub mod text;

#[cfg(test)]
pub(crate) mod cases;

#[cfg(test)]
mod tests {
    use super::cases::stored;
    use super::text::{CapturedText, TextField, TEXT_MAX_BYTES};

    #[test]
    fn the_text_entry_cuts_on_a_character_boundary_and_keeps_a_path_tail() {
        let short = CapturedText::enter(TextField::Title, "Inbox".to_string());
        assert_eq!(stored(&short, TextField::Title), "Inbox");
        assert!(!short.was_cut());

        let exact = "a".repeat(TEXT_MAX_BYTES);
        let whole = CapturedText::enter(TextField::Title, exact.clone());
        assert_eq!(stored(&whole, TextField::Title), exact);
        assert!(!whole.was_cut());

        let over = format!("{}é", "a".repeat(TEXT_MAX_BYTES - 1));
        assert_eq!(over.len(), TEXT_MAX_BYTES + 1);
        let head = CapturedText::enter(TextField::Title, over);
        assert_eq!(
            stored(&head, TextField::Title),
            "a".repeat(TEXT_MAX_BYTES - 1)
        );
        assert!(head.was_cut());

        let wide = format!("a{}", "\u{1F600}".repeat(256));
        let head = CapturedText::enter(TextField::BundleId, wide);
        let kept = stored(&head, TextField::BundleId);
        assert_eq!(kept, format!("a{}", "\u{1F600}".repeat(255)));
        assert_eq!(kept.len(), 1021);

        let path = format!("C:\\{}\\bin\\app.exe", "dir\\".repeat(500));
        let tail = CapturedText::enter(TextField::ExecutablePath, path.clone());
        let kept = stored(&tail, TextField::ExecutablePath);
        assert_eq!(kept.len(), TEXT_MAX_BYTES);
        assert!(kept.ends_with("\\bin\\app.exe"));
        assert!(path.ends_with(&kept));
        assert!(tail.was_cut());

        let accents = format!("{}x", "é".repeat(600));
        let tail = CapturedText::enter(TextField::ExecutablePath, accents);
        let kept = stored(&tail, TextField::ExecutablePath);
        assert_eq!(kept, format!("{}x", "é".repeat(511)));
        assert_eq!(kept.len(), TEXT_MAX_BYTES - 1);

        let title_of_accents = format!("{}x", "é".repeat(600));
        let head = CapturedText::enter(TextField::Title, title_of_accents);
        assert_eq!(stored(&head, TextField::Title), "é".repeat(512));
    }

    #[test]
    fn captured_text_prints_a_placeholder() {
        let text = CapturedText::enter(TextField::Title, "Quarterly plan".to_string());
        assert_eq!(format!("{text:?}"), "CapturedText(..)");
    }
}
