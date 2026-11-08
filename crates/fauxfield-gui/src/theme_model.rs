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
    raws.into_iter().map(tokens_from).collect()
}

fn tokens_from(raw: RawTheme) -> Result<ThemeTokens> {
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
    let or = |key: &str, fallback: Rgba8| get(key).unwrap_or(fallback);

    let editor = need("editor.background")?;
    let surface = or("surface.background", editor);
    let title_bar = or("title_bar.background", surface);
    let text = need("text")?;
    let text_muted = or("text.muted", text);
    let border = or("border", surface);
    let accent = or("text.accent", text);
    let string = or("syntax.string", text);
    Ok(ThemeTokens {
        name: raw.name.clone(),
        appearance,
        background: or("background", title_bar),
        title_bar,
        status_bar: or("status_bar.background", title_bar),
        surface,
        panel: or("panel.background", surface),
        elevated: or("elevated_surface.background", surface),
        editor,
        tab_bar: or("tab_bar.background", surface),
        tab_active: or("tab.active_background", editor),
        tab_inactive: or("tab.inactive_background", surface),
        border,
        border_variant: or("border.variant", border),
        border_focused: or("border.focused", accent),
        text,
        text_muted,
        text_placeholder: or("text.placeholder", text_muted),
        text_accent: accent,
        icon_muted: or("icon.muted", text_muted),
        element_hover: or("element.hover", surface),
        element_selected: or("element.selected", surface),
        created: or("created", string),
        modified: or("modified", text),
        deleted: or("deleted", text),
        syntax_function: or("syntax.function", accent),
        syntax_type: or("syntax.type", text),
        syntax_keyword: or("syntax.keyword", text),
        syntax_string: string,
        syntax_constant: or("syntax.constant", or("syntax.number", text)),
        syntax_number: or("syntax.number", text),
        syntax_property: or("syntax.property", text),
        syntax_comment: or("syntax.comment", text_muted),
    })
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
