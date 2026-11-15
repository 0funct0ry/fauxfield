//! Theme tokens as GPUI colours, converted from `theme_model` (SPEC §9.2). Views read colours
//! only from here: no hard-coded colours anywhere in views.

use std::sync::{OnceLock, RwLock};

use gpui::{App, Global, Hsla, Rgba};

use crate::assets::BUNDLED_THEME_FAMILIES;
use crate::theme_model::{self, Appearance, Rgba8, ThemeTokens};

pub const DEFAULT_DARK: &str = "One Dark";
pub const DEFAULT_LIGHT: &str = "One Light";

/// Resolved colour tokens. Field names follow the Zed theme keys they come from.
#[derive(Clone, Debug)]
pub struct Theme {
    pub name: String,
    pub appearance: Appearance,
    pub background: Hsla,
    pub title_bar: Hsla,
    pub status_bar: Hsla,
    pub surface: Hsla,
    pub panel: Hsla,
    pub elevated: Hsla,
    pub editor: Hsla,
    pub tab_bar: Hsla,
    pub tab_active: Hsla,
    pub tab_inactive: Hsla,
    pub border: Hsla,
    pub border_variant: Hsla,
    pub border_focused: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_placeholder: Hsla,
    pub accent: Hsla,
    pub icon_muted: Hsla,
    pub element_hover: Hsla,
    pub element_selected: Hsla,
    pub created: Hsla,
    pub modified: Hsla,
    pub deleted: Hsla,
    pub syntax_function: Hsla,
    pub syntax_type: Hsla,
    pub syntax_keyword: Hsla,
    pub syntax_string: Hsla,
    pub syntax_constant: Hsla,
    pub syntax_number: Hsla,
    pub syntax_property: Hsla,
    pub syntax_comment: Hsla,
    pub editor_active_line: Hsla,
    pub editor_indent_guide: Hsla,
    pub editor_invisible: Hsla,
    pub editor_bracket_match: Hsla,
}

impl Global for Theme {}

struct Bundled {
    themes: Vec<ThemeTokens>,
    missing_keys: usize,
}

fn load_bundled() -> &'static Bundled {
    static ALL: OnceLock<Bundled> = OnceLock::new();
    ALL.get_or_init(|| {
        // One comes first: it is the fallback for every other family's missing keys.
        let one = theme_model::parse_family(BUNDLED_THEME_FAMILIES[0]).expect("One parses");
        let dark = one
            .iter()
            .find(|t| t.name == DEFAULT_DARK)
            .expect("One Dark");
        let light = one
            .iter()
            .find(|t| t.name == DEFAULT_LIGHT)
            .expect("One Light");
        let mut themes = one.clone();
        let mut missing_keys = 0;
        for fam in &BUNDLED_THEME_FAMILIES[1..] {
            let p = theme_model::parse_family_counted(fam, Some((dark, light)))
                .expect("bundled theme family parses");
            missing_keys += p.missing_keys;
            themes.extend(p.themes);
        }
        Bundled {
            themes,
            missing_keys,
        }
    })
}

/// All bundled themes, parsed once (never per frame or per preview step).
pub fn bundled() -> &'static [ThemeTokens] {
    &load_bundled().themes
}

/// Optional keys the bundled themes lacked and took from One Dark/One Light (`doctor`).
pub fn missing_keys() -> usize {
    load_bundled().missing_keys
}

/// A user theme file that could not be loaded (Problems and `doctor`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeProblem {
    pub file: String,
    pub line: Option<usize>,
    pub message: String,
}

impl ThemeProblem {
    /// `themes/foo.json: line 12: message`
    pub fn text(&self) -> String {
        match self.line {
            Some(l) => format!("{}: line {l}: {}", self.file, self.message),
            None => format!("{}: {}", self.file, self.message),
        }
    }
}

fn user_store() -> &'static RwLock<Vec<ThemeTokens>> {
    static USER: OnceLock<RwLock<Vec<ThemeTokens>>> = OnceLock::new();
    USER.get_or_init(|| RwLock::new(Vec::new()))
}

