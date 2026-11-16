//! The Settings tab (M17.02, SPEC §13, §20.34): rows over `settings.toml` driven by
//! `fauxfield_store::settings::SETTING_DEFS`, and the keymap editor over `keymap.toml`.
//! Writes go through `toml_edit` (comments, order and unknown keys survive); the file watcher
//! brings external edits back into the UI.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use fauxfield_store::settings::{self as st, Apply, SettingDef, SettingKind};
use gpui::{
    AnyElement, App, AppContext, ClipboardItem, Context, Entity, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, ParentElement, StatefulInteractiveElement, Styled, Subscription,
    div, prelude::FluentBuilder, px,
};

use crate::actions;
use crate::chord::{CHORD_TIMEOUT, ChordState};
use crate::combo_box::{ComboEvent, ComboItem, ComboLook, ComboSite, ComboSize, ComboWidth};
use crate::keymap::{self, Keymap, Platform, Source, display_keys};
use crate::search::CommandEntry;
use crate::shell::Shell;
use crate::tabs::TabKind;
use crate::text_input::{InputEvent, TextInput};
use crate::theme::Theme;
use crate::ui::{checkbox_row, eid, icon, switch, toolbar_button};
use crate::view::Problem;

pub const SETTINGS_TAB_ID: &str = "settings";
const KEYMAP_CATEGORY: &str = "Keymap";
const FONTS: &[&str] = &[
    "IBM Plex Sans",
    "IBM Plex Mono",
    "JetBrains Mono",
    "Menlo",
    "Monaco",
    "Consolas",
    "SF Mono",
    "Helvetica Neue",
    "Segoe UI",
    "system-ui",
];
const CONTEXTS: &[&str] = &[
    "Shell",
    "Tree",
    "List",
    "Table",
    "Editor",
    "CodeEditor",
    "Picker",
];

/// What a conflicting recording does on Save.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Unresolved,
    /// The other actions lose this binding.
    Replace,
    /// Both keep it; the new binding gets a context.
    AllowBoth,
}

/// The record-binding modal.
pub struct RecordDialog {
    pub action: String,
    pub strokes: Vec<String>,
    pub last: Option<Instant>,
    pub conflicts: Vec<(String, String)>,
    pub resolution: Resolution,
    pub context: String,
}

pub struct SettingsState {
    pub category: usize,
    pub search: Entity<TextInput>,
    pub keymap_search: Entity<TextInput>,
    pub inputs: RefCell<HashMap<String, Entity<TextInput>>>,
    pub focus_key: Option<String>,
    pub errors: HashMap<String, String>,
    pub raw: toml::Table,
    pub record: Option<RecordDialog>,
    pub subs: RefCell<Vec<Subscription>>,
}

/// A resolved keymap with everything derived from it.
pub(crate) struct KeymapBuild {
    pub keymap: Keymap,
    pub sources: Vec<Source>,
    pub issues: Vec<String>,
    pub chord: ChordState,
    pub commands: Vec<CommandEntry>,
}

/// Layers defaults < vim preset < `keymap.toml`, registers it with GPUI and builds the chord
/// machine and palette entries. Used at start-up and on every keymap hot reload.
pub(crate) fn resolve_keymap(config_dir: Option<&Path>, vim: bool, cx: &mut App) -> KeymapBuild {
    let src = config_dir.and_then(|d| std::fs::read_to_string(d.join("keymap.toml")).ok());
    let (keymap, sources, issues) =
        Keymap::layered(Platform::current(), vim, src.as_deref(), actions::is_known);
    let mut issues: Vec<String> = issues.into_iter().map(|i| i.message).collect();
    issues.extend(actions::install(cx, &keymap));
    let chords: Vec<(String, String)> = keymap
        .chords()
        .filter_map(|b| {
            let keys: Option<Vec<String>> =
                b.keys.split_whitespace().map(actions::canonical).collect();
            Some((keys?.join(" "), b.action.clone()))
        })
        .collect();
    let prefixes: Vec<&str> = chords
        .iter()
        .filter_map(|(k, _)| k.split_whitespace().next())
        .collect();
    let singles: Vec<(String, String)> = keymap
        .bindings
        .iter()
        .filter(|b| !b.is_chord())
        .filter_map(|b| Some((actions::canonical(&b.keys)?, b.action.clone())))
        .filter(|(k, _)| prefixes.contains(&k.as_str()))
        .collect();
    KeymapBuild {
        chord: ChordState::new(&chords, &singles),
        commands: actions::commands(&keymap),
        keymap,
        sources,
        issues,
    }
}

pub(crate) fn config_problem(code: &str, message: String, file: &Path) -> Problem {
    Problem {
        warning: true,
        code: code.into(),
        message,
        file: file.display().to_string(),
        path: String::new(),
        target: None,
    }
}

fn toml_to_edit(v: &toml::Value) -> Option<toml_edit::Value> {
    use toml::Value as V;
    Some(match v {
        V::Boolean(b) => (*b).into(),
        V::Integer(i) => (*i).into(),
        V::Float(f) => (*f).into(),
        V::String(s) => s.as_str().into(),
        V::Array(a) => {
            let mut arr = toml_edit::Array::new();
            for x in a {
                arr.push(toml_to_edit(x)?);
            }
            toml_edit::Value::Array(arr)
        }
        _ => return None,
    })
}

fn display_value(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Float(f) => format!("{f}"),
        other => other.to_string(),
    }
}

/// The current value of `def` as shown: the file's, else the default.
fn current(raw: &toml::Table, def: &SettingDef) -> Option<toml::Value> {
    let mut cur: Option<&toml::Value> = None;
    let mut table = raw;
    let parts: Vec<&str> = def.key.split('.').collect();
    for (i, p) in parts.iter().enumerate() {
        match table.get(*p) {
            Some(v) if i + 1 == parts.len() => cur = Some(v),
            Some(toml::Value::Table(t)) => table = t,
            _ => break,
        }
    }
    let v = cur.cloned().or_else(|| st::default_value(def))?;
    // `show_whitespace` may be a bool in the file.
    Some(match (def.key, v) {
        ("editor.show_whitespace", toml::Value::Boolean(b)) => {
            toml::Value::String(if b { "all" } else { "none" }.into())
        }
        (_, v) => v,
    })
}

