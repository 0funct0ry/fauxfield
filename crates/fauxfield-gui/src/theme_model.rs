//! GPUI-free Zed theme-family parsing (schema v0.2.0, SPEC §9.2). Missing keys fall back to
//! related tokens so partial third-party themes still render.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    /// Parses `#rgb`, `#rrggbb` and `#rrggbbaa`.
    pub fn parse(s: &str) -> Option<Rgba8> {
        let h = s.strip_prefix('#')?;
        let nib = |c: char| c.to_digit(16).map(|d| d as u8);
        let cs: Vec<u8> = h.chars().map(nib).collect::<Option<_>>()?;
        match cs.len() {
            3 => Some(Rgba8 {
                r: cs[0] * 17,
                g: cs[1] * 17,
                b: cs[2] * 17,
                a: 255,
            }),
            6 | 8 => {
                let b = |i: usize| cs[i] * 16 + cs[i + 1];
                Some(Rgba8 {
                    r: b(0),
                    g: b(2),
                    b: b(4),
                    a: if cs.len() == 8 { b(6) } else { 255 },
                })
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    Dark,
    Light,
}

/// Every colour the views may use. Field names follow the Zed keys they come from.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeTokens {
    pub name: String,
    pub appearance: Appearance,
    pub background: Rgba8,
    pub title_bar: Rgba8,
    pub status_bar: Rgba8,
    pub surface: Rgba8,
    pub panel: Rgba8,
    pub elevated: Rgba8,
    pub editor: Rgba8,
    pub tab_bar: Rgba8,
    pub tab_active: Rgba8,
    pub tab_inactive: Rgba8,
    pub border: Rgba8,
    pub border_variant: Rgba8,
    pub border_focused: Rgba8,
    pub text: Rgba8,
    pub text_muted: Rgba8,
    pub text_placeholder: Rgba8,
    pub text_accent: Rgba8,
    pub icon_muted: Rgba8,
    pub element_hover: Rgba8,
    pub element_selected: Rgba8,
    pub created: Rgba8,
    pub modified: Rgba8,
    pub deleted: Rgba8,
    pub syntax_function: Rgba8,
    pub syntax_type: Rgba8,
    pub syntax_keyword: Rgba8,
    pub syntax_string: Rgba8,
    pub syntax_constant: Rgba8,
    pub syntax_number: Rgba8,
    pub syntax_property: Rgba8,
    pub syntax_comment: Rgba8,
}

#[derive(Deserialize)]
struct Family {
    themes: Option<Vec<RawTheme>>,
    // A bare single theme is accepted too.
    name: Option<String>,
    appearance: Option<String>,
    style: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Deserialize)]
struct RawTheme {
    name: String,
    appearance: String,
    style: HashMap<String, serde_json::Value>,
}

/// Parses a theme family (or a single theme) into tokens, in file order.
pub fn parse_family(json: &str) -> Result<Vec<ThemeTokens>> {
    Ok(parse_family_counted(json, None)?.themes)
}

/// A parsed family plus how many optional keys were absent and taken from the fallback theme.
pub struct Parsed {
    pub themes: Vec<ThemeTokens>,
    pub missing_keys: usize,
}

/// Like [`parse_family`], but a missing optional key falls back to the same token of
/// `fallbacks` (One Dark for dark themes, One Light for light ones; SPEC §9.2) and is counted.
/// Without fallbacks the key is derived from other tokens of the same theme.
pub fn parse_family_counted(
    json: &str,
    fallbacks: Option<(&ThemeTokens, &ThemeTokens)>,
) -> Result<Parsed> {
    let fam: Family = serde_json::from_str(json).context("theme file is not valid JSON")?;
    let mut raws = fam.themes.unwrap_or_default();
    if let (Some(name), Some(appearance), Some(style)) = (fam.name, fam.appearance, fam.style) {
        raws.push(RawTheme {
            name,
            appearance,
            style,
        });
    }
    if raws.is_empty() {
        bail!("theme file contains no themes");
    }
    let mut missing_keys = 0;
    let themes = raws
        .into_iter()
        .map(|r| tokens_from(r, fallbacks, &mut missing_keys))
        .collect::<Result<_>>()?;
    Ok(Parsed {
        themes,
        missing_keys,
    })
}