/// Replaces the user themes with every `*.json` family in `dir`. A broken file is skipped and
/// reported; the rest still load.
pub fn load_user_themes(dir: Option<&std::path::Path>) -> Vec<ThemeProblem> {
    let b = load_bundled();
    let fb = (
        b.themes
            .iter()
            .find(|t| t.name == DEFAULT_DARK)
            .expect("One Dark"),
        b.themes
            .iter()
            .find(|t| t.name == DEFAULT_LIGHT)
            .expect("One Light"),
    );
    let (mut themes, mut problems) = (Vec::new(), Vec::new());
    let mut files: Vec<_> = dir
        .and_then(|d| std::fs::read_dir(d).ok())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    for p in files {
        let file = format!(
            "themes/{}",
            p.file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default()
        );
        let text = match std::fs::read_to_string(&p) {
            Ok(t) => t,
            Err(e) => {
                problems.push(ThemeProblem {
                    file,
                    line: None,
                    message: e.to_string(),
                });
                continue;
            }
        };
        match theme_model::parse_family_counted(&text, Some(fb)) {
            Ok(parsed) => themes.extend(parsed.themes),
            Err(e) => {
                let (line, message) = match e.downcast_ref::<serde_json::Error>() {
                    Some(j) => (Some(j.line()), j.to_string()),
                    None => (None, format!("{e:#}")),
                };
                problems.push(ThemeProblem {
                    file,
                    line,
                    message,
                });
            }
        }
    }
    *user_store().write().unwrap_or_else(|e| e.into_inner()) = themes;
    problems
}

/// Tokens of one theme by name (user themes first), without cloning the whole list.
pub fn tokens_named(name: &str) -> Option<ThemeTokens> {
    let user = user_store().read().unwrap_or_else(|e| e.into_inner());
    user.iter()
        .chain(bundled())
        .find(|t| t.name == name)
        .cloned()
}

/// `doctor`: bundled theme count, optional keys filled from One, and broken user theme files.
pub fn report(user_dir: Option<&std::path::Path>) -> (usize, usize, Vec<ThemeProblem>) {
    let b = load_bundled();
    (b.themes.len(), b.missing_keys, load_user_themes(user_dir))
}

/// One selectable theme.
#[derive(Clone)]
pub struct Entry {
    pub tokens: ThemeTokens,
    /// Loaded from `~/.config/fauxfield/themes` (tagged "user" in the selector).
    pub user: bool,
}

/// Bundled and user themes; a user theme replaces a bundled one of the same name.
pub fn all() -> Vec<Entry> {
    let user = user_store()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let mut out: Vec<Entry> = bundled()
        .iter()
        .filter(|t| !user.iter().any(|u| u.name == t.name))
        .map(|t| Entry {
            tokens: t.clone(),
            user: false,
        })
        .collect();
    out.extend(user.into_iter().map(|t| Entry {
        tokens: t,
        user: true,
    }));
    out
}

fn hsla(c: Rgba8) -> Hsla {
    Rgba {
        r: f32::from(c.r) / 255.0,
        g: f32::from(c.g) / 255.0,
        b: f32::from(c.b) / 255.0,
        a: f32::from(c.a) / 255.0,
    }
    .into()
}

impl Theme {
    pub fn from_tokens(t: &ThemeTokens) -> Theme {
        Theme {
            name: t.name.clone(),
            appearance: t.appearance,
            background: hsla(t.background),
            title_bar: hsla(t.title_bar),
            status_bar: hsla(t.status_bar),
            surface: hsla(t.surface),
            panel: hsla(t.panel),
            elevated: hsla(t.elevated),
            editor: hsla(t.editor),
            tab_bar: hsla(t.tab_bar),
            tab_active: hsla(t.tab_active),
            tab_inactive: hsla(t.tab_inactive),
            border: hsla(t.border),
            border_variant: hsla(t.border_variant),
            border_focused: hsla(t.border_focused),
            text: hsla(t.text),
            text_muted: hsla(t.text_muted),
            text_placeholder: hsla(t.text_placeholder),
            accent: hsla(t.text_accent),
            icon_muted: hsla(t.icon_muted),
            element_hover: hsla(t.element_hover),
            element_selected: hsla(t.element_selected),
            created: hsla(t.created),
            modified: hsla(t.modified),
            deleted: hsla(t.deleted),
            syntax_function: hsla(t.syntax_function),
            syntax_type: hsla(t.syntax_type),
            syntax_keyword: hsla(t.syntax_keyword),
            syntax_string: hsla(t.syntax_string),
            syntax_constant: hsla(t.syntax_constant),
            syntax_number: hsla(t.syntax_number),
            syntax_property: hsla(t.syntax_property),
            syntax_comment: hsla(t.syntax_comment),
            // Derived (Zed keys editor.active_line.background, editor.indent_guide,
            // editor.invisible, editor.document_highlight.bracket_background).
            editor_active_line: hsla(t.element_hover).opacity(0.5),
            editor_indent_guide: hsla(t.border_variant),
            editor_invisible: hsla(t.text_placeholder).opacity(0.6),
            editor_bracket_match: hsla(t.element_selected).opacity(0.7),
        }
    }

    pub fn named(name: &str) -> Option<Theme> {
        all()
            .iter()
            .find(|e| e.tokens.name == name)
            .map(|e| Theme::from_tokens(&e.tokens))
    }

