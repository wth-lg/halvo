//! Installed-application inventory.
//!
//! The documented macOS inventory source is the set of `.app` bundles in the
//! standard Applications directories (researcher-1, researcher-4). This is a
//! utility, not a capture modality: it lists what is installed so a later phase
//! can name the app behind a foreground event. It reads only directory names and,
//! best-effort, the `CFBundleIdentifier` from an XML `Info.plist`; a binary plist
//! yields `None` for the bundle id rather than pulling in a plist parser Halvo
//! does not otherwise need.

use std::path::{Path, PathBuf};

/// One installed application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledApp {
    /// The bundle display name (the `.app` directory name without its suffix).
    pub name: String,
    /// The absolute path to the `.app` bundle.
    pub path: PathBuf,
    /// The `CFBundleIdentifier` if it could be read from an XML `Info.plist`.
    pub bundle_id: Option<String>,
}

/// The standard directories macOS keeps applications in.
fn standard_app_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
    ];
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join("Applications"));
    }
    dirs
}

/// Best-effort read of `CFBundleIdentifier` from an XML `Info.plist`. Returns
/// `None` for a binary plist or a missing key.
fn read_bundle_id(app_path: &Path) -> Option<String> {
    let plist = app_path.join("Contents").join("Info.plist");
    let text = std::fs::read_to_string(&plist).ok()?;
    // XML plist shape: <key>CFBundleIdentifier</key> <string>the.id</string>.
    let after_key = text.split("<key>CFBundleIdentifier</key>").nth(1)?;
    let open = after_key.find("<string>")? + "<string>".len();
    let close = after_key[open..].find("</string>")?;
    let id = after_key[open..open + close].trim().to_string();
    if id.is_empty() {
        None
    } else {
        Some(id)
    }
}

/// List the `.app` bundles directly inside `dir`. A directory that does not exist
/// contributes nothing.
pub fn scan_dir(dir: &Path) -> Vec<InstalledApp> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut apps = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_app = path
            .extension()
            .map(|ext| ext.eq_ignore_ascii_case("app"))
            .unwrap_or(false);
        if !is_app {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let bundle_id = read_bundle_id(&path);
        apps.push(InstalledApp {
            name,
            path,
            bundle_id,
        });
    }
    apps.sort_by(|a, b| a.name.cmp(&b.name));
    apps
}

/// List installed applications across the standard Applications directories.
pub fn installed_applications() -> Vec<InstalledApp> {
    let mut apps = Vec::new();
    for dir in standard_app_dirs() {
        apps.extend(scan_dir(&dir));
    }
    apps
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scans_app_bundles_and_reads_an_xml_bundle_id() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("Widget.app");
        fs::create_dir_all(app.join("Contents")).unwrap();
        fs::write(
            app.join("Contents").join("Info.plist"),
            "<plist><dict><key>CFBundleIdentifier</key><string>com.example.widget</string></dict></plist>",
        )
        .unwrap();
        // A non-app directory is ignored.
        fs::create_dir_all(tmp.path().join("notes")).unwrap();

        let apps = scan_dir(tmp.path());
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Widget");
        assert_eq!(apps[0].bundle_id.as_deref(), Some("com.example.widget"));
    }

    #[test]
    fn app_without_readable_plist_has_no_bundle_id() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("Bare.app")).unwrap();
        let apps = scan_dir(tmp.path());
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Bare");
        assert_eq!(apps[0].bundle_id, None);
    }

    #[test]
    fn missing_directory_scans_to_empty() {
        assert!(scan_dir(Path::new("/no/such/dir/at/all")).is_empty());
    }
}
