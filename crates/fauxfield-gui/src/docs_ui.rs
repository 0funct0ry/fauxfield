//! API documentation (M17.06, SPEC §12.4): the Server tab's "API documentation" section, the
//! docs actions (toggle, open, copy URL, export OpenAPI) and the field commits.

use fauxfield_engine::OpenapiInfo;
use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
    div, prelude::*, px,
};

use crate::copy;
use crate::https_ui::HttpsField;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::ui::toolbar_button;
use crate::view::Server;

type DocsHandler = Box<dyn Fn(&mut Shell, &mut Context<Shell>)>;

/// `docs_site.path` rules of SPEC §12.4 (E113).
pub(crate) fn docs_path_ok(p: &str) -> bool {
    p.starts_with('/') && p != "/" && !p.ends_with('/') && !p.contains([':', '*', '{', '}'])
}

impl Shell {
    /// The Server's OpenAPI state from the engine (works while the Server is stopped).
    pub(crate) fn docs_info(&self, server: &str, document: bool) -> Option<OpenapiInfo> {
        self.engine()?.openapi_get(server, document).ok()
    }

    fn docs_url(&self, server: &str) -> Option<String> {
        self.docs_info(server, false).and_then(|i| i.url)
    }

    pub(crate) fn toggle_docs(&mut self, server: &str, cx: &mut Context<Self>) {
        let on = self
            .ws
            .find_server(server)
            .and_then(|s| s.docs_site.as_ref().map(|d| d.enabled))
            .unwrap_or(false);
        let sid = server.to_string();
        self.store_op(cx, |s| {
            s.set_server_table_bool(&sid, "docs_site", "enabled", !on)
        });
    }

    pub(crate) fn open_docs(&mut self, server: &str, cx: &mut Context<Self>) {
        match self.docs_url(server) {
            Some(url) => cx.open_url(&url),
            None => self.notify(copy::DOCS_NOT_RUNNING.into(), cx),
        }
    }

