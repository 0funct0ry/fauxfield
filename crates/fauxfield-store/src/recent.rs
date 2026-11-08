use std::path::{Path, PathBuf};

const MAX: usize = 12;

/// Recent workspaces from a JSON list file (most recent first). Never fails.
pub fn load_recent(file: &Path) -> Vec<PathBuf> {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<PathBuf>>(&t).ok())
        .unwrap_or_default()
}

/// Move `ws` to the front of the recent list and save it.
pub fn push_recent(file: &Path, ws: &Path) -> std::io::Result<()> {
    let mut list = load_recent(file);
    list.retain(|p| p != ws);
    list.insert(0, ws.to_path_buf());
    list.truncate(MAX);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(
        file,
        serde_json::to_string_pretty(&list).unwrap_or_default(),
    )
}
