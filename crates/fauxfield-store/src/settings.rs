//! Shared `settings.toml` loader, writer and watcher (SPEC §13, §20.34). The GUI, the CLI
//! (`theme set`) and `fauxfield-mcp` all read settings through this module. Writes go through
//! `toml_edit` so comments and unrelated keys survive, and land atomically.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};
use serde::Deserialize;
use toml_edit::{DocumentMut, Item, Table, value};

/// How the theme follows the OS appearance (`theme.mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

/// The `theme` key: `{ mode, light, dark }`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ThemeSetting {
    pub mode: ThemeMode,
    pub light: String,
    pub dark: String,
}

impl Default for ThemeSetting {
    fn default() -> Self {
        Self {
            mode: ThemeMode::System,
            light: "One Light".into(),
            dark: "One Dark".into(),
        }
    }
}

/// The `[mcp]` table. Every key is optional.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct McpTable {
    pub transport: Option<String>,
    pub allow: Option<Vec<String>>,
    pub socket: Option<PathBuf>,
    pub port: Option<u16>,
    #[serde(default)]
    pub redact: Vec<String>,
}

/// Typed view of settings.toml. Unknown keys are ignored here (kept on disk by the writer).
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeSetting,
    pub ui_font_family: Option<String>,
    pub ui_font_size: Option<f32>,
    pub buffer_font_family: Option<String>,
    pub buffer_font_size: Option<f32>,
    pub mcp: McpTable,
    /// `[editor]` (and `[editor.languages.*]`) as a raw table; the editor parses its own keys.
    pub editor: toml::Table,
}

impl Settings {
    pub fn from_toml(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| format!("invalid settings.toml: {e}"))
    }

    /// Load `path`; a missing file means defaults.
    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(t) => Self::from_toml(&t).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// Load from [`settings_path`]; no config directory means defaults.
    pub fn load_default() -> Result<Self, String> {
        match settings_path() {
            Some(p) => Self::load(&p),
            None => Ok(Self::default()),
        }
    }
}

/// `$FAUXFIELD_SETTINGS`, else `<config dir>/fauxfield/settings.toml`.
pub fn settings_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FAUXFIELD_SETTINGS") {
        return Some(PathBuf::from(p));
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }?;
    Some(base.join("fauxfield").join("settings.toml"))
}

/// Names and appearances of the themes bundled in the GUI (kept here so the CLI lists them
/// without embedding the JSON; a GUI test checks this list against the bundled files).
pub const BUNDLED_THEMES: &[(&str, &str)] = &[
    ("One Dark", "dark"),
    ("One Light", "light"),
    ("Ayu Dark", "dark"),
    ("Ayu Light", "light"),
    ("Ayu Mirage", "dark"),
    ("Gruvbox Dark", "dark"),
    ("Gruvbox Dark Hard", "dark"),
    ("Gruvbox Dark Soft", "dark"),
    ("Gruvbox Light", "light"),
    ("Gruvbox Light Hard", "light"),
    ("Gruvbox Light Soft", "light"),
    ("Rosé Pine", "dark"),
    ("Rosé Pine Moon", "dark"),
    ("Rosé Pine Dawn", "light"),
    ("Catppuccin Latte", "light"),
    ("Catppuccin Frappé", "dark"),
    ("Catppuccin Macchiato", "dark"),
    ("Catppuccin Mocha", "dark"),
    ("Tokyo Night", "dark"),
    ("Tokyo Night Light", "light"),
    ("Tokyo Night Storm", "dark"),
    ("Tokyo Night Moon", "dark"),
    ("Nord Dark", "dark"),
    ("Nord Darker", "dark"),
    ("Nord Light", "light"),
    ("Nord Dark - No Italics", "dark"),
    ("Nord Light - No Italics", "light"),
];

/// `<settings dir>/themes`, where user theme JSON lives.
pub fn user_themes_dir() -> Option<PathBuf> {
    settings_path()?.parent().map(|p| p.join("themes"))
}

