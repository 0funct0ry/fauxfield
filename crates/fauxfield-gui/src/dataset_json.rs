//! The JSON view of a Dataset tab (M10.07): the live rows in the reusable [`CodeEditor`]
//! (syntax highlight, folding-free full editing, undo, find, format), pretty or compact (one
//! row per line), edited in place and applied to the live rows, so the Table view shows the
//! edits. Rows larger than [`SEED_EDIT_MAX`] fall back to the read-only list.

use gpui::{
    AnyElement, Context, Entity, InteractiveElement, ParentElement, Styled, div, prelude::*, px,
};
use serde_json::Value;

use crate::code_editor::{CodeEditor, EditorEvent, Language};
use crate::dataset_edit::SEED_EDIT_MAX;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::view::Dataset;

/// The editor of one Dataset's JSON view.
pub struct JsonEd {
    pub editor: Entity<CodeEditor>,
    pub pretty: bool,
    /// Text last loaded from (or applied to) the live rows.
    pub base: String,
    /// `Shell::ds_gen` when `base` was loaded.
    generation: u64,
    /// Why the text cannot be applied, if it cannot.
    pub problem: Option<String>,
    _sub: gpui::Subscription,
}

impl JsonEd {
    pub fn dirty(&self, cx: &gpui::App) -> bool {
        self.editor.read(cx).text() != self.base
    }
}

