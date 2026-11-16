//! HTTPS, CORS and static-file controls (M10, SPEC §5.7 to §5.9): the Server tab's TLS and CORS
//! sections, the static Route tab with its file browser, and their commit logic.

use fauxfield_engine::tls;
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::*, px,
};

use crate::assets::BUFFER_FONT;
use crate::response_pane::InlineTarget;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::ui::toolbar_button;
use crate::view::{Route, Server};

/// Server fields edited inline in the TLS and CORS sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpsField {
    AlsoHttp,
    Cert,
    Key,
    Origins,
    Methods,
    Headers,
    MaxAge,
    /// API documentation (M17.06).
    DocsPath,
    DocsTitle,
    DocsVersion,
    DocsDescription,
}

impl HttpsField {
    fn label(self) -> &'static str {
        match self {
            Self::AlsoHttp => "Also HTTP port",
            Self::Cert => "Certificate (PEM)",
            Self::Key => "Private key (PEM)",
            Self::Origins => "Origins",
            Self::Methods => "Methods",
            Self::Headers => "Headers",
            Self::MaxAge => "Max age (s)",
            Self::DocsPath => "Path",
            Self::DocsTitle => "Title",
            Self::DocsVersion => "Version",
            Self::DocsDescription => "Description",
        }
    }
    fn id(self) -> &'static str {
        match self {
            Self::AlsoHttp => "tls-also-http",
            Self::Cert => "tls-cert",
            Self::Key => "tls-key",
            Self::Origins => "cors-origins",
            Self::Methods => "cors-methods",
            Self::Headers => "cors-headers",
            Self::MaxAge => "cors-max-age",
            Self::DocsPath => "docs.path",
            Self::DocsTitle => "docs.title",
            Self::DocsVersion => "docs.version",
            Self::DocsDescription => "docs.description",
        }
    }
    fn placeholder(self) -> &'static str {
        match self {
            Self::AlsoHttp => "none",
            Self::Cert => "certs/server.pem",
            Self::Key => "certs/server-key.pem",
            Self::Origins => "http://localhost:*, https://app.test or *",
            Self::Methods => "GET, POST, PUT, PATCH, DELETE",
            Self::Headers => "* or content-type, authorization",
            Self::MaxAge => "600",
            Self::DocsPath => "/__docs",
            Self::DocsTitle => "Server name",
            Self::DocsVersion => "1.0.0",
            Self::DocsDescription => "Markdown; defaults to the Server docs",
        }
    }
    fn value(self, s: &Server) -> String {
        let t = s.tls.clone().unwrap_or_default();
        let c = s.cors.clone().unwrap_or_default();
        let d = s.docs_site.clone().unwrap_or_default();
        match self {
            Self::AlsoHttp => t.also_http_port.map(|p| p.to_string()).unwrap_or_default(),
            Self::Cert => t.cert.unwrap_or_default(),
            Self::Key => t.key.unwrap_or_default(),
            Self::Origins => c.origins.join(", "),
            Self::Methods => c.methods.join(", "),
            Self::Headers => c.headers.join(", "),
            Self::MaxAge => c.max_age.map(|p| p.to_string()).unwrap_or_default(),
            Self::DocsPath => d.path.unwrap_or_default(),
            Self::DocsTitle => d.title.unwrap_or_default(),
            Self::DocsVersion => d.version.unwrap_or_default(),
            Self::DocsDescription => d.description.unwrap_or_default(),
        }
    }
}

/// Static Route fields edited inline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaticField {
    Index,
    TemplateExt,
}

pub const LOCALHOST_ORIGINS: [&str; 4] = [
    "http://localhost:*",
    "http://127.0.0.1:*",
    "https://localhost:*",
    "https://127.0.0.1:*",
];

fn split_list(text: &str) -> Vec<String> {
    text.split([',', ' ', '\n'])
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(String::from)
        .collect()
}

fn inside_workspace(p: &str) -> bool {
    !p.starts_with('/') && !p.contains('\\') && !p.split('/').any(|c| c == "..")
}

fn expires(c: &tls::CertStatus) -> String {
    match &c.warning {
        Some(w) => w.clone(),
        None => format!("expires in {} days", c.days_left),
    }
}

