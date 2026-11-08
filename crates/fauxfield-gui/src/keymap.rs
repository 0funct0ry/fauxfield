//! GPUI-free keymap: the bundled default TOML plus an optional user override (SPEC §9.8).
//! Bindings are keyed by action id (`quick_search`), so the palette, GPUI and the chord machine
//! all read one resolved table.

use anyhow::{Context, Result};
use serde::Deserialize;

pub const DEFAULT_KEYMAP: &str = include_str!("../assets/keymap/default.toml");

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct BindingDef {
    pub action: String,
    pub keys: String,
    #[serde(default)]
    pub context: Option<String>,
    /// Optional `platform = "macos"` or `["windows", "linux"]`; absent means every platform.
    #[serde(default)]
    pub platform: Option<PlatformSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum PlatformSpec {
    One(String),
    Many(Vec<String>),
}

impl PlatformSpec {
    fn names(&self) -> Vec<&str> {
        match self {
            PlatformSpec::One(s) => vec![s.as_str()],
            PlatformSpec::Many(v) => v.iter().map(String::as_str).collect(),
        }
    }
}

/// The platforms a keymap `platform` field can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Macos,
    Windows,
    Linux,
}

impl Platform {
    pub fn current() -> Platform {
        if cfg!(target_os = "macos") {
            Platform::Macos
        } else if cfg!(target_os = "windows") {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Platform::Macos => "macos",
            Platform::Windows => "windows",
            Platform::Linux => "linux",
        }
    }
}

impl BindingDef {
    /// A chord has more than one keystroke, e.g. `secondary-k secondary-t`.
    pub fn is_chord(&self) -> bool {
        self.keys.split_whitespace().count() > 1
    }
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    binding: Vec<BindingDef>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keymap {
    pub bindings: Vec<BindingDef>,
}

impl Keymap {
    /// Parses for the running platform.
    pub fn parse(src: &str) -> Result<Keymap> {
        Keymap::parse_for(src, Platform::current())
    }

    /// Parses and drops bindings whose `platform` does not include `platform`. An unknown
    /// platform name is an error (reported as a keymap problem by `load`), not a panic.
    pub fn parse_for(src: &str, platform: Platform) -> Result<Keymap> {
        let f: File = toml::from_str(src).context("keymap is not valid TOML")?;
        let mut bindings = Vec::new();
        for b in f.binding {
            if let Some(spec) = &b.platform {
                for n in spec.names() {
                    if !matches!(n, "macos" | "windows" | "linux") {
                        anyhow::bail!(
                            "binding for {:?}: unknown platform {n:?} (expected macos, windows or linux)",
                            b.action
                        );
                    }
                }
                if !spec.names().contains(&platform.name()) {
                    continue;
                }
            }
            bindings.push(b);
        }
        Ok(Keymap { bindings })
    }

    pub fn bundled() -> Keymap {
        Keymap::bundled_for(Platform::current())
    }

    pub fn bundled_for(platform: Platform) -> Keymap {
        Keymap::parse_for(DEFAULT_KEYMAP, platform).expect("bundled keymap parses")
    }

    /// Applies a user keymap: an action named there loses its default bindings. An empty `keys`
    /// unbinds the action.
    pub fn merged(mut self, user: Keymap) -> Keymap {
        for u in &user.bindings {
            self.bindings.retain(|b| b.action != u.action);
        }
        self.bindings
            .extend(user.bindings.into_iter().filter(|b| !b.keys.is_empty()));
        self
    }

    /// Bundled keymap plus the optional `keymap.toml` in `config_dir`. A broken user file is
    /// reported and ignored, never fatal.
    pub fn load(config_dir: Option<&std::path::Path>) -> (Keymap, Option<String>) {
        let base = Keymap::bundled();
        let Some(dir) = config_dir else {
            return (base, None);
        };
        let path = dir.join("keymap.toml");
        match std::fs::read_to_string(&path) {
            Err(_) => (base, None),
            Ok(src) => match Keymap::parse(&src) {
                Ok(user) => (base.merged(user), None),
                Err(e) => (base, Some(format!("{}: {e:#}", path.display()))),
            },
        }
    }

    /// The first binding of an action, as written (`secondary-shift-p`).
    pub fn binding_for(&self, action: &str) -> Option<&str> {
        self.bindings
            .iter()
            .find(|b| b.action == action)
            .map(|b| b.keys.as_str())
    }

