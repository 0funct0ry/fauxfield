//! Proxy and partial mocking controls (M11, SPEC §7.1): the Server tab's Proxy section and the
//! per-Route Forward / Pin toggles, with their commit logic.

use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::*, px,
};

use crate::response_pane::InlineTarget;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::view::Server;

/// Server fields edited inline in the Proxy section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProxyField {
    Upstream,
    Timeout,
    /// "Name: value" entry that adds or replaces an upstream request header.
    RequestHeader,
    /// "Name: value" entry that adds or replaces a client response header.
    ResponseHeader,
}

impl ProxyField {
    fn id(self) -> &'static str {
        match self {
            Self::Upstream => "proxy-upstream",
            Self::Timeout => "proxy-timeout",
            Self::RequestHeader => "proxy-req-header-add",
            Self::ResponseHeader => "proxy-resp-header-add",
        }
    }
    fn placeholder(self) -> &'static str {
        match self {
            Self::Upstream => "https://api.example.com or {{ vars.upstream }}",
            Self::Timeout => "30000",
            Self::RequestHeader | Self::ResponseHeader => "Name: value (Enter adds)",
        }
    }
    pub(crate) fn numeric(self) -> bool {
        self == Self::Timeout
    }
}

fn valid_header_name(n: &str) -> bool {
    !n.is_empty()
        && n.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

impl Shell {
    /// The upstream text with its templates rendered against the variables in scope.
    /// `Err` carries the template error.
    pub(crate) fn upstream_preview(&self, s: &Server) -> Result<String, String> {
        let raw = s
            .proxy
            .as_ref()
            .and_then(|p| p.upstream.clone())
            .unwrap_or_default();
        if !raw.contains("{{") && !raw.contains("{%") {
            return Ok(raw);
        }
        let vars: serde_json::Map<String, serde_json::Value> = self
            .scope_vars_of(&s.id)
            .into_iter()
            .map(|v| (v.key, serde_json::Value::String(v.value)))
            .collect();
        let input = fauxfield_engine::template::Input {
            context: serde_json::json!({
                "vars": vars,
                "env": self.active_env,
                "server": { "name": s.name, "port": s.port },
            }),
            root: &self.store.as_ref().map(|st| st.root()).unwrap_or_default(),
            seed: s.fake_seed,
            route_id: "",
            counter: 0,
            seq: None,
        };
        fauxfield_engine::template::render(&raw, &input)
            .map(|r| r.trim().to_string())
            .map_err(|e| e.to_string())
    }

    fn proxy_field(
        &self,
        t: &Theme,
        s: &Server,
        f: ProxyField,
        value: &str,
        w: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.inline_field(
            t,
            f.id().into(),
            InlineTarget::Proxy {
                server: s.id.clone(),
                field: f,
            },
            value,
            f.placeholder(),
            w,
            true,
            cx,
        )
        .into_any_element()
    }

    fn proxy_toggle(
        &self,
        t: &Theme,
        s: &Server,
        key: &'static str,
        on: bool,
        red: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sid = s.id.clone();
        let label = if on { "On" } else { "Off" };
        let btn = self.state_button(
            t,
            format!("proxy-{key}-{}", s.id),
            label.into(),
            on,
            70.,
            move |this, cx| this.toggle_proxy(&sid, key, cx),
            cx,
        );
        let btn = if red && on {
            btn.border_color(t.deleted).text_color(t.deleted)
        } else {
            btn
        };
        crate::dev_id!(
            btn,
            format!("proxy-{}", key.replace('_', "-")),
            Button,
            label,
            Press(format!("proxy-{key}:{}", s.id))
        )
        .into_any_element()
    }

    fn proxy_headers(
        &self,
        t: &Theme,
        s: &Server,
        title: &str,
        field: ProxyField,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let req = field == ProxyField::RequestHeader;
        let rows: Vec<(String, String)> = s
            .proxy
            .as_ref()
            .map(|p| {
                let m = if req {
                    &p.request_headers
                } else {
                    &p.response_headers
                };
                m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
            })
            .unwrap_or_default();
        let kind = if req {
            "request_headers"
        } else {
            "response_headers"
        };
        let mut col = div().flex().flex_col().gap(px(4.)).child(
            div()
                .text_size(px(11.))
                .text_color(t.text_muted)
                .child(title.to_string()),
        );
        for (i, (k, v)) in rows.into_iter().enumerate() {
            let (sid, name) = (s.id.clone(), k.clone());
            let id = format!("proxy-{}-remove-{i}", if req { "req" } else { "resp" });
            col = col.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .w(px(520.))
                            .text_size(px(12.))
                            .child(format!("{k}: {v}")),
                    )
                    .child(crate::dev_id!(
                        div()
                            .id(crate::ui::eid(format!("{id}-{}", s.id)))
                            .px(px(6.))
                            .cursor_pointer()
                            .text_size(px(12.))
                            .text_color(t.text_muted)
                            .hover(|d| d.text_color(t.deleted))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove_proxy_header(&sid, kind, &name, cx)
                            }))
                            .child("✕"),
                        id,
                        Button,
                        format!("Remove header {k}"),
                        Press(format!("proxy-header-remove:{}:{i}", s.id))
                    )),
            );
        }
        col.child(self.proxy_field(t, s, field, "", 520., cx))
    }

    /// Server tab "Proxy": enable, mode, upstream with preview, timeout, headers, insecure.
    pub(crate) fn server_proxy_section(
        &self,
        t: &Theme,
        s: &Server,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let p = s.proxy.clone().unwrap_or_default();
        let sid = s.id.clone();
        let mode = p.mode_name().to_string();
        let mode_combo = {
            use crate::combo_box::*;
            let sid = sid.clone();
            self.combo_field(
                cx,
                ComboSite {
                    key: format!("proxy-mode-{}", s.id),
                    items: fauxfield_model::PROXY_MODES
                        .iter()
                        .map(|m| ComboItem::plain(*m))
                        .collect(),
                    value: Some(mode.clone().into()),
                    fallback: Some(mode.clone().into()),
                    color: None,
                    tip: None,
                    width: ComboWidth::Fixed(140.),
                    look: ComboLook::Field,
                    size: ComboSize::Small,
                    on_event: std::rc::Rc::new(move |this, ev, cx| {
                        if let ComboEvent::Change(m) = ev {
                            this.set_proxy_mode(&sid, m, cx);
                        }
                    }),
                },
            )
        };
        let preview = match self.upstream_preview(s) {
            Ok(v) if v.is_empty() => ("Not set".to_string(), false),
            Ok(v) => (format!("→ {v}"), false),
            Err(e) => (format!("Template error: {e}"), true),
        };
        let mode_hint = if mode == "passthrough" {
            "passthrough: every request goes upstream unless the Route is pinned."
        } else {
            "fallback: matched Routes are mocked; unmatched requests go upstream."
        };
        let upstream = p.upstream.clone().unwrap_or_default();
        let timeout = p.timeout_ms.map(|v| v.to_string()).unwrap_or_default();
        let lbl = |l: &str, el: AnyElement| self.https_label(t, l, el);
        div()
            .mt(px(14.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .mt(px(4.))
                    .pb(px(4.))
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child("PROXY"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap(px(12.))
                    .child(lbl(
                        "Enabled",
                        self.proxy_toggle(t, s, "enabled", p.enabled, false, cx),
                    ))
                    .child(lbl("Mode", mode_combo))
                    .child(lbl(
                        "Preserve host",
                        self.proxy_toggle(t, s, "preserve_host", p.preserve_host, false, cx),
                    ))
                    .child(lbl(
                        "Timeout (ms)",
                        self.proxy_field(t, s, ProxyField::Timeout, &timeout, 100., cx),
                    )),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child(mode_hint),
            )
            .child(lbl(
                "Upstream",
                self.proxy_field(t, s, ProxyField::Upstream, &upstream, 520., cx),
            ))
            .child(crate::dev_id!(
                div()
                    .text_size(px(11.))
                    .text_color(if preview.1 { t.deleted } else { t.text_muted })
                    .child(preview.0.clone()),
                "proxy-upstream-preview",
                Text,
                preview.0,
                None
            ))
            .child(self.proxy_headers(
                t,
                s,
                "Request headers (to upstream)",
                ProxyField::RequestHeader,
                cx,
            ))
            .child(self.proxy_headers(
                t,
                s,
                "Response headers (to client)",
                ProxyField::ResponseHeader,
                cx,
            ))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap(px(12.))
                    .child(lbl(
                        "Accept invalid certificates",
                        self.proxy_toggle(t, s, "insecure", p.insecure, true, cx),
                    ))
                    .when(p.insecure, |d| {
                        d.child(
                            div()
                                .text_size(px(11.))
                                .text_color(t.deleted)
                                .child("Insecure: upstream certificates are not verified."),
                        )
                    }),
            )
    }

    // ---- commits ----------------------------------------------------------------------------

    pub(crate) fn toggle_proxy(&mut self, server: &str, key: &'static str, cx: &mut Context<Self>) {
        let p = self
            .ws
            .find_server(server)
            .and_then(|s| s.proxy.clone())
            .unwrap_or_default();
        let on = match key {
            "enabled" => p.enabled,
            "preserve_host" => p.preserve_host,
            _ => p.insecure,
        };
        let sid = server.to_string();
        self.store_op(cx, |s| s.set_server_table_bool(&sid, "proxy", key, !on));
    }

    pub(crate) fn set_proxy_mode(&mut self, server: &str, mode: &str, cx: &mut Context<Self>) {
        let sid = server.to_string();
        let m = mode.to_string();
        self.store_op(cx, |s| {
            s.set_server_table_str(
                &sid,
                "proxy",
                "mode",
                (m != "fallback").then_some(m.as_str()),
            )
        });
    }

    pub(crate) fn remove_proxy_header(
        &mut self,
        server: &str,
        kind: &'static str,
        name: &str,
        cx: &mut Context<Self>,
    ) {
        let (sid, n) = (server.to_string(), name.to_string());
        self.store_op(cx, |s| s.set_proxy_header(&sid, kind, &n, None));
    }

    pub(crate) fn commit_proxy_field(
        &mut self,
        server: &str,
        f: ProxyField,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let v = text.trim();
        let sid = server.to_string();
        match f {
            ProxyField::Upstream => {
                let url_ok = v.is_empty()
                    || v.contains("{{")
                    || v.starts_with("http://")
                    || v.starts_with("https://");
                if !url_ok {
                    return self.notify(
                        "The upstream must be an http:// or https:// URL.".into(),
                        cx,
                    );
                }
                self.store_op(cx, |s| {
                    s.set_server_table_str(&sid, "proxy", "upstream", (!v.is_empty()).then_some(v))
                });
            }
            ProxyField::Timeout => {
                let n = if v.is_empty() {
                    None
                } else {
                    match v.parse::<u64>() {
                        Ok(n) if n > 0 => Some(n as i64),
                        _ => return self.notify("The timeout is a whole number of ms.".into(), cx),
                    }
                };
                self.store_op(cx, |s| {
                    s.set_server_table_int(&sid, "proxy", "timeout_ms", n)
                });
            }
            ProxyField::RequestHeader | ProxyField::ResponseHeader => {
                if v.is_empty() {
                    return;
                }
                let Some((name, val)) = v.split_once(':') else {
                    return self.notify("Enter the header as Name: value.".into(), cx);
                };
                let (name, val) = (name.trim().to_string(), val.trim().to_string());
                if !valid_header_name(&name) {
                    return self.notify(format!("`{name}` is not a valid header name."), cx);
                }
                let kind = if f == ProxyField::RequestHeader {
                    "request_headers"
                } else {
                    "response_headers"
                };
                self.store_op(cx, |s| s.set_proxy_header(&sid, kind, &name, Some(&val)));
            }
        }
    }

    pub(crate) fn toggle_route_flag(
        &mut self,
        route: &str,
        flag: &'static str,
        cx: &mut Context<Self>,
    ) {
        let on = self
            .ws
            .find_route(route)
            .map(|(_, r)| if flag == "proxy" { r.forward } else { r.pinned })
            .unwrap_or(false);
        let rid = route.to_string();
        self.store_op(cx, |s| s.set_route_flag(&rid, flag, !on));
    }
}