fn visible_defs(search: &str, category: &str) -> Vec<&'static SettingDef> {
    let q = search.trim().to_lowercase();
    st::SETTING_DEFS
        .iter()
        .filter(|d| !d.hidden)
        .filter(|d| {
            if q.is_empty() {
                d.category == category
            } else {
                d.label.to_lowercase().contains(&q)
                    || d.description.to_lowercase().contains(&q)
                    || d.key.to_lowercase().contains(&q)
            }
        })
        .collect()
}

/// Label with the first case-insensitive match of `q` in the accent colour.
fn highlighted(t: &Theme, text: &str, q: &str, size: f32, color: gpui::Hsla) -> AnyElement {
    let q = q.trim().to_lowercase();
    let lower = text.to_lowercase();
    let pos = (!q.is_empty() && lower.len() == text.len())
        .then(|| lower.find(&q))
        .flatten()
        .filter(|p| text.is_char_boundary(*p) && text.is_char_boundary(p + q.len()));
    let row = div()
        .flex()
        .flex_row()
        .text_size(px(size))
        .text_color(color);
    match pos {
        Some(p) => row
            .child(text[..p].to_string())
            .child(
                div()
                    .text_color(t.accent)
                    .child(text[p..p + q.len()].to_string()),
            )
            .child(text[p + q.len()..].to_string())
            .into_any_element(),
        None => row.child(text.to_string()).into_any_element(),
    }
}

/// `secondary-shift-p` from a GPUI keystroke; `None` for a bare modifier press.
pub fn stroke_string(ev: &KeyDownEvent) -> Option<String> {
    let ks = &ev.keystroke;
    let key = ks.key.as_str();
    if matches!(
        key,
        "shift" | "control" | "alt" | "platform" | "function" | ""
    ) {
        return None;
    }
    let m = &ks.modifiers;
    let mut out = String::new();
    let (secondary, ctrl) = if cfg!(target_os = "macos") {
        (m.platform, m.control)
    } else {
        (m.control, false)
    };
    if secondary {
        out.push_str("secondary-");
    }
    if ctrl {
        out.push_str("ctrl-");
    }
    if m.alt {
        out.push_str("alt-");
    }
    if m.shift {
        out.push_str("shift-");
    }
    out.push_str(key);
    Some(out)
}

impl Shell {
    // ---- state ------------------------------------------------------------------------