/// Live rows as editor text: pretty-printed, or compact with one row per line.
pub fn rows_text(rows: &Value, pretty: bool) -> String {
    if pretty {
        return serde_json::to_string_pretty(rows).unwrap_or_default();
    }
    match rows {
        Value::Array(a) if a.is_empty() => "[]".into(),
        Value::Array(a) => {
            let lines: Vec<String> = a
                .iter()
                .map(|r| format!("  {}", serde_json::to_string(r).unwrap_or_default()))
                .collect();
            format!("[\n{}\n]", lines.join(",\n"))
        }
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// Parse editor text into rows: an array of objects (or one object, a singleton), with the
/// declared column types, `required` and `unique` checked.
pub fn parse_rows(text: &str, cols: &[fauxfield_model::DatasetColumn]) -> Result<Value, String> {
    if let Err(e) = crate::code_editor::format::validate_json(text) {
        return Err(format!("{}:{}: {}", e.line, e.col, e.message));
    }
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    match &v {
        Value::Array(a) => {
            if let Some(i) = a.iter().position(|r| !r.is_object()) {
                return Err(format!("Row {} is not an object.", i + 1));
            }
            for (i, r) in a.iter().enumerate() {
                for c in cols {
                    let cell = r.get(&c.name).unwrap_or(&Value::Null);
                    fauxfield_model::columns::check_cell(cols, &c.name, cell, a, Some(i))
                        .map_err(|m| format!("Row {}: {m}", i + 1))?;
                }
            }
        }
        Value::Object(_) => {}
        _ => return Err("Use an array of objects (or one object).".into()),
    }
    Ok(v)
}

impl Shell {
    fn json_rows(&self, d: &Dataset) -> Option<Value> {
        self.engine()?.dataset_rows(&d.server_id, &d.name).ok()
    }

    /// The JSON view body, or `None` when the rows are too large for the editor.
    pub(crate) fn json_editor_body(
        &self,
        t: &Theme,
        d: &Dataset,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let key = d.key();
        let rows = self.json_rows(d)?;
        let mut eds = self.json_eds.borrow_mut();
        let pretty = eds.get(&key).is_none_or(|e| e.pretty);
        let text = rows_text(&rows, pretty);
        if text.len() > SEED_EDIT_MAX {
            return None;
        }
        let generation = self.ds_gen;
        let entry = eds.entry(key.clone()).or_insert_with(|| {
            let ed = cx.new(|cx| CodeEditor::new(Language::Json, cx).line_numbers(true));
            ed.update(cx, |e, cx| e.set_text(&text, cx));
            let k = key.clone();
            let sub = cx.subscribe(&ed, move |this: &mut Shell, _, ev: &EditorEvent, cx| {
                if matches!(ev, EditorEvent::Changed) {
                    this.json_changed(&k, cx);
                }
            });
            JsonEd {
                editor: ed,
                pretty: true,
                base: text.clone(),
                generation,
                problem: None,
                _sub: sub,
            }
        });
        // Follow the live rows (table edits, HTTP writes, reset) while there are no edits here.
        if !entry.dirty(cx) && (entry.base != text || entry.generation != generation) {
            entry.base = text.clone();
            entry.generation = generation;
            entry.problem = None;
            entry.editor.update(cx, |e, cx| e.set_text(&text, cx));
        }
        let st = self.editor_settings.clone();
        entry.editor.update(cx, |e, cx| e.apply_settings(&st, cx));
        let dirty = entry.dirty(cx);
        let problem = entry.problem.clone();
        let ed = entry.editor.clone();
        drop(eds);
        let n = match &rows {
            Value::Array(a) => a.len(),
            _ => 1,
        };
        let (k1, k2, k3, k4) = (key.clone(), key.clone(), key.clone(), key.clone());
        let state = match (&problem, dirty) {
            (Some(p), _) => p.clone(),
            (None, true) => "Edited: Apply to update the rows".to_string(),
            (None, false) => format!("{n} {} · in sync", if n == 1 { "row" } else { "rows" }),
        };
        let bar = div()
            .flex_none()
            .h(px(32.))
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(8.))
            .border_b_1()
            .border_color(t.border_variant)
            .text_size(px(12.))
            .child(crate::dev_id!(
                div()
                    .id(crate::ui::eid(format!("ds-json-state-{key}")))
                    .min_w(px(0.))
                    .truncate()
                    .text_color(if problem.is_some() {
                        t.deleted
                    } else if dirty {
                        t.modified
                    } else {
                        t.text_muted
                    })
                    .child(state.clone()),
                format!("ds-json-state-{key}"),
                Text,
                state,
                None
            ))
            .child(div().flex_1())
            .child(json_btn(
                crate::ui::icon_button(
                    t,
                    "ds-json-pretty",
                    "list-tree",
                    "Pretty-print",
                    pretty,
                    true,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.set_json_pretty(&k1, true, cx))),
                "ds-json-pretty",
                "Pretty-print",
                "toggle_dataset_json_format",
            ))
            .child(json_btn(
                crate::ui::icon_button(
                    t,
                    "ds-json-compact",
                    "rows-2",
                    "Compact (one row per line)",
                    !pretty,
                    true,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.set_json_pretty(&k2, false, cx))),
                "ds-json-compact",
                "Compact",
                "toggle_dataset_json_format",
            ))
            .child(div().w(px(1.)).h(px(16.)).bg(t.border_variant))
            .child(json_btn(
                crate::ui::icon_button(t, "ds-json-revert", "undo-2", "Revert edits", false, dirty)
                    .on_click(cx.listener(move |this, _, _, cx| this.revert_json(&k3, cx))),
                "ds-json-revert",
                "Revert edits",
                "revert_dataset_json",
            ))
            .child(json_btn(
                crate::ui::icon_button(
                    t,
                    "ds-json-apply",
                    "check",
                    "Apply edits to the rows",
                    dirty && problem.is_none(),
                    dirty && problem.is_none(),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.apply_json(&k4, cx);
                })),
                "ds-json-apply",
                "Apply edits to the rows",
                "apply_dataset_json",
            ));
        Some(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(bar)
                .child(div().flex_1().min_h_0().child(ed))
                .into_any_element(),
        )
    }

    #[cfg(test)]
    pub(crate) fn json_changed_for_test(&mut self, key: &str, cx: &mut Context<Self>) {
        self.json_changed(key, cx)
    }

    fn json_changed(&mut self, key: &str, cx: &mut Context<Self>) {
        let cols = self
            .ws
            .find_dataset(key)
            .map(|d| d.column_defs.clone())
            .unwrap_or_default();
        if let Some(e) = self.json_eds.borrow_mut().get_mut(key) {
            let text = e.editor.read(cx).text().to_string();
            e.problem = if text == e.base {
                None
            } else {
                parse_rows(&text, &cols).err()
            };
        }
        cx.notify();
    }

    /// Apply the JSON view's text to the live rows. Returns false (and says why) when it
    /// cannot be applied.
    pub(crate) fn apply_json(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let Some(d) = self.ws.find_dataset(key).cloned() else {
            return false;
        };
        let Some(text) = self
            .json_eds
            .borrow()
            .get(key)
            .filter(|e| e.dirty(cx))
            .map(|e| e.editor.read(cx).text().to_string())
        else {
            return true;
        };
        let rows = match parse_rows(&text, &d.column_defs) {
            Ok(r) => r,
            Err(m) => {
                self.notify(format!("Cannot apply the JSON: {m}"), cx);
                return false;
            }
        };
        let Some(e) = self.engine() else {
            self.notify("No host is running.".into(), cx);
            return false;
        };
        if let Err(err) = e.dataset_set(&d.server_id, &d.name, rows) {
            self.notify(err.to_string(), cx);
            return false;
        }
        if let Some(j) = self.json_eds.borrow_mut().get_mut(key) {
            // Clean now; the next render reloads the formatted rows.
            j.base = text;
            j.problem = None;
        }
        self.ds_gen += 1;
        self.write_through(&d, cx);
        self.sync_running();
        self.notify(format!("Applied the JSON to {}", d.name), cx);
        cx.notify();
        true
    }

    pub(crate) fn revert_json(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(j) = self.json_eds.borrow_mut().get_mut(key) {
            let base = j.base.clone();
            j.problem = None;
            j.editor.update(cx, |e, cx| e.set_text(&base, cx));
        }
        cx.notify();
    }

    /// Pretty or compact. Unapplied edits are re-formatted when they parse; otherwise the
    /// switch is refused so nothing is lost.
    pub(crate) fn set_json_pretty(&mut self, key: &str, pretty: bool, cx: &mut Context<Self>) {
        let cols = self
            .ws
            .find_dataset(key)
            .map(|d| d.column_defs.clone())
            .unwrap_or_default();
        let mut eds = self.json_eds.borrow_mut();
        let Some(j) = eds.get_mut(key) else { return };
        if j.pretty == pretty {
            return;
        }
        let dirty = j.dirty(cx);
        let text = j.editor.read(cx).text().to_string();
        let edited = if dirty {
            match parse_rows(&text, &cols) {
                Ok(v) => Some(v),
                Err(m) => {
                    drop(eds);
                    return self.notify(format!("Fix the JSON first: {m}"), cx);
                }
            }
        } else {
            None
        };
        j.pretty = pretty;
        match edited {
            // Keep the edits, re-formatted; they stay unapplied.
            Some(v) => {
                let t = rows_text(&v, pretty);
                j.editor.update(cx, |e, cx| e.set_text(&t, cx));
            }
            // Clean: reload the live rows in the new format.
            None => {
                let rows = self.ws.find_dataset(key).and_then(|d| self.json_rows(d));
                match rows {
                    Some(r) => {
                        let t = rows_text(&r, pretty);
                        j.base = t.clone();
                        j.editor.update(cx, |e, cx| e.set_text(&t, cx));
                    }
                    None => j.base.clear(),
                }
            }
        }
        drop(eds);
        cx.notify();
    }

    pub(crate) fn toggle_json_format(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.active_dataset_key() else {
            return;
        };
        let pretty = self.json_eds.borrow().get(&key).is_none_or(|e| e.pretty);
        self.set_json_pretty(&key, !pretty, cx);
    }

    /// Leaving the JSON view applies valid edits; invalid ones keep the view open.
    pub(crate) fn leave_json_view(&mut self, cx: &mut Context<Self>) -> bool {
        match self.active_dataset_key() {
            Some(k) => self.apply_json(&k, cx),
            None => true,
        }
    }
}