/// Write `theme` into settings.toml, keeping comments and other keys. `slot` (when given)
/// sets `theme.light` or `theme.dark` to `name`.
pub fn write_theme(
    path: &Path,
    mode: Option<ThemeMode>,
    slot: Option<(ThemeMode, &str)>,
) -> Result<(), String> {
    update(path, |doc| {
        let table = doc
            .entry("theme")
            .or_insert_with(|| Item::Table(Table::new()))
            .as_table_like_mut()
            .ok_or("`theme` in settings.toml must be a table")?;
        if let Some((which, name)) = slot {
            table.insert(
                if which == ThemeMode::Dark {
                    "dark"
                } else {
                    "light"
                },
                value(name),
            );
        }
        if let Some(m) = mode {
            table.insert("mode", value(m.as_str()));
        }
        Ok(())
    })
}

/// Read-modify-write settings.toml through `toml_edit`, atomically.
pub fn update(
    path: &Path,
    edit: impl FnOnce(&mut DocumentMut) -> Result<(), &'static str>,
) -> Result<(), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e| format!("settings.toml is not valid TOML: {e}"))?;
    edit(&mut doc).map_err(str::to_owned)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, doc.to_string())
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Debounced watcher over a set of paths (files or directories). Calls `on_change` once per
/// burst of file-system events. Dropping it stops the thread. Missing paths are skipped, so
/// a watched directory that appears later needs a new watcher.
pub struct ConfigWatcher {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    _watcher: notify::RecommendedWatcher,
}

impl Drop for ConfigWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Watch `paths`; a file's parent directory is watched so atomic renames are seen.
pub fn watch(
    paths: &[PathBuf],
    debounce: Duration,
    on_change: impl Fn() + Send + 'static,
) -> Result<ConfigWatcher, String> {
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok() {
            let _ = tx.send(());
        }
    })
    .map_err(|e| e.to_string())?;
    for p in paths {
        let target = if p.is_dir() {
            p.clone()
        } else {
            p.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| p.clone())
        };
        if target.exists() {
            watcher
                .watch(&target, RecursiveMode::NonRecursive)
                .map_err(|e| e.to_string())?;
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::spawn(move || {
        let (mut pending, mut last) = (false, Instant::now());
        while !flag.load(Ordering::SeqCst) {
            if rx.recv_timeout(Duration::from_millis(20)).is_ok() {
                while rx.try_recv().is_ok() {}
                pending = true;
                last = Instant::now();
            }
            if pending && last.elapsed() >= debounce {
                pending = false;
                on_change();
            }
        }
    });
    Ok(ConfigWatcher {
        stop,
        thread: Some(thread),
        _watcher: watcher,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_theme_parse() {
        let s = Settings::from_toml("theme = { mode = \"dark\", dark = \"Nord\" }").unwrap();
        assert_eq!(s.theme.mode, ThemeMode::Dark);
        assert_eq!(s.theme.dark, "Nord");
        assert_eq!(s.theme.light, "One Light");
        assert_eq!(
            Settings::from_toml("").unwrap().theme,
            ThemeSetting::default()
        );
    }

    #[test]
    fn write_theme_keeps_comments() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.toml");
        std::fs::write(&p, "# mine\nui_font_size = 13\n").unwrap();
        write_theme(&p, Some(ThemeMode::Dark), Some((ThemeMode::Dark, "Nord"))).unwrap();
        let t = std::fs::read_to_string(&p).unwrap();
        assert!(t.contains("# mine") && t.contains("ui_font_size"));
        let s = Settings::load(&p).unwrap();
        assert_eq!(
            (s.theme.mode, s.theme.dark.as_str()),
            (ThemeMode::Dark, "Nord")
        );
    }

    #[test]
    fn watcher_fires_on_write() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.toml");
        std::fs::write(&p, "").unwrap();
        let (tx, rx) = mpsc::channel();
        let _w = watch(std::slice::from_ref(&p), Duration::from_millis(30), move || {
            let _ = tx.send(());
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        std::fs::write(&p, "ui_font_size = 14\n").unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(3)).is_ok());
    }
}