    pub(crate) fn ensure_settings(&mut self, cx: &mut Context<Self>) {
        if self.settings.is_some() {
            return;
        }
        let search = cx.new(|cx| {
            TextInput::new(cx)
                .placeholder("Search settings")
                .cleanable(true)
        });
        let keymap_search = cx.new(|cx| {
            TextInput::new(cx)
                .placeholder("Search actions and bindings")
                .cleanable(true)
        });
        let mut subs = Vec::new();
        for input in [&search, &keymap_search] {
            subs.push(cx.subscribe(input, |_, _, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    cx.notify();
                }
            }));
        }
        self.settings = Some(SettingsState {
            category: 0,
            search,
            keymap_search,
            inputs: RefCell::default(),
            focus_key: None,
            errors: HashMap::new(),
            raw: self.read_settings_table(),
            record: None,
            subs: RefCell::new(subs),
        });
    }

    fn read_settings_table(&self) -> toml::Table {
        self.settings_path
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| t.parse().ok())
            .unwrap_or_default()
    }

    /// Re-reads settings.toml into the tab (after a write or an external edit) and drops the
    /// text inputs' stale errors.
    pub(crate) fn refresh_settings_view(&mut self, cx: &mut Context<Self>) {
        let raw = self.read_settings_table();
        if let Some(s) = self.settings.as_mut() {
            s.raw = raw;
            s.errors.clear();
            // Text inputs re-sync from the file the next time they render.
            let inputs: Vec<_> = s.inputs.borrow().values().cloned().collect();
            for i in inputs {
                i.update(cx, |_, cx| cx.notify());
            }
        }
    }

    pub fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.ensure_settings(cx);
        self.tabs
            .open_pinned(SETTINGS_TAB_ID, TabKind::Settings, "Settings");
        self.save_layout();
        cx.notify();
    }

    pub fn settings_focus_search(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        self.open_settings(cx);
        if let Some(s) = &self.settings {
            s.search.read(cx).focus(window);
        }
    }

    pub fn settings_open_file(&mut self, cx: &mut Context<Self>) {
        let Some(p) = self.settings_path.clone() else {
            self.notify("There is no settings file in this session.".into(), cx);
            return;
        };
        self.open_config_file(&p, cx);
    }

    pub fn keymap_open_file(&mut self, cx: &mut Context<Self>) {
        let Some(p) = self.keymap_file() else {
            self.notify("There is no keymap file in this session.".into(), cx);
            return;
        };
        self.open_config_file(&p, cx);
    }

    fn open_config_file(&mut self, p: &Path, cx: &mut Context<Self>) {
        if !p.exists() {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            if std::fs::write(p, "").is_err() {
                self.notify(format!("Cannot create {}", p.display()), cx);
                return;
            }
        }
        cx.open_url(&format!("file://{}", p.display()));
    }

    pub(crate) fn keymap_file(&self) -> Option<PathBuf> {
        self.config_dir.as_ref().map(|d| d.join("keymap.toml"))
    }

    // ---- writes -----------------------------------------------------------------------

    /// Set or reset (`None`) one key, then apply it.
    pub fn settings_set(&mut self, key: &str, val: Option<toml::Value>, cx: &mut Context<Self>) {
        let Some(path) = self.settings_path.clone() else {
            self.notify("There is no settings file in this session.".into(), cx);
            return;
        };
        let edit = val.as_ref().and_then(toml_to_edit);
        if val.is_some() && edit.is_none() {
            return;
        }
        match st::write_key(&path, key, edit) {
            Ok(()) => {
                self.reload_settings(cx);
                self.refresh_settings_view(cx);
            }
            Err(e) => self.notify(e, cx),
        }
    }

    pub fn settings_reset_section(&mut self, cx: &mut Context<Self>) {
        let Some(cat) = self.settings_category() else {
            return;
        };
        let Some(path) = self.settings_path.clone() else {
            return;
        };
        match st::reset_category(&path, cat) {
            Ok(()) => {
                self.reload_settings(cx);
                self.refresh_settings_view(cx);
            }
            Err(e) => self.notify(e, cx),
        }
    }

    fn settings_category(&self) -> Option<&'static str> {
        self.settings
            .as_ref()
            .and_then(|s| st::CATEGORIES.get(s.category).copied())
    }

    /// Commit a text input row (Enter or blur): parse, validate, write or show the error.
    fn settings_commit_input(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(def) = st::find_def(key) else { return };
        let Some(input) = self
            .settings
            .as_ref()
            .and_then(|s| s.inputs.borrow().get(key).cloned())
        else {
            return;
        };
        let text = input.read(cx).value().trim().to_string();
        if text.is_empty() {
            if let Some(s) = self.settings.as_mut() {
                s.errors.remove(key);
            }
            self.settings_set(key, None, cx);
            return;
        }
        let parsed = match def.kind {
            SettingKind::Int { .. } => text
                .parse::<i64>()
                .map(toml::Value::Integer)
                .map_err(|_| "must be a whole number".to_string()),
            SettingKind::Float { .. } => text
                .parse::<f64>()
                .map(toml::Value::Float)
                .map_err(|_| "must be a number".to_string()),
            _ => Ok(toml::Value::String(text)),
        };
        let checked = parsed.and_then(|v| st::check_value(def, &v).map(|()| v));
        match checked {
            Ok(v) => {
                if let Some(s) = self.settings.as_mut() {
                    s.errors.remove(key);
                }
                self.settings_set(key, Some(v), cx);
            }
            Err(msg) => {
                if let Some(s) = self.settings.as_mut() {
                    s.errors.insert(key.to_string(), msg);
                }
                cx.notify();
            }
        }
    }

    // ---- rendering --------------------------------------------------------------------

    pub(crate) fn render_settings_tab(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(s) = self.settings.as_ref() else {
            return div().size_full().into_any_element();
        };
        let query = s.search.read(cx).value().to_string();
        let searching = !query.trim().is_empty();
        let category = st::CATEGORIES
            .get(s.category)
            .copied()
            .unwrap_or("Appearance");

        // Category list.
        let mut cats = div()
            .w(px(168.))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(px(8.))
            .border_r_1()
            .border_color(t.border);
        for (i, c) in st::CATEGORIES.iter().enumerate() {
            let on = !searching && i == s.category;
            cats = cats.child(crate::dev_id!(
                div()
                    .id(eid(format!("settings-cat-{i}")))
                    .h(px(24.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .text_color(t.text)
                    .hover(|d| d.bg(t.element_hover))
                    .when(on, |d| d.bg(t.element_selected))
                    .child(*c)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(s) = this.settings.as_mut() {
                            s.category = i;
                        }
                        cx.notify();
                    })),
                format!("settings.category.{i}"),
                Button,
                *c,
                Press(format!("set-cat:{i}"))
            ));
        }

        // Rows.
        let defs = visible_defs(&query, category);
        let mut rows = div().flex().flex_col().gap(px(2.));
        for d in &defs {
            rows = rows.child(self.render_setting_row(t, s, d, &query, cx));
        }
        if defs.is_empty() {
            rows = rows.child(crate::dev_id!(
                div()
                    .id("settings-empty")
                    .p(px(16.))
                    .text_size(px(12.))
                    .text_color(t.text_muted)
                    .child(format!("No settings match \"{}\".", query.trim())),
                "settings.empty",
                Text,
                "No settings match",
                None
            ));
        }

        let in_keymap = !searching && category == KEYMAP_CATEGORY;
        let issues = self
            .problems
            .iter()
            .filter(|p| p.code == "settings.invalid")
            .count();

        let footer = div()
            .flex()
            .flex_row()
            .gap(px(8.))
            .pt(px(8.))
            .child(crate::dev_id!(
                toolbar_button(t, "settings-open-file")
                    .h(px(24.))
                    .px(px(10.))
                    .child("Open settings.toml")
                    .on_click(cx.listener(|this, _, _, cx| this.settings_open_file(cx))),
                "settings.open_file",
                Button,
                "Open settings.toml",
                Action("settings_open_file")
            ))
            .child(crate::dev_id!(
                toolbar_button(t, "settings-reset-section")
                    .h(px(24.))
                    .px(px(10.))
                    .child("Reset section")
                    .on_click(cx.listener(|this, _, _, cx| this.settings_reset_section(cx))),
                "settings.reset_section",
                Button,
                "Reset section",
                Action("settings_reset_section")
            ))
            .when(issues > 0, |d| {
                d.child(
                    div()
                        .text_size(px(12.))
                        .text_color(t.deleted)
                        .child(format!("{issues} invalid value(s): defaults in effect")),
                )
            });

        let body = div()
            .id("settings-body")
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_y_scroll()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().w(px(360.)).child(crate::dev_id!(
                div().id("settings-search").child(s.search.clone()),
                "settings.search",
                Input,
                "Search settings",
                None
            )))
            .child(
                div()
                    .text_size(px(14.))
                    .text_color(t.text)
                    .child(if searching {
                        "Search results".to_string()
                    } else {
                        category.to_string()
                    }),
            )
            .child(rows)
            .when(in_keymap, |d| d.child(self.render_keymap_editor(t, s, cx)))
            .child(footer);

        crate::dev_id!(
            div()
                .id("settings-tab")
                .size_full()
                .flex()
                .flex_row()
                .bg(t.editor)
                .child(cats)
                .child(body),
            "settings.tab",
            Panel,
            "Settings",
            None
        )
        .into_any_element()
    }

    fn render_setting_row(
        &self,
        t: &Theme,
        s: &SettingsState,
        d: &'static SettingDef,
        query: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cur = current(&s.raw, d);
        let modified = st::is_modified(&s.raw, d);
        let key = d.key;
        let error = s.errors.get(key).cloned();

        let control: AnyElement = match d.kind {
            SettingKind::Bool => {
                let on = cur.as_ref().and_then(toml::Value::as_bool).unwrap_or(false);
                if d.disabled {
                    switch(t, on).opacity(0.5).into_any_element()
                } else {
                    crate::dev_id!(
                        switch(t, on)
                            .id(eid(format!("set-{key}")))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.settings_set(key, Some(toml::Value::Boolean(!on)), cx)
                            })),
                        format!("settings.{key}"),
                        Button,
                        d.label,
                        Press(format!("set-toggle:{key}"))
                    )
                    .into_any_element()
                }
            }
            SettingKind::Enum(opts) => {
                let items = opts.iter().map(|o| ComboItem::new(*o, *o)).collect();
                self.setting_combo(cx, key, items, cur.as_ref())
            }
            SettingKind::Font => {
                let mut names: Vec<String> = FONTS.iter().map(|f| f.to_string()).collect();
                if let Some(c) = cur.as_ref().map(display_value)
                    && !names.contains(&c)
                {
                    names.insert(0, c);
                }
                let items = names
                    .iter()
                    .map(|n| ComboItem::new(n.clone(), n.clone()))
                    .collect();
                self.setting_combo(cx, key, items, cur.as_ref())
            }
            SettingKind::Text if key.starts_with("theme.") => {
                let want = if key == "theme.light" {
                    "light"
                } else {
                    "dark"
                };
                let items = st::BUNDLED_THEMES
                    .iter()
                    .filter(|(_, a)| *a == want)
                    .map(|(n, _)| ComboItem::new(*n, *n))
                    .collect();
                self.setting_combo(cx, key, items, cur.as_ref())
            }
            SettingKind::Multi(opts) => {
                let on: Vec<String> = cur
                    .as_ref()
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                let mut col = div().flex().flex_row().gap(px(8.));
                for o in opts {
                    let checked = on.iter().any(|x| x == o);
                    let next: Vec<toml::Value> = opts
                        .iter()
                        .filter(|x| (**x == *o) != on.iter().any(|y| y == **x))
                        .map(|x| toml::Value::String((*x).into()))
                        .collect();
                    col = col.child(crate::dev_id!(
                        checkbox_row(t, eid(format!("set-{key}-{o}")), checked, *o).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.settings_set(key, Some(toml::Value::Array(next.clone())), cx)
                            })
                        ),
                        format!("settings.{key}.{o}"),
                        Button,
                        *o,
                        None
                    ));
                }
                col.into_any_element()
            }
            SettingKind::Int { .. }
            | SettingKind::Float { .. }
            | SettingKind::Text
            | SettingKind::Path => self.setting_text(t, s, d, cur.as_ref(), error.is_some(), cx),
        };

        let reset = crate::dev_id!(
            div()
                .id(eid(format!("set-reset-{key}")))
                .size(px(18.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|d| d.bg(t.element_hover))
                .child(icon("rotate-ccw", 12., t.icon_muted))
                .on_click(cx.listener(move |this, _, _, cx| this.settings_set(key, None, cx))),
            format!("settings.reset.{key}"),
            Button,
            format!("Reset {}", d.label),
            Press(format!("set-reset:{key}"))
        );

        let note = match d.apply {
            Apply::NewSessions => Some("Applies to new sessions."),
            Apply::Instant => None,
        };

        div()
            .id(eid(format!("setting-{key}")))
            .flex()
            .flex_row()
            .items_start()
            .gap(px(12.))
            .py(px(6.))
            .border_b_1()
            .border_color(t.border_variant)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.))
                            .child(highlighted(t, d.label, query, 13., t.text))
                            .when(modified, |r| {
                                r.child(crate::dev_id!(
                                    div().size(px(6.)).rounded(px(3.)).bg(t.accent),
                                    format!("settings.modified.{key}"),
                                    Text,
                                    "modified",
                                    None
                                ))
                            }),
                    )
                    .child(highlighted(t, d.description, query, 12., t.text_muted))
                    .child(highlighted(t, key, query, 11., t.text_placeholder))
                    .when_some(note, |c, n| {
                        c.child(div().text_size(px(11.)).text_color(t.text_muted).child(n))
                    })
                    .when_some(error, |c, e| {
                        c.child(crate::dev_id!(
                            div()
                                .id(eid(format!("set-error-{key}")))
                                .text_size(px(12.))
                                .text_color(t.deleted)
                                .child(e.clone()),
                            format!("settings.error.{key}"),
                            Text,
                            e,
                            None
                        ))
                    }),
            )
            .child(div().w(px(220.)).flex_none().child(control))
            .child(reset)
            .into_any_element()
    }

    fn setting_combo(
        &self,
        cx: &mut Context<Self>,
        key: &'static str,
        items: Vec<ComboItem>,
        cur: Option<&toml::Value>,
    ) -> AnyElement {
        let v = cur.map(display_value);
        self.combo_field(
            cx,
            ComboSite {
                key: format!("set-{key}"),
                items,
                value: v.clone().map(Into::into),
                fallback: v.map(Into::into),
                color: None,
                tip: None,
                width: ComboWidth::Fixed(210.),
                look: ComboLook::Field,
                size: ComboSize::Small,
                on_event: std::rc::Rc::new(move |this, ev, cx| {
                    if let ComboEvent::Change(v) = ev {
                        this.settings_set(key, Some(toml::Value::String(v.to_string())), cx);
                    }
                }),
            },
        )
    }

    fn setting_text(
        &self,
        _t: &Theme,
        s: &SettingsState,
        d: &'static SettingDef,
        cur: Option<&toml::Value>,
        invalid: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = d.key;
        let shown = cur.map(display_value).unwrap_or_default();
        let existing = s.inputs.borrow().get(key).cloned();
        let input = match existing {
            Some(i) => i,
            None => {
                let default = st::default_value(d)
                    .map(|v| display_value(&v))
                    .unwrap_or_default();
                let i = cx.new(|cx| {
                    TextInput::new(cx)
                        .text(&shown)
                        .placeholder(default)
                        .size(crate::text_input::InputSize::Small)
                });
                let sub = cx.subscribe(
                    &i,
                    move |this: &mut Shell, _, ev: &InputEvent, cx| match ev {
                        InputEvent::PressEnter { .. } => this.settings_commit_input(key, cx),
                        InputEvent::Blur => {
                            if let Some(s) = this.settings.as_mut()
                                && s.focus_key.as_deref() == Some(key)
                            {
                                s.focus_key = None;
                            }
                            this.settings_commit_input(key, cx);
                        }
                        InputEvent::Focus => {
                            if let Some(s) = this.settings.as_mut() {
                                s.focus_key = Some(key.to_string());
                            }
                        }
                        _ => {}
                    },
                );
                s.subs.borrow_mut().push(sub);
                s.inputs.borrow_mut().insert(key.to_string(), i.clone());
                i
            }
        };
        // Follow the file unless the user is typing in this row.
        if s.focus_key.as_deref() != Some(key) && input.read(cx).value() != shown {
            input.update(cx, |i, cx| i.set_value(&shown, cx));
        }
        input.update(cx, |i, cx| i.set_invalid(invalid, cx));
        crate::dev_id!(
            div().id(eid(format!("set-{key}"))).child(input),
            format!("settings.{key}"),
            Input,
            d.label,
            None
        )
        .into_any_element()
    }

    // ---- keymap editor ----------------------------------------------------------------

    fn render_keymap_editor(
        &self,
        t: &Theme,
        s: &SettingsState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let q = s.keymap_search.read(cx).value().trim().to_lowercase();
        let mac = cfg!(target_os = "macos");
        let mut rows = div().flex().flex_col();
        let mut shown = 0usize;
        for id in actions::all_ids() {
            let label = actions::label_of(id);
            let binds: Vec<(usize, &keymap::BindingDef)> = self
                .keymap
                .bindings
                .iter()
                .enumerate()
                .filter(|(_, b)| b.action == id)
                .collect();
            let keys_text = binds
                .iter()
                .map(|(_, b)| display_keys(&b.keys, mac))
                .collect::<Vec<_>>()
                .join(", ");
            if !q.is_empty()
                && !label.to_lowercase().contains(&q)
                && !id.contains(&q)
                && !keys_text.to_lowercase().contains(&q)
            {
                continue;
            }
            shown += 1;
            let source = binds
                .iter()
                .map(|(i, _)| self.keymap_sources.get(*i).map_or("default", |s| s.label()))
                .fold(Vec::<&str>::new(), |mut a, x| {
                    if !a.contains(&x) {
                        a.push(x);
                    }
                    a
                })
                .join(", ");
            let user = binds
                .iter()
                .any(|(i, _)| self.keymap_sources.get(*i) == Some(&Source::User));
            let copy_text = format!(
                "[[binding]]\naction = \"{id}\"\nkeys = \"{}\"\n",
                binds.first().map(|(_, b)| b.keys.as_str()).unwrap_or("")
            );
            let small = |name: String, text: &'static str| {
                toolbar_button(t, eid(name))
                    .h(px(20.))
                    .px(px(6.))
                    .child(text)
            };
            rows = rows.child(
                div()
                    .id(eid(format!("keymap-row-{id}")))
                    .h(px(28.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .border_b_1()
                    .border_color(t.border_variant)
                    .child(
                        div()
                            .w(px(260.))
                            .truncate()
                            .text_size(px(12.))
                            .text_color(t.text)
                            .child(label),
                    )
                    .child(
                        div()
                            .w(px(150.))
                            .truncate()
                            .text_size(px(12.))
                            .text_color(if keys_text.is_empty() {
                                t.text_placeholder
                            } else {
                                t.text
                            })
                            .child(if keys_text.is_empty() {
                                "none".to_string()
                            } else {
                                keys_text
                            }),
                    )
                    .child(
                        div()
                            .w(px(90.))
                            .text_size(px(11.))
                            .text_color(t.text_muted)
                            .child(if source.is_empty() {
                                "-".to_string()
                            } else {
                                source
                            }),
                    )
                    .child(crate::dev_id!(
                        small(format!("keymap-rec-{id}"), "Record").on_click(
                            cx.listener(move |this, _, _, cx| this.keymap_start_record(id, cx))
                        ),
                        format!("keymap.record.{id}"),
                        Button,
                        "Record",
                        Press(format!("km-record:{id}"))
                    ))
                    .child(crate::dev_id!(
                        small(format!("keymap-clear-{id}"), "Clear").on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.keymap_write(id, &[(String::new(), None)], cx)
                            }
                        )),
                        format!("keymap.clear.{id}"),
                        Button,
                        "Clear",
                        Press(format!("km-clear:{id}"))
                    ))
                    .when(user, |r| {
                        r.child(crate::dev_id!(
                            small(format!("keymap-reset-{id}"), "Reset").on_click(
                                cx.listener(move |this, _, _, cx| this.keymap_write(id, &[], cx))
                            ),
                            format!("keymap.reset.{id}"),
                            Button,
                            "Reset",
                            Press(format!("km-reset:{id}"))
                        ))
                    })
                    .child(crate::dev_id!(
                        small(format!("keymap-copy-{id}"), "Copy").on_click(cx.listener(
                            move |this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                                this.notify("Binding copied".into(), cx);
                            }
                        )),
                        format!("keymap.copy.{id}"),
                        Button,
                        "Copy",
                        None
                    )),
            );
        }
        if shown == 0 {
            rows = rows.child(
                div()
                    .p(px(12.))
                    .text_size(px(12.))
                    .text_color(t.text_muted)
                    .child("No actions match."),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .pt(px(12.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .text_color(t.text)
                            .child("Keybindings"),
                    )
                    .child(div().w(px(280.)).child(crate::dev_id!(
                        div().id("keymap-search").child(s.keymap_search.clone()),
                        "keymap.search",
                        Input,
                        "Search actions and bindings",
                        None
                    )))
                    .child(crate::dev_id!(
                        toolbar_button(t, "keymap-open-file")
                            .h(px(24.))
                            .px(px(10.))
                            .child("Open keymap.toml")
                            .on_click(cx.listener(|this, _, _, cx| this.keymap_open_file(cx))),
                        "keymap.open_file",
                        Button,
                        "Open keymap.toml",
                        Action("keymap_open_file")
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(8.))
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child(div().w(px(260.)).child("Action"))
                    .child(div().w(px(150.)).child("Binding"))
                    .child(div().w(px(90.)).child("Source")),
            )
            .child(rows)
            .into_any_element()
    }

    // ---- keymap writes and recording --------------------------------------------------

    /// Replace the user entries of `action` in keymap.toml (empty slice resets it) and reload.
    pub(crate) fn keymap_write(
        &mut self,
        action: &str,
        entries: &[(String, Option<String>)],
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.keymap_file() else {
            self.notify("There is no keymap file in this session.".into(), cx);
            return;
        };
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        match keymap::write_user_bindings(&text, action, entries) {
            Ok(out) => {
                if let Some(d) = path.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                if let Err(e) = std::fs::write(&path, out) {
                    self.notify(format!("Cannot write {}: {e}", path.display()), cx);
                    return;
                }
                self.reload_keymap(cx);
            }
            Err(e) => self.notify(format!("{e:#}"), cx),
        }
    }

    /// Rebuild the keymap, chord machine, palette entries and Problems from the files.
    pub(crate) fn reload_keymap(&mut self, cx: &mut Context<Self>) {
        let b = resolve_keymap(self.config_dir.as_deref(), self.vim_list, cx);
        self.keymap = b.keymap;
        self.keymap_sources = b.sources;
        self.chord = b.chord;
        self.commands = b.commands;
        let file = self.keymap_file().unwrap_or_default();
        self.keymap_problems = b
            .issues
            .into_iter()
            .map(|m| config_problem("keymap.invalid", m, &file))
            .collect();
        self.rebuild_config_problems();
        cx.notify();
    }

    pub fn keymap_start_record(&mut self, action: &str, cx: &mut Context<Self>) {
        self.ensure_settings(cx);
        if let Some(s) = self.settings.as_mut() {
            s.record = Some(RecordDialog {
                action: action.to_string(),
                strokes: Vec::new(),
                last: None,
                conflicts: Vec::new(),
                resolution: Resolution::Unresolved,
                context: "Shell".into(),
            });
        }
        cx.notify();
    }

    pub fn keymap_cancel_record(&mut self, cx: &mut Context<Self>) {
        if let Some(s) = self.settings.as_mut() {
            s.record = None;
        }
        cx.notify();
    }

    /// A key press while the record modal is open. Returns whether it was consumed.
    pub(crate) fn record_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        if self.settings.as_ref().is_none_or(|s| s.record.is_none()) {
            return false;
        }
        let m = &ev.keystroke.modifiers;
        let bare = !m.platform && !m.control && !m.alt && !m.shift;
        if bare && ev.keystroke.key == "escape" {
            self.keymap_cancel_record(cx);
            return true;
        }
        let Some(stroke) = stroke_string(ev) else {
            return true;
        };
        self.record_stroke(stroke, Instant::now(), cx);
        true
    }

    /// Adds a stroke; a second stroke within the chord timeout makes a chord.
    pub(crate) fn record_stroke(&mut self, stroke: String, now: Instant, cx: &mut Context<Self>) {
        let Some(rec) = self.settings.as_mut().and_then(|s| s.record.as_mut()) else {
            return;
        };
        let chord_ok = rec.strokes.len() == 1
            && rec
                .last
                .is_some_and(|l| now.duration_since(l) < CHORD_TIMEOUT);
        if chord_ok {
            rec.strokes.push(stroke);
        } else {
            rec.strokes = vec![stroke];
        }
        rec.last = Some(now);
        rec.resolution = Resolution::Unresolved;
        let keys = rec.strokes.join(" ");
        let action = rec.action.clone();
        let conflicts: Vec<(String, String)> =
            keymap::conflicts(&self.keymap, &action, &keys, None)
                .into_iter()
                .map(|b| (b.action.clone(), b.keys.clone()))
                .collect();
        if let Some(rec) = self.settings.as_mut().and_then(|s| s.record.as_mut()) {
            rec.conflicts = conflicts;
        }
        cx.notify();
    }

    pub fn keymap_set_resolution(&mut self, r: Resolution, cx: &mut Context<Self>) {
        if let Some(rec) = self.settings.as_mut().and_then(|s| s.record.as_mut()) {
            rec.resolution = r;
        }
        cx.notify();
    }

    pub fn keymap_set_context(&mut self, ctx: &str, cx: &mut Context<Self>) {
        if let Some(rec) = self.settings.as_mut().and_then(|s| s.record.as_mut()) {
            rec.context = ctx.to_string();
        }
        cx.notify();
    }

    /// Save the recorded binding, applying the chosen conflict resolution.
    pub fn keymap_save_record(&mut self, cx: &mut Context<Self>) {
        let Some(rec) = self.settings.as_ref().and_then(|s| s.record.as_ref()) else {
            return;
        };
        if rec.strokes.is_empty()
            || (!rec.conflicts.is_empty() && rec.resolution == Resolution::Unresolved)
        {
            return;
        }
        let (action, keys) = (rec.action.clone(), rec.strokes.join(" "));
        let ctx = (rec.resolution == Resolution::AllowBoth).then(|| rec.context.clone());
        let replace: Vec<(String, String)> = if rec.resolution == Resolution::Replace {
            rec.conflicts.clone()
        } else {
            Vec::new()
        };
        // The other action keeps its other bindings; it just loses this one.
        for (other, other_keys) in replace {
            let remaining: Vec<(String, Option<String>)> = self
                .keymap
                .bindings
                .iter()
                .filter(|b| b.action == other && b.keys != other_keys)
                .map(|b| (b.keys.clone(), b.context.clone()))
                .collect();
            let entries = if remaining.is_empty() {
                vec![(String::new(), None)]
            } else {
                remaining
            };
            self.keymap_write(&other, &entries, cx);
        }
        self.keymap_write(&action, &[(keys, ctx)], cx);
        self.keymap_cancel_record(cx);
    }

    pub(crate) fn render_record_dialog(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(rec) = self.settings.as_ref().and_then(|s| s.record.as_ref()) else {
            return div().into_any_element();
        };
        let mac = cfg!(target_os = "macos");
        let keys = rec.strokes.join(" ");
        let shown = if keys.is_empty() {
            "Press the keys…".to_string()
        } else {
            display_keys(&keys, mac)
        };
        let conflicted = !rec.conflicts.is_empty();
        let can_save =
            !rec.strokes.is_empty() && (!conflicted || rec.resolution != Resolution::Unresolved);
        fn choice(
            t: &Theme,
            cx: &mut Context<Shell>,
            id: &'static str,
            label: &'static str,
            on: bool,
            r: Resolution,
        ) -> AnyElement {
            crate::dev_id!(
                checkbox_row(t, eid(id), on, label)
                    .on_click(cx.listener(move |this, _, _, cx| this.keymap_set_resolution(r, cx))),
                format!("keymap.{id}"),
                Button,
                label,
                Press(format!(
                    "km-resolve:{}",
                    if r == Resolution::Replace {
                        "replace"
                    } else {
                        "both"
                    }
                ))
            )
            .into_any_element()
        }
        let ctx_combo = (rec.resolution == Resolution::AllowBoth).then(|| {
            self.combo_field(
                cx,
                ComboSite {
                    key: "keymap-context".into(),
                    items: CONTEXTS.iter().map(|c| ComboItem::new(*c, *c)).collect(),
                    value: Some(rec.context.clone().into()),
                    fallback: Some(rec.context.clone().into()),
                    color: None,
                    tip: None,
                    width: ComboWidth::Fixed(160.),
                    look: ComboLook::Field,
                    size: ComboSize::Small,
                    on_event: std::rc::Rc::new(|this, ev, cx| {
                        if let ComboEvent::Change(v) = ev {
                            this.keymap_set_context(v.as_ref(), cx);
                        }
                    }),
                },
            )
        });
        let button = |id: &'static str, label: &'static str| {
            toolbar_button(t, eid(id))
                .h(px(26.))
                .px(px(12.))
                .child(label)
        };
        let panel = div()
            .id("keymap-record")
            .w(px(440.))
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .rounded(px(8.))
            .border_1()
            .border_color(t.border)
            .bg(t.panel)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().text_size(px(14.)).text_color(t.text).child(format!(
                "Record binding: {}",
                actions::label_of(&rec.action)
            )))
            .child(crate::dev_id!(
                div()
                    .id("keymap-recorded")
                    .h(px(36.))
                    .flex()
                    .items_center()
                    .px(px(10.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(t.border_focused)
                    .bg(t.editor)
                    .text_size(px(14.))
                    .text_color(if keys.is_empty() {
                        t.text_placeholder
                    } else {
                        t.text
                    })
                    .child(shown.clone()),
                "keymap.recorded",
                Text,
                shown,
                None
            ))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child("Press a combination, or two within 400 ms for a chord. Esc cancels."),
            )
            .when(conflicted, |d| {
                let others = rec
                    .conflicts
                    .iter()
                    .map(|(a, _)| actions::label_of(a))
                    .collect::<Vec<_>>()
                    .join(", ");
                d.child(crate::dev_id!(
                    div()
                        .id("keymap-conflict")
                        .text_size(px(12.))
                        .text_color(t.deleted)
                        .child(format!("Conflicts with: {others}")),
                    "keymap.conflict",
                    Text,
                    format!("Conflicts with: {others}"),
                    None
                ))
                .child(choice(
                    t,
                    cx,
                    "keymap-replace",
                    "Replace: the other binding is removed",
                    rec.resolution == Resolution::Replace,
                    Resolution::Replace,
                ))
                .child(choice(
                    t,
                    cx,
                    "keymap-both",
                    "Allow both, by context",
                    rec.resolution == Resolution::AllowBoth,
                    Resolution::AllowBoth,
                ))
                .when_some(ctx_combo, |d, c| d.child(c))
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap(px(8.))
                    .child(crate::dev_id!(
                        button("keymap-cancel", "Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.keymap_cancel_record(cx))),
                        "keymap.cancel",
                        Button,
                        "Cancel",
                        Press("km-cancel:x".to_string())
                    ))
                    .child(crate::dev_id!(
                        button("keymap-save", "Save")
                            .opacity(if can_save { 1.0 } else { 0.5 })
                            .on_click(cx.listener(|this, _, _, cx| this.keymap_save_record(cx))),
                        "keymap.save",
                        Button,
                        "Save",
                        Press("km-save:x".to_string())
                    )),
            );
        div()
            .id("keymap-record-scrim")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(t.background.opacity(0.6))
            .child(panel)
            .into_any_element()
    }
}

/// Movement of the vim list-navigation preset over the sidebar tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListMove {
    Next,
    Prev,
    First,
    Last,
    Collapse,
    Expand,
    Open,
}

/// Index the selection moves to, over `len` rows from `cur` (`None` = nothing selected).
pub fn list_target(cur: Option<usize>, len: usize, mv: ListMove) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match (mv, cur) {
        (ListMove::First, _) => 0,
        (ListMove::Last, _) => len - 1,
        (ListMove::Next, None) => 0,
        (ListMove::Next, Some(i)) => (i + 1).min(len - 1),
        (ListMove::Prev, None) => len - 1,
        (ListMove::Prev, Some(i)) => i.saturating_sub(1),
        (_, Some(i)) => i,
        (_, None) => return None,
    })
}