/// Tag a JSON view button for dev-mcp (it has a tooltip; its action is in the palette).
fn json_btn(
    el: gpui::Stateful<gpui::Div>,
    id: &'static str,
    label: &'static str,
    action: &'static str,
) -> AnyElement {
    let _ = (id, label, action);
    crate::dev_id_meta!(
        el,
        crate::dev_mcp::tree::Meta {
            tooltip: true,
            ..Default::default()
        },
        id,
        Button,
        label,
        Action(action)
    )
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compact_is_one_row_per_line_and_round_trips() {
        let v = json!([{"id":1,"a":"x"},{"id":2,"a":"y"}]);
        let c = rows_text(&v, false);
        assert_eq!(c.lines().count(), 4);
        assert_eq!(parse_rows(&c, &[]).unwrap(), v);
        assert_eq!(parse_rows(&rows_text(&v, true), &[]).unwrap(), v);
        assert_eq!(rows_text(&json!([]), false), "[]");
    }

    #[test]
    fn parse_refuses_non_objects_and_constraint_breaks() {
        assert!(parse_rows("[1,2]", &[]).is_err());
        assert!(parse_rows("[{\"a\":", &[]).is_err());
        let mut c = fauxfield_model::DatasetColumn::new("id", "int");
        c.unique = true;
        assert!(parse_rows("[{\"id\":1},{\"id\":1}]", std::slice::from_ref(&c)).is_err());
        assert!(parse_rows("[{\"id\":\"x\"}]", std::slice::from_ref(&c)).is_err());
        assert!(parse_rows("[{\"id\":1},{\"id\":2}]", &[c]).is_ok());
    }
}
