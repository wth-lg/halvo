use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledApp {
    pub name: String,
    pub path: PathBuf,
    /// The `CFBundleIdentifier` if it could be read from an XML `Info.plist`.
    pub bundle_id: Option<String>,
}

fn standard_app_dirs(home: &Path) -> Vec<PathBuf> {
    vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
        home.join("Applications"),
    ]
}

fn xml_bundle_id(app_path: &Path) -> Option<String> {
    let plist = app_path.join("Contents").join("Info.plist");
    let text = std::fs::read_to_string(&plist).ok()?;
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
        let bundle_id = xml_bundle_id(&path);
        apps.push(InstalledApp {
            name,
            path,
            bundle_id,
        });
    }
    apps.sort_by(|a, b| a.name.cmp(&b.name));
    apps
}

/// List installed applications across the standard Applications directories,
/// with `home` as the home directory whose `Applications` folder is read.
pub fn installed_applications(home: &Path) -> Vec<InstalledApp> {
    let mut apps = Vec::new();
    for dir in standard_app_dirs(home) {
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
    fn the_home_directory_is_a_parameter_whose_applications_folder_is_read() {
        let home = tempfile::tempdir().unwrap();
        let app = home.path().join("Applications").join("Gadget.app");
        fs::create_dir_all(&app).unwrap();
        assert_eq!(
            standard_app_dirs(home.path()),
            [
                PathBuf::from("/Applications"),
                PathBuf::from("/System/Applications"),
                home.path().join("Applications"),
            ]
        );
        let found: Vec<PathBuf> = installed_applications(home.path())
            .into_iter()
            .map(|app| app.path)
            .filter(|path| path.starts_with(home.path()))
            .collect();
        assert_eq!(found, [app]);
    }

    #[test]
    fn a_bundle_one_directory_deeper_is_not_listed() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("Utilities").join("Deep.app")).unwrap();
        assert!(scan_dir(tmp.path()).is_empty());
    }

    #[test]
    fn missing_directory_scans_to_empty() {
        assert!(scan_dir(Path::new("/no/such/dir/at/all")).is_empty());
    }
}
