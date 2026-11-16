//! Docs editing (M15 step 4 without the live preview, plus the M17.06 OpenAPI fields): a Markdown
//! editor for the `docs` of a Server, Folder and Route, and the Route's `[routes.openapi]`
//! overrides. The generated API documentation reads all of them (SPEC §12.4).

use std::collections::HashMap;

use gpui::{
    AnyElement, Context, Entity, InteractiveElement, IntoElement, ParentElement, Styled, div,
    prelude::*, px,
};

use crate::code_editor::{CodeEditor, EditorEvent, Language};
use crate::copy;
use crate::response_pane::InlineTarget;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::view::Route;
use fauxfield_model::RouteOpenapi;
use fauxfield_store::Target;

/// One editor per documented item (`server:<id>`, `folder:<id>`, `route:<id>`) and the text it
/// last showed or saved. An outside edit replaces the text only while the user has no unsaved typing.
pub(crate) type DocsEditors = HashMap<String, (Entity<CodeEditor>, String)>;

/// `[routes.openapi]` fields edited as one-line inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenapiField {
    Summary,
    Tags,
    OperationId,
    Path,
    RequestSchema,
}

impl OpenapiField {
    pub const ALL: [OpenapiField; 5] = [
        Self::Summary,
        Self::Tags,
        Self::OperationId,
        Self::Path,
        Self::RequestSchema,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Summary => copy::OPENAPI_SUMMARY,
            Self::Tags => copy::OPENAPI_TAGS,
            Self::OperationId => copy::OPENAPI_OPERATION_ID,
            Self::Path => copy::OPENAPI_PATH,
            Self::RequestSchema => copy::OPENAPI_REQUEST_SCHEMA,
        }
    }
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Summary => "openapi.summary",
            Self::Tags => "openapi.tags",
            Self::OperationId => "openapi.operation_id",
            Self::Path => "openapi.path",
            Self::RequestSchema => "openapi.request_schema",
        }
    }
    fn placeholder(self) -> &'static str {
        match self {
            Self::Summary => "List orders",
            Self::Tags => "Orders, Billing",
            Self::OperationId => "list_orders",
            Self::Path => "/orders/{id} (needed for regex Routes)",
            Self::RequestSchema => "schemas/order.json",
        }
    }
    fn value(self, o: &RouteOpenapi) -> String {
        match self {
            Self::Summary => o.summary.clone().unwrap_or_default(),
            Self::Tags => o.tags.join(", "),
            Self::OperationId => o.operation_id.clone().unwrap_or_default(),
            Self::Path => o.path.clone().unwrap_or_default(),
            Self::RequestSchema => o.request_schema.clone().unwrap_or_default(),
        }
    }
}

fn confined(p: &str) -> bool {
    !p.starts_with('/') && !p.contains('\\') && !p.split('/').any(|c| c == "..")
}

impl Shell {
    /// The Markdown editor for `key`, created on first use. `current` is the file's text.
    pub(crate) fn docs_editor(
        &self,
        key: &str,
        current: &str,
        cx: &mut Context<Self>,
    ) -> Entity<CodeEditor> {
        if let Some((ed, synced)) = self.docs_editors.borrow_mut().get_mut(key) {
            // A change on disk shows up unless the user has typed something not saved yet.
            if synced != current {
                let unsaved = ed.read(cx).text() != synced.as_str();
                if !unsaved {
                    ed.update(cx, |e, cx| e.set_text(current, cx));
                }
                *synced = current.to_string();
            }
            return ed.clone();
        }
        let st = self.editor_settings.clone();
        let ed = cx.new(|cx| {
            let mut e = CodeEditor::new(Language::Markdown, cx)
                .line_numbers(false)
                .placeholder(copy::DOCS_EDITOR_PLACEHOLDER);
            e.apply_settings(&st, cx);
            e
        });
        ed.update(cx, |e, cx| e.set_text(current, cx));
        let k = key.to_string();
        cx.subscribe(&ed, move |this, _, ev: &EditorEvent, cx| {
            if matches!(ev, EditorEvent::Blurred) {
                this.commit_docs(&k, cx);
            }
        })
        .detach();
        self.docs_editors
            .borrow_mut()
            .insert(key.to_string(), (ed.clone(), current.to_string()));
        ed
    }

    /// Write the editor's text to the item's `docs` key (an empty text removes the key).
    pub(crate) fn commit_docs(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some((ed, synced)) = self.docs_editors.borrow().get(key).cloned() else {
            return;
        };
        let text = ed.read(cx).text().to_string();
        if text == synced {
            return;
        }
        let Some((kind, id)) = key.split_once(':') else {
            return;
        };
        let target = match kind {
            "server" => Target::Server(id.into()),
            "folder" => Target::Folder(id.into()),
            _ => Target::Route(id.into()),
        };
        if let Some(e) = self.docs_editors.borrow_mut().get_mut(key) {
            e.1 = text.clone();
        }
        self.store_op(cx, |s| {
            s.set_str(
                &target,
                "docs",
                Some(text.as_str()).filter(|t| !t.trim().is_empty()),
            )
        });
    }

