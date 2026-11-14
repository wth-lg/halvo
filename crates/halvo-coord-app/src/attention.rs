use halvo_coord::words;

/// What the tray shows for the count of agents under `needs_you` (I10.3), which the app's
/// `set_attention` will apply, in the unit's second pull request (no `set_attention` exists
/// yet): for a count above zero the icon variant, the tooltip `tray.needs`
/// with the count on both systems and the count as the title on macOS alone, which Windows
/// does not support; for zero the tray's own icon, tooltip and title.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attention {
    /// The count.
    pub count: u32,
    /// The icon variant in place of the tray's own icon.
    pub variant: bool,
    /// The tooltip in place of the tray's own, or none.
    pub tooltip: Option<String>,
    /// The title in place of the tray's own, or none.
    pub title: Option<String>,
}

impl Attention {
    /// What the tray shows for `count`.
    pub fn of(count: u32) -> Attention {
        if count == 0 {
            return Attention {
                count,
                variant: false,
                tooltip: None,
                title: None,
            };
        }
        let n = count.to_string();
        Attention {
            count,
            variant: true,
            tooltip: words::text("tray.needs", &[("count", &n)]).ok(),
            title: cfg!(target_os = "macos").then_some(n),
        }
    }
}
