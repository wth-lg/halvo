use crate::handler::MAIN;
use crate::hub::Agents;
use tauri::{Manager, Runtime, Window, WindowEvent};

/// What U0.2's verdict asks of the window `main` when the person closes it (contracts I10.2,
/// I11.3: `hosted_mac=hidden hosted_win=destroy`). This run writes no window code: the capture
/// run's window applies the form through a Revision of its C10.9, and both forms end every
/// waiting `agents_next` with `{"closed":"yes"}` through the hooks of `Core`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Close {
    /// The window is hidden and kept, as on macOS.
    Hide,
    /// The window is destroyed and made again at the next show, as on Windows.
    Destroy,
}

/// The form of U0.2's verdict for the system of this build.
pub const CLOSE: Close = if cfg!(target_os = "windows") {
    Close::Destroy
} else {
    Close::Hide
};

/// The hook of the app's window events: the window `main` destroyed calls `Core::destroyed` of
/// the `Agents` that the app manages; every other event and window changes nothing.
pub fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    if window.label() != MAIN || !matches!(event, WindowEvent::Destroyed) {
        return;
    }
    if let Some(agents) = window.try_state::<Agents>() {
        agents.0.destroyed();
    }
}
