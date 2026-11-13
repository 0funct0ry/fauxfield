//! Dataset tab (M8, M10.03): live table (the DataTable component) and JSON views over the
//! running engine's rows, add and delete row, reset, export. Rows are read a page at a time
//! (`Engine::dataset_page`) so 100k-row Datasets stay smooth. The table is read-only for now;
//! M10.04 builds editing on its events.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, App, ClipboardItem, Context, Entity, InteractiveElement, IntoElement, MouseButton,
    ParentElement, StatefulInteractiveElement, Styled, Window, div, prelude::*, px, uniform_list,
};
use serde_json::Value;

use crate::data_table::{
    Column, ColumnSort, DataTable, TableDelegate, TableEvent, TableOptions, TableState,
    TableViewModel,
};
use crate::layout::TableLayout;
use crate::text_input::{InputEvent, InputSize, TextInput};

use crate::assets::BUFFER_FONT;
use crate::shell::{PromptAction, Shell};
use crate::theme::Theme;
use crate::ui::{icon, toolbar_button};
use crate::view::Dataset;

pub const DS_ROW_H: f32 = 24.0;

/// Cell text: strings bare, everything else as compact JSON.
pub fn cell_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        other => other.to_string(),
    }
}

/// Column order: the key first, then fields in first-seen order across `rows`.
pub fn columns(rows: &[Value], key: &str) -> Vec<String> {
    let mut cols: Vec<String> = vec![];
    for r in rows {
        if let Value::Object(o) = r {
            for k in o.keys() {
                if !cols.contains(k) {
                    cols.push(k.clone());
                }
            }
        }
    }
    if let Some(i) = cols.iter().position(|c| c == key) {
        let k = cols.remove(i);
        cols.insert(0, k);
    }
    cols
}

/// `name:kind, price:float` into `(name, kind)` pairs (kind defaults to `lorem.word`).
pub fn parse_fields(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = vec![];
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (n, k) = part.split_once(':').unwrap_or((part, "lorem.word"));
        out.push((n.trim().to_string(), k.trim().to_string()));
    }
    // Validate by building the template once.
    fauxfield_engine::crud_seed_template(&out, 1)?;
    Ok(out)
}

impl Shell {
    pub(crate) fn render_dataset_tab(
        &self,
        t: &Theme,
        d: &Dataset,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let engine = self.host.as_ref().map(|h| h.engine().clone());
        let total = engine
            .as_ref()
            .and_then(|e| e.dataset_page(&d.server_id, &d.name, 0, 1).ok())
            .map_or(d.rows, |(n, _)| n);
        let seed_mode = self.ds_seed_mode;
        let json_mode = self.ds_json_mode && !seed_mode;
        let key = d.key();
        let (k1, k2, k3, k4, k5) = (
            key.clone(),
            key.clone(),
            key.clone(),
            key.clone(),
            key.clone(),
        );
        let hover = t.element_hover;
        let muted = t.text_muted;
        let seg = move |t: &Theme, id: &'static str, label: &'static str, on: bool| {
            toolbar_button(t, id)
                .when(on, |d| d.bg(t.element_selected))
                .child(label)
        };
        let header = div()
            .h(px(40.))
            .flex_none()
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(8.))
            .border_b_1()
            .border_color(t.border_variant)
            .child(icon("table", 14., t.modified))
            .child(div().text_size(px(14.)).child(d.name.clone()))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(muted)
                    .child(d.server.clone()),
            )
            .child(div().flex_1())
            .child(crate::dev_id!(
                seg(t, "ds-view-table", "Table", !json_mode && !seed_mode).on_click(cx.listener(
                    |this, _, _, cx| {
                        if this.ds_json_mode && !this.leave_json_view(cx) {
                            return;
                        }
                        this.ds_json_mode = false;
                        this.ds_seed_mode = false;
                        cx.notify();
                    }
                )),
                "ds-view-table",
                Button,
                "Table",
                Press("ds-view:table".into())
            ))
            .child(crate::dev_id!(
                seg(t, "ds-view-json", "JSON", json_mode).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.ds_json_mode = true;
                        this.ds_seed_mode = false;
                        cx.notify();
                    }
                )),
                "ds-view-json",
                Button,
                "JSON",
                Press("ds-view:json".into())
            ))
            .child(crate::dev_id!(
                toolbar_button(t, "ds-add-row")
                    .child("Add row")
                    .on_click(cx.listener(move |this, _, _, cx| this.add_dataset_row(&k1, cx))),
                "ds-add-row",
                Button,
                "Add row",
                Action("add_dataset_row")
            ))
            .child(crate::dev_id!(
                div().w(px(160.)).child(self.ds_filter_input(&key, cx)),
                "ds-filter",
                Input,
                "Filter rows",
                None
            ))
            .child(self.ds_hamburger(d, total, cx));
        let body: AnyElement = if seed_mode {
            self.seed_editor_body(t, d, cx)
        } else if let Some(body) = json_mode.then(|| self.json_editor_body(t, d, cx)).flatten() {
            body
        } else if json_mode && total > 0 {
            let (server, name) = (d.server_id.clone(), d.name.clone());
            uniform_list(
                "ds-json",
                total,
                cx.processor(
                    move |this: &mut Shell, range: std::ops::Range<usize>, _w, cx| {
                        let t = crate::theme::theme(cx).clone();
                        let rows = this
                            .host
                            .as_ref()
                            .and_then(|h| {
                                h.engine()
                                    .dataset_page(&server, &name, range.start, range.len())
                                    .ok()
                            })
                            .map(|(_, r)| r)
                            .unwrap_or_default();
                        rows.into_iter()
                            .enumerate()
                            .map(|(n, r)| {
                                div()
                                    .id(crate::ui::eid(format!("ds-json-{}", range.start + n)))
                                    .h(px(DS_ROW_H))
                                    .px(px(12.))
                                    .flex()
                                    .items_center()
                                    .font_family(BUFFER_FONT)
                                    .text_size(px(11.))
                                    .text_color(t.text)
                                    .truncate()
                                    .child(serde_json::to_string(&r).unwrap_or_default())
                            })
                            .collect::<Vec<_>>()
                    },
                ),
            )
            .track_scroll(self.ds_scroll.clone())
            .flex_1()
            .min_h_0()
            .into_any_element()
        } else {
            match self.dataset_table(d, cx) {
                Some(table) => div()
                    .flex_1()
                    .min_h_0()
                    .p(px(8.))
                    .child(
                        DataTable::new(&table)
                            .accessibility_label(format!("Dataset {}", d.name))
                            .with_size(crate::data_table::Size::Small),
                    )
                    .into_any_element(),
                None => div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .text_color(muted)
                    .child("No host is running.")
                    .into_any_element(),
            }
        };
        let _ = (hover, k2, k3, k4, k5);
        let status = self.ds_status(t, d, total, cx);
        let edit_error = self
            .ds_tables
            .borrow()
            .get(&d.key())
            .and_then(|e| e.read(cx).delegate().shared.edit_error.borrow().clone())
            .map(|m| {
                crate::dev_id!(
                    div()
                        .id("ds-edit-error")
                        .flex_none()
                        .px(px(12.))
                        .py(px(3.))
                        .text_size(px(12.))
                        .text_color(t.deleted)
                        .child(m.clone()),
                    "ds-edit-error",
                    Text,
                    m,
                    None
                )
            });
        let selected = self
            .ds_tables
            .borrow()
            .get(&d.key())
            .map_or(0, |e| e.read(cx).delegate().selected_count());
        let selection_bar = (selected > 0).then(|| self.ds_selection_bar(t, d, selected, cx));
        let generator = (!d.generator_fields.is_empty()).then(|| {
            div()
                .flex_none()
                .px(px(12.))
                .py(px(6.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .border_b_1()
                .border_color(t.border_variant)
                .text_size(px(12.))
                .child(crate::dev_id!(
                    div()
                        .id(crate::ui::eid(format!("ds-gen-{}", d.key())))
                        .text_color(muted)
                        .child(format!("Generator seed: {}", d.generator_fields.join(", "))),
                    format!("ds-generator-{}", d.key()),
                    Text,
                    "Generator seed",
                    None
                ))
                .children(d.generator_preview.iter().enumerate().map(|(i, r)| {
                    crate::dev_id!(
                        div()
                            .id(crate::ui::eid(format!("ds-gen-row-{}-{i}", d.key())))
                            .truncate()
                            .text_size(px(11.))
                            .child(r.clone()),
                        format!("ds-generator-row-{}-{i}", d.key()),
                        Text,
                        r.clone(),
                        None
                    )
                }))
        });
        div()
            .id(crate::ui::eid(format!("ds-tab-{}", d.key())))
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .child(status)
            .children(selection_bar)
            .children(edit_error)
            .children(generator)
            .child(body)
    }
}