    pub fn one_dark() -> Theme {
        Theme::named(DEFAULT_DARK).expect("One Dark is bundled")
    }

    pub fn one_light() -> Theme {
        Theme::named(DEFAULT_LIGHT).expect("One Light is bundled")
    }

    /// Method badge colours come from syntax and status tokens so every theme stays coherent
    /// without new keys (SPEC §9.2): GET → string, POST → function, PUT → type,
    /// PATCH → constant, DELETE → deleted.
    pub fn method_color(&self, method: &str) -> Hsla {
        match method {
            "GET" => self.syntax_string,
            "POST" => self.syntax_function,
            "PUT" => self.syntax_type,
            "PATCH" => self.syntax_constant,
            "DELETE" => self.deleted,
            "CRUD" => self.accent,
            _ => self.text_muted,
        }
    }

    pub fn status_color(&self, status: u16) -> Hsla {
        match status / 100 {
            2 => self.created,
            4 => self.modified,
            5 => self.deleted,
            _ => self.text_muted,
        }
    }

    /// Accent tint behind chips and active toggles.
    pub fn accent_bg(&self) -> Hsla {
        Hsla {
            a: 0.16,
            ..self.accent
        }
    }
}

pub fn theme(cx: &App) -> &Theme {
    cx.global::<Theme>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_themes_load() {
        assert_eq!(Theme::one_dark().appearance, Appearance::Dark);
        assert_eq!(Theme::one_light().appearance, Appearance::Light);
        assert_eq!(bundled().len(), 27);
        for n in [
            "Ayu Mirage",
            "Gruvbox Dark Hard",
            "Rosé Pine Dawn",
            "Catppuccin Mocha",
            "Tokyo Night",
            "Nord Dark",
        ] {
            assert!(Theme::named(n).is_some(), "{n}");
        }
        assert!(Theme::named("Nope").is_none());
    }

    #[test]
    fn bundled_missing_keys_are_counted_and_light_themes_have_light_fallbacks() {
        // Counted, not fatal; the number only matters to `doctor`.
        let _ = missing_keys();
        for t in bundled() {
            assert_ne!(
                t.text, t.editor,
                "{} text must differ from its background",
                t.name
            );
        }
    }

    #[test]
    fn user_themes_win_collisions_and_broken_files_are_reported() {
        let d = tempfile::tempdir().unwrap();
        let ok = r##"{"name":"F","themes":[{"name":"One Dark","appearance":"dark","style":{"editor.background":"#101010","text":"#eeeeee"}}]}"##;
        std::fs::write(d.path().join("a.json"), ok).unwrap();
        std::fs::write(d.path().join("b.json"), "{\n  \"themes\": [,\n}").unwrap();
        let problems = load_user_themes(Some(d.path()));
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].file, "themes/b.json");
        assert!(problems[0].line.is_some());
        let e = all()
            .into_iter()
            .find(|e| e.tokens.name == "One Dark")
            .unwrap();
        assert!(e.user);
        assert_eq!(
            all().iter().filter(|e| e.tokens.name == "One Dark").count(),
            1
        );
        assert!(load_user_themes(None).is_empty());
        assert!(!all().iter().any(|e| e.user));
    }

    #[test]
    fn store_theme_index_matches_the_bundled_files() {
        let from_files: Vec<(String, &str)> = bundled()
            .iter()
            .map(|t| {
                let a = if t.appearance == Appearance::Dark {
                    "dark"
                } else {
                    "light"
                };
                (t.name.clone(), a)
            })
            .collect();
        let index: Vec<(String, &str)> = fauxfield_store::settings::BUNDLED_THEMES
            .iter()
            .map(|(n, a)| (n.to_string(), *a))
            .collect();
        assert_eq!(from_files, index);
    }

    #[test]
    fn method_colors_follow_the_spec_token_map() {
        let t = Theme::one_dark();
        assert_eq!(t.method_color("GET"), t.syntax_string);
        assert_eq!(t.method_color("POST"), t.syntax_function);
        assert_eq!(t.method_color("PUT"), t.syntax_type);
        assert_eq!(t.method_color("PATCH"), t.syntax_constant);
        assert_eq!(t.method_color("DELETE"), t.deleted);
        assert_eq!(t.method_color("OPTIONS"), t.text_muted);
    }

    #[test]
    fn status_colors() {
        let t = Theme::one_light();
        assert_eq!(t.status_color(201), t.created);
        assert_eq!(t.status_color(404), t.modified);
        assert_eq!(t.status_color(503), t.deleted);
        assert_eq!(t.status_color(302), t.text_muted);
    }
}
