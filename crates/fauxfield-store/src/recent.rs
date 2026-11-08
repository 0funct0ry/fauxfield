use std::path::{Path, PathBuf};

const MAX: usize = 10;

/// Recent workspaces from a JSON list file (most recent first). Never fails.
pub fn load_recent(file: &Path) -> Vec<PathBuf> {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<PathBuf>>(&t).ok())
        .unwrap_or_default()
}

fn save(file: &Path, list: &[PathBuf]) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(file, serde_json::to_string_pretty(list).unwrap_or_default())
}

/// Move `ws` to the front of the recent list and save it.
pub fn push_recent(file: &Path, ws: &Path) -> std::io::Result<()> {
    let mut list = load_recent(file);
    list.retain(|p| p != ws);
    list.insert(0, ws.to_path_buf());
    list.truncate(MAX);
    save(file, &list)
}

/// Drop `ws` from the recent list (the folder is never touched).
pub fn remove_recent(file: &Path, ws: &Path) -> std::io::Result<()> {
    let mut list = load_recent(file);
    list.retain(|p| p != ws);
    save(file, &list)
}

/// Empty the recent list.
pub fn clear_recent(file: &Path) -> std::io::Result<()> {
    save(file, &[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupes_orders_caps_removes_and_survives_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("recent.json");
        assert!(load_recent(&f).is_empty());
        for i in 0..12 {
            push_recent(&f, Path::new(&format!("/w/{i}"))).unwrap();
        }
        let l = load_recent(&f);
        assert_eq!(l.len(), 10);
        assert_eq!(l[0], Path::new("/w/11"));
        push_recent(&f, Path::new("/w/5")).unwrap();
        let l = load_recent(&f);
        assert_eq!(l[0], Path::new("/w/5"));
        assert_eq!(l.iter().filter(|p| *p == Path::new("/w/5")).count(), 1);
        remove_recent(&f, Path::new("/w/5")).unwrap();
        assert!(!load_recent(&f).contains(&PathBuf::from("/w/5")));
        clear_recent(&f).unwrap();
        assert!(load_recent(&f).is_empty());
        std::fs::write(&f, "{not json").unwrap();
        assert!(load_recent(&f).is_empty());
        push_recent(&f, Path::new("/w/1")).unwrap();
        assert_eq!(load_recent(&f).len(), 1);
    }
}
