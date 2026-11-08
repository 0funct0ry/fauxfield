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
    pub fn parse(src: &str) -> Result<Keymap> {
        let f: File = toml::from_str(src).context("keymap is not valid TOML")?;
        Ok(Keymap {
            bindings: f.binding,
        })
    }

    pub fn bundled() -> Keymap {
        Keymap::parse(DEFAULT_KEYMAP).expect("bundled keymap parses")
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
    fn display_is_platform_aware() {
        assert_eq!(display_keys("secondary-shift-p", true), "⌘⇧P");
        assert_eq!(display_keys("secondary-shift-p", false), "Ctrl+Shift+P");
        assert_eq!(display_keys("secondary-k secondary-t", true), "⌘K ⌘T");
        assert_eq!(display_keys("secondary-enter", true), "⌘↵");
        assert_eq!(display_keys("secondary-alt-b", false), "Ctrl+Alt+B");
    }
}
