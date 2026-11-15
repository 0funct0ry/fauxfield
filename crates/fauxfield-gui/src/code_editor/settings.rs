//! `[editor]` and `[editor.languages.<lang>]` from `settings.toml` (SPEC §13). Bad values fall
//! back to the default and produce a notice. GPUI-free.

use std::collections::HashMap;

use super::entity::Options;
use super::language_config::Language;

#[derive(Clone, Debug, Default, PartialEq)]
struct Partial {
    tab_size: Option<usize>,
    hard_tabs: Option<bool>,
    soft_wrap: Option<bool>,
    show_whitespace: Option<bool>,
    indent_guides: Option<bool>,
    line_numbers: Option<bool>,
    auto_close: Option<bool>,
    cursor_blink: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditorSettings {
    base: Partial,
    languages: HashMap<String, Partial>,
    /// Pretty-print valid JSON bodies on save (moved here from `layout.json`).
    pub format_json_on_save: bool,
}

fn get_bool(t: &toml::Table, key: &str, scope: &str, notes: &mut Vec<String>) -> Option<bool> {
    match t.get(key)? {
        toml::Value::Boolean(b) => Some(*b),
        _ => {
            notes.push(format!(
                "settings.toml: {scope}{key} must be true or false; using the default"
            ));
            None
        }
    }
}

fn parse_partial(t: &toml::Table, scope: &str, notes: &mut Vec<String>) -> Partial {
    let tab_size = match t.get("tab_size") {
        None => None,
        Some(toml::Value::Integer(n)) if (1..=16).contains(n) => Some(*n as usize),
        Some(_) => {
            notes.push(format!(
                "settings.toml: {scope}tab_size must be 1 to 16; using the default"
            ));
            None
        }
    };
    // show_whitespace: a boolean, or "none" | "selection" | "all".
    let show_whitespace = match t.get("show_whitespace") {
        None => None,
        Some(toml::Value::Boolean(b)) => Some(*b),
        Some(toml::Value::String(s)) if s == "none" => Some(false),
        Some(toml::Value::String(s)) if s == "selection" || s == "all" => Some(true),
        Some(_) => {
            notes.push(format!("settings.toml: {scope}show_whitespace must be none, selection or all; using the default"));
            None
        }
    };
    Partial {
        tab_size,
        hard_tabs: get_bool(t, "hard_tabs", scope, notes),
        soft_wrap: get_bool(t, "soft_wrap", scope, notes),
        show_whitespace,
        indent_guides: get_bool(t, "indent_guides", scope, notes),
        line_numbers: get_bool(t, "line_numbers", scope, notes),
        auto_close: get_bool(t, "auto_close", scope, notes),
        cursor_blink: get_bool(t, "cursor_blink", scope, notes),
    }
}

impl EditorSettings {
    /// Parses the whole `settings.toml` text; returns the settings and notices for bad values.
    pub fn parse(text: &str) -> (EditorSettings, Vec<String>) {
        let mut notes = Vec::new();
        let table: toml::Table = match text.parse() {
            Ok(t) => t,
            Err(e) => {
                return (
                    EditorSettings::default(),
                    vec![format!("settings.toml: {}", e.message())],
                );
            }
        };
        let Some(ed) = table.get("editor").and_then(|v| v.as_table()) else {
            return (EditorSettings::default(), notes);
        };
        let base = parse_partial(ed, "editor.", &mut notes);
        let format_json_on_save =
            get_bool(ed, "format_json_on_save", "editor.", &mut notes).unwrap_or(false);
        let mut languages = HashMap::new();
        if let Some(langs) = ed.get("languages").and_then(|v| v.as_table()) {
            for (name, v) in langs {
                let Some(lang) = Language::from_name(name) else {
                    notes.push(format!(
                        "settings.toml: unknown language {name:?} in [editor.languages]"
                    ));
                    continue;
                };
                if let Some(t) = v.as_table() {
                    languages.insert(
                        lang.name().to_string(),
                        parse_partial(t, &format!("editor.languages.{name}."), &mut notes),
                    );
                }
            }
        }
        (
            EditorSettings {
                base,
                languages,
                format_json_on_save,
            },
            notes,
        )
    }

    /// Reads `settings.toml`; a missing file is the defaults.
    pub fn load(settings_file: Option<&std::path::Path>) -> (EditorSettings, Vec<String>) {
        let Some(file) = settings_file else {
            return (EditorSettings::default(), Vec::new());
        };
        match std::fs::read_to_string(file) {
            Ok(t) => EditorSettings::parse(&t),
            Err(_) => (EditorSettings::default(), Vec::new()),
        }
    }

    /// Effective options for a language: defaults, then `[editor]`, then the language table.
    pub fn options_for(&self, lang: Language) -> Options {
        let mut o = Options::default();
        let lang_p = self.languages.get(lang.name());
        macro_rules! pick {
            ($field:ident) => {
                if let Some(v) = lang_p.and_then(|p| p.$field).or(self.base.$field) {
                    o.$field = v;
                }
            };
        }
        pick!(tab_size);
        pick!(hard_tabs);
        pick!(soft_wrap);
        pick!(show_whitespace);
        pick!(indent_guides);
        pick!(line_numbers);
        pick!(auto_close);
        pick!(cursor_blink);
        o
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_without_a_file() {
        let (s, n) = EditorSettings::parse("");
        assert!(n.is_empty());
        let o = s.options_for(Language::Json);
        assert_eq!(
            (o.tab_size, o.soft_wrap, o.line_numbers, o.auto_close),
            (2, false, true, true)
        );
    }

    #[test]
    fn base_and_language_overrides() {
        let (s, n) = EditorSettings::parse(
            "[editor]\ntab_size = 4\nsoft_wrap = false\nformat_json_on_save = true\n[editor.languages.markdown]\nsoft_wrap = true\nshow_whitespace = \"all\"\n",
        );
        assert!(n.is_empty(), "{n:?}");
        assert!(s.format_json_on_save);
        assert_eq!(s.options_for(Language::Json).tab_size, 4);
        assert!(!s.options_for(Language::Json).soft_wrap);
        assert!(s.options_for(Language::Markdown).soft_wrap);
        assert!(s.options_for(Language::Markdown).show_whitespace);
    }

    #[test]
    fn bad_values_fall_back_with_notices() {
        let (s, n) = EditorSettings::parse(
            "[editor]\ntab_size = 99\nsoft_wrap = \"yes\"\nshow_whitespace = \"lots\"\n[editor.languages.cobol]\ntab_size = 3\n",
        );
        assert_eq!(n.len(), 4, "{n:?}");
        let o = s.options_for(Language::Json);
        assert_eq!((o.tab_size, o.soft_wrap), (2, false));
    }

    #[test]
    fn broken_toml_is_one_notice() {
        let (s, n) = EditorSettings::parse("[editor");
        assert_eq!(n.len(), 1);
        assert_eq!(s, EditorSettings::default());
    }
}
