//! Theme tokens as GPUI colours, converted from `theme_model` (SPEC §9.2). Views read colours
//! only from here: no hard-coded colours anywhere in views.

use std::sync::OnceLock;

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

/// All bundled themes, parsed once (never per frame or per preview step).
pub fn bundled() -> &'static [ThemeTokens] {
    static ALL: OnceLock<Vec<ThemeTokens>> = OnceLock::new();
    ALL.get_or_init(|| {
        BUNDLED_THEME_FAMILIES
            .iter()
            .flat_map(|f| theme_model::parse_family(f).expect("bundled theme family parses"))
            .collect()
    })
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
        bundled()
            .iter()
            .find(|t| t.name == name)
            .map(Theme::from_tokens)
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
        assert_eq!(bundled().len(), 2);
        assert!(Theme::named("Nope").is_none());
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
