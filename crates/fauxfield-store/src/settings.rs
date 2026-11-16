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

/// The `[traffic]` table.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct TrafficTable {
    pub max_rows: Option<u64>,
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
    pub autosave_delay_ms: Option<u64>,
    pub confirm_on_quit_with_running_servers: Option<bool>,
    pub restore_workspaces_on_start: Option<bool>,
    /// Reserved: there is no telemetry in v1.0, the key is always read as off.
    pub telemetry: Option<bool>,
    pub ui_scale: Option<f32>,
    /// `system` | `on` | `off`.
    pub reduce_motion: Option<String>,
    pub high_contrast: Option<bool>,
    pub vim_list_navigation: Option<bool>,
    pub check_for_updates: Option<bool>,
    pub traffic: TrafficTable,
    pub mcp: McpTable,
    /// `[editor]` (and `[editor.languages.*]`) as a raw table; the editor parses its own keys.
    pub editor: toml::Table,
}

impl Settings {
    /// Parse settings.toml text strictly: an invalid value is an error (the CLI and MCP refuse
    /// to start on it). The GUI uses [`Settings::from_toml_with_issues`], which drops invalid
    /// values (the default stays in effect) and reports them as Problems.
    pub fn from_toml(text: &str) -> Result<Self, String> {
        let (s, issues) = Self::parse_with(text, false)?;
        match issues.first() {
            Some(i) => Err(format!("invalid settings.toml: {}: {}", i.key, i.message)),
            None => Ok(s),
        }
    }

    pub fn from_toml_with_issues(text: &str) -> Result<(Self, Vec<SettingsIssue>), String> {
        Self::parse_with(text, true)
    }

    fn parse_with(text: &str, lenient_mcp: bool) -> Result<(Self, Vec<SettingsIssue>), String> {
        let mut table: toml::Table = text
            .parse()
            .map_err(|e| format!("invalid settings.toml: {e}"))?;
        let issues = if lenient_mcp {
            sanitize(&mut table)
        } else {
            // `fauxfield mcp` validates `[mcp]` itself (clearer messages, `--flag` precedence).
            let mcp = table.remove("mcp");
            let issues = sanitize(&mut table);
            if let Some(m) = mcp {
                table.insert("mcp".into(), m);
            }
            issues
        };
        let s = Self::deserialize(table).map_err(|e| format!("invalid settings.toml: {e}"))?;
        Ok((s, issues))
    }