impl Shell {
    /// j/k, g g/G, h/l and Enter over the sidebar tree.
    pub fn list_move(&mut self, mv: ListMove, cx: &mut Context<Self>) {
        let rows = crate::tree::flatten(
            &self.ws,
            &self.collapsed,
            &self.active_env,
            self.sidebar_tab,
        );
        let cur = self
            .selected
            .as_ref()
            .and_then(|s| rows.iter().position(|r| &r.id == s));
        let Some(i) = list_target(cur, rows.len(), mv) else {
            return;
        };
        let row = rows[i].clone();
        match mv {
            ListMove::Collapse if row.expandable && row.expanded => self.toggle_row(&row.id),
            ListMove::Expand if row.expandable && !row.expanded => self.toggle_row(&row.id),
            ListMove::Open => {
                self.click_row(&row.id, 2, cx);
                return;
            }
            ListMove::Collapse | ListMove::Expand => {}
            _ => self.selected = Some(row.id),
        }
        cx.notify();
    }
}

#[cfg(feature = "dev-mcp")]
impl Shell {
    /// The `ui.state` view of the Settings tab.
    pub(crate) fn settings_ui_state(
        &self,
        cx: &App,
    ) -> Option<crate::dev_mcp::state::SettingsUiState> {
        use crate::dev_mcp::state::{
            KeymapRowState, RecordState, SettingRowState, SettingsUiState,
        };
        let s = self.settings.as_ref()?;
        let search = s.search.read(cx).value().to_string();
        let category = st::CATEGORIES
            .get(s.category)
            .copied()
            .unwrap_or("Appearance");
        let rows = visible_defs(&search, category)
            .into_iter()
            .map(|d| SettingRowState {
                key: d.key.to_string(),
                label: d.label.to_string(),
                value: current(&s.raw, d)
                    .as_ref()
                    .map(display_value)
                    .unwrap_or_default(),
                modified: st::is_modified(&s.raw, d),
                error: s.errors.get(d.key).cloned(),
            })
            .collect();
        let keymap = self
            .keymap
            .bindings
            .iter()
            .zip(&self.keymap_sources)
            .map(|(b, src)| KeymapRowState {
                action: b.action.clone(),
                keys: b.keys.clone(),
                source: src.label().to_string(),
            })
            .collect();
        Some(SettingsUiState {
            category: category.to_string(),
            search,
            vim_list_navigation: self.vim_list,
            rows,
            keymap,
            keymap_search: s.keymap_search.read(cx).value().to_string(),
            record: s.record.as_ref().map(|r| RecordState {
                action: r.action.clone(),
                keys: r.strokes.join(" "),
                conflicts: r.conflicts.iter().map(|(a, _)| a.clone()).collect(),
                resolution: match r.resolution {
                    Resolution::Unresolved => "unresolved",
                    Resolution::Replace => "replace",
                    Resolution::AllowBoth => "allow_both",
                },
            }),
        })
    }
}