    pub fn chords(&self) -> impl Iterator<Item = &BindingDef> {
        self.bindings.iter().filter(|b| b.is_chord())
    }
}

/// Renders `secondary-shift-p` for display: `⌘⇧P` on macOS, `Ctrl+Shift+P` elsewhere.
pub fn display_keys(keys: &str, mac: bool) -> String {
    keys.split_whitespace()
        .map(|stroke| display_stroke(stroke, mac))
        .collect::<Vec<_>>()
        .join(" ")
}

fn display_stroke(stroke: &str, mac: bool) -> String {
    let parts: Vec<&str> = stroke.split('-').collect();
    // A trailing empty part means the key itself is `-`.
    let mut out = String::new();
    let mut sep = |s: &str| {
        out.push_str(s);
        if !mac {
            out.push('+');
        }
    };
    let (mods, key) = parts.split_at(parts.len() - 1);
    for m in mods {
        match (*m, mac) {
            ("secondary", true) | ("cmd", true) => sep("⌘"),
            ("secondary", false) | ("ctrl", false) => sep("Ctrl"),
            ("ctrl", true) => sep("⌃"),
            ("shift", true) => sep("⇧"),
            ("shift", false) => sep("Shift"),
            ("alt", true) => sep("⌥"),
            ("alt", false) => sep("Alt"),
            (other, _) => sep(other),
        }
    }
    out.push_str(&match key[0] {
        "enter" => "↵".to_string(),
        "backspace" if mac => "⌫".to_string(),
        "backspace" => "Backspace".to_string(),
        "escape" => "Esc".to_string(),
        k => k.to_uppercase(),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_has_every_spec_binding() {
        let k = Keymap::bundled();
        for (a, keys) in [
            ("quick_search", "secondary-k"),
            ("command_palette", "secondary-shift-p"),
            ("toggle_left_dock", "secondary-b"),
            ("toggle_bottom_dock", "secondary-`"),
            ("toggle_right_dock", "secondary-alt-b"),
            ("show_sidebar_servers", "secondary-alt-1"),
            ("show_sidebar_environments", "secondary-alt-2"),
            ("show_sidebar_datasets", "secondary-alt-3"),
            ("select_theme", "secondary-k secondary-t"),
            ("reset_state", "secondary-shift-backspace"),
        ] {
            assert_eq!(k.binding_for(a), Some(keys), "{a}");
        }
        assert_eq!(k.chords().count(), 1);
    }

    #[test]
    fn user_overrides_replace_and_unbind() {
        let user = Keymap::parse(
            "[[binding]]\naction='quick_search'\nkeys='secondary-p'\n\
             [[binding]]\naction='close_tab'\nkeys=''\n",
        )
        .unwrap();
        let k = Keymap::bundled().merged(user);
        assert_eq!(k.binding_for("quick_search"), Some("secondary-p"));
        assert_eq!(k.binding_for("close_tab"), None);
        assert_eq!(k.binding_for("run_all"), Some("secondary-shift-r"));
    }

    #[test]
    fn broken_user_file_is_reported_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("keymap.toml"), "[[binding").unwrap();
        let (k, err) = Keymap::load(Some(dir.path()));
        assert_eq!(k, Keymap::bundled());
        assert!(err.unwrap().contains("keymap.toml"));
        let (_, none) = Keymap::load(Some(tempfile::tempdir().unwrap().path()));
        assert!(none.is_none());
    }

    #[test]
    fn platform_field_filters_and_rejects_unknown() {
        let src = "[[binding]]\naction='redo'\nkeys='secondary-shift-z'\nplatform='macos'\n\
                   [[binding]]\naction='redo'\nkeys='secondary-y'\nplatform=['windows','linux']\n\
                   [[binding]]\naction='undo'\nkeys='secondary-z'\n";
        let mac = Keymap::parse_for(src, Platform::Macos).unwrap();
        assert_eq!(mac.binding_for("redo"), Some("secondary-shift-z"));
        assert_eq!(mac.bindings.len(), 2);
        let win = Keymap::parse_for(src, Platform::Windows).unwrap();
        assert_eq!(win.binding_for("redo"), Some("secondary-y"));
        let lin = Keymap::parse_for(src, Platform::Linux).unwrap();
        assert_eq!(lin.binding_for("redo"), Some("secondary-y"));
        let bad = "[[binding]]\naction='undo'\nkeys='x'\nplatform='beos'\n";
        let e = Keymap::parse_for(bad, Platform::Macos)
            .unwrap_err()
            .to_string();
        assert!(e.contains("beos"), "{e}");
    }

    #[test]
    fn bad_platform_in_user_file_is_a_problem_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("keymap.toml"),
            "[[binding]]\naction='undo'\nkeys='x'\nplatform='beos'\n",
        )
        .unwrap();
        let (k, err) = Keymap::load(Some(dir.path()));
        assert_eq!(k, Keymap::bundled());
        assert!(err.unwrap().contains("beos"));
    }

    #[test]
    fn no_binding_conflicts_on_any_platform() {
        for p in [Platform::Macos, Platform::Windows, Platform::Linux] {
            let k = Keymap::bundled_for(p);
            let mut seen = std::collections::HashMap::new();
            for b in &k.bindings {
                let key = (b.keys.clone(), b.context.clone());
                if let Some(prev) = seen.insert(key, b.action.clone()) {
                    panic!("{p:?}: {} and {} share {:?}", prev, b.action, b.keys);
                }
            }
        }
        let mac = Keymap::bundled_for(Platform::Macos);
        for (keys, action) in [
            ("cmd-h", "hide_app"),
            ("cmd-m", "minimize_window"),
            ("cmd-alt-h", "hide_others"),
        ] {
            for b in mac.bindings.iter().filter(|b| b.keys == keys) {
                assert_eq!(b.action, action, "{keys} is reserved on macOS");
            }
        }
    }

    #[test]
    fn display_is_platform_aware() {
        assert_eq!(display_keys("secondary-shift-p", true), "⌘⇧P");
        assert_eq!(display_keys("secondary-shift-p", false), "Ctrl+Shift+P");
        assert_eq!(display_keys("secondary-k secondary-t", true), "⌘K ⌘T");
        assert_eq!(display_keys("secondary-enter", true), "⌘↵");
        assert_eq!(display_keys("secondary-alt-b", false), "Ctrl+Alt+B");
    }
}
