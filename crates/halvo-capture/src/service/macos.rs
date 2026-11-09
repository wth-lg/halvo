//! macOS LaunchAgent install for the capture daemon.
//!
//! The daemon runs as a per-user LaunchAgent so capture starts after the user
//! logs in (researcher-1: RunAtLoad after login), in its own process, never
//! sharing a writer with the UI. The install is idempotent: it writes the plist
//! only when it is absent or its content differs, so running it twice leaves one
//! correct plist and changes nothing the second time. Activation through
//! `launchctl` is left to the onboarding flow; this type owns the plist on disk,
//! which is what "install, idempotent" means and what the gate checks.

use std::path::{Path, PathBuf};

use crate::service::{ServiceError, ServiceInstaller, ServiceStatus};

/// The default LaunchAgent label (and plist base name).
pub const DEFAULT_LABEL: &str = "com.halvo.capture";

/// Installs and removes the capture LaunchAgent plist.
pub struct LaunchAgentInstaller {
    label: String,
    binary_path: PathBuf,
    agents_dir: PathBuf,
}

impl LaunchAgentInstaller {
    /// An installer for `binary_path`, writing into the current user's
    /// `~/Library/LaunchAgents`.
    pub fn new(binary_path: impl Into<PathBuf>) -> Result<Self, ServiceError> {
        let home = std::env::var("HOME").map_err(|_| ServiceError::Op("HOME is not set".into()))?;
        Ok(Self::with_agents_dir(
            DEFAULT_LABEL,
            binary_path,
            PathBuf::from(home).join("Library").join("LaunchAgents"),
        ))
    }

    /// An installer with an explicit label and LaunchAgents directory (used by
    /// tests to target a temporary directory).
    pub fn with_agents_dir(
        label: impl Into<String>,
        binary_path: impl Into<PathBuf>,
        agents_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            label: label.into(),
            binary_path: binary_path.into(),
            agents_dir: agents_dir.into(),
        }
    }

    /// The plist file this installer owns.
    pub fn plist_path(&self) -> PathBuf {
        self.agents_dir.join(format!("{}.plist", self.label))
    }

    /// The exact plist content this installer wants on disk. Deterministic, so
    /// an unchanged install is a byte-for-byte match and writes nothing.
    pub fn desired_plist(&self) -> String {
        let program = xml_escape(&self.binary_path.to_string_lossy());
        let label = xml_escape(&self.label);
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n\
<dict>\n\
\t<key>Label</key>\n\
\t<string>{label}</string>\n\
\t<key>ProgramArguments</key>\n\
\t<array>\n\
\t\t<string>{program}</string>\n\
\t</array>\n\
\t<key>RunAtLoad</key>\n\
\t<true/>\n\
\t<key>KeepAlive</key>\n\
\t<true/>\n\
\t<key>ProcessType</key>\n\
\t<string>Background</string>\n\
</dict>\n\
</plist>\n"
        )
    }
}

/// Escape the five XML special characters so a path or label is safe in the plist.
fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Whether the file at `path` already holds exactly `content`.
fn file_matches(path: &Path, content: &str) -> bool {
    std::fs::read_to_string(path)
        .map(|existing| existing == content)
        .unwrap_or(false)
}

impl ServiceInstaller for LaunchAgentInstaller {
    fn install(&self) -> Result<(), ServiceError> {
        let desired = self.desired_plist();
        let path = self.plist_path();
        // Idempotent: if the plist already matches, do nothing.
        if file_matches(&path, &desired) {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ServiceError::Op(format!("create {}: {e}", parent.display())))?;
        }
        std::fs::write(&path, desired)
            .map_err(|e| ServiceError::Op(format!("write {}: {e}", path.display())))?;
        Ok(())
    }

    fn uninstall(&self) -> Result<(), ServiceError> {
        let path = self.plist_path();
        // Idempotent: removing an already-absent plist is success.
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(ServiceError::Op(format!("remove {}: {e}", path.display()))),
        }
    }

    fn status(&self) -> Result<ServiceStatus, ServiceError> {
        if self.plist_path().exists() {
            Ok(ServiceStatus::Installed)
        } else {
            Ok(ServiceStatus::NotInstalled)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installer(dir: &std::path::Path) -> LaunchAgentInstaller {
        LaunchAgentInstaller::with_agents_dir(
            "com.halvo.capture.test",
            "/usr/local/bin/halvo-capture",
            dir,
        )
    }

    #[test]
    fn install_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let inst = installer(tmp.path());

        assert_eq!(inst.status().unwrap(), ServiceStatus::NotInstalled);
        inst.install().unwrap();
        assert_eq!(inst.status().unwrap(), ServiceStatus::Installed);
        let first = std::fs::read_to_string(inst.plist_path()).unwrap();

        // Running it again changes nothing and does not error.
        inst.install().unwrap();
        let second = std::fs::read_to_string(inst.plist_path()).unwrap();
        assert_eq!(first, second);
        assert_eq!(inst.status().unwrap(), ServiceStatus::Installed);
    }

    #[test]
    fn uninstall_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let inst = installer(tmp.path());
        inst.install().unwrap();
        inst.uninstall().unwrap();
        assert_eq!(inst.status().unwrap(), ServiceStatus::NotInstalled);
        // Removing again is still success.
        inst.uninstall().unwrap();
        assert_eq!(inst.status().unwrap(), ServiceStatus::NotInstalled);
    }

    #[test]
    fn plist_contains_the_label_and_binary_and_run_at_load() {
        let tmp = tempfile::tempdir().unwrap();
        let inst = installer(tmp.path());
        let plist = inst.desired_plist();
        assert!(plist.contains("com.halvo.capture.test"));
        assert!(plist.contains("/usr/local/bin/halvo-capture"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
    }

    #[test]
    fn a_changed_binary_path_rewrites_the_plist() {
        let tmp = tempfile::tempdir().unwrap();
        let a = LaunchAgentInstaller::with_agents_dir(
            "com.halvo.capture.test",
            "/old/path",
            tmp.path(),
        );
        a.install().unwrap();
        let b = LaunchAgentInstaller::with_agents_dir(
            "com.halvo.capture.test",
            "/new/path",
            tmp.path(),
        );
        b.install().unwrap();
        let on_disk = std::fs::read_to_string(b.plist_path()).unwrap();
        assert!(on_disk.contains("/new/path"));
        assert!(!on_disk.contains("/old/path"));
    }
}
