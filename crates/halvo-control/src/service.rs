//! How the app and the daemon start: the service arguments, the login item's
//! plist and the Windows start value.

use crate::model::APP_BUNDLE_ID;

/// The argument that starts the capture daemon (contracts.md C3.1).
pub const DAEMON_ARG: &str = "--service-run";

/// The argument of an app started at login, which shows no window (C14.7).
pub const BACKGROUND_ARG: &str = "--background";

/// The label of the app's login item (RD-08).
pub const LOGIN_ITEM_LABEL: &str = APP_BUNDLE_ID;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_service_argument_has_one_spelling() {
        assert_eq!(DAEMON_ARG, "--service-run");
        assert_eq!(BACKGROUND_ARG, "--background");
        assert_eq!(LOGIN_ITEM_LABEL, "com.halvo.desktop");
    }
}