// ---- actions on Datasets ------------------------------------------------------------------

impl Shell {
    pub(crate) fn engine(&self) -> Option<fauxfield_engine::Engine> {
        self.host.as_ref().map(|h| h.engine().clone())
    }

    /// Reset one Dataset (`key` is `<server id>:<name>`) to its seed.
    pub(crate) fn reset_dataset(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(d) = self.ws.find_dataset(key).cloned() else {
            return;
        };
        let Some(e) = self.engine() else {
            self.notify("No host is running.".into(), cx);
            return;
        };
        self.ds_gen += 1;
        match e.dataset_reset(&d.server_id, &d.name) {
            Ok(()) => self.notify(format!("Reset {} to its seed", d.name), cx),
            Err(err) => self.notify(err.to_string(), cx),
        }
        self.sync_running();
        cx.notify();
    }

    /// The Reset State action: the open Dataset tab, else every Server (SPEC §6.4).
    pub(crate) fn reset_state_action(&mut self, cx: &mut Context<Self>) {
        if let Some(t) = self
            .tabs
            .active()
            .filter(|t| t.kind == crate::tabs::TabKind::Dataset)
        {
            let key = t.id.clone();
            return self.reset_dataset(&key, cx);
        }
        let Some(e) = self.engine() else {
            self.notify("No host is running.".into(), cx);
            return;
        };
        match e.reset_state(None) {
            Ok(()) => self.notify(
                "Reset state: datasets, counters and runtime variables".into(),
                cx,
            ),
            Err(err) => self.notify(err.to_string(), cx),
        }
        self.sync_running();
        cx.notify();
    }

