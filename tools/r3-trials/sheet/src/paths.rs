use std::path::{Path, PathBuf};

pub fn shown(path: &Path) -> String {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(dir) => dir.join(path),
            Err(_) => return file_name(path),
        }
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .map(Path::to_path_buf);
    let roots: Vec<PathBuf> = root
        .iter()
        .flat_map(|root| [root.canonicalize().ok(), Some(root.clone())])
        .flatten()
        .collect();
    let candidates = [absolute.canonicalize().ok(), Some(absolute.clone())];
    for candidate in candidates.iter().flatten() {
        for root in &roots {
            if let Ok(rest) = candidate.strip_prefix(root) {
                if !rest.as_os_str().is_empty() {
                    return rest.display().to_string();
                }
            }
        }
    }
    file_name(path)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}