    pub(crate) fn copy_docs_url(&mut self, server: &str, cx: &mut Context<Self>) {
        match self.docs_url(server) {
            Some(url) => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(url));
                self.notify(copy::DOCS_URL_COPIED.into(), cx);
            }
            None => self.notify(copy::DOCS_NOT_RUNNING.into(), cx),
        }
    }

    pub(crate) fn copy_openapi_json(&mut self, server: &str, cx: &mut Context<Self>) {
        match self.docs_info(server, true).and_then(|i| i.document) {
            Some(doc) => {
                let text = serde_json::to_string_pretty(&doc).unwrap_or_default();
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                self.notify(copy::DOCS_JSON_COPIED.into(), cx);
            }
            None => self.notify(copy::DOCS_NO_DOCUMENT.into(), cx),
        }
    }

    /// File dialog, then write the document; works while the Server is stopped.
    pub(crate) fn export_openapi(&mut self, server: &str, cx: &mut Context<Self>) {
        let Some(doc) = self.docs_info(server, true).and_then(|i| i.document) else {
            return self.notify(copy::DOCS_NO_DOCUMENT.into(), cx);
        };
        let name = self
            .ws
            .find_server(server)
            .map(|s| format!("{}-openapi.json", s.name.to_lowercase().replace(' ', "-")))
            .unwrap_or_else(|| "openapi.json".into());
        let dir = self.ws.root.clone();
        let rx = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = rx.await else { return };
            let text = serde_json::to_string_pretty(&doc).unwrap_or_default();
            let res = std::fs::write(&path, format!("{text}\n"));
            this.update(cx, |this, cx| match res {
                Ok(()) => this.notify(
                    copy::fill(copy::DOCS_SAVED, &[&path.display().to_string()]),
                    cx,
                ),
                Err(e) => this.notify(copy::fill(copy::DOCS_SAVE_FAILED, &[&e.to_string()]), cx),
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn commit_docs_field(
        &mut self,
        server: &str,
        f: HttpsField,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let v = text.trim();
        let (key, label) = match f {
            HttpsField::DocsPath => ("path", "path"),
            HttpsField::DocsTitle => ("title", "title"),
            HttpsField::DocsVersion => ("version", "version"),
            _ => ("description", "description"),
        };
        if f == HttpsField::DocsPath && !v.is_empty() && !docs_path_ok(v) {
            return self.notify(copy::DOCS_PATH_INVALID.into(), cx);
        }
        let _ = label;
        let sid = server.to_string();
        self.store_op(cx, |s| {
            s.set_server_table_str(&sid, "docs_site", key, (!v.is_empty()).then_some(v))
        });
    }

    pub(crate) fn server_docs_section(
        &self,
        t: &Theme,
        s: &Server,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let d = s.docs_site.clone().unwrap_or_default();
        let info = self.docs_info(&s.id, false);
        let live = info.as_ref().and_then(|i| i.url.clone());
        let sid = s.id.clone();
        let (a, b, c, e) = (sid.clone(), sid.clone(), sid.clone(), sid.clone());
        let status = match (&live, &info) {
            (Some(url), _) => url.clone(),
            (None, _) if !d.enabled => copy::DOCS_OFF.to_string(),
            _ => copy::DOCS_START_TO_PUBLISH.to_string(),
        };
        let counts = info.as_ref().map(|i| {
            copy::fill(
                copy::DOCS_COUNTS,
                &[&i.documented.to_string(), &i.skipped.to_string()],
            )
        });
        let button = |id: &'static str,
                      label: &'static str,
                      enabled: bool,
                      tip: &'static str,
                      handler: DocsHandler,
                      cx: &mut Context<Self>| {
            let el = toolbar_button(t, crate::ui::eid(format!("{id}-{}", s.id)))
                .child(label)
                .when(!enabled, |d| {
                    d.opacity(0.5)
                        .cursor_default()
                        .tooltip(crate::ui::tooltip(tip.into()))
                })
                .when(enabled, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| handler(this, cx)))
                });
            crate::dev_id!(el, id, Button, label, Press(format!("{id}:{}", s.id)))
        };
        div()
            .mt(px(14.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .mt(px(4.))
                    .pb(px(4.))
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child(copy::DOCS_TITLE),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap(px(12.))
                    .child(
                        self.https_label(
                            t,
                            copy::DOCS_PUBLISH,
                            crate::dev_id!(
                                self.state_button(
                                    t,
                                    format!("docs-enabled-{}", s.id),
                                    if d.enabled { "On" } else { "Off" }.into(),
                                    d.enabled,
                                    70.,
                                    move |this, cx| this.toggle_docs(&a, cx),
                                    cx,
                                ),
                                "docs.enabled",
                                Button,
                                if d.enabled { "On" } else { "Off" },
                                Press(format!("docs.enabled:{}", s.id))
                            )
                            .into_any_element(),
                        ),
                    )
                    .child(self.docs_field(t, s, HttpsField::DocsPath, 200., cx))
                    .child(self.docs_field(t, s, HttpsField::DocsVersion, 100., cx)),
            )
            .child(self.docs_field(t, s, HttpsField::DocsTitle, 520., cx))
            .child(self.docs_field(t, s, HttpsField::DocsDescription, 520., cx))
            .child(
                div()
                    .id(crate::ui::eid(format!("docs-status-{}", s.id)))
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child(match counts {
                        Some(c) => format!("{status} · {c}"),
                        None => status,
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(button(
                        "docs.open",
                        copy::DOCS_OPEN,
                        live.is_some(),
                        copy::DOCS_OPEN_DISABLED,
                        Box::new(move |this, cx| this.open_docs(&b, cx)),
                        cx,
                    ))
                    .child(button(
                        "docs.copy_url",
                        copy::DOCS_COPY_URL,
                        live.is_some(),
                        copy::DOCS_OPEN_DISABLED,
                        Box::new(move |this, cx| this.copy_docs_url(&c, cx)),
                        cx,
                    ))
                    .child(button(
                        "docs.copy_json",
                        copy::DOCS_COPY_JSON,
                        true,
                        "",
                        Box::new({
                            let id = sid.clone();
                            move |this, cx| this.copy_openapi_json(&id, cx)
                        }),
                        cx,
                    ))
                    .child(button(
                        "docs.save",
                        copy::DOCS_SAVE,
                        true,
                        "",
                        Box::new(move |this, cx| this.export_openapi(&e, cx)),
                        cx,
                    )),
            )
    }

    fn docs_field(
        &self,
        t: &Theme,
        s: &Server,
        f: HttpsField,
        w: f32,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        self.https_field(t, s, f, w, cx)
    }
}