    /// Apply `f` to a Dataset's live rows and store the result.
    pub(crate) fn edit_dataset(
        &mut self,
        key: &str,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Value) -> Result<(), String>,
    ) {
        self.ds_gen += 1;
        let Some(d) = self.ws.find_dataset(key).cloned() else {
            return;
        };
        let Some(e) = self.engine() else {
            self.notify("No host is running.".into(), cx);
            return;
        };
        let res = e
            .dataset_rows(&d.server_id, &d.name)
            .map_err(|x| x.to_string())
            .and_then(|mut rows| {
                f(&mut rows)?;
                e.dataset_set(&d.server_id, &d.name, rows)
                    .map_err(|x| x.to_string())
            });
        if let Err(m) = res {
            self.notify(m, cx);
        }
        self.sync_running();
        cx.notify();
    }

    pub(crate) fn export_dataset(&mut self, key: &str, path: &str, cx: &mut Context<Self>) {
        let Some(d) = self.ws.find_dataset(key).cloned() else {
            return;
        };
        let Some(e) = self.engine() else {
            self.notify("No host is running.".into(), cx);
            return;
        };
        let res = e
            .dataset_rows(&d.server_id, &d.name)
            .map_err(|x| x.to_string())
            .and_then(|rows| {
                let fmt = fauxfield_engine::import::Format::detect(std::path::Path::new(path))
                    .unwrap_or(fauxfield_engine::import::Format::Json);
                fauxfield_engine::import::export(&rows, fmt)
                    .and_then(|b| std::fs::write(path, b).map_err(|x| format!("{path}: {x}")))
            });
        match res {
            Ok(()) => self.notify(format!("Exported {} to {path}", d.name), cx),
            Err(m) => self.notify(m, cx),
        }
    }

    /// New Dataset… prompt: an empty Dataset (no seed, no Route) in the current Server.
    pub(crate) fn create_dataset_now(&mut self, server: &str, name: &str, cx: &mut Context<Self>) {
        let (sid, n) = (server.to_string(), name.to_string());
        if self
            .store_op(cx, |s| s.create_dataset(&sid, &n, None, None, None))
            .is_some()
        {
            self.sidebar_tab = crate::tree::SidebarTab::Datasets;
            self.layout.sidebar_tab = self.sidebar_tab.id().into();
            self.open_tab(
                &format!("{server}:{name}"),
                crate::tabs::TabKind::Dataset,
                name,
                true,
            );
            self.notify(format!("Created dataset {name}"), cx);
        }
    }

    pub(crate) fn rename_dataset_now(&mut self, key: &str, new: &str, cx: &mut Context<Self>) {
        let Some(d) = self.ws.datasets.iter().find(|d| d.key() == key).cloned() else {
            return;
        };
        if d.name == new {
            return;
        }
        let (sid, old, n) = (d.server_id.clone(), d.name.clone(), new.to_string());
        if let Some(rep) = self.store_op(cx, |s| s.rename_dataset(&sid, &old, &n)) {
            if let Some(i) = self.tabs.position(key) {
                self.tabs.close(i);
                self.open_tab(
                    &format!("{}:{new}", d.server_id),
                    crate::tabs::TabKind::Dataset,
                    new,
                    true,
                );
            }
            let mut msg = format!("Renamed {old} to {new}");
            if !rep.rewritten_routes.is_empty() {
                msg += &format!(" ({} route(s) updated)", rep.rewritten_routes.len());
            }
            if !rep.free_text_warnings.is_empty() {
                msg += &format!(
                    "; {} template(s) still use `data.{old}`",
                    rep.free_text_warnings.len()
                );
            }
            self.notify(msg, cx);
        }
    }

    pub(crate) fn duplicate_dataset_now(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(d) = self.ws.datasets.iter().find(|d| d.key() == key).cloned() else {
            return;
        };
        let (sid, name) = (d.server_id.clone(), d.name.clone());
        if let Some(new) = self.store_op(cx, |s| s.duplicate_dataset(&sid, &name)) {
            self.open_tab(
                &format!("{}:{new}", d.server_id),
                crate::tabs::TabKind::Dataset,
                &new,
                true,
            );
            self.notify(format!("Duplicated as {new}"), cx);
        }
    }

    /// Delete a Dataset: straight away when unused, else through the confirmation modal.
    pub(crate) fn request_delete_dataset(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(d) = self.ws.datasets.iter().find(|d| d.key() == key).cloned() else {
            return;
        };
        let (sid, name) = (d.server_id.clone(), d.name.clone());
        let refs = self
            .store
            .as_ref()
            .and_then(|s| s.dataset_references(&sid, &name).ok())
            .unwrap_or_default();
        let mut list: Vec<String> = refs
            .routes
            .iter()
            .map(|r| format!("Route {} {}", r.method, r.path))
            .collect();
        list.extend(refs.actions.iter().map(|a| {
            format!(
                "Action {} step {} ({}) of Response {}",
                a.route_id, a.step, a.op, a.response_id
            )
        }));
        list.extend(
            refs.rules
                .iter()
                .map(|r| format!("Rule {} {} in Response {}", r.target, r.key, r.response_id)),
        );
        list.extend(
            refs.free_text
                .iter()
                .map(|t| format!("Template {}", t.location)),
        );
        self.lc.dataset_delete = Some(crate::lifecycle::DatasetDelete {
            key: key.into(),
            name,
            server_id: sid,
            refs: list,
            remove_seed: false,
            remove_state: false,
        });
        cx.notify();
    }

    pub(crate) fn delete_dataset_now(
        &mut self,
        dd: &crate::lifecycle::DatasetDelete,
        cx: &mut Context<Self>,
    ) {
        let opts = fauxfield_store::DeleteOpts {
            force: true,
            remove_seed: dd.remove_seed,
            remove_state: dd.remove_state,
        };
        let (sid, name) = (dd.server_id.clone(), dd.name.clone());
        if self
            .store_op(cx, |s| s.delete_dataset_checked(&sid, &name, &opts))
            .is_some()
        {
            if let Some(i) = self.tabs.position(&dd.key) {
                self.tabs.close(i);
            }
            self.selected = None;
            self.notify(format!("Deleted dataset {}", dd.name), cx);
        }
    }

    /// Start the New CRUD resource wizard (name, path, fields) for the current Server.
    pub(crate) fn begin_new_crud(&mut self, w: &mut gpui::Window, cx: &mut Context<Self>) {
        let server = self
            .current_server_id()
            .or_else(|| self.ws.servers.first().map(|s| s.id.clone()));
        match server {
            Some(server) => self.begin_prompt(PromptAction::NewCrudName { server }, "", w, cx),
            None => self.notify("Create a server first.".into(), cx),
        }
    }

    /// Wizard result: a `seed_template` file, a `[[datasets]]` entry and a `crud` Route.
    pub(crate) fn create_crud_resource(
        &mut self,
        server: &str,
        name: &str,
        path: &str,
        fields: &str,
        cx: &mut Context<Self>,
    ) {
        let parsed =
            parse_fields(fields).and_then(|f| fauxfield_engine::crud_seed_template(&f, 20));
        let tpl = match parsed {
            Ok(t) => t,
            Err(m) => {
                self.notify(m, cx);
                return;
            }
        };
        let spec = fauxfield_store::DatasetSpec {
            name: name.to_string(),
            seed_template: Some(tpl),
            id_strategy: Some("increment".into()),
            crud_path: Some(path.to_string()),
            ..Default::default()
        };
        let sid = server.to_string();
        if self
            .store_op(cx, |s| s.upsert_dataset(&sid, &spec))
            .is_some()
        {
            let key = format!("{server}:{name}");
            self.sidebar_tab = crate::tree::SidebarTab::Datasets;
            self.layout.sidebar_tab = self.sidebar_tab.id().into();
            self.open_tab(&key, crate::tabs::TabKind::Dataset, name, true);
            self.notify(format!("Created CRUD resource {name} at {path}"), cx);
        }
    }

    /// Replace sidebar row counts with the live ones from the engine.
    pub(crate) fn sync_dataset_counts(&mut self) {
        let Some(e) = self.engine() else { return };
        let Ok(list) = e.datasets(None) else { return };
        for d in &mut self.ws.datasets {
            if let Some(i) = list
                .iter()
                .find(|i| i.server_id == d.server_id && i.name == d.name)
            {
                d.rows = i.rows;
                d.error = i.error.clone();
            }
        }
    }
}

// ---- the DataTable over a Dataset (M10.03) -------------------------------------------------

const PAGE: usize = 256;
/// Key and width of the checkbox column (M10.07).
pub const SEL_KEY: &str = "\u{1}select";
const SEL_W: f32 = 34.0;
const DEFAULT_COL_W: f32 = 160.0;
/// Approximate advance of one monospace character at 11 px, for fit-to-content.
const CHAR_W: f32 = 6.6;

/// Parse typed cell text for a column type (`None`: undeclared, so JSON when it parses, else text).
/// Empty text is `null` for every type except `string` (M10.04).
pub fn parse_cell(ty: Option<&str>, text: &str) -> Result<Value, String> {
    let t = text.trim();
    match ty {
        Some("string") => Ok(Value::String(text.to_string())),
        _ if t.is_empty() => Ok(Value::Null),
        Some("int") => t
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| "Not a whole number.".into()),
        Some("float") => t
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| "Not a number.".into()),
        Some("bool") => match t {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err("Use true or false.".into()),
        },
        Some("datetime") => {
            let v = Value::String(t.to_string());
            if fauxfield_model::columns::value_fits_type("datetime", &v) {
                Ok(v)
            } else {
                Err("Use a date like 2026-01-31 or 2026-01-31T10:00:00Z.".into())
            }
        }
        Some("json") => serde_json::from_str(t).map_err(|e| format!("Not JSON: {e}")),
        _ => Ok(serde_json::from_str(t).unwrap_or_else(|_| Value::String(text.to_string()))),
    }
}

/// The cell being edited in a DatasetTable (a `TextInput` rendered inside the cell).
pub struct CellEdit {
    pub row: usize,
    pub col: usize,
    pub input: Entity<TextInput>,
    focus_pending: bool,
    _sub: gpui::Subscription,
}