fn tokens_from(
    raw: RawTheme,
    fallbacks: Option<(&ThemeTokens, &ThemeTokens)>,
    missing: &mut usize,
) -> Result<ThemeTokens> {
    let appearance = match raw.appearance.as_str() {
        "dark" => Appearance::Dark,
        "light" => Appearance::Light,
        other => bail!("theme {}: unknown appearance {other:?}", raw.name),
    };
    let style = &raw.style;
    let get = |key: &str| -> Option<Rgba8> {
        let v = match key.strip_prefix("syntax.") {
            Some(k) => style.get("syntax")?.get(k)?.get("color")?,
            None => style.get(key)?,
        };
        Rgba8::parse(v.as_str()?)
    };
    let need = |key: &str| get(key).with_context(|| format!("theme {}: missing {key}", raw.name));
    let one = fallbacks.map(|(dark, light)| match appearance {
        Appearance::Dark => dark,
        Appearance::Light => light,
    });
    let count = std::cell::Cell::new(0usize);
    let or = |key: &str, field: fn(&ThemeTokens) -> Rgba8, chain: Rgba8| {
        get(key).unwrap_or_else(|| match one {
            Some(o) => {
                count.set(count.get() + 1);
                field(o)
            }
            None => chain,
        })
    };

    let editor = need("editor.background")?;
    let surface = or("surface.background", |o| o.surface, editor);
    let title_bar = or("title_bar.background", |o| o.title_bar, surface);
    let text = need("text")?;
    let text_muted = or("text.muted", |o| o.text_muted, text);
    let border = or("border", |o| o.border, surface);
    let accent = or("text.accent", |o| o.text_accent, text);
    let string = or("syntax.string", |o| o.syntax_string, text);
    let result = ThemeTokens {
        name: raw.name.clone(),
        appearance,
        background: or("background", |o| o.background, title_bar),
        title_bar,
        status_bar: or("status_bar.background", |o| o.status_bar, title_bar),
        surface,
        panel: or("panel.background", |o| o.panel, surface),
        elevated: or("elevated_surface.background", |o| o.elevated, surface),
        editor,
        tab_bar: or("tab_bar.background", |o| o.tab_bar, surface),
        tab_active: or("tab.active_background", |o| o.tab_active, editor),
        tab_inactive: or("tab.inactive_background", |o| o.tab_inactive, surface),
        border,
        border_variant: or("border.variant", |o| o.border_variant, border),
        border_focused: or("border.focused", |o| o.border_focused, accent),
        text,
        text_muted,
        text_placeholder: or("text.placeholder", |o| o.text_placeholder, text_muted),
        text_accent: accent,
        icon_muted: or("icon.muted", |o| o.icon_muted, text_muted),
        element_hover: or("element.hover", |o| o.element_hover, surface),
        element_selected: or("element.selected", |o| o.element_selected, surface),
        created: or("created", |o| o.created, string),
        modified: or("modified", |o| o.modified, text),
        deleted: or("deleted", |o| o.deleted, text),
        syntax_function: or("syntax.function", |o| o.syntax_function, accent),
        syntax_type: or("syntax.type", |o| o.syntax_type, text),
        syntax_keyword: or("syntax.keyword", |o| o.syntax_keyword, text),
        syntax_string: string,
        syntax_constant: or(
            "syntax.constant",
            |o| o.syntax_constant,
            or("syntax.number", |o| o.syntax_number, text),
        ),
        syntax_number: or("syntax.number", |o| o.syntax_number, text),
        syntax_property: or("syntax.property", |o| o.syntax_property, text),
        syntax_comment: or("syntax.comment", |o| o.syntax_comment, text_muted),
    };
    *missing += count.get();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = include_str!("../assets/themes/one.json");

    #[test]
    fn parses_hex_forms() {
        assert_eq!(
            Rgba8::parse("#abc"),
            Some(Rgba8 {
                r: 0xaa,
                g: 0xbb,
                b: 0xcc,
                a: 255
            })
        );
        assert_eq!(Rgba8::parse("#11223344").unwrap().a, 0x44);
        assert_eq!(Rgba8::parse("#12345"), None);
        assert_eq!(Rgba8::parse("red"), None);
    }

    #[test]
    fn bundled_family_has_both_one_themes() {
        let t = parse_family(ONE).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].name, "One Dark");
        assert_eq!(t[0].appearance, Appearance::Dark);
        assert_eq!(t[1].name, "One Light");
        assert_eq!(t[1].appearance, Appearance::Light);
        assert_eq!(t[0].border_focused, Rgba8::parse("#47679eff").unwrap());
    }

    #[test]
    fn missing_keys_fall_back_and_required_keys_error() {
        let min = r##"{"name":"M","appearance":"dark","style":{"editor.background":"#000000","text":"#ffffff"}}"##;
        let t = &parse_family(min).unwrap()[0];
        assert_eq!(t.surface, t.editor);
        assert_eq!(t.border_focused, t.text_accent);
        assert!(parse_family(r#"{"name":"M","appearance":"dark","style":{}}"#).is_err());
        assert!(parse_family(r#"{"themes":[]}"#).is_err());
        assert!(parse_family("not json").is_err());
    }
}
