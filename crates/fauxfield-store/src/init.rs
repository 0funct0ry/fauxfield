use std::path::{Path, PathBuf};

use fauxfield_model::new_id;

use crate::store::StoreError;

/// Nearest ancestor of `start` (inclusive) containing `fauxfield.toml`.
pub fn discover(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|d| d.join("fauxfield.toml").is_file())
        .map(Path::to_path_buf)
}

/// File-name slug for a server name: lowercase ascii, dashes.
pub fn slugify(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let s = s.trim_end_matches('-').to_string();
    if s.is_empty() { "server".into() } else { s }
}

/// Create the SPEC §4.1 layout with one example server. Refuses to overwrite a workspace.
pub fn init_workspace(root: &Path) -> Result<Vec<PathBuf>, StoreError> {
    if root.join("fauxfield.toml").exists() {
        return Err(StoreError::Invalid(format!(
            "{} already contains fauxfield.toml",
            root.display()
        )));
    }
    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace");
    for d in ["servers", "environments", "data", "files", "docs"] {
        std::fs::create_dir_all(root.join(d))?;
    }
    let manifest = format!(
        "schema = 1\nname = \"{name}\"\ndefault_environment = \"local\"\n\n[variables]\napi_version = \"v1\"\n\n[defaults]\nhost = \"127.0.0.1\"\n"
    );
    let env = "name = \"local\"\n\n[variables]\nbase_url = \"http://localhost:3001\"\n";
    let server = format!(
        "schema = 1\nid = \"{}\"\nname = \"Example API\"\nenabled = true\nhost = \"127.0.0.1\"\nport = 3001\n\n[[routes]]\nid = \"{}\"\nname = \"Health\"\nmethod = \"GET\"\npath = \"/health\"\n\n  [[routes.responses]]\n  id = \"{}\"\n  name = \"OK\"\n  default = true\n  status = 200\n  content_type = \"application/json\"\n  body = '{{\"status\": \"ok\"}}'\n",
        new_id("srv"),
        new_id("rte"),
        new_id("rsp")
    );
    let files = [
        ("fauxfield.toml", manifest),
        ("environments/local.toml", env.to_string()),
        ("servers/example-api.toml", server),
        (".gitignore", ".fauxfield/\n".to_string()),
    ];
    let mut written = Vec::new();
    for (rel, text) in files {
        let p = root.join(rel);
        std::fs::write(&p, text)?;
        written.push(p);
    }
    Ok(written)
}
