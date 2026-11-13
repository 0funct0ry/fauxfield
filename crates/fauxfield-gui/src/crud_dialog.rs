//! Create CRUD Routes… (M10.07): an opt-in modal that adds a `crud` Route for an existing
//! Dataset, with a base path, a Folder and the operations to serve.

use gpui::{
    Context, Entity, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::*, px,
};

use crate::combo_box::{ComboEvent, ComboItem, ComboLook, ComboSite, ComboSize, ComboWidth};
use crate::shell::Shell;
use crate::text_input::{InputEvent, TextInput};
use crate::theme::Theme;
use crate::ui::{eid, toolbar_button};

pub struct CrudDialog {
    pub server: String,
    pub dataset: String,
    /// Folder id, empty for none.
    pub folder: String,
    pub path: Entity<TextInput>,
    pub ops: [bool; 6],
    _sub: Subscription,
}

pub const OP_LABELS: [(&str, &str, &str); 6] = [
    ("list", "List", "GET"),
    ("get", "Get", "GET"),
    ("create", "Create", "POST"),
    ("update", "Update", "PUT"),
    ("patch", "Patch", "PATCH"),
    ("delete", "Delete", "DELETE"),
];

/// Lines of the Routes a base path and operation set generate, for the preview.
pub fn preview_routes(path: &str, ops: &[bool; 6]) -> Vec<String> {
    let base = path.trim_end_matches('/');
    OP_LABELS
        .iter()
        .zip(ops)
        .filter(|(_, on)| **on)
        .map(|((op, _, verb), _)| match *op {
            "list" | "create" => format!("{verb} {base}"),
            _ => format!("{verb} {base}/:id"),
        })
        .collect()
}

pub fn path_error(path: &str) -> Option<String> {
    let p = path.trim();
    if !p.starts_with('/') {
        Some("Path must start with `/`.".into())
    } else if p.contains(char::is_whitespace) {
        Some("Path may not contain spaces.".into())
    } else {
        None
    }
}

impl Shell {
    /// Create CRUD Routes… for the active Dataset tab.
    pub(crate) fn begin_crud_routes(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.active_dataset_key() else {
            return;
        };
        if let Some(d) = self.ws.find_dataset(&key).cloned() {
            self.begin_crud_dialog(&d.server_id, &d.name, w, cx);
        }
    }

    pub(crate) fn begin_crud_dialog(
        &mut self,
        server: &str,
        dataset: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = cx.new(|cx| {
            TextInput::new(cx)
                .placeholder("/products")
                .text_selected(&format!("/{dataset}"))
        });
        let sub = cx.subscribe_in(
            &path,
            window,
            |this: &mut Shell, _, ev: &InputEvent, w, cx| match ev {
                InputEvent::PressEnter { .. } => this.submit_crud_dialog(w, cx),
                InputEvent::Escape => this.close_crud_dialog(w, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            },
        );
        path.read(cx).focus(window);
        self.crud_dialog = Some(CrudDialog {
            server: server.into(),
            dataset: dataset.into(),
            folder: String::new(),
            path,
            ops: [true; 6],
            _sub: sub,
        });
        cx.notify();
    }

    pub(crate) fn close_crud_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.crud_dialog.take().is_some() {
            window.focus(&self.focus);
            cx.notify();
        }
    }

    pub(crate) fn crud_op(&mut self, op: &str, arg: &str, w: &mut Window, cx: &mut Context<Self>) {
        match op {
            "create" => return self.submit_crud_dialog(w, cx),
            "cancel" => return self.close_crud_dialog(w, cx),
            "toggle" => {
                if let Some(d) = self.crud_dialog.as_mut()
                    && let Some(i) = OP_LABELS.iter().position(|(o, ..)| *o == arg)
                {
                    d.ops[i] = !d.ops[i];
                }
            }
            _ => {}
        }
        cx.notify();
    }