impl Shell {
    pub(crate) fn https_label(&self, t: &Theme, l: &str, el: AnyElement) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child(l.to_string()),
            )
            .child(el)
    }

    pub(crate) fn https_field(
        &self,
        t: &Theme,
        s: &Server,
        f: HttpsField,
        w: f32,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let el = self.inline_field(
            t,
            f.id().into(),
            InlineTarget::Https {
                server: s.id.clone(),
                field: f,
            },
            &f.value(s),
            f.placeholder(),
            w,
            true,
            cx,
        );
        self.https_label(t, f.label(), el)
    }

    /// A bordered On/Off or cycling button.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn state_button(
        &self,
        t: &Theme,
        id: String,
        text: String,
        active: bool,
        w: f32,
        on_click: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(crate::ui::eid(id))
            .w(px(w))
            .h(px(26.))
            .px(px(8.))
            .flex()
            .items_center()
            .rounded(px(4.))
            .border_1()
            .border_color(t.border_variant)
            .cursor_pointer()
            .hover(|s| s.border_color(t.border))
            .text_size(px(12.))
            .text_color(if active { t.text } else { t.text_muted })
            .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)))
            .child(text)
    }

    fn ca_status(&self) -> tls::CaStatus {
        let mut slot = self.ca_cache.borrow_mut();
        slot.get_or_insert_with(|| {
            let dd = tls::default_data_dir().unwrap_or_default();
            tls::ca_status(&dd)
        })
        .clone()
    }

    /// Server tab "TLS": enable, mode, files, also-HTTP port, certificate and CA trust state.
    pub(crate) fn server_tls_section(
        &self,
        t: &Theme,
        s: &Server,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let tls_cfg = s.tls.clone().unwrap_or_default();
        let on = tls_cfg.enabled;
        let files = tls_cfg.mode_name() == "files";
        let (sid_a, sid_b) = (s.id.clone(), s.id.clone());
        let ca = self.ca_status();
        let trust_label = match ca.trust {
            tls::TrustState::Trusted => "trusted on this machine",
            tls::TrustState::Untrusted => "not trusted yet",
            tls::TrustState::Unknown => "trust state unknown",
        };
        let mut d = div()
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
                    .child("TLS"),
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
                            "HTTPS",
                            crate::dev_id!(
                                self.state_button(
                                    t,
                                    format!("tls-enabled-{}", s.id),
                                    if on { "On" } else { "Off" }.into(),
                                    on,
                                    70.,
                                    move |this, cx| this.toggle_tls(&sid_a, cx),
                                    cx,
                                ),
                                "tls-enabled",
                                Button,
                                if on { "On" } else { "Off" },
                                Press(format!("tls-enabled:{}", s.id))
                            )
                            .into_any_element(),
                        ),
                    )
                    .child(
                        self.https_label(
                            t,
                            "Mode",
                            crate::dev_id!(
                                self.state_button(
                                    t,
                                    format!("tls-mode-{}", s.id),
                                    if files { "files" } else { "dev-ca" }.into(),
                                    true,
                                    90.,
                                    move |this, cx| this.cycle_tls_mode(&sid_b, cx),
                                    cx,
                                ),
                                "tls-mode",
                                Button,
                                if files { "files" } else { "dev-ca" },
                                Press(format!("tls-mode:{}", s.id))
                            )
                            .into_any_element(),
                        ),
                    )
                    .child(self.https_field(t, s, HttpsField::AlsoHttp, 120., cx)),
            );
        if files {
            d = d.child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(12.))
                    .child(self.https_field(t, s, HttpsField::Cert, 240., cx))
                    .child(self.https_field(t, s, HttpsField::Key, 240., cx)),
            );
        }
        let cert_line = match (&s.cert, s.running && on) {
            (Some(c), true) => format!("Certificate {} ({}).", expires(c), c.mode),
            _ if on && !files => "A leaf certificate for localhost, 127.0.0.1, ::1 and the host is issued by the dev CA when the Server starts.".into(),
            _ if on => "Expiry is checked when the Server starts; a warning shows 14 days ahead.".into(),
            _ => "Off: the Server speaks plain HTTP. With also HTTP port set, it serves both.".into(),
        };
        d = d.child(crate::dev_id!(
            div()
                .text_size(px(11.))
                .text_color(if s.cert.as_ref().is_some_and(|c| c.warning.is_some()) {
                    t.modified
                } else {
                    t.text_muted
                })
                .child(cert_line.clone()),
            "tls-cert-status",
            Text,
            cert_line,
            None
        ));
        if !files {
            d = d.child(self.trust_block(t, &ca, trust_label, cx));
        }
        d
    }

    /// Dev CA trust: status, the button, and the confirmation that shows the exact command.
    fn trust_block(
        &self,
        t: &Theme,
        ca: &tls::CaStatus,
        trust_label: &str,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let trusted = ca.trust == tls::TrustState::Trusted;
        let add = !trusted;
        let cmd = tls::gui_form(if add {
            &ca.trust_command
        } else {
            &ca.untrust_command
        });
        let mut d = div().flex().flex_col().gap(px(6.)).child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.))
                .child(crate::dev_id!(
                    div()
                        .text_size(px(12.))
                        .child(format!("Dev CA: {trust_label}")),
                    "tls-ca-state",
                    Text,
                    format!("Dev CA: {trust_label}"),
                    None
                ))
                .child(crate::dev_id!(
                    toolbar_button(t, "tls-trust")
                        .child(if add {
                            "Trust dev CA…"
                        } else {
                            "Untrust dev CA…"
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.trust_confirm = Some(add);
                            this.trust_note = None;
                            cx.notify();
                        })),
                    "tls-trust",
                    Button,
                    if add {
                        "Trust dev CA"
                    } else {
                        "Untrust dev CA"
                    },
                    Action("trust_dev_ca")
                )),
        );
        if self.trust_confirm.is_some() {
            let run_cmd = cmd.clone();
            d = d.child(
                div()
                    .p(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(t.border_variant)
                    .child(div().text_size(px(11.)).text_color(t.text_muted).child(
                        "This command changes your system trust store. Review it, then run:",
                    ))
                    .child(crate::dev_id!(
                        div()
                            .font_family(BUFFER_FONT)
                            .text_size(px(11.))
                            .child(cmd.clone()),
                        "tls-trust-command",
                        Text,
                        cmd.clone(),
                        None
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(8.))
                            .child(crate::dev_id!(
                                toolbar_button(t, "tls-trust-run").child("Run").on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.run_trust(run_cmd.clone(), cx)
                                    })
                                ),
                                "tls-trust-run",
                                Button,
                                "Run",
                                None
                            ))
                            .child(crate::dev_id!(
                                toolbar_button(t, "tls-trust-copy").child("Copy").on_click({
                                    let c = cmd.clone();
                                    cx.listener(move |this, _, _, cx| {
                                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                            c.clone(),
                                        ));
                                        this.notify("Copied command".into(), cx);
                                    })
                                }),
                                "tls-trust-copy",
                                Button,
                                "Copy",
                                None
                            ))
                            .child(crate::dev_id!(
                                toolbar_button(t, "tls-trust-cancel")
                                    .child("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.trust_confirm = None;
                                        cx.notify();
                                    })),
                                "tls-trust-cancel",
                                Button,
                                "Cancel",
                                None
                            )),
                    ),
            );
        }
        if let Some(n) = &self.trust_note {
            d = d.child(
                div()
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child(n.clone()),
            );
        }
        d
    }

    /// Server tab "CORS": presets, enable, credentials and the lists.
    pub(crate) fn server_cors_section(
        &self,
        t: &Theme,
        s: &Server,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let c = s.cors.clone().unwrap_or_default();
        let (sid_a, sid_b, sid_c, sid_d) = (s.id.clone(), s.id.clone(), s.id.clone(), s.id.clone());
        let preset = |this: &Self,
                      id: &'static str,
                      label: &'static str,
                      sid: String,
                      local: bool,
                      cx: &mut Context<Self>| {
            let _ = this;
            crate::dev_id!(
                toolbar_button(t, id).child(label).on_click(
                    cx.listener(move |this, _, _, cx| this.apply_cors_preset(&sid, local, cx),)
                ),
                id,
                Button,
                label,
                Press(format!("{id}:{}", s.id))
            )
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
                    .child("CORS"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(div().text_size(px(11.)).text_color(t.text_muted).child("Presets"))
                    .child(preset(self, "cors-preset-local", "Allow localhost dev servers", sid_a, true, cx))
                    .child(preset(self, "cors-preset-all", "Allow all", sid_b, false, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap(px(12.))
                    .child(self.https_label(
                        t,
                        "Enabled",
                        crate::dev_id!(
                            self.state_button(
                                t,
                                format!("cors-enabled-{}", s.id),
                                if c.enabled { "On" } else { "Off" }.into(),
                                c.enabled,
                                70.,
                                move |this, cx| this.toggle_cors(&sid_c, "enabled", cx),
                                cx,
                            ),
                            "cors-enabled",
                            Button,
                            if c.enabled { "On" } else { "Off" },
                            Press(format!("cors-enabled:{}", s.id))
                        )
                        .into_any_element(),
                    ))
                    .child(self.https_label(
                        t,
                        "Credentials",
                        crate::dev_id!(
                            self.state_button(
                                t,
                                format!("cors-credentials-{}", s.id),
                                if c.credentials { "On" } else { "Off" }.into(),
                                c.credentials,
                                70.,
                                move |this, cx| this.toggle_cors(&sid_d, "credentials", cx),
                                cx,
                            ),
                            "cors-credentials",
                            Button,
                            if c.credentials { "On" } else { "Off" },
                            Press(format!("cors-credentials:{}", s.id))
                        )
                        .into_any_element(),
                    ))
                    .child(self.https_field(t, s, HttpsField::MaxAge, 100., cx)),
            )
            .child(self.https_field(t, s, HttpsField::Origins, 520., cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(12.))
                    .child(self.https_field(t, s, HttpsField::Methods, 260., cx))
                    .child(self.https_field(t, s, HttpsField::Headers, 240., cx)),
            )
            .child(
                div().text_size(px(11.)).text_color(t.text_muted).child(
                    "Preflights are answered automatically unless a Route handles OPTIONS. * with Credentials on reflects the request origin.",
                ),
            )
    }

    // ---- commits ----------------------------------------------------------------------------

    pub(crate) fn toggle_tls(&mut self, server: &str, cx: &mut Context<Self>) {
        let on = self
            .ws
            .find_server(server)
            .and_then(|s| s.tls.as_ref().map(|t| t.enabled))
            .unwrap_or(false);
        let sid = server.to_string();
        self.store_op(cx, |s| s.set_server_table_bool(&sid, "tls", "enabled", !on));
    }

    pub(crate) fn cycle_tls_mode(&mut self, server: &str, cx: &mut Context<Self>) {
        let files = self
            .ws
            .find_server(server)
            .and_then(|s| s.tls.as_ref().map(|t| t.mode_name() == "files"))
            .unwrap_or(false);
        let sid = server.to_string();
        self.store_op(cx, |s| {
            s.set_server_table_str(&sid, "tls", "mode", (!files).then_some("files"))
        });
    }

    pub(crate) fn toggle_cors(&mut self, server: &str, key: &'static str, cx: &mut Context<Self>) {
        let c = self
            .ws
            .find_server(server)
            .and_then(|s| s.cors.clone())
            .unwrap_or_default();
        let now = if key == "enabled" {
            c.enabled
        } else {
            c.credentials
        };
        let sid = server.to_string();
        self.store_op(cx, |s| s.set_server_table_bool(&sid, "cors", key, !now));
    }

    pub(crate) fn apply_cors_preset(&mut self, server: &str, local: bool, cx: &mut Context<Self>) {
        let sid = server.to_string();
        self.store_op(cx, |s| {
            s.set_server_table_bool(&sid, "cors", "enabled", true)?;
            let origins: Vec<String> = if local {
                LOCALHOST_ORIGINS.iter().map(|x| x.to_string()).collect()
            } else {
                vec!["*".into()]
            };
            s.set_server_table_list(&sid, "cors", "origins", &origins)?;
            s.set_server_table_list(
                &sid,
                "cors",
                "methods",
                &["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"].map(String::from),
            )?;
            s.set_server_table_list(&sid, "cors", "headers", &["*".to_string()])?;
            s.set_server_table_bool(&sid, "cors", "credentials", local)
        });
    }

    pub(crate) fn commit_https_field(
        &mut self,
        server: &str,
        f: HttpsField,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let v = text.trim();
        let sid = server.to_string();
        match f {
            HttpsField::DocsPath
            | HttpsField::DocsTitle
            | HttpsField::DocsVersion
            | HttpsField::DocsDescription => self.commit_docs_field(server, f, text, cx),
            HttpsField::AlsoHttp | HttpsField::MaxAge => {
                let n = if v.is_empty() {
                    None
                } else {
                    match v.parse::<u32>() {
                        Ok(n) if f == HttpsField::MaxAge || (1..=65535).contains(&n) => Some(n),
                        _ => {
                            return self.notify(format!("{} is a whole number.", f.label()), cx);
                        }
                    }
                };
                if f == HttpsField::AlsoHttp
                    && n.is_some_and(|n| {
                        self.ws
                            .find_server(server)
                            .is_some_and(|s| u32::from(s.port) == n)
                    })
                {
                    return self
                        .notify("The HTTP port must differ from the server port.".into(), cx);
                }
                let (table, key) = if f == HttpsField::AlsoHttp {
                    ("tls", "also_http_port")
                } else {
                    ("cors", "max_age")
                };
                self.store_op(cx, |s| {
                    s.set_server_table_int(&sid, table, key, n.map(i64::from))
                });
            }
            HttpsField::Cert | HttpsField::Key => {
                if !v.is_empty() && !inside_workspace(v) {
                    return self.notify(
                        "The file must be a path inside the workspace, e.g. certs/server.pem."
                            .into(),
                        cx,
                    );
                }
                let key = if f == HttpsField::Cert { "cert" } else { "key" };
                self.store_op(cx, |s| {
                    s.set_server_table_str(&sid, "tls", key, (!v.is_empty()).then_some(v))
                });
            }
            HttpsField::Origins | HttpsField::Methods | HttpsField::Headers => {
                let key = match f {
                    HttpsField::Origins => "origins",
                    HttpsField::Methods => "methods",
                    _ => "headers",
                };
                let mut items = split_list(v);
                if f == HttpsField::Methods {
                    items.iter_mut().for_each(|m| *m = m.to_ascii_uppercase());
                }
                self.store_op(cx, |s| s.set_server_table_list(&sid, "cors", key, &items));
            }
        }
    }

    pub(crate) fn run_trust(&mut self, cmd: String, cx: &mut Context<Self>) {
        self.trust_note = Some("Running…".into());
        cx.notify();
        let bg = cx
            .background_executor()
            .spawn(async move { tls::run_command(&cmd) });
        cx.spawn(async move |this, cx| {
            let r = bg.await;
            this.update(cx, |this, cx| {
                this.trust_confirm = None;
                *this.ca_cache.borrow_mut() = None;
                this.trust_note = Some(match r {
                    Ok(()) => "Done. Restart the browser if it was open.".into(),
                    Err(e) => format!("The command failed: {e}"),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ---- static Route tab -------------------------------------------------------------------

    pub(crate) fn render_static_route_tab(
        &self,
        t: &Theme,
        s: &Server,
        r: &Route,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let root = self.store.as_ref().map(|st| st.root()).unwrap_or_default();
        let model_route = fauxfield_model::Route {
            dir: r.dir.clone(),
            template_extensions: r.template_extensions.clone(),
            ..serde_json::from_value(
                serde_json::json!({ "id": r.id, "path": r.path, "kind": "static" }),
            )
            .expect("route defaults")
        };
        let files = fauxfield_engine::static_files(&root, &model_route);
        let base = {
            let b = r.path.trim_end_matches("/*").trim_end_matches('/');
            format!("{}{}", s.prefix, b)
        };
        let rid = r.id.clone();
        let picked = self
            .static_preview
            .clone()
            .filter(|(id, _)| *id == r.id)
            .map(|(_, f)| f);
        let preview = picked.as_ref().map(|f| {
            let p = root.join(r.dir.clone().unwrap_or_default()).join(f);
            let mut text = std::fs::read(&p)
                .map(|b| String::from_utf8_lossy(&b[..b.len().min(8192)]).into_owned())
                .unwrap_or_else(|e| format!("Cannot read: {e}"));
            if text.contains('\0') {
                text = "(binary file)".into();
            }
            text
        });
        div()
            .id(crate::ui::eid(format!("route-tab-{}", r.id)))
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(40.))
                    .flex_none()
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .border_b_1()
                    .border_color(t.border_variant)
                    .child(crate::ui::method_badge(t, "FILES"))
                    .child(div().text_size(px(14.)).child(r.path.clone())),
            )
            .child(
                div()
                    .id("static-tab-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_end()
                            .gap(px(12.))
                            .child(self.https_label(
                                t,
                                "Directory",
                                crate::dev_id!(
                                    div()
                                        .h(px(26.))
                                        .px(px(8.))
                                        .flex()
                                        .items_center()
                                        .min_w(px(240.))
                                        .rounded(px(4.))
                                        .border_1()
                                        .border_color(t.border_variant)
                                        .font_family(BUFFER_FONT)
                                        .child(r.dir.clone().unwrap_or_else(|| "(not set)".into())),
                                    "static-dir",
                                    Text,
                                    r.dir.clone().unwrap_or_default(),
                                    None
                                )
                                .into_any_element(),
                            ))
                            .child(crate::dev_id!(
                                toolbar_button(t, "static-pick-dir").child("Choose directory…").on_click({
                                    let rid = rid.clone();
                                    cx.listener(move |this, _, window, cx| {
                                        this.pick_static_dir(&rid, window, cx)
                                    })
                                }),
                                "static-pick-dir",
                                Button,
                                "Choose directory",
                                Press(format!("static-pick-dir:{}", r.id))
                            ))
                            .child(self.static_field(t, r, StaticField::Index, 160., cx))
                            .child(self.static_field(t, r, StaticField::TemplateExt, 160., cx)),
                    )
                    .child(div().text_color(t.text_muted).child(format!(
                        "Serves GET and HEAD under {base}/ with MIME detection, ETag, Last-Modified and ranges. Files ending in a template extension render through Tera (page.html.tera is served as text/html)."
                    )))
                    .child(
                        div()
                            .mt(px(8.))
                            .text_size(px(11.))
                            .text_color(t.text_muted)
                            .child(format!("FILES ({})", files.len())),
                    )
                    .children(files.iter().take(500).map(|f| {
                        let (rid, path) = (rid.clone(), f.path.clone());
                        let sel = picked.as_deref() == Some(f.path.as_str());
                        crate::dev_id!(
                            div()
                                .id(crate::ui::eid(format!("static-file-{}", f.path)))
                                .h(px(22.))
                                .px(px(6.))
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(10.))
                                .rounded(px(3.))
                                .cursor_pointer()
                                .when(sel, |d| d.bg(t.element_selected))
                                .hover(|d| d.bg(t.element_hover))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.static_preview = Some((rid.clone(), path.clone()));
                                    cx.notify();
                                }))
                                .child(div().flex_1().font_family(BUFFER_FONT).child(f.path.clone()))
                                .child(div().text_color(t.text_muted).child(f.mime.clone()))
                                .child(div().w(px(70.)).text_color(t.text_muted).child(format!("{} B", f.size)))
                                .when(f.templated, |d| d.child(div().text_color(t.modified).child("template"))),
                            format!("static-file-{}", f.path),
                            Button,
                            f.path.clone(),
                            Press(format!("static-file:{}:{}", r.id, f.path))
                        )
                    }))
                    .when_some(preview, |d, text| {
                        d.child(
                            div()
                                .mt(px(8.))
                                .p(px(8.))
                                .rounded(px(4.))
                                .border_1()
                                .border_color(t.border_variant)
                                .font_family(BUFFER_FONT)
                                .text_size(px(11.))
                                .child(crate::dev_id!(
                                    div().child(text.clone()),
                                    "static-preview",
                                    Text,
                                    text.chars().take(120).collect::<String>(),
                                    None
                                )),
                        )
                    }),
            )
    }

    fn static_field(
        &self,
        t: &Theme,
        r: &Route,
        f: StaticField,
        w: f32,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let (label, id, value, ph) = match f {
            StaticField::Index => (
                "Index file",
                "static-index",
                r.index.clone().unwrap_or_default(),
                "index.html",
            ),
            StaticField::TemplateExt => (
                "Template extensions",
                "static-template-ext",
                r.template_extensions.join(", "),
                ".tera",
            ),
        };
        let el = self.inline_field(
            t,
            id.into(),
            InlineTarget::Static {
                route: r.id.clone(),
                field: f,
            },
            &value,
            ph,
            w,
            true,
            cx,
        );
        self.https_label(t, label, el)
    }

    pub(crate) fn commit_static_field(
        &mut self,
        route: &str,
        f: StaticField,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        use fauxfield_store::Target as T;
        let v = text.trim();
        let id = route.to_string();
        match f {
            StaticField::Index => {
                if !v.is_empty() && !inside_workspace(v) {
                    return self
                        .notify("The index is a file name inside the directory.".into(), cx);
                }
                self.store_op(cx, |s| {
                    s.set_str(&T::Route(id), "index", (!v.is_empty()).then_some(v))
                });
            }
            StaticField::TemplateExt => {
                let items = split_list(v);
                if items.iter().any(|e| !e.starts_with('.') || e.len() < 2) {
                    return self.notify(
                        "Template extensions start with a dot, e.g. .tera".into(),
                        cx,
                    );
                }
                self.store_op(cx, |s| {
                    s.set_str_list(&T::Route(id), "template_extensions", &items)
                });
            }
        }
    }

    pub(crate) fn pick_static_dir(
        &mut self,
        route: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Serve this folder".into()),
        });
        let route = route.to_string();
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(p) = paths.into_iter().next()
            {
                this.update(cx, |this, cx| this.static_dir_chosen(&route, p, cx))
                    .ok();
            }
        })
        .detach();
    }

    /// The chosen folder must live inside the workspace (SPEC §4.1: files are workspace-relative).
    pub(crate) fn static_dir_chosen(
        &mut self,
        route: &str,
        dir: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) {
        use fauxfield_store::Target as T;
        let Some(root) = self.store.as_ref().map(|s| s.root()) else {
            return;
        };
        let rel = match (dir.canonicalize(), root.canonicalize()) {
            (Ok(d), Ok(r)) => d
                .strip_prefix(&r)
                .ok()
                .map(|p| p.to_string_lossy().replace('\\', "/")),
            _ => None,
        };
        match rel {
            Some(rel) if !rel.is_empty() => {
                let id = route.to_string();
                self.static_preview = None;
                self.store_op(cx, |s| s.set_str(&T::Route(id), "dir", Some(&rel)));
            }
            _ => self.notify("Choose a folder inside the workspace.".into(), cx),
        }
    }

    /// Action `new_static_route`: pick a folder and serve it under `/<folder>/*`.
    pub(crate) fn begin_new_static(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let Some(server) = self.current_server_id() else {
            return self.notify("Select a server first.".into(), cx);
        };
        self.begin_new_static_in(&server, window, cx);
    }

    /// Pick a folder and serve it from Server `server`.
    pub(crate) fn begin_new_static_in(
        &mut self,
        server: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let server = server.to_string();
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Serve this folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(p) = paths.into_iter().next()
            {
                this.update(cx, |this, cx| this.new_static_chosen(&server, p, cx))
                    .ok();
            }
        })
        .detach();
    }

    pub(crate) fn new_static_chosen(
        &mut self,
        server: &str,
        dir: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.store.as_ref().map(|s| s.root()) else {
            return;
        };
        let rel = match (dir.canonicalize(), root.canonicalize()) {
            (Ok(d), Ok(r)) => d
                .strip_prefix(&r)
                .ok()
                .map(|p| p.to_string_lossy().replace('\\', "/")),
            _ => None,
        };
        let Some(rel) = rel.filter(|r| !r.is_empty()) else {
            return self.notify("Choose a folder inside the workspace.".into(), cx);
        };
        let name = rel.rsplit('/').next().unwrap_or("files").to_string();
        let sid = server.to_string();
        if let Some(id) = self.store_op(cx, |s| {
            s.create_static_route(&sid, None, &format!("/{name}/*"), &rel)
        }) {
            self.click_row(&id, 2, cx);
        }
    }
}