    /// The Markdown editor with a heading and a hint, for the Route, Folder and Server tabs.
    pub(crate) fn docs_block(
        &self,
        t: &Theme,
        key: String,
        dev_id: &str,
        current: &str,
        hint: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ed = self.docs_editor(&key, current, cx);
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child(hint),
            )
            .child(crate::dev_id!(
                div()
                    .id(crate::ui::eid(format!("{dev_id}-box")))
                    .h(px(240.))
                    .w_full()
                    .border_1()
                    .border_color(t.border_variant)
                    .rounded(px(4.))
                    .overflow_hidden()
                    .cursor_text()
                    .child(ed),
                dev_id,
                Input,
                copy::DOCS_EDITOR_LABEL,
                None
            ))
            .into_any_element()
    }

    /// The Route's Docs sub-tab: Markdown docs, then the `[routes.openapi]` overrides.
    pub(crate) fn render_route_docs(
        &self,
        t: &Theme,
        r: &Route,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let heading = |s: &str| {
            div()
                .mt(px(8.))
                .pb(px(4.))
                .text_size(px(11.))
                .text_color(t.text_muted)
                .child(s.to_uppercase())
        };
        let rid = r.id.clone();
        let hidden = r.openapi.hidden;
        let fields = OpenapiField::ALL.map(|f| {
            let el = self.inline_field(
                t,
                f.id().into(),
                InlineTarget::Openapi {
                    route: r.id.clone(),
                    field: f,
                },
                &f.value(&r.openapi),
                f.placeholder(),
                -1.,
                true,
                cx,
            );
            self.https_label(t, f.label(), el)
        });
        div()
            .id(crate::ui::eid(format!("route-docs-{}", r.id)))
            .size_full()
            .overflow_y_scroll()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(heading(copy::DOCS_SECTION))
            .child(self.docs_block(
                t,
                format!("route:{}", r.id),
                "route-docs",
                &r.docs,
                copy::DOCS_ROUTE_HINT,
                cx,
            ))
            .child(heading(copy::OPENAPI_SECTION))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child(copy::OPENAPI_HINT),
            )
            .child(
                self.https_label(
                    t,
                    copy::OPENAPI_HIDDEN,
                    crate::dev_id!(
                        self.state_button(
                            t,
                            format!("openapi-hidden-{}", r.id),
                            if hidden { "On" } else { "Off" }.into(),
                            hidden,
                            70.,
                            move |this, cx| this.toggle_openapi_hidden(&rid, cx),
                            cx,
                        ),
                        "openapi.hidden",
                        Button,
                        if hidden { "On" } else { "Off" },
                        Press(format!("openapi.hidden:{}", r.id))
                    )
                    .into_any_element(),
                ),
            )
            .children(fields)
    }

    pub(crate) fn toggle_openapi_hidden(&mut self, route: &str, cx: &mut Context<Self>) {
        let on = self
            .ws
            .find_route(route)
            .is_some_and(|(_, r)| r.openapi.hidden);
        let rid = route.to_string();
        self.store_op(cx, |s| s.set_route_openapi_bool(&rid, "hidden", !on));
    }

    pub(crate) fn commit_openapi_field(
        &mut self,
        route: &str,
        f: OpenapiField,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let v = text.trim();
        let rid = route.to_string();
        match f {
            OpenapiField::Tags => {
                let items: Vec<String> = v
                    .split(',')
                    .map(str::trim)
                    .filter(|x| !x.is_empty())
                    .map(String::from)
                    .collect();
                self.store_op(cx, |s| s.set_route_openapi_list(&rid, "tags", &items));
            }
            _ => {
                if f == OpenapiField::OperationId && v.contains(char::is_whitespace) {
                    return self.notify(copy::OPENAPI_OPERATION_ID_INVALID.into(), cx);
                }
                if f == OpenapiField::Path && !v.is_empty() && !v.starts_with('/') {
                    return self.notify(copy::OPENAPI_PATH_INVALID.into(), cx);
                }
                if f == OpenapiField::RequestSchema && !v.is_empty() && !confined(v) {
                    return self.notify(copy::SCHEMA_PATH_INVALID.into(), cx);
                }
                let key = match f {
                    OpenapiField::Summary => "summary",
                    OpenapiField::OperationId => "operation_id",
                    OpenapiField::Path => "path",
                    _ => "request_schema",
                };
                self.store_op(cx, |s| {
                    s.set_route_openapi_str(&rid, key, (!v.is_empty()).then_some(v))
                });
            }
        }
    }

    /// Response `schema` (a JSON Schema file inside the workspace).
    pub(crate) fn commit_response_schema(
        &mut self,
        response: &str,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let v = text.trim();
        if !v.is_empty() && !confined(v) {
            return self.notify(copy::SCHEMA_PATH_INVALID.into(), cx);
        }
        let id = response.to_string();
        self.store_op(cx, |s| {
            s.set_str(
                &Target::Response(id),
                "schema",
                (!v.is_empty()).then_some(v),
            )
        });
    }
}
