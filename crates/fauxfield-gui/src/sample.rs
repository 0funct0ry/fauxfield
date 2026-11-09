//! Sample request of a Route (SPEC §20.15): method, query, headers, cookies and body that Send
//! test uses so rules can be exercised from the GUI. Saved per Route in `.fauxfield/ui.json`
//! under `sample_requests` (runtime-only state, not a workspace file).

use fauxfield_engine::TestRequest;
use gpui::{
    AnyElement, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::*, px,
};

use crate::code_editor::{CodeEditor, Language};
use crate::response_pane::InlineTarget;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::view::{Route, Server};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sample {
    /// Empty: the Route's own method.
    pub method: String,
    /// `a=1&b=2`, without the leading `?`.
    pub query: String,
    /// `Name: value` per line.
    pub headers: String,
    /// `a=1; b=2`.
    pub cookies: String,
    pub body: String,
}

impl Sample {
    pub fn from_json(v: &serde_json::Value) -> Sample {
        let s = |k: &str| {
            v.get(k)
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .to_string()
        };
        Sample {
            method: s("method"),
            query: s("query"),
            headers: s("headers"),
            cookies: s("cookies"),
            body: s("body"),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "method": self.method, "query": self.query, "headers": self.headers,
            "cookies": self.cookies, "body": self.body,
        })
    }

    /// The request Send test fires for `route` on `server`.
    pub fn request(&self, server: &Server, route: &Route) -> TestRequest {
        let base = if route.method.eq_ignore_ascii_case("any") {
            "GET"
        } else {
            &route.method
        };
        let method = if self.method.trim().is_empty() {
            base.to_string()
        } else {
            self.method.trim().to_ascii_uppercase()
        };
        let mut path = format!(
            "{}/{}",
            server.prefix.trim_end_matches('/'),
            route.path.trim_start_matches('/')
        );
        let q = self.query.trim().trim_start_matches('?');
        if !q.is_empty() {
            path.push('?');
            path.push_str(q);
        }
        let mut headers: Vec<(String, String)> = self
            .headers
            .lines()
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .filter(|(k, _)| !k.is_empty())
            .collect();
        if !self.cookies.trim().is_empty() {
            headers.push(("cookie".into(), self.cookies.trim().to_string()));
        }
        TestRequest {
            method,
            path,
            headers,
            body: self.body.clone(),
        }
    }
}

/// The open Send test dialog: the draft of the inline fields (headers and body live in the
/// two editors).
#[derive(Clone, Debug)]
pub struct SampleDraft {
    pub route: String,
    pub sample: Sample,
}

pub(crate) fn load_sample(root: &std::path::Path, route: &str) -> Sample {
    let st = fauxfield_store::load_ui_state(root);
    st.get("sample_requests")
        .and_then(|m| m.get(route))
        .map(Sample::from_json)
        .unwrap_or_default()
}

impl Shell {
    fn sample_root(&self) -> Option<std::path::PathBuf> {
        self.store.as_ref().map(|s| s.root())
    }

    /// The saved sample of `route` (empty when none).
    pub(crate) fn sample_of(&self, route: &str) -> Sample {
        self.sample_root()
            .map(|r| load_sample(&r, route))
            .unwrap_or_default()
    }