    /// Like [`Settings::load`] but also returns the validation issues.
    pub fn load_with_issues(path: &Path) -> Result<(Self, Vec<SettingsIssue>), String> {
        match std::fs::read_to_string(path) {
            Ok(t) => {
                Self::from_toml_with_issues(&t).map_err(|e| format!("{}: {e}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((Self::default(), vec![])),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
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

/// One invalid value in settings.toml; the default stays in effect for `key`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsIssue {
    pub key: String,
    pub message: String,
}

/// Where a change applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Apply {
    Instant,
    /// Read when a session starts (MCP, traffic).
    NewSessions,
}

/// Control kind of a setting row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingKind {
    Bool,
    Int { min: i64, max: i64 },
    Float { min: f64, max: f64, step: f64 },
    Text,
    Enum(&'static [&'static str]),
    Font,
    Path,
    Multi(&'static [&'static str]),
}

/// Metadata for one setting. `key` is the dotted TOML path. One table drives the Settings tab,
/// the JSON schema and the generated reference.
#[derive(Debug, Clone, Copy)]
pub struct SettingDef {
    pub key: &'static str,
    pub category: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub kind: SettingKind,
    /// Default as TOML text (`true`, `300`, `"IBM Plex Sans"`); empty means unset.
    pub default: &'static str,
    pub apply: Apply,
    /// Not shown in the Settings tab (reserved keys).
    pub hidden: bool,
    /// Shown but not editable.
    pub disabled: bool,
}

pub const CATEGORIES: &[&str] = &[
    "Appearance",
    "Editor",
    "Servers & Engine",
    "MCP & Agents",
    "Traffic",
    "Keymap",
    "Accessibility",
    "Advanced",
];

const fn def(
    key: &'static str,
    category: &'static str,
    label: &'static str,
    description: &'static str,
    kind: SettingKind,
    default: &'static str,
) -> SettingDef {
    SettingDef {
        key,
        category,
        label,
        description,
        kind,
        default,
        apply: Apply::Instant,
        hidden: false,
        disabled: false,
    }
}

const TRANSPORTS: &[&str] = &["stdio", "unix", "pipe", "both", "http"];
const MCP_CLASSES: &[&str] = &["read", "write", "execute"];
const WHITESPACE: &[&str] = &["none", "selection", "all"];
const MOTION: &[&str] = &["system", "on", "off"];
const THEME_MODES: &[&str] = &["system", "light", "dark"];

/// Every setting of SPEC §13 plus the M17 keys.
pub const SETTING_DEFS: &[SettingDef] = &[
    def(
        "theme.mode",
        "Appearance",
        "Appearance mode",
        "Follow the OS (system) or pin light or dark.",
        SettingKind::Enum(THEME_MODES),
        "\"system\"",
    ),
    def(
        "theme.light",
        "Appearance",
        "Light theme",
        "Theme used in light mode.",
        SettingKind::Text,
        "\"One Light\"",
    ),
    def(
        "theme.dark",
        "Appearance",
        "Dark theme",
        "Theme used in dark mode.",
        SettingKind::Text,
        "\"One Dark\"",
    ),
    def(
        "ui_font_family",
        "Appearance",
        "UI font",
        "Font for the interface.",
        SettingKind::Font,
        "\"IBM Plex Sans\"",
    ),
    def(
        "ui_font_size",
        "Appearance",
        "UI font size",
        "Interface text size in points.",
        SettingKind::Float {
            min: 8.0,
            max: 32.0,
            step: 1.0,
        },
        "13",
    ),
    def(
        "buffer_font_family",
        "Appearance",
        "Code font",
        "Font for editors and tables.",
        SettingKind::Font,
        "\"IBM Plex Mono\"",
    ),
    def(
        "buffer_font_size",
        "Appearance",
        "Code font size",
        "Editor text size in points.",
        SettingKind::Float {
            min: 8.0,
            max: 32.0,
            step: 1.0,
        },
        "14",
    ),
    def(
        "ui_scale",
        "Appearance",
        "Interface scale",
        "Scale the whole interface, 0.8 to 2.0.",
        SettingKind::Float {
            min: 0.8,
            max: 2.0,
            step: 0.1,
        },
        "1.0",
    ),
    def(
        "editor.tab_size",
        "Editor",
        "Tab size",
        "Spaces per indent level, 1 to 16.",
        SettingKind::Int { min: 1, max: 16 },
        "2",
    ),
    def(
        "editor.hard_tabs",
        "Editor",
        "Hard tabs",
        "Indent with tab characters.",
        SettingKind::Bool,
        "false",
    ),
    def(
        "editor.soft_wrap",
        "Editor",
        "Soft wrap",
        "Wrap long lines.",
        SettingKind::Bool,
        "false",
    ),
    def(
        "editor.show_whitespace",
        "Editor",
        "Show whitespace",
        "none, selection or all.",
        SettingKind::Enum(WHITESPACE),
        "\"none\"",
    ),
    def(
        "editor.indent_guides",
        "Editor",
        "Indent guides",
        "Draw vertical indent guides.",
        SettingKind::Bool,
        "true",
    ),
    def(
        "editor.line_numbers",
        "Editor",
        "Line numbers",
        "Show the line number gutter.",
        SettingKind::Bool,
        "true",
    ),
    def(
        "editor.auto_close",
        "Editor",
        "Auto close brackets",
        "Insert the closing bracket or quote.",
        SettingKind::Bool,
        "true",
    ),
    def(
        "editor.cursor_blink",
        "Editor",
        "Cursor blink",
        "Blink the text cursor.",
        SettingKind::Bool,
        "true",
    ),
    def(
        "editor.format_json_on_save",
        "Editor",
        "Format JSON on save",
        "Pretty-print valid JSON bodies when saving.",
        SettingKind::Bool,
        "false",
    ),
    def(
        "autosave_delay_ms",
        "Servers & Engine",
        "Autosave delay",
        "Milliseconds to wait after an edit before writing the file.",
        SettingKind::Int { min: 0, max: 60000 },
        "300",
    ),
    def(
        "confirm_on_quit_with_running_servers",
        "Servers & Engine",
        "Confirm quit with running Servers",
        "Ask before quitting while Servers run.",
        SettingKind::Bool,
        "true",
    ),
    def(
        "restore_workspaces_on_start",
        "Servers & Engine",
        "Restore Workspaces on start",
        "Reopen the Workspaces of the last session.",
        SettingKind::Bool,
        "true",
    ),
    SettingDef {
        apply: Apply::NewSessions,
        ..def(
            "mcp.transport",
            "MCP & Agents",
            "MCP transport",
            "Default for `fauxfield mcp`; flags win.",
            SettingKind::Enum(TRANSPORTS),
            "\"stdio\"",
        )
    },
    SettingDef {
        apply: Apply::NewSessions,
        ..def(
            "mcp.allow",
            "MCP & Agents",
            "Allowed tool classes",
            "Tool classes agents may call.",
            SettingKind::Multi(MCP_CLASSES),
            "[\"read\", \"write\", \"execute\"]",
        )
    },
    SettingDef {
        apply: Apply::NewSessions,
        ..def(
            "mcp.socket",
            "MCP & Agents",
            "MCP socket",
            "Unix socket path for the unix transport.",
            SettingKind::Path,
            "",
        )
    },
    SettingDef {
        apply: Apply::NewSessions,
        ..def(
            "traffic.max_rows",
            "Traffic",
            "Traffic rows kept",
            "Recordings kept in memory.",
            SettingKind::Int {
                min: 100,
                max: 10_000_000,
            },
            "100000",
        )
    },
    def(
        "vim_list_navigation",
        "Keymap",
        "Vim list navigation",
        "j/k, g g/G, h/l in lists, trees and tables.",
        SettingKind::Bool,
        "false",
    ),
    def(
        "reduce_motion",
        "Accessibility",
        "Reduce motion",
        "system follows the OS.",
        SettingKind::Enum(MOTION),
        "\"system\"",
    ),
    def(
        "high_contrast",
        "Accessibility",
        "High contrast",
        "Use stronger borders and text colours.",
        SettingKind::Bool,
        "false",
    ),
    SettingDef {
        disabled: true,
        ..def(
            "telemetry",
            "Advanced",
            "Telemetry",
            "There is no telemetry in v1.0; the key is reserved.",
            SettingKind::Bool,
            "false",
        )
    },
    SettingDef {
        ..def(
            "check_for_updates",
            "Advanced",
            "Check for updates",
            "Once a day, ask GitHub for the latest release and show a status-bar notice when one is newer. Off by default; nothing is ever downloaded or installed.",
            SettingKind::Bool,
            "false",
        )
    },
];

pub fn find_def(key: &str) -> Option<&'static SettingDef> {
    SETTING_DEFS.iter().find(|d| d.key == key)
}