/// What the cells tell the shell: the last pressed cell and pointer position (for the row menu).
#[derive(Default)]
pub struct DsShared {
    pub cell: Cell<Option<(usize, usize)>>,
    pub pointer: Cell<(f32, f32)>,
    /// The last row click carried Shift or Cmd/Ctrl, so the host selection already handled it.
    pub mod_click: Cell<bool>,
    /// A cell editor just closed; the Shell gives focus back to the table.
    pub refocus: Cell<bool>,
    /// Why the open cell edit was refused (type, required, unique), shown above the table.
    pub edit_error: std::cell::RefCell<Option<String>>,
    /// Cell edits to undo and redo (M10.07).
    pub undo: std::cell::RefCell<Vec<CellOp>>,
    pub redo: std::cell::RefCell<Vec<CellOp>>,
}

/// One committed cell edit, by live row index.
#[derive(Clone, Debug, PartialEq)]
pub struct CellOp {
    pub live: usize,
    pub column: String,
    pub old: Value,
    pub new: Value,
}

/// Where the cursor goes after an edit is committed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditMove {
    Stay,
    Down,
    Next,
    Prev,
}

/// The cell after (`forward`) or before `(row, col)` in reading order over data columns
/// `1..cols`, wrapping to the next or previous row; `None` past either end.
pub fn step_cell(
    row: usize,
    col: usize,
    cols: usize,
    rows: usize,
    forward: bool,
) -> Option<(usize, usize)> {
    if cols < 2 || rows == 0 {
        return None;
    }
    if forward {
        if col + 1 < cols {
            Some((row, col + 1))
        } else if row + 1 < rows {
            Some((row + 1, 1))
        } else {
            None
        }
    } else if col > 1 {
        Some((row, col - 1))
    } else if row > 0 {
        Some((row - 1, cols - 1))
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DsRowAction {
    CopyCell,
    CopyRowJson,
    DeleteRow,
    DuplicateRows,
}

/// Rows of one Dataset for the DataTable: a page cache over `Engine::dataset_page`, and a
/// sorted snapshot while a column is sorted.
pub struct DatasetTable {
    /// Row selection by live index (M10.07).
    pub selection: crate::dataset_select::RowSelection,
    engine: Engine,
    server_id: String,
    name: String,
    key_field: String,
    columns: Vec<Column>,
    order: Vec<String>,
    widths: HashMap<String, f32>,
    count: usize,
    generation: u64,
    pages: HashMap<usize, Vec<Value>>,
    snapshot: Option<Vec<Value>>,
    perm: Option<Vec<usize>>,
    sort: Option<(String, ColumnSort)>,
    /// Case-insensitive substring filter over the JSON text of each row (M10.07).
    filter: String,
    /// Declared columns `(name, type)`; they come first and exist even with no rows.
    declared: Vec<(String, String)>,
    pub editing: Option<CellEdit>,
    pub shared: Rc<DsShared>,
}

use fauxfield_engine::Engine;

fn cmp_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    match (a, b) {
        (Value::Null, Value::Null) => Equal,
        (Value::Null, _) => Greater,
        (_, Value::Null) => Less,
        (Value::Number(x), Value::Number(y)) => {
            x.as_f64().partial_cmp(&y.as_f64()).unwrap_or(Equal)
        }
        (Value::String(x), Value::String(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (x, y) => cell_text(x).cmp(&cell_text(y)),
    }
}

/// Row order of `rows` sorted by `field` (stable; nulls last when ascending).
pub fn sort_permutation(rows: &[Value], field: &str, descending: bool) -> Vec<usize> {
    let mut perm: Vec<usize> = (0..rows.len()).collect();
    let get = |i: usize| rows[i].get(field).unwrap_or(&Value::Null);
    perm.sort_by(|&a, &b| {
        let o = cmp_values(get(a), get(b));
        if descending { o.reverse() } else { o }
    });
    perm
}

impl DatasetTable {
    pub fn new(
        engine: Engine,
        d: &Dataset,
        saved: Option<&TableLayout>,
        generation: u64,
    ) -> DatasetTable {
        let mut t = DatasetTable {
            selection: Default::default(),
            engine,
            server_id: d.server_id.clone(),
            name: d.name.clone(),
            key_field: d.key.clone(),
            columns: vec![],
            order: saved.map(|s| s.order.clone()).unwrap_or_default(),
            widths: saved
                .map(|s| s.widths.iter().map(|(k, v)| (k.clone(), *v)).collect())
                .unwrap_or_default(),
            count: 0,
            generation,
            pages: HashMap::new(),
            snapshot: None,
            perm: None,
            sort: None,
            filter: String::new(),
            declared: d.columns.clone(),
            editing: None,
            shared: Rc::new(DsShared::default()),
        };
        t.reload();
        t
    }

    pub fn stale(&self, rows: usize, generation: u64) -> bool {
        self.generation != generation || self.count != rows
    }

    /// Drop caches and re-read the columns and the row count.
    pub fn invalidate(&mut self, generation: u64) {
        self.generation = generation;
        self.pages.clear();
        self.snapshot = None;
        self.perm = None;
        self.reload();
        self.rebuild_view();
    }

    /// Set the row filter (empty clears it) and rebuild the displayed rows.
    pub fn set_filter(&mut self, text: &str) {
        self.filter = text.trim().to_lowercase();
        self.rebuild_view();
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Displayed rows: all of them, or the sorted and filtered subset (a snapshot and a
    /// permutation of live indices).
    fn rebuild_view(&mut self) {
        if self.sort.is_none() && self.filter.is_empty() {
            self.perm = None;
            self.snapshot = None;
            return;
        }
        let rows = match self.engine.dataset_rows(&self.server_id, &self.name) {
            Ok(Value::Array(a)) => a,
            Ok(other) => vec![other],
            Err(_) => return,
        };
        let mut perm = match &self.sort {
            Some((key, s)) => sort_permutation(&rows, key, *s == ColumnSort::Descending),
            None => (0..rows.len()).collect(),
        };
        if !self.filter.is_empty() {
            let f = &self.filter;
            perm.retain(|&i| rows[i].to_string().to_lowercase().contains(f.as_str()));
        }
        self.perm = Some(perm);
        self.snapshot = Some(rows);
    }

    /// Rows currently displayed (after the filter).
    pub fn shown(&self) -> usize {
        self.perm.as_ref().map_or(self.count, Vec::len)
    }

    fn reload(&mut self) {
        let (total, head) = self
            .engine
            .dataset_page(&self.server_id, &self.name, 0, 100)
            .unwrap_or((0, vec![]));
        self.count = total;
        let mut keys = columns(&head, &self.key_field);
        for (i, (n, _)) in self.declared.iter().enumerate() {
            keys.retain(|k| k != n);
            keys.insert(i.min(keys.len()), n.clone());
        }
        // Saved order first; columns it does not know keep their natural order after them.
        let pos = |k: &String| self.order.iter().position(|o| o == k).unwrap_or(usize::MAX);
        keys.sort_by_key(pos);
        self.columns = keys
            .into_iter()
            .map(|k| {
                let w = self.widths.get(&k).copied().unwrap_or(DEFAULT_COL_W);
                let mut c = Column::new(k.clone(), k.clone()).sortable().width(w);
                if let Some((sk, s)) = &self.sort
                    && *sk == k
                {
                    c.sort = *s;
                }
                c
            })
            .collect();
        self.columns.insert(
            0,
            Column::new(SEL_KEY, "")
                .width(SEL_W)
                .fixed_left()
                .resizable(false)
                .movable(false)
                .selectable(false),
        );
    }

    /// Live indices of the displayed rows, in display order.
    pub fn display(&self) -> Vec<usize> {
        (0..self.shown()).map(|i| self.actual_index(i)).collect()
    }

    pub fn select_all(&mut self) {
        let d = self.display();
        self.selection.set = d.iter().copied().collect();
        self.selection.anchor = d.first().copied();
    }

    /// True when the host selection is wider than the table's own single selection.
    pub fn selected_count(&self) -> usize {
        self.selection.len()
    }

    /// First data column (the one after the checkbox column).
    pub const FIRST_DATA: usize = 1;

    /// True when `i` is the checkbox column.
    fn is_sel(&self, i: usize) -> bool {
        self.columns.get(i).is_some_and(|c| c.key == SEL_KEY)
    }

    /// True when the declared columns differ from `cols` (the caller then `invalidate`s).
    pub fn set_declared(&mut self, cols: &[(String, String)]) -> bool {
        if self.declared == cols {
            return false;
        }
        self.declared = cols.to_vec();
        true
    }

    fn col_type(&self, key: &str) -> Option<&str> {
        self.declared
            .iter()
            .find(|(n, _)| n == key)
            .map(|(_, t)| t.as_str())
    }

    /// Start editing a cell: a bool toggles at once, anything else opens a `TextInput`
    /// (Enter, Tab or blur commits, Esc cancels).
    pub fn begin_edit(&mut self, row: usize, col: usize, cx: &mut Context<TableState<Self>>) {
        self.begin_edit_with(row, col, None, cx)
    }

    /// Like [`begin_edit`](Self::begin_edit); `initial` replaces the cell text (typing starts
    /// an edit).
    pub fn begin_edit_with(
        &mut self,
        row: usize,
        col: usize,
        initial: Option<&str>,
        cx: &mut Context<TableState<Self>>,
    ) {
        if col >= self.columns.len() || self.is_sel(col) || row >= self.shown() {
            return;
        }
        let key = self.columns[col].key.clone();
        if self.col_type(&key) == Some("bool") && initial.is_none() {
            let cur = self.row_value(row).and_then(|r| r.get(&key).cloned());
            let next = Value::Bool(!matches!(cur, Some(Value::Bool(true))));
            *self.shared.edit_error.borrow_mut() = self
                .commit_value(row, &key, next)
                .err()
                .map(|m| format!("{key}: {m}"));
            let g = self.generation;
            self.invalidate(g);
            cx.notify();
            return;
        }
        let text = match initial {
            Some(i) => i.to_string(),
            None => {
                let text = TableDelegate::cell_text(self, row, col, cx);
                if self
                    .row_value(row)
                    .is_some_and(|r| r.get(&key).is_none_or(Value::is_null))
                {
                    String::new()
                } else {
                    text
                }
            }
        };
        *self.shared.edit_error.borrow_mut() = None;
        let input = cx.new(|cx| {
            TextInput::new(cx)
                .flush()
                .size(InputSize::Small)
                .text_selected(&text)
        });
        let sub = cx.subscribe(
            &input,
            |state: &mut TableState<DatasetTable>, _, ev: &InputEvent, cx| match ev {
                InputEvent::PressEnter { .. } => {
                    Self::finish_edit(state, true, false, EditMove::Down, cx)
                }
                InputEvent::Tab { reverse } => Self::finish_edit(
                    state,
                    true,
                    false,
                    if *reverse {
                        EditMove::Prev
                    } else {
                        EditMove::Next
                    },
                    cx,
                ),
                InputEvent::Blur => Self::finish_edit(state, true, true, EditMove::Stay, cx),
                InputEvent::Escape => Self::finish_edit(state, false, false, EditMove::Stay, cx),
                _ => {}
            },
        );
        self.editing = Some(CellEdit {
            row,
            col,
            input,
            focus_pending: true,
            _sub: sub,
        });
        cx.notify();
    }

    /// Write one value to a displayed row through the engine and record it for undo. The error
    /// is the engine's (type, required, unique).
    fn commit_value(&mut self, row: usize, key: &str, val: Value) -> Result<(), String> {
        let old = self.row_value(row).and_then(|r| r.get(key).cloned());
        if old.as_ref() == Some(&val) || (old.is_none() && val.is_null()) {
            return Ok(());
        }
        let ix = self.actual_index(row);
        self.engine
            .dataset_update_cell(&self.server_id, &self.name, ix, key, val.clone())
            .map_err(|e| e.to_string())?;
        self.shared.undo.borrow_mut().push(CellOp {
            live: ix,
            column: key.to_string(),
            old: old.unwrap_or(Value::Null),
            new: val,
        });
        self.shared.redo.borrow_mut().clear();
        Ok(())
    }

    /// Commit (or cancel) the open edit. An invalid value keeps the editor open with the reason
    /// shown unless `lenient` (blur), which discards it. `mv` moves to the next cell.
    pub(crate) fn finish_edit(
        state: &mut TableState<DatasetTable>,
        commit: bool,
        lenient: bool,
        mv: EditMove,
        cx: &mut Context<TableState<DatasetTable>>,
    ) {
        let d = state.delegate_mut();
        let Some((row, col, input)) = d.editing.as_ref().map(|e| (e.row, e.col, e.input.clone()))
        else {
            return;
        };
        if commit {
            let text = input.read(cx).value().to_string();
            let key = d.columns[col].key.clone();
            let outcome =
                parse_cell(d.col_type(&key), &text).and_then(|val| d.commit_value(row, &key, val));
            if let Err(m) = outcome
                && !lenient
            {
                input.update(cx, |i, cx| i.set_invalid(true, cx));
                *d.shared.edit_error.borrow_mut() = Some(format!("{key}: {m}"));
                cx.notify();
                return;
            }
        }
        *d.shared.edit_error.borrow_mut() = None;
        d.editing = None;
        d.shared.refocus.set(true);
        let g = d.generation;
        d.invalidate(g);
        let (cols, rows) = (d.columns.len(), d.shown());
        let target = match mv {
            EditMove::Stay => None,
            EditMove::Down => (row + 1 < rows).then_some((row + 1, col)),
            EditMove::Next => step_cell(row, col, cols, rows, true),
            EditMove::Prev => step_cell(row, col, cols, rows, false),
        };
        state.refresh(cx);
        if let Some((r, c)) = target {
            state.delegate_mut().begin_edit(r, c, cx);
        }
        cx.notify();
    }

    pub fn column_keys(&self) -> Vec<String> {
        self.columns
            .iter()
            .filter(|c| c.key != SEL_KEY)
            .map(|c| c.key.clone())
            .collect()
    }

    /// `widths` has one entry per displayed column, the checkbox column included.
    pub fn set_widths(&mut self, widths: &[f32]) {
        for (c, w) in self.columns.iter_mut().zip(widths) {
            c.width = *w;
            if c.key != SEL_KEY {
                self.widths.insert(c.key.clone(), *w);
            }
        }
    }

    fn apply_sort(&mut self, key: &str, s: ColumnSort) {
        if s == ColumnSort::Default || s == ColumnSort::None {
            self.sort = None;
        } else {
            self.sort = Some((key.to_string(), s));
        }
        self.rebuild_view();
    }

    /// Index in the live rows of displayed row `i` (differs while sorted).
    pub fn actual_index(&self, i: usize) -> usize {
        self.perm
            .as_ref()
            .and_then(|p| p.get(i).copied())
            .unwrap_or(i)
    }

    fn ensure(&mut self, i: usize) {
        if self.snapshot.is_some() {
            return;
        }
        let page = i / PAGE;
        if !self.pages.contains_key(&page) {
            let rows = self
                .engine
                .dataset_page(&self.server_id, &self.name, page * PAGE, PAGE)
                .map(|(_, r)| r)
                .unwrap_or_default();
            self.pages.insert(page, rows);
        }
    }

    pub fn row_value(&self, i: usize) -> Option<Value> {
        let ix = self.actual_index(i);
        if let Some(s) = &self.snapshot {
            return s.get(ix).cloned();
        }
        self.pages
            .get(&(ix / PAGE))
            .and_then(|p| p.get(ix % PAGE))
            .cloned()
            .or_else(|| {
                self.engine
                    .dataset_page(&self.server_id, &self.name, ix, 1)
                    .ok()
                    .and_then(|(_, mut r)| r.pop())
            })
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

impl TableDelegate for DatasetTable {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.shown()
    }

    fn column(&self, ix: usize, _: &App) -> Column {
        self.columns[ix].clone()
    }

    fn render_td(
        &mut self,
        row: usize,
        col: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> AnyElement {
        if let Some(ed) = self.editing.as_mut()
            && ed.row == row
            && ed.col == col
        {
            if ed.focus_pending {
                ed.focus_pending = false;
                ed.input.read(cx).focus(window);
            }
            return crate::dev_id!(
                div().size_full().child(ed.input.clone()),
                format!("ds-edit-{row}-{col}"),
                Input,
                "Edit cell",
                None
            )
            .into_any_element();
        }
        self.ensure(self.actual_index(row));
        if self.is_sel(col) {
            let t = crate::theme::theme(cx).clone();
            let live = self.actual_index(row);
            let on = self.selection.contains(live);
            return crate::dev_id!(
                div()
                    .id(crate::ui::eid(format!("ds-select-{row}")))
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(crate::ui::checkbox(&t, on))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |state, _, _, cx| {
                            state.delegate_mut().selection.toggle(live);
                            state.refresh(cx);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    ),
                format!("ds-select-{row}"),
                Button,
                "Select row",
                Press(format!("ds-select:{row}"))
            )
            .into_any_element();
        }
        let key = self.columns[col].key.clone();
        let v = self
            .row_value(row)
            .and_then(|r| r.get(&key).cloned())
            .unwrap_or(Value::Null);
        let t = crate::theme::theme(cx).clone();
        let shared = self.shared.clone();
        let shared2 = self.shared.clone();
        div()
            .size_full()
            .flex()
            .items_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .font_family(BUFFER_FONT)
            .text_size(px(11.))
            .text_color(if v.is_null() { t.text_muted } else { t.text })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |state, ev: &gpui::MouseDownEvent, _, cx| {
                    shared.cell.set(Some((row, col)));
                    shared
                        .pointer
                        .set((f32::from(ev.position.x), f32::from(ev.position.y)));
                    let m = ev.modifiers;
                    state
                        .delegate()
                        .shared
                        .mod_click
                        .set(m.shift || m.secondary());
                    if (m.shift || m.secondary()) && ev.click_count == 1 {
                        let d = state.delegate_mut();
                        let display = d.display();
                        d.selection.click(&display, row, m.shift, m.secondary());
                        state.refresh(cx);
                        cx.notify();
                    }
                }),
            )
            .on_mouse_down(MouseButton::Right, move |ev, _, _| {
                shared2.cell.set(Some((row, col)));
                shared2
                    .pointer
                    .set((f32::from(ev.position.x), f32::from(ev.position.y)));
            })
            .child(cell_text(&v))
            .into_any_element()
    }

    fn perform_sort(
        &mut self,
        col: usize,
        sort: ColumnSort,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        let key = self.columns[col].key.clone();
        for (i, c) in self.columns.iter_mut().enumerate() {
            c.sort = if i == col { sort } else { ColumnSort::Default };
        }
        self.apply_sort(&key, sort);
        if sort == ColumnSort::Default {
            self.sort = None;
        }
    }

    fn fit_column(&self, col: usize, _: &App) -> Option<f32> {
        let key = &self.columns.get(col)?.key;
        if key == SEL_KEY {
            return Some(SEL_W);
        }
        let rows = self
            .engine
            .dataset_page(&self.server_id, &self.name, 0, 1000)
            .map(|(_, r)| r)
            .unwrap_or_default();
        let widest = rows
            .iter()
            .map(|r| {
                cell_text(r.get(key).unwrap_or(&Value::Null))
                    .chars()
                    .count()
            })
            .max()
            .unwrap_or(0);
        // The header text plus the sort icon (0.75 rem + 2 px padding each side).
        let header = key.chars().count() as f32 * 6.5 + 16.0;
        Some((widest as f32 * CHAR_W).max(header))
    }

    fn move_column(&mut self, from: usize, to: usize, _: &mut Context<TableState<Self>>) {
        let c = self.columns.remove(from);
        self.columns.insert(to, c);
        self.order = self.column_keys();
    }

    fn cell_text(&self, row: usize, col: usize, _: &App) -> String {
        let key = &self.columns[col].key;
        if key == SEL_KEY {
            return String::new();
        }
        self.row_value(row)
            .and_then(|r| r.get(key).map(cell_text))
            .unwrap_or_default()
    }

    fn cell_label(&self, row: usize, col: usize, cx: &App) -> Option<String> {
        if self.is_sel(col) {
            return None;
        }
        Some(TableDelegate::cell_text(self, row, col, cx))
    }

    fn render_th(
        &mut self,
        col: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Option<AnyElement> {
        if !self.is_sel(col) {
            return None;
        }
        let t = crate::theme::theme(cx).clone();
        let on = self.selection.all_selected(&self.display());
        Some(
            crate::dev_id!(
                div()
                    .id("ds-select-all")
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(crate::ui::checkbox(&t, on))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|state, _, _, cx| {
                            let d = state.delegate_mut();
                            let all = d.display();
                            d.selection.toggle_all(&all);
                            state.refresh(cx);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    ),
                "ds-select-all",
                Button,
                "Select all rows",
                Action("select_all_dataset_rows")
            )
            .into_any_element(),
        )
    }

    fn render_tr(
        &mut self,
        row: usize,
        tr: gpui::Stateful<gpui::Div>,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> gpui::Stateful<gpui::Div> {
        let t = crate::theme::theme(cx).clone();
        if self.selection.contains(self.actual_index(row)) {
            tr.bg(t.element_selected)
        } else {
            tr
        }
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Option<AnyElement> {
        let t = crate::theme::theme(cx).clone();
        let filtered = !self.filter.is_empty();
        let add = crate::dev_id!(
            div()
                .id("ds-add-row-inline")
                .h(px(28.))
                .px(px(12.))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded(px(4.))
                .cursor_pointer()
                .text_color(t.text)
                .hover(|d| d.bg(t.element_hover))
                .child("+ Add row")
                .on_click(|_, w, cx| {
                    w.dispatch_action(Box::new(crate::actions::AddDatasetRow), cx)
                }),
            "ds-add-row-inline",
            Button,
            "Add row",
            Action("add_dataset_row")
        );
        Some(
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_start()
                .pt(px(6.))
                .px(px(4.))
                .gap(px(2.))
                .text_size(px(12.))
                .text_color(t.text_muted)
                .when(!filtered, |d| d.child(add))
                .child(div().px(px(12.)).child(if filtered {
                    "No rows match the filter."
                } else {
                    "No rows yet."
                }))
                .into_any_element(),
        )
    }
}

impl Shell {
    /// The DataTable of a Dataset, created on first use and refreshed when rows changed.
    pub(crate) fn dataset_table(
        &self,
        d: &Dataset,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TableState<DatasetTable>>> {
        let key = d.key();
        let generation = self.ds_gen;
        let existing = self.ds_tables.borrow().get(&key).cloned();
        let ent = match existing {
            Some(e) => e,
            None => {
                let engine = self.engine()?;
                let layout = self.layout.dataset_tables.get(&key).cloned();
                let delegate = DatasetTable::new(engine, d, layout.as_ref(), generation);
                let opts = TableOptions {
                    col_selectable: false,
                    stretch: true,
                    ..TableOptions::default()
                };
                let id = format!("ds-{key}");
                let ent = cx.new(|cx| TableState::new(id, delegate, opts, cx));
                let k = key.clone();
                let sub = cx.subscribe(&ent, move |this: &mut Shell, ent, ev: &TableEvent, cx| {
                    this.on_ds_table_event(&k, &ent, ev, cx)
                });
                self.ds_table_subs.borrow_mut().push(sub);
                // Selection changes redraw the tab (the selection bar lives in the Shell view).
                let last = std::rc::Rc::new(Cell::new((0usize, 0usize)));
                let obs = cx.observe(&ent, move |_, e, cx| {
                    let sel = &e.read(cx).delegate().selection;
                    let sig = (sel.len(), sel.set.iter().sum::<usize>());
                    if last.replace(sig) != sig {
                        cx.notify();
                    }
                });
                self.ds_table_subs.borrow_mut().push(obs);
                self.ds_tables.borrow_mut().insert(key, ent.clone());
                ent
            }
        };
        let rows = d.rows;
        let declared = d.columns.clone();
        ent.update(cx, |s, cx| {
            let changed = s.delegate_mut().set_declared(&declared);
            if changed || s.delegate().stale(rows, generation) {
                s.delegate_mut().invalidate(generation);
                s.refresh(cx);
            }
        });
        Some(ent)
    }

    fn on_ds_table_event(
        &mut self,
        key: &str,
        ent: &Entity<TableState<DatasetTable>>,
        ev: &TableEvent,
        cx: &mut Context<Self>,
    ) {
        match ev {
            TableEvent::ColumnWidthsChanged(w) => {
                let keys = ent.read(cx).delegate().column_keys();
                let w = w.clone();
                ent.update(cx, |s, _| s.delegate_mut().set_widths(&w));
                // The checkbox column is not persisted.
                let w: Vec<f32> = w.into_iter().skip(1).collect();
                let l = self
                    .layout
                    .dataset_tables
                    .entry(key.to_string())
                    .or_default();
                l.order = keys.clone();
                l.widths = keys.into_iter().zip(w).collect();
                self.save_layout();
            }
            TableEvent::MoveColumn(..) => {
                let keys = ent.read(cx).delegate().column_keys();
                self.layout
                    .dataset_tables
                    .entry(key.to_string())
                    .or_default()
                    .order = keys;
                self.save_layout();
            }
            TableEvent::SelectRow(r) => {
                self.ds_row_targets.borrow_mut().insert(key.to_string());
                // A plain click or an arrow key selects just that row; a Shift or Cmd/Ctrl click
                // was already applied to the host selection by the cell.
                let r = *r;
                ent.update(cx, |s, _| {
                    let d = s.delegate_mut();
                    if d.shared.mod_click.replace(false) {
                        return;
                    }
                    let live = d.actual_index(r);
                    d.selection.set = std::iter::once(live).collect();
                    d.selection.anchor = Some(live);
                });
                cx.notify();
            }
            TableEvent::ClearSelection => {
                self.ds_row_targets.borrow_mut().remove(key);
                ent.update(cx, |s, _| s.delegate_mut().selection.clear());
                cx.notify();
            }
            TableEvent::DoubleClickedRow(r) => {
                let col = ent
                    .read(cx)
                    .delegate()
                    .shared
                    .cell
                    .get()
                    .map_or(1, |(_, c)| c.max(1));
                let r = *r;
                ent.update(cx, |s, cx| s.delegate_mut().begin_edit(r, col, cx));
            }
            TableEvent::RightClickedRow(Some(_)) => {
                self.ds_row_targets.borrow_mut().insert(key.to_string());
                let (x, y) = ent.read(cx).delegate().shared.pointer.get();
                self.set_menu(crate::shell::RowMenu {
                    row_id: key.to_string(),
                    kind: crate::tree::RowKind::DatasetRow,
                    x,
                    y,
                });
                cx.notify();
            }
            _ => {}
        }
    }

    /// True when the open Dataset tab has a selected or right-clicked row.
    pub(crate) fn dataset_row_target_exists(&self) -> bool {
        self.tabs
            .active()
            .filter(|t| t.kind == crate::tabs::TabKind::Dataset)
            .is_some_and(|t| self.ds_row_targets.borrow().contains(&t.id))
    }

    pub(crate) fn active_dataset_row_action(&mut self, a: DsRowAction, cx: &mut Context<Self>) {
        let Some(key) = self
            .tabs
            .active()
            .filter(|t| t.kind == crate::tabs::TabKind::Dataset)
            .map(|t| t.id.clone())
        else {
            return;
        };
        self.dataset_row_action(&key, a, cx);
    }

    /// Copy cell, copy row as JSON, or delete the table's right-clicked (else selected) row.
    pub(crate) fn dataset_row_action(&mut self, key: &str, a: DsRowAction, cx: &mut Context<Self>) {
        let Some(ent) = self.ds_tables.borrow().get(key).cloned() else {
            return;
        };
        let (row, actual, value, col_key) = {
            let st = ent.read(cx);
            let Some(row) = st.right_clicked_row().or(st.selected_row()) else {
                return;
            };
            let d = st.delegate();
            let col = d
                .shared
                .cell
                .get()
                .filter(|(r, _)| *r == row)
                .map_or(1, |(_, c)| c.max(1));
            let col_key = st.model.columns.get(col).map(|c| c.key.clone());
            (row, d.actual_index(row), d.row_value(row), col_key)
        };
        let _ = row;
        // A multi-row selection that contains the target row is what the action applies to.
        let sel = ent.read(cx).delegate().selection.clone();
        let multi = sel.len() > 1 && sel.contains(actual);
        match a {
            DsRowAction::CopyCell => {
                let text = value
                    .zip(col_key)
                    .and_then(|(v, k)| v.get(&k).map(cell_text))
                    .unwrap_or_default();
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.notify("Copied cell".into(), cx);
            }
            DsRowAction::CopyRowJson if multi => self.copy_selected_rows(cx),
            DsRowAction::DuplicateRows => {
                if !multi {
                    ent.update(cx, |s, _| {
                        let sel = &mut s.delegate_mut().selection;
                        sel.set = std::iter::once(actual).collect();
                        sel.anchor = Some(actual);
                    });
                }
                self.duplicate_selected_rows(cx)
            }
            DsRowAction::DeleteRow if multi => {
                self.ds_row_targets.borrow_mut().remove(key);
                self.delete_selected_rows(cx)
            }
            DsRowAction::CopyRowJson => {
                let text = value
                    .map(|v| serde_json::to_string_pretty(&v).unwrap_or_default())
                    .unwrap_or_default();
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.notify("Copied row as JSON".into(), cx);
            }
            DsRowAction::DeleteRow => {
                ent.update(cx, |s, cx| {
                    s.clear_selection(cx);
                    s.set_right_clicked_row(None, cx);
                });
                self.ds_row_targets.borrow_mut().remove(key);
                self.delete_rows_now(key, &[actual], cx);
            }
        }
    }

    /// `ui.state` view of the open Dataset's table.
    pub fn dataset_table_view(&self, key: &str, cx: &App) -> Option<TableViewModel> {
        let ent = self.ds_tables.borrow().get(key).cloned()?;
        Some(ent.read(cx).view_model(cx))
    }
}

// ---- dev-mcp drivers: pointer events cannot be injected, so these do what the pointer would ----

#[cfg(feature = "dev-mcp")]
impl Shell {
    fn dev_table(
        &self,
        table: &str,
    ) -> Result<Entity<TableState<DatasetTable>>, fauxfield_mcp::UiError> {
        table
            .strip_prefix("ds-")
            .and_then(|k| self.ds_tables.borrow().get(k).cloned())
            .ok_or_else(|| fauxfield_mcp::UiError::Failed(format!("no table {table:?}")))
    }

    pub(crate) fn dev_table_op(
        &mut self,
        table: &str,
        op: crate::dev_mcp::tree::DtOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), fauxfield_mcp::UiError> {
        use crate::data_table::ClickMods;
        use crate::dev_mcp::tree::DtOp;
        let ent = self.dev_table(table)?;
        ent.update(cx, |s, cx| match op {
            DtOp::Header(c) => s.header_click(c, ClickMods::plain(), window, cx),
            DtOp::Row(r) => s.row_click(r, ClickMods::plain(), window, cx),
            DtOp::RowDouble(r) => s.row_click(r, ClickMods::double(), window, cx),
        });
        Ok(())
    }

    pub(crate) fn dev_table_command(
        &mut self,
        table: &str,
        op: &str,
        col: &str,
        width: Option<f32>,
        index: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Result<serde_json::Value, fauxfield_mcp::UiError> {
        let fail = |m: String| Err(fauxfield_mcp::UiError::Failed(m));
        let ent = self.dev_table(table)?;
        let Some(ix) = ent.read(cx).model.columns.iter().position(|c| c.key == col) else {
            return fail(format!("no column {col:?}"));
        };
        match op {
            "resize" => {
                let Some(w) = width else {
                    return fail("resize needs `width`".into());
                };
                ent.update(cx, |s, cx| s.resize_col(ix, w, cx));
            }
            "fit" => ent.update(cx, |s, cx| s.fit_col(ix, cx)),
            "move_column" => {
                let Some(gap) = index else {
                    return fail("move_column needs `index`".into());
                };
                let moved = ent.update(cx, |s, cx| s.move_col(ix, gap, cx));
                if !moved {
                    return fail("that move is a no-op or crosses a fixed region".into());
                }
            }
            other => return fail(format!("unknown table op {other:?}")),
        }
        Ok(serde_json::json!({ "ok": true }))
    }
}

#[cfg(test)]
mod edit_tests {
    use super::*;

    #[test]
    fn tab_and_shift_tab_walk_data_columns_and_wrap() {
        // 4 columns: checkbox (0) plus data 1..=3; 2 rows.
        assert_eq!(step_cell(0, 1, 4, 2, true), Some((0, 2)));
        assert_eq!(step_cell(0, 3, 4, 2, true), Some((1, 1)));
        assert_eq!(step_cell(1, 3, 4, 2, true), None);
        assert_eq!(step_cell(1, 1, 4, 2, false), Some((0, 3)));
        assert_eq!(step_cell(0, 1, 4, 2, false), None);
        assert_eq!(step_cell(0, 2, 4, 2, false), Some((0, 1)));
    }

    #[test]
    fn cells_parse_by_type() {
        assert_eq!(parse_cell(Some("int"), " 12 "), Ok(Value::from(12)));
        assert!(parse_cell(Some("int"), "1.5").is_err());
        assert_eq!(parse_cell(Some("int"), ""), Ok(Value::Null));
        assert_eq!(parse_cell(Some("bool"), "true"), Ok(Value::Bool(true)));
        assert!(parse_cell(Some("bool"), "yes").is_err());
        assert!(parse_cell(Some("datetime"), "tomorrow").is_err());
        assert!(parse_cell(Some("datetime"), "2026-02-03").is_ok());
        assert!(parse_cell(Some("json"), "{\"a\":").is_err());
        assert_eq!(
            parse_cell(Some("string"), ""),
            Ok(Value::String(String::new()))
        );
    }
}
