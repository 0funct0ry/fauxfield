//! Live Tera preview of the selected Response (M6, SPEC §8 and §9.5): the Response rendered
//! against the Route's saved sample request, shown below the body editor. Edits are debounced
//! (~150 ms); the render itself is cheap enough to run synchronously after the timer. Nothing
//! here touches a file: the preview is derived, runtime-only state.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use fauxfield_engine::SampleRequest;
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::*, px,
};

use crate::code_editor::{CodeEditor, Language, tera_ctx};
use crate::shell::Shell;
use crate::theme::Theme;
use crate::ui::icon;

/// Debounce between the last edit and the render.
pub const PREVIEW_DEBOUNCE_MS: u64 = 150;
/// How the Preview pane shows rendered JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewView {
    /// Re-indented (valid JSON only).
    Pretty,
    /// All whitespace removed (valid JSON only).
    Minified,
    /// Exactly the bytes the Server sends.
    Raw,
}

impl PreviewView {
    pub const ALL: [(PreviewView, &'static str); 3] = [
        (PreviewView::Pretty, "Pretty"),
        (PreviewView::Minified, "Minified"),
        (PreviewView::Raw, "Raw"),
    ];
}

/// True when the Preview toggle applies to output in `lang` (JSON, XML and HTML).
pub fn view_applies(lang: Language) -> bool {
    matches!(lang, Language::Json | Language::Xml | Language::Html)
}

/// The text the Preview pane shows for rendered `body`. Output that is not well formed, and
/// languages without a formatter, are shown as is, so broken output stays visible.
pub fn view_text(view: PreviewView, lang: Language, body: &str) -> String {
    use crate::code_editor::format;
    if view == PreviewView::Raw {
        return body.to_string();
    }
    let pretty = view == PreviewView::Pretty;
    let out = match lang {
        Language::Json if format::validate_json(body).is_ok() => {
            if pretty {
                format::format_json(body, "  ")
            } else {
                format::minify_json(body)
            }
        }
        Language::Xml | Language::Html => {
            if pretty {
                format::format_markup(body, "  ")
            } else {
                format::minify_markup(body)
            }
        }
        _ => return body.to_string(),
    };
    out.unwrap_or_else(|_| body.to_string())
}

/// Render time as shown in the pane: `0.12 ms`.
pub fn fmt_ms(ms: f64) -> String {
    format!("{ms:.2} ms")
}

/// Variable names offered by completion: Server variables and the active Environment's.
pub fn variable_names(server_vars: impl Iterator<Item = String>, env: &[String]) -> Vec<String> {
    let mut v: Vec<String> = server_vars.chain(env.iter().cloned()).collect();
    v.sort();
    v.dedup();
    v
}

impl Shell {
    /// Names of the variables in scope for the Response `id` (Server file variables, then the
    /// active Environment's).
    pub(crate) fn variable_names_for(&self, response: &str) -> Vec<String> {
        let Some((route, _, _)) = self.find_response(response) else {
            return vec![];
        };
        self.variable_names_for_route(&route)
    }

    /// Names of the variables in scope for the Route `route` (same sources).
    pub(crate) fn variable_names_for_route(&self, route: &str) -> Vec<String> {
        let Some((server, _)) = self.ws.find_route(route) else {
            return vec![];
        };
        let from_file: Vec<String> = self
            .store
            .as_ref()
            .map(|s| {
                s.snapshot()
                    .servers
                    .iter()
                    .filter(|(_, m)| m.id == server.id)
                    .flat_map(|(_, m)| m.variables.keys().cloned().collect::<Vec<_>>())
                    .collect()
            })
            .unwrap_or_default();
        let env: Vec<String> = self
            .ws
            .find_environment(&self.active_env)
            .map(|e| e.vars.iter().map(|v| v.key.clone()).collect())
            .unwrap_or_default();
        variable_names(from_file.into_iter(), &env)
    }

    /// Keep the editor's Tera diagnostics current: recomputed only when its text or the
    /// Response's `template` flag changed.
    pub(crate) fn sync_tera_diagnostics(&mut self, response: &str, cx: &mut Context<Self>) {
        let Some(ed) = self.body_editors.get(response).cloned() else {
            return;
        };
        let template = self.response_by_id(response).is_none_or(|r| r.template);
        let rev = ed.read(cx).revision();
        if self.body_diag.get(response) == Some(&(rev, template)) {
            return;
        }
        self.body_diag.insert(response.to_string(), (rev, template));
        let d = if template {
            tera_ctx::diagnostics(ed.read(cx).text())
        } else {
            vec![]
        };
        ed.update(cx, |e, cx| e.set_diagnostics(d, cx));
    }