    fn submit_crud_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.crud_dialog.as_ref() else {
            return;
        };
        let path = d.path.read(cx).value().trim().to_string();
        if path_error(&path).is_some() || !d.ops.iter().any(|o| *o) {
            return;
        }
        let ops: Vec<String> = OP_LABELS
            .iter()
            .zip(d.ops)
            .filter(|(_, on)| *on)
            .map(|((o, ..), _)| o.to_string())
            .collect();
        let (sid, name, folder) = (d.server.clone(), d.dataset.clone(), d.folder.clone());
        let spec = fauxfield_store::DatasetSpec {
            name: name.clone(),
            crud_path: Some(path.clone()),
            crud_folder: Some(folder).filter(|f| !f.is_empty()),
            crud_ops: ops,
            ..Default::default()
        };
        if let Some(route) = self.store_op(cx, |s| s.upsert_dataset(&sid, &spec)) {
            self.notify(
                if route.is_some() {
                    format!("Created CRUD Routes for {name} at {path}")
                } else {
                    format!("{name} already has a CRUD Route")
                },
                cx,
            );
            self.close_crud_dialog(window, cx);
        }
    }

    pub(crate) fn render_crud_dialog(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let d = self.crud_dialog.as_ref().expect("dialog is open");
        let path = d.path.read(cx).value().to_string();
        let perr = path_error(&path);
        let none_ops = !d.ops.iter().any(|o| *o);
        let label = |s: &'static str| div().text_size(px(12.)).text_color(t.text_muted).child(s);
        let srv = self.ws.servers.iter().find(|s| s.id == d.server);
        let combo = |cx: &mut Context<Self>,
                     key: &str,
                     items: Vec<ComboItem>,
                     value: String,
                     fallback: String,
                     on: crate::combo_box::ComboHandler| {
            self.combo_field(
                cx,
                ComboSite {
                    key: key.into(),
                    items,
                    value: Some(value.into()),
                    fallback: Some(fallback.into()),
                    color: None,
                    tip: None,
                    width: ComboWidth::Flex,
                    look: ComboLook::Field,
                    size: ComboSize::Medium,
                    on_event: on,
                },
            )
        };
        let server_combo = combo(
            cx,
            "crud-server",
            self.ws
                .servers
                .iter()
                .map(|s| ComboItem::new(s.id.clone(), s.name.clone()))
                .collect(),
            d.server.clone(),
            srv.map(|s| s.name.clone()).unwrap_or_default(),
            std::rc::Rc::new(|this, ev, _| {
                if let ComboEvent::Change(v) = ev
                    && let Some(d) = this.crud_dialog.as_mut()
                    && d.server != v.as_ref()
                {
                    d.server = v.to_string();
                    d.folder.clear();
                    d.dataset = this
                        .ws
                        .datasets
                        .iter()
                        .find(|x| x.server_id == d.server)
                        .map(|x| x.name.clone())
                        .unwrap_or_default();
                }
            }),
        );
        let dataset_combo = combo(
            cx,
            "crud-dataset",
            self.ws
                .datasets
                .iter()
                .filter(|x| x.server_id == d.server)
                .map(|x| ComboItem::plain(x.name.clone()))
                .collect(),
            d.dataset.clone(),
            d.dataset.clone(),
            std::rc::Rc::new(|this, ev, _| {
                if let ComboEvent::Change(v) = ev
                    && let Some(d) = this.crud_dialog.as_mut()
                {
                    d.dataset = v.to_string();
                }
            }),
        );
        let mut folder_items = vec![ComboItem::new("", "(none)")];
        let folder_name = srv
            .and_then(|s| s.folders.iter().find(|f| f.id == d.folder))
            .map(|f| f.name.clone())
            .unwrap_or_else(|| "(none)".into());
        if let Some(s) = srv {
            folder_items.extend(
                s.folders
                    .iter()
                    .map(|f| ComboItem::new(f.id.clone(), f.name.clone())),
            );
        }
        let folder_combo = combo(
            cx,
            "crud-folder",
            folder_items,
            d.folder.clone(),
            folder_name,
            std::rc::Rc::new(|this, ev, _| {
                if let ComboEvent::Change(v) = ev
                    && let Some(d) = this.crud_dialog.as_mut()
                {
                    d.folder = v.to_string();
                }
            }),
        );
        let toggles: Vec<_> = OP_LABELS
            .iter()
            .enumerate()
            .map(|(i, (op, text, _))| {
                let id = format!("crud-op-{op}");
                let on = d.ops[i];
                let b = crate::ui::checkbox_row(t, eid(id.clone()), on, *text)
                    .on_click(cx.listener(move |this, _, w, cx| this.crud_op("toggle", op, w, cx)));
                crate::dev_id!(b, id, Button, *text, Press(format!("crud-toggle:{op}")))
            })
            .collect();
        let lines = preview_routes(&path, &d.ops);
        let can = perr.is_none() && !none_ops && !d.dataset.is_empty();
        let create = toolbar_button(t, eid("crud-create"))
            .h(px(26.))
            .px(px(12.))
            .when(can, |b| b.bg(t.element_selected))
            .when(!can, |b| b.opacity(0.5))
            .child("Create Routes")
            .on_click(cx.listener(|this, _, w, cx| this.crud_op("create", "", w, cx)));
        let cancel = toolbar_button(t, eid("crud-cancel"))
            .h(px(26.))
            .px(px(12.))
            .child("Cancel")
            .on_click(cx.listener(|this, _, w, cx| this.crud_op("cancel", "", w, cx)));
        let path_el = crate::dev_id!(
            div().child(d.path.clone()),
            "crud-path",
            Input,
            "Base path",
            None
        );
        crate::dev_id_z!(
            div()
                .id("crud-backdrop")
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .items_center()
                .pt(px(90.))
                .bg(t.background.opacity(0.6))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .id("crud-dialog")
                        .key_context("Dialog")
                        .track_focus(&self.crud_dialog_focus)
                        .on_key_down(cx.listener(|this, ev: &KeyDownEvent, w, cx| {
                            match ev.keystroke.key.as_str() {
                                "escape" => this.close_crud_dialog(w, cx),
                                "enter" => this.submit_crud_dialog(w, cx),
                                _ => return,
                            }
                            cx.stop_propagation();
                        }))
                        .w(px(480.))
                        .p(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(t.border)
                        .bg(t.elevated)
                        .shadow_lg()
                        .child(div().text_size(px(14.)).child("Create CRUD Routes"))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(label("Server"))
                                .child(server_combo)
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(label("Dataset"))
                                .child(dataset_combo)
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(label("Folder"))
                                .child(folder_combo)
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(label("Base path"))
                                .child(path_el)
                                .when_some(perr, |c, e| {
                                    c.child(div().text_size(px(11.)).text_color(t.deleted).child(e))
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_wrap()
                                .gap(px(4.))
                                .children(toggles)
                        )
                        .child(
                            div()
                                .id("crud-preview")
                                .p(px(8.))
                                .rounded(px(4.))
                                .bg(t.surface)
                                .text_size(px(11.))
                                .font_family("monospace")
                                .children(if lines.is_empty() {
                                    vec!["Pick at least one operation.".to_string()]
                                } else {
                                    lines
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .justify_end()
                                .gap(px(8.))
                                .child(crate::dev_id!(
                                    cancel,
                                    "crud-cancel",
                                    Button,
                                    "Cancel",
                                    Press("crud-cancel".into())
                                ))
                                .child(crate::dev_id!(
                                    create,
                                    "crud-create",
                                    Button,
                                    "Create Routes",
                                    Press("crud-create".into())
                                )),
                        ),
                ),
            2,
            "crud-dialog",
            Modal,
            "Create CRUD Routes",
            None
        )
    }
}

#[cfg(feature = "dev-mcp")]
impl Shell {
    /// `ui.state` view of the Create CRUD Routes modal.
    pub(crate) fn crud_dialog_state(
        &self,
        cx: &gpui::App,
    ) -> Option<crate::dev_mcp::state::CrudDialogState> {
        let d = self.crud_dialog.as_ref()?;
        let path = d.path.read(cx).value().to_string();
        Some(crate::dev_mcp::state::CrudDialogState {
            server: d.server.clone(),
            dataset: d.dataset.clone(),
            folder: d.folder.clone(),
            ops: OP_LABELS
                .iter()
                .zip(d.ops)
                .filter(|(_, on)| *on)
                .map(|((o, ..), _)| o.to_string())
                .collect(),
            preview: preview_routes(&path, &d.ops),
            path,
        })
    }
}