fn dotted<'a>(t: &'a toml::Table, key: &str) -> Option<&'a toml::Value> {
    let mut parts = key.split('.');
    let mut cur = t.get(parts.next()?)?;
    for p in parts {
        cur = cur.as_table()?.get(p)?;
    }
    Some(cur)
}

fn remove_dotted(t: &mut toml::Table, key: &str) {
    match key.split_once('.') {
        None => {
            t.remove(key);
        }
        Some((head, rest)) => {
            if let Some(toml::Value::Table(sub)) = t.get_mut(head) {
                remove_dotted(sub, rest);
            }
        }
    }
}

/// Check one value against `def`; `Err` carries the message.
pub fn check_value(def: &SettingDef, v: &toml::Value) -> Result<(), String> {
    use toml::Value as V;
    let num = |v: &V| match v {
        V::Integer(i) => Some(*i as f64),
        V::Float(f) => Some(*f),
        _ => None,
    };
    match (def.kind, v) {
        (SettingKind::Bool, V::Boolean(_)) => Ok(()),
        (SettingKind::Bool, _) => Err("must be true or false".into()),
        (SettingKind::Int { min, max }, V::Integer(i)) if (min..=max).contains(i) => Ok(()),
        (SettingKind::Int { min, max }, _) => {
            Err(format!("must be a whole number from {min} to {max}"))
        }
        (SettingKind::Float { min, max, .. }, v) => match num(v) {
            Some(n) if n >= min && n <= max => Ok(()),
            _ => Err(format!("must be a number from {min} to {max}")),
        },
        (SettingKind::Enum(opts), V::String(s)) if opts.contains(&s.as_str()) => Ok(()),
        (SettingKind::Enum(opts), V::Boolean(_)) if def.key == "editor.show_whitespace" => {
            let _ = opts;
            Ok(())
        }
        (SettingKind::Enum(opts), _) => Err(format!("must be one of {}", opts.join(", "))),
        (SettingKind::Text | SettingKind::Font | SettingKind::Path, V::String(_)) => Ok(()),
        (SettingKind::Text | SettingKind::Font | SettingKind::Path, _) => {
            Err("must be a string".into())
        }
        (SettingKind::Multi(opts), V::Array(a)) => {
            if a.iter()
                .all(|x| x.as_str().is_some_and(|s| opts.contains(&s)))
            {
                Ok(())
            } else {
                Err(format!("must be a list of: {}", opts.join(", ")))
            }
        }
        (SettingKind::Multi(_), _) => Err("must be a list".into()),
    }
}