    pub fn open_sample_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(route) = self.current_route_id() else {
            return self.notify("Open a route first.".into(), cx);
        };
        let sample = self.sample_of(&route);
        let st = self.editor_settings.clone();
        let eds: [Entity<CodeEditor>; 2] = [
            ("Name: value, one per line", &sample.headers),
            ("Request body", &sample.body),
        ]
        .map(|(ph, text)| {
            cx.new(|cx| {
                let mut e = CodeEditor::new(Language::Plain, cx)
                    .line_numbers(false)
                    .placeholder(ph);
                e.apply_settings(&st, cx);
                e.set_text(text, cx);
                e
            })
        });
        self.sample_editors = Some(eds);
        self.sample_dialog = Some(SampleDraft { route, sample });
        cx.notify();
    }

    pub(crate) fn close_sample_dialog(&mut self, cx: &mut Context<Self>) {
        if self.sample_dialog.take().is_some() {
            self.inline = None;
            self.sample_editors = None;
            cx.notify();
        }
    }

    /// Save the draft to ui.json; with `send`, fire it at the Route's Server.
    pub(crate) fn save_sample(&mut self, send: bool, w: &mut gpui::Window, cx: &mut Context<Self>) {
        self.commit_inline(crate::response_pane::Nav::Stay, w, cx);
        let (Some(mut d), Some(eds), Some(root)) = (
            self.sample_dialog.clone(),
            self.sample_editors.clone(),
            self.sample_root(),
        ) else {
            return;
        };
        d.sample.headers = eds[0].read(cx).text().to_string();
        d.sample.body = eds[1].read(cx).text().to_string();
        let mut st = fauxfield_store::load_ui_state(&root);
        if !st.is_object() {
            st = serde_json::json!({});
        }
        st["sample_requests"][&d.route] = d.sample.to_json();
        let _ = fauxfield_store::save_ui_state(&root, &st);
        self.preview_samples.remove(&d.route);
        self.close_sample_dialog(cx);
        if send {
            self.send_test(cx);
        } else {
            self.notify("Saved sample request".into(), cx);
        }
    }

    pub(crate) fn render_sample_dialog(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let (Some(d), Some(eds)) = (&self.sample_dialog, &self.sample_editors) else {
            return div().into_any_element();
        };
        let label = |s: &str| {
            div()
                .w(px(70.))
                .flex_none()
                .text_size(px(11.))
                .text_color(t.text_muted)
                .child(s.to_string())
        };
        let field = |name: &str, el: AnyElement| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .child(label(name))
                .child(el)
        };
        let editor = |name: &str, id: &'static str, h: f32, ed: &Entity<CodeEditor>| {
            div()
                .flex()
                .flex_row()
                .gap(px(8.))
                .child(label(name))
                .child(crate::dev_id!(
                    div()
                        .id(id)
                        .flex_1()
                        .h(px(h))
                        .border_1()
                        .border_color(t.border_variant)
                        .rounded(px(3.))
                        .child(ed.clone()),
                    id,
                    Input,
                    name,
                    None
                ))
        };
        let route = d.route.clone();
        let button = |id: &'static str, text: &'static str| {
            crate::ui::toolbar_button(t, crate::ui::eid(id))
                .h(px(26.))
                .px(px(12.))
                .child(text)
        };
        let inline = |id: &str, target: InlineTarget, v: &str, ph: &str, cx: &mut Context<Self>| {
            self.inline_field(t, id.to_string(), target, v, ph, 420., false, cx)
        };
        crate::dev_id_z!(
            div()
                .id("sample-dialog")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(t.background.opacity(0.6))
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .w(px(560.))
                        .p(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(t.border)
                        .bg(t.elevated)
                        .child(div().text_size(px(14.)).child("Sample request"))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(t.text_muted)
                                .child("Send test uses this request, so rules can be exercised."),
                        )
                        .child(field(
                            "Method",
                            inline(
                                "sample-method",
                                InlineTarget::SampleMethod(route.clone()),
                                &d.sample.method,
                                "route method",
                                cx,
                            ),
                        ))
                        .child(field(
                            "Query",
                            inline(
                                "sample-query",
                                InlineTarget::SampleQuery(route.clone()),
                                &d.sample.query,
                                "role=admin&page=2",
                                cx,
                            ),
                        ))
                        .child(field(
                            "Cookies",
                            inline(
                                "sample-cookies",
                                InlineTarget::SampleCookies(route.clone()),
                                &d.sample.cookies,
                                "session=abc; theme=dark",
                                cx,
                            ),
                        ))
                        .child(editor("Headers", "sample-headers", 80., &eds[0]))
                        .child(editor("Body", "sample-body", 110., &eds[1]))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .justify_end()
                                .gap(px(8.))
                                .child(crate::dev_id!(
                                    button("sample-cancel", "Cancel").on_click(
                                        cx.listener(|this, _, _, cx| this.close_sample_dialog(cx))
                                    ),
                                    "sample-cancel",
                                    Button,
                                    "Cancel",
                                    Press("sample-cancel".to_string())
                                ))
                                .child(crate::dev_id!(
                                    button("sample-save", "Save").on_click(cx.listener(
                                        |this, _, w, cx| this.save_sample(false, w, cx)
                                    )),
                                    "sample-save",
                                    Button,
                                    "Save",
                                    Press("sample-save".to_string())
                                ))
                                .child(crate::dev_id!(
                                    button("sample-send", "Save and send").on_click(
                                        cx.listener(|this, _, w, cx| this.save_sample(true, w, cx))
                                    ),
                                    "sample-send",
                                    Button,
                                    "Save and send",
                                    Press("sample-send".to_string())
                                )),
                        ),
                ),
            1,
            "sample-dialog",
            Modal,
            "Sample request",
            None
        )
        .into_any_element()
    }
}