    /// Called every frame for the selected Response: when anything the preview depends on
    /// changed, restart the debounce timer.
    pub(crate) fn ensure_preview(&mut self, response: &str, cx: &mut Context<Self>) {
        let Some((route_id, _, resp)) = self.find_response(response) else {
            return;
        };
        let Some((server, route)) = self.ws.find_route(&route_id) else {
            return;
        };
        let (fake_seed, route_headers) = (server.fake_seed, route.headers.clone());
        let text = self
            .body_editors
            .get(response)
            .map(|e| e.read(cx).text().to_string());
        let sample = crate::sample::Sample::default();
        let mut h = DefaultHasher::new();
        (
            &resp.id,
            text.as_deref().unwrap_or(&resp.body),
            &resp.headers,
            resp.template,
            &resp.content_type,
            resp.status,
            &resp.body_file,
            &route_headers,
            fake_seed,
            (
                &sample.method,
                &sample.query,
                &sample.headers,
                &sample.cookies,
                &sample.body,
            ),
        )
            .hash(&mut h);
        let key = h.finish();
        if self.preview_key == key {
            return;
        }
        self.preview_key = key;
        self.preview_epoch += 1;
        let (epoch, id) = (self.preview_epoch, response.to_string());
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(PREVIEW_DEBOUNCE_MS))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.preview_epoch == epoch {
                    this.compute_preview(&id, cx);
                }
            });
        })
        .detach();
    }

    /// Render `response` now and store the result (also feeds the rendered-body viewer).
    pub(crate) fn compute_preview(&mut self, response: &str, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let Some((route_id, _, _)) = self.find_response(response) else {
            return;
        };
        let sample = crate::sample::Sample::default();
        let Some((vs, vr)) = self.ws.find_route(&route_id) else {
            return;
        };
        let treq = sample.request(vs, vr);
        let req = SampleRequest {
            method: treq.method,
            url: treq.path,
            headers: treq.headers,
            body: treq.body,
            datasets: None,
        };
        let snap = store.snapshot();
        let found = snap.servers.iter().find_map(|(_, s)| {
            s.routes.iter().find(|r| r.id == route_id).and_then(|r| {
                r.responses
                    .iter()
                    .find(|x| x.id == response)
                    .map(|x| (s, r, x))
            })
        });
        let Some((ms, mr, mx)) = found else {
            return;
        };
        let mut mx = mx.clone();
        // The editor's live text wins over the stored body while it is being edited.
        if mx.body_file.is_none()
            && let Some(ed) = self.body_editors.get(response)
        {
            mx.body = Some(ed.read(cx).text().to_string());
        }
        let p = fauxfield_engine::preview(&snap.root, ms, mr, &mx, &req, None);
        let ct = p
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.clone())
            .or(mx.content_type.clone())
            .unwrap_or_default();
        let lang = Language::detect(&ct, &p.body);
        let shown = view_text(self.preview_view, lang, &p.body);
        let ed = self
            .body_editors
            .entry(format!("{response}#preview"))
            .or_insert_with(|| cx.new(|cx| CodeEditor::new(lang, cx).read_only(true)))
            .clone();
        let st = self.editor_settings.clone();
        ed.update(cx, |e, cx| {
            if e.text() != shown {
                e.set_text(&shown, cx);
            }
            e.set_language(lang, cx);
            e.apply_settings(&st, cx);
        });
        self.preview = Some((response.to_string(), p));
        cx.notify();
    }

    /// The collapsible Preview pane below the body editor.
    pub(crate) fn render_preview_pane(
        &self,
        t: &Theme,
        response: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = !self.preview_closed;
        let p = self
            .preview
            .as_ref()
            .filter(|(id, _)| id == response)
            .map(|(_, p)| p);
        let viewer = self
            .body_editors
            .get(&format!("{response}#preview"))
            .cloned();
        let viewer_lang = viewer.as_ref().map(|v| v.read(cx).lang());
        let json_msg = p.and_then(|p| p.json_error.clone());
        let head = div()
            .id("preview-toggle")
            .h(px(26.))
            .flex_none()
            .px(px(12.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.))
            .cursor_pointer()
            .border_t_1()
            .border_color(t.border_variant)
            .text_size(px(11.))
            .text_color(t.text_muted)
            .hover(|s| s.bg(t.element_hover))
            .on_click(cx.listener(|this, _, _, cx| {
                this.preview_closed = !this.preview_closed;
                cx.notify();
            }))
            .child(icon(
                if open {
                    "chevron-down"
                } else {
                    "chevron-right"
                },
                12.,
                t.icon_muted,
            ))
            .child(div().child("Preview"))
            .children(p.map(|p| {
                crate::dev_id!(
                    div()
                        .id("preview-status")
                        .text_color(t.status_color(p.status))
                        .child(p.status.to_string()),
                    "preview-status",
                    Text,
                    p.status.to_string(),
                    None
                )
            }))
            .children(p.map(|p| {
                let label = fmt_ms(p.render_ms);
                crate::dev_id!(
                    div().id("preview-time").child(label.clone()),
                    "preview-time",
                    Text,
                    label,
                    None
                )
            }))
            .child(div().flex_1())
            .children(
                p.filter(|_| viewer_lang.is_some_and(view_applies))
                    .map(|_| {
                        div()
                            .id("preview-view")
                            .flex()
                            .flex_row()
                            .gap(px(2.))
                            .children(PreviewView::ALL.into_iter().map(|(v, label)| {
                                let on = self.preview_view == v;
                                crate::dev_id!(
                                    div()
                                        .id(crate::ui::eid(format!("preview-view-{label}")))
                                        .px(px(6.))
                                        .rounded(px(3.))
                                        .cursor_pointer()
                                        .when(on, |d| d.bg(t.element_selected).text_color(t.text))
                                        .hover(|s| s.bg(t.element_hover))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.preview_view = v;
                                            if let Some((id, _)) = this.preview.clone() {
                                                this.compute_preview(&id, cx);
                                            }
                                        }))
                                        .child(label),
                                    format!("preview-view-{label}"),
                                    Button,
                                    label,
                                    None
                                )
                            }))
                    }),
            )
            .children(p.and_then(|p| p.error.clone()).map(|e| {
                crate::dev_id!(
                    div()
                        .id("preview-error")
                        .flex_1()
                        .truncate()
                        .text_color(t.deleted)
                        .child(format!("✗ {e}")),
                    "preview-error",
                    Text,
                    e,
                    None
                )
            }))
            .children(json_msg.map(|m| {
                crate::dev_id!(
                    div()
                        .id("preview-json-error")
                        .flex_1()
                        .truncate()
                        .text_color(t.deleted)
                        .child(format!("✗ Rendered body is not valid JSON: {m}")),
                    "preview-json-error",
                    Text,
                    m,
                    None
                )
            }));
        crate::dev_id!(
            div()
                .id("preview-pane")
                .flex_none()
                .flex()
                .flex_col()
                .child(head)
                .when(open, |d| {
                    d.child(self.resize_handle_h(t, crate::shell::Resize::Preview, cx))
                        .child(
                            div()
                                .id("preview-body")
                                .h(px(self.layout.preview_height))
                                .flex_none()
                                .children(viewer),
                        )
                }),
            "preview-pane",
            Panel,
            "Preview",
            None
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_has_two_decimals() {
        assert_eq!(fmt_ms(0.123), "0.12 ms");
        assert_eq!(fmt_ms(12.0), "12.00 ms");
    }

    #[test]
    fn view_text_pretty_minified_raw() {
        // What a formatted `for` template renders to: stray blank lines and a lone comma.
        let body = "[\n\n  {\"a\": 1,\n \"p\": 32.00}\n  ,\n\n  {\"a\": 2, \"p\": 8.00}\n]";
        assert_eq!(
            view_text(PreviewView::Pretty, Language::Json, body),
            "[\n  {\n    \"a\": 1,\n    \"p\": 32.00\n  },\n  {\n    \"a\": 2,\n    \"p\": 8.00\n  }\n]"
        );
        assert_eq!(
            view_text(PreviewView::Minified, Language::Json, body),
            r#"[{"a":1,"p":32.00},{"a":2,"p":8.00}]"#
        );
        assert_eq!(view_text(PreviewView::Raw, Language::Json, body), body);
        // Not JSON: shown unchanged in every mode.
        assert_eq!(
            view_text(PreviewView::Pretty, Language::Json, "{oops"),
            "{oops"
        );
        let xml = "<a>\n\n<b>1</b></a>";
        assert_eq!(
            view_text(PreviewView::Pretty, Language::Xml, xml),
            "<a>\n  <b>1</b>\n</a>"
        );
        assert_eq!(
            view_text(PreviewView::Minified, Language::Xml, xml),
            "<a><b>1</b></a>"
        );
        assert_eq!(
            view_text(PreviewView::Pretty, Language::Yaml, "a:   1"),
            "a:   1"
        );
        assert!(!view_applies(Language::Yaml) && view_applies(Language::Html));
    }

    #[test]
    fn variables_are_sorted_and_unique() {
        let v = variable_names(
            ["b".to_string(), "a".to_string()].into_iter(),
            &["a".into()],
        );
        assert_eq!(v, ["a", "b"]);
    }
}