/// Validate `table`, removing invalid values so the defaults take over. `[editor.languages.*]`
/// is validated by the editor itself.
pub fn sanitize(table: &mut toml::Table) -> Vec<SettingsIssue> {
    let mut issues = Vec::new();
    for d in SETTING_DEFS {
        let Some(v) = dotted(table, d.key) else {
            continue;
        };
        if let Err(message) = check_value(d, v) {
            issues.push(SettingsIssue {
                key: d.key.to_owned(),
                message,
            });
            remove_dotted(table, d.key);
        }
    }
    issues
}

/// Validate settings.toml text without building [`Settings`]; a TOML syntax error is one issue
/// with an empty key.
pub fn validate_text(text: &str) -> Vec<SettingsIssue> {
    match text.parse::<toml::Table>() {
        Ok(mut t) => sanitize(&mut t),
        Err(e) => vec![SettingsIssue {
            key: String::new(),
            message: format!("not valid TOML: {e}"),
        }],
    }
}

fn parse_default(d: &SettingDef) -> Option<toml_edit::Value> {
    if d.default.is_empty() {
        return None;
    }
    d.default.parse::<toml_edit::Value>().ok()
}

/// The default of `def` as a TOML value.
pub fn default_value(def: &SettingDef) -> Option<toml::Value> {
    toml::from_str::<toml::Table>(&format!("v = {}", def.default))
        .ok()
        .and_then(|mut t| t.remove("v"))
}

/// Whether `key` is set in `text` to something other than its default.
pub fn is_modified(table: &toml::Table, def: &SettingDef) -> bool {
    match dotted(table, def.key) {
        None => false,
        Some(v) => default_value(def).as_ref() != Some(v),
    }
}

fn table_at<'a>(
    doc: &'a mut DocumentMut,
    head: &[&str],
) -> Result<&'a mut dyn toml_edit::TableLike, &'static str> {
    let mut cur: &mut dyn toml_edit::TableLike = doc.as_table_mut();
    for h in head {
        cur = cur
            .entry(h)
            .or_insert_with(|| Item::Table(Table::new()))
            .as_table_like_mut()
            .ok_or("a settings table in settings.toml has the wrong type")?;
    }
    Ok(cur)
}

