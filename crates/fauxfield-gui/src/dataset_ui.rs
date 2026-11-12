//! Dataset tab (M8): live table and JSON views over the running engine's rows, inline edit,
//! add and delete row, reset, export. Rows are read a window at a time (`Engine::dataset_page`)
//! so 10k-row Datasets stay smooth.

use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::*, px, uniform_list,
};
use serde_json::Value;

use crate::assets::BUFFER_FONT;
use crate::shell::{PromptAction, Shell};
use crate::theme::Theme;
use crate::ui::{icon, toolbar_button};
use crate::view::Dataset;

pub const DS_ROW_H: f32 = 24.0;
const CELL_W: f32 = 168.0;
const IDX_W: f32 = 52.0;

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
        let (total, head) = engine
            .as_ref()
            .and_then(|e| e.dataset_page(&d.server_id, &d.name, 0, 100).ok())
            .unwrap_or((d.rows, vec![]));
        let cols = columns(&head, &d.key);
        let json_mode = self.ds_json_mode;
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
            .child(div().text_size(px(12.)).text_color(muted).child(format!(
                "{} · {} {}{}",
                d.server,
                total,
                if total == 1 { "row" } else { "rows" },
                if d.persist { " · persisted" } else { "" }
            )))
            .child(div().flex_1())
            .child(crate::dev_id!(
                seg(t, "ds-view-table", "Table", !json_mode).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.ds_json_mode = false;
                        cx.notify();
                    }
                )),
                "ds-view-table",
                Button,
                "Table",
                None
            ))
            .child(crate::dev_id!(
                seg(t, "ds-view-json", "JSON", json_mode).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.ds_json_mode = true;
                        cx.notify();
                    }
                )),
                "ds-view-json",
                Button,
                "JSON",
                None
            ))
            .child(crate::dev_id!(
                toolbar_button(t, "ds-add-row")
                    .child("Add row")
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.begin_prompt(PromptAction::AddRow { key: k1.clone() }, "{}", w, cx)
                    })),
                "ds-add-row",
                Button,
                "Add row",
                None
            ))
            .child(crate::dev_id!(
                toolbar_button(t, "ds-export")
                    .child("Export…")
                    .on_click(cx.listener(move |this, _, w, cx| { this.begin_export(&k2, w, cx) })),
                "ds-export",
                Button,
                "Export",
                None
            ))
            .child(crate::dev_id!(
                toolbar_button(t, "ds-reset")
                    .child("Reset")
                    .on_click(cx.listener(move |this, _, _, cx| this.reset_dataset(&k3, cx))),
                "ds-reset",
                Button,
                "Reset to seed",
                Action("reset_state")
            ));
        let banner = d.error.clone().map(|e| {
            div()
                .px(px(12.))
                .py(px(4.))
                .text_size(px(12.))
                .text_color(t.deleted)
                .child(format!("Seed problem: {e}"))
        });
        let body: AnyElement = if total == 0 {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(12.))
                .text_color(muted)
                .child("No rows. Add one, or edit the seed file and reset.")
                .into_any_element()
        } else if json_mode {
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
            let cols2 = cols.clone();
            let (server, name, ds_key) = (d.server_id.clone(), d.name.clone(), key.clone());
            let table_w = IDX_W + cols.len() as f32 * CELL_W + 32.;
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .h(px(DS_ROW_H))
                        .flex_none()
                        .w(px(table_w))
                        .flex()
                        .items_center()
                        .px(px(8.))
                        .border_b_1()
                        .border_color(t.border_variant)
                        .text_size(px(11.))
                        .text_color(muted)
                        .child(div().w(px(IDX_W)).flex_none().child("#"))
                        .children(
                            cols.iter().map(|c| {
                                div().w(px(CELL_W)).flex_none().truncate().child(c.clone())
                            }),
                        ),
                )
                .child(
                    uniform_list(
                        "ds-table",
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
                                        let i = range.start + n;
                                        this.render_dataset_row(&t, &ds_key, i, &cols2, &r, cx)
                                    })
                                    .collect::<Vec<_>>()
                            },
                        ),
                    )
                    .track_scroll(self.ds_scroll.clone())
                    .flex_1()
                    .min_h_0(),
                )
                .into_any_element()
        };
        let _ = (hover, k4, k5);
        let used_by = div()
            .flex_none()
            .px(px(12.))
            .py(px(6.))
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .border_b_1()
            .border_color(t.border_variant)
            .text_size(px(12.))
            .child(div().text_color(muted).child("Used by"))
            .when(d.used_by.is_empty(), |x| {
                x.child(crate::dev_id!(
                    div()
                        .id(crate::ui::eid(format!("ds-unused-{}", d.key())))
                        .text_color(muted)
                        .child("nothing: no Route uses this Dataset"),
                    format!("ds-used-by-none-{}", d.key()),
                    Text,
                    "Unused",
                    None
                ))
            })
            .children(d.used_by.iter().map(|(rid, label)| {
                let rid2 = rid.clone();
                crate::dev_id!(
                    toolbar_button(t, crate::ui::eid(format!("ds-used-by-{rid}")))
                        .child(format!("Route {label}"))
                        .on_click(
                            cx.listener(move |this, _, _, cx| { this.click_row(&rid2, 2, cx) })
                        ),
                    format!("ds-used-by-{rid}"),
                    Button,
                    label.clone(),
                    Row(rid.clone())
                )
            }));
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
            .child(used_by)
            .children(generator)
            .children(banner)
            .child(body)
    }

    fn render_dataset_row(
        &self,
        t: &Theme,
        key: &str,
        i: usize,
        cols: &[String],
        row: &Value,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hover = t.element_hover;
        let muted = t.text_muted;
        let del_key = key.to_string();
        div()
            .id(crate::ui::eid(format!("ds-row-{i}")))
            .h(px(DS_ROW_H))
            .px(px(8.))
            .flex()
            .flex_row()
            .items_center()
            .font_family(BUFFER_FONT)
            .text_size(px(11.))
            .hover(move |s| s.bg(hover))
            .child(
                div()
                    .w(px(IDX_W))
                    .flex_none()
                    .text_color(muted)
                    .child((i + 1).to_string()),
            )
            .children(cols.iter().map(|c| {
                let v = row.get(c).cloned().unwrap_or(Value::Null);
                let (k, col) = (key.to_string(), c.clone());
                let initial = serde_json::to_string(&v).unwrap_or_default();
                div()
                    .id(crate::ui::eid(format!("ds-cell-{i}-{c}")))
                    .w(px(CELL_W))
                    .flex_none()
                    .truncate()
                    .cursor_pointer()
                    .child(cell_text(&v))
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.begin_prompt(
                            PromptAction::EditCell {
                                key: k.clone(),
                                index: i,
                                col: col.clone(),
                            },
                            &initial,
                            w,
                            cx,
                        )
                    }))
            }))
            .child(
                div()
                    .id(crate::ui::eid(format!("ds-del-{i}")))
                    .size(px(20.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_color(muted)
                    .child("×")
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.delete_dataset_row(&del_key, i, cx)),
                    ),
            )
            .into_any_element()
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

    pub(crate) fn delete_dataset_row(&mut self, key: &str, i: usize, cx: &mut Context<Self>) {
        self.edit_dataset(key, cx, move |rows| match rows {
            Value::Array(a) if i < a.len() => {
                a.remove(i);
                Ok(())
            }
            _ => Err("That row no longer exists.".into()),
        });
    }

    pub(crate) fn begin_export(&mut self, key: &str, w: &mut gpui::Window, cx: &mut Context<Self>) {
        let Some(d) = self.ws.find_dataset(key) else {
            return;
        };
        let path = self.ws.root.join(format!("{}.export.json", d.name));
        self.begin_prompt(
            PromptAction::ExportDataset {
                key: key.to_string(),
            },
            &path.to_string_lossy(),
            w,
            cx,
        );
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
                std::fs::write(
                    path,
                    serde_json::to_string_pretty(&rows).unwrap_or_default(),
                )
                .map_err(|x| format!("{path}: {x}"))
            });
        match res {
            Ok(()) => self.notify(format!("Exported {} to {path}", d.name), cx),
            Err(m) => self.notify(m, cx),
        }
    }

    /// New Dataset… prompt: an empty Dataset (no seed, no Route) in the current Server.
    pub(crate) fn begin_new_dataset(&mut self, w: &mut gpui::Window, cx: &mut Context<Self>) {
        let server = self
            .current_server_id()
            .or_else(|| self.ws.servers.first().map(|s| s.id.clone()));
        match server {
            Some(server) => self.begin_prompt(PromptAction::NewDataset { server }, "", w, cx),
            None => self.notify("Create a server first.".into(), cx),
        }
    }

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