#[cfg(feature = "dev-mcp")]
impl Shell {
    /// `ui.click_element` ops of the Settings tab and keymap editor (see `Shell::press`).
    pub(crate) fn press_settings(
        &mut self,
        op: &str,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        match op {
            "set-cat" => {
                let i: usize = id
                    .parse()
                    .map_err(|_| "set-cat needs an index".to_string())?;
                self.ensure_settings(cx);
                if let Some(s) = self.settings.as_mut() {
                    s.category = i.min(st::CATEGORIES.len() - 1);
                }
                cx.notify();
            }
            "set-toggle" => {
                let def = st::find_def(id).ok_or_else(|| format!("no setting {id:?}"))?;
                let on = self
                    .settings
                    .as_ref()
                    .and_then(|s| current(&s.raw, def))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                self.settings_set(id, Some(toml::Value::Boolean(!on)), cx);
            }
            "set-reset" => self.settings_set(id, None, cx),
            "km-record" => self.keymap_start_record(id, cx),
            "km-clear" => self.keymap_write(id, &[(String::new(), None)], cx),
            "km-reset" => self.keymap_write(id, &[], cx),
            "km-save" => self.keymap_save_record(cx),
            "km-cancel" => self.keymap_cancel_record(cx),
            "km-resolve" => self.keymap_set_resolution(
                if id == "replace" {
                    Resolution::Replace
                } else {
                    Resolution::AllowBoth
                },
                cx,
            ),
            "km-stroke" => self.record_stroke(id.to_string(), Instant::now(), cx),
            _ => return Err(format!("unknown settings op {op:?}")),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_target_clamps() {
        assert_eq!(list_target(None, 0, ListMove::Next), None);
        assert_eq!(list_target(None, 3, ListMove::Next), Some(0));
        assert_eq!(list_target(Some(2), 3, ListMove::Next), Some(2));
        assert_eq!(list_target(Some(0), 3, ListMove::Prev), Some(0));
        assert_eq!(list_target(Some(1), 3, ListMove::Last), Some(2));
        assert_eq!(list_target(Some(1), 3, ListMove::First), Some(0));
        assert_eq!(list_target(Some(1), 3, ListMove::Open), Some(1));
    }

    #[test]
    fn search_filters_by_label_description_and_key() {
        let n = |q: &str| visible_defs(q, "Appearance").len();
        assert!(n("") >= 5);
        assert!(
            visible_defs("theme", "Appearance")
                .iter()
                .any(|d| d.key == "theme.dark")
        );
        assert!(
            visible_defs("tab_size", "Appearance")
                .iter()
                .any(|d| d.key == "editor.tab_size")
        );
        assert!(visible_defs("zzzz-none", "Appearance").is_empty());
        // hidden keys never show
        assert!(!visible_defs("check_for_updates", "Advanced").is_empty());
    }

    #[test]
    fn every_visible_setting_has_a_control_kind_and_category() {
        for d in st::SETTING_DEFS {
            assert!(st::CATEGORIES.contains(&d.category));
        }
    }
}