fn set_in(
    doc: &mut DocumentMut,
    key: &str,
    val: Option<toml_edit::Value>,
) -> Result<(), &'static str> {
    let parts: Vec<&str> = key.split('.').collect();
    let (last, head) = parts.split_last().ok_or("empty key")?;
    match val {
        Some(v) => {
            table_at(doc, head)?.insert(last, Item::Value(v));
        }
        None => {
            // Remove without creating tables that did not exist.
            let mut cur: &mut dyn toml_edit::TableLike = doc.as_table_mut();
            for h in head {
                match cur.get_mut(h).and_then(Item::as_table_like_mut) {
                    Some(n) => cur = n,
                    None => return Ok(()),
                }
            }
            cur.remove(last);
        }
    }
    Ok(())
}

/// Set `key` (dotted) to `val`, or remove it (reset) when `None`. Comments, order and unknown
/// keys are kept.
pub fn write_key(path: &Path, key: &str, val: Option<toml_edit::Value>) -> Result<(), String> {
    update(path, |doc| set_in(doc, key, val))
}

/// Remove every key of `category` (reset to defaults).
pub fn reset_category(path: &Path, category: &str) -> Result<(), String> {
    update(path, |doc| {
        for d in SETTING_DEFS
            .iter()
            .filter(|d| d.category == category && !d.disabled)
        {
            set_in(doc, d.key, None)?;
        }
        Ok(())
    })
}

/// The default of `key` as a `toml_edit` value, for writing it explicitly.
pub fn default_edit_value(key: &str) -> Option<toml_edit::Value> {
    find_def(key).and_then(parse_default)
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
    fn invalid_values_fall_back_and_report() {
        let (s, issues) = Settings::from_toml_with_issues(
            "ui_scale = 5\nreduce_motion = \"maybe\"\n[traffic]\nmax_rows = 500\n[editor]\ntab_size = 99\n",
        )
        .unwrap();
        assert_eq!(s.ui_scale, None);
        assert_eq!(s.reduce_motion, None);
        assert_eq!(s.traffic.max_rows, Some(500));
        for k in ["ui_scale", "editor.tab_size", "reduce_motion"] {
            assert!(issues.iter().any(|i| i.key == k), "{k}");
        }
        assert_eq!(issues.len(), 3);
    }

    #[test]
    fn write_key_and_reset_keep_comments_and_unknown_keys() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.toml");
        std::fs::write(
            &p,
            "# mine\nfuture_key = 1\n[editor]\n# tabs\ntab_size = 4\n",
        )
        .unwrap();
        write_key(&p, "editor.soft_wrap", Some(true.into())).unwrap();
        write_key(&p, "ui_scale", Some(1.5.into())).unwrap();
        let t = std::fs::read_to_string(&p).unwrap();
        assert!(t.contains("# mine") && t.contains("# tabs") && t.contains("future_key"));
        assert!(t.contains("soft_wrap = true") && t.contains("ui_scale = 1.5"));
        write_key(&p, "editor.soft_wrap", None).unwrap();
        reset_category(&p, "Appearance").unwrap();
        let t = std::fs::read_to_string(&p).unwrap();
        assert!(!t.contains("soft_wrap") && !t.contains("ui_scale"));
        assert!(t.contains("tab_size = 4") && t.contains("future_key"));
        // Removing a key under a missing table does not create the table.
        write_key(&p, "mcp.transport", None).unwrap();
        assert!(!std::fs::read_to_string(&p).unwrap().contains("[mcp]"));
    }

    #[test]
    fn every_default_is_valid_and_keys_unique() {
        let mut seen = std::collections::HashSet::new();
        for d in SETTING_DEFS {
            assert!(seen.insert(d.key), "duplicate {}", d.key);
            assert!(CATEGORIES.contains(&d.category), "{}", d.key);
            if let Some(v) = default_value(d) {
                check_value(d, &v).unwrap_or_else(|e| panic!("{}: {e}", d.key));
            }
        }
    }

    #[test]
    fn watcher_fires_on_write() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.toml");
        std::fs::write(&p, "").unwrap();
        let (tx, rx) = mpsc::channel();
        let _w = watch(
            std::slice::from_ref(&p),
            Duration::from_millis(30),
            move || {
                let _ = tx.send(());
            },
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        std::fs::write(&p, "ui_font_size = 14\n").unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(3)).is_ok());
    }
}
