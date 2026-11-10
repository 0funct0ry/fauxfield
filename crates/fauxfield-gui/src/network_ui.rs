//! Network and error simulation controls (M9, SPEC §5.5, §5.6): the Server tab's Network and
//! Rate limit sections, the Response Fault controls, and their commit logic.

use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};

use crate::combo_box::{ComboEvent, ComboItem, ComboLook, ComboSite, ComboSize, ComboWidth};
use crate::response_pane::InlineTarget;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::view::{Response, Server};

/// Server fields edited inline in the Network and Rate limit sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetField {
    Latency,
    Jitter,
    Bandwidth,
    Requests,
    Window,
    Key,
}

impl NetField {
    pub fn numeric(self) -> bool {
        matches!(
            self,
            Self::Latency | Self::Jitter | Self::Bandwidth | Self::Requests
        )
    }
    fn table(self) -> &'static str {
        match self {
            Self::Latency | Self::Jitter | Self::Bandwidth => "network",
            _ => "rate_limit",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Self::Latency => "latency_ms",
            Self::Jitter => "jitter_ms",
            Self::Bandwidth => "bandwidth_kbps",
            Self::Requests => "requests",
            Self::Window => "window",
            Self::Key => "key",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Latency => "Latency (ms)",
            Self::Jitter => "Jitter (ms)",
            Self::Bandwidth => "Bandwidth (kbps)",
            Self::Requests => "Requests",
            Self::Window => "Window",
            Self::Key => "Key",
        }
    }
    fn id(self) -> &'static str {
        match self {
            Self::Latency => "net-latency",
            Self::Jitter => "net-jitter",
            Self::Bandwidth => "net-bandwidth",
            Self::Requests => "rate-requests",
            Self::Window => "rate-window",
            Self::Key => "rate-key",
        }
    }
    fn value(self, s: &Server) -> String {
        let rl = s
            .rate_limit
            .clone()
            .unwrap_or_else(|| serde_json::from_str("{}").expect("RateLimit defaults"));
        match self {
            Self::Latency => nz(s.network.latency_ms),
            Self::Jitter => nz(s.network.jitter_ms),
            Self::Bandwidth => nz(s.network.bandwidth_kbps),
            Self::Requests => rl.requests.to_string(),
            Self::Window => rl.window,
            Self::Key => rl.key,
        }
    }
    fn placeholder(self) -> &'static str {
        match self {
            Self::Latency | Self::Jitter => "0",
            Self::Bandwidth => "0 (unlimited)",
            Self::Requests => "100",
            Self::Window => "1m",
            Self::Key => "ip, header:x-api-key or query:user",
        }
    }
}

fn nz(n: u64) -> String {
    if n == 0 { String::new() } else { n.to_string() }
}

pub fn fault_tip(f: &str) -> &'static str {
    match f {
        "timeout" => "Timeout: hold the request until the client gives up",
        "reset" => "Reset: abort the connection (TCP RST)",
        "close" => "Close: send the headers, then drop the connection",
        "empty" => "Empty: send no body",
        "malformed" => "Malformed: cut the body short",
        "slow_body" => "Slow body: stream at the network bandwidth",
        _ => "No fault",
    }
}

impl Shell {
    fn labelled(&self, t: &Theme, l: &str, el: AnyElement) -> gpui::Div {
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

    fn net_field(
        &self,
        t: &Theme,
        s: &Server,
        f: NetField,
        w: f32,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let el = self.inline_field(
            t,
            f.id().into(),
            InlineTarget::Net {
                server: s.id.clone(),
                field: f,
            },
            &f.value(s),
            f.placeholder(),
            w,
            true,
            cx,
        );
        self.labelled(t, f.label(), el)
    }

    /// Server tab "Network": latency, jitter and bandwidth of every response (SPEC §5.6).
    pub(crate) fn server_network_section(
        &self,
        t: &Theme,
        s: &Server,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let p = self.network_profile;
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
                    .child("NETWORK"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(12.))
                    .child(self.net_field(t, s, NetField::Latency, 110., cx))
                    .child(self.net_field(t, s, NetField::Jitter, 110., cx))
                    .child(self.net_field(t, s, NetField::Bandwidth, 150., cx)),
            )
            .child(crate::dev_id!(
                div().text_size(px(11.)).text_color(if p == fauxfield_engine::NetworkProfile::None {
                    t.text_muted
                } else {
                    t.modified
                }).child(if p == fauxfield_engine::NetworkProfile::None {
                    "Delay added to every response; the delay on a response adds on top. Bandwidth 0 is unlimited.".to_string()
                } else {
                    format!("The {} profile overrides these values until it is switched back to None (status bar).", p.label())
                }),
                "net-note",
                Text,
                "Network note",
                None
            ))
    }

    /// Server tab "Rate limit": 429 with Retry-After and X-RateLimit-* headers past the limit.
    pub(crate) fn server_rate_limit_section(
        &self,
        t: &Theme,
        s: &Server,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let on = s.rate_limit.as_ref().is_some_and(|r| r.enabled);
        let sid = s.id.clone();
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
                    .child("RATE LIMIT"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap(px(12.))
                    .child(self.labelled(
                        t,
                        "Enabled",
                        crate::dev_id!(
                            div()
                                .id(crate::ui::eid(format!("rate-enabled-{}", s.id)))
                                .w(px(70.))
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
                                .text_color(if on { t.text } else { t.text_muted })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_rate_limit(&sid, cx)
                                }))
                                .child(if on { "On" } else { "Off" }),
                            "rate-enabled",
                            Button,
                            if on { "On" } else { "Off" },
                            Press(format!("rate-enabled:{}", s.id))
                        )
                        .into_any_element(),
                    ))
                    .child(self.net_field(t, s, NetField::Requests, 90., cx))
                    .child(self.net_field(t, s, NetField::Window, 80., cx))
                    .child(self.net_field(t, s, NetField::Key, 250., cx)),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child("Past the limit a request gets 429 with Retry-After and X-RateLimit-* headers. Window: 30s, 1m, 2h."),
            )
    }

    /// Response properties: Fault kind (combobox) and Fault rate.
    pub(crate) fn response_fault_fields(
        &self,
        t: &Theme,
        resp: &Response,
        cx: &mut Context<Self>,
    ) -> (AnyElement, AnyElement) {
        let has = !resp.fault.is_empty();
        let name = if has {
            resp.fault.clone()
        } else {
            "none".to_string()
        };
        let rid = resp.id.clone();
        let kind = self.combo_field(
            cx,
            ComboSite {
                key: format!("resp-fault-{}", resp.id),
                items: fauxfield_model::FAULTS
                    .iter()
                    .map(|f| ComboItem::plain(*f))
                    .collect(),
                value: Some(name.clone().into()),
                fallback: Some(name.into()),
                color: has.then_some(t.modified),
                tip: Some(fault_tip(&resp.fault).to_string()),
                width: ComboWidth::Fixed(120.),
                look: ComboLook::Field,
                size: ComboSize::Medium,
                on_event: std::rc::Rc::new(move |this, ev, cx| {
                    if let ComboEvent::Change(f) = ev {
                        this.set_response_fault(&rid, f, cx);
                    }
                }),
            },
        );
        let rate = self.inline_field(
            t,
            "resp-fault-rate".into(),
            InlineTarget::FaultRate(resp.id.clone()),
            &resp.fault_rate,
            "1",
            56.,
            true,
            cx,
        );
        (kind, rate)
    }

    /// Sets the Response fault (`none` clears it and its rate).
    pub(crate) fn set_response_fault(&mut self, id: &str, fault: &str, cx: &mut Context<Self>) {
        use fauxfield_store::Target as T;
        let none = fault == "none" || fault.is_empty();
        let (id, fault) = (id.to_string(), fault.to_string());
        self.store_op(cx, |s| {
            let t = T::Response(id.clone());
            s.set_str(&t, "fault", (!none).then_some(fault.as_str()))?;
            if none {
                s.set_item_none(&t, "fault_rate")?;
            }
            Ok(())
        });
    }

    pub(crate) fn toggle_rate_limit(&mut self, server: &str, cx: &mut Context<Self>) {
        let on = self
            .ws
            .find_server(server)
            .and_then(|s| s.rate_limit.as_ref().map(|r| r.enabled))
            .unwrap_or(false);
        let sid = server.to_string();
        self.store_op(cx, |s| {
            if on {
                s.set_server_table_bool(&sid, "rate_limit", "enabled", false)
            } else {
                s.set_server_table_bool(&sid, "rate_limit", "enabled", true)
            }
        });
    }

    pub(crate) fn commit_net_field(
        &mut self,
        server: &str,
        f: NetField,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let v = text.trim();
        let sid = server.to_string();
        if f.numeric() {
            let Ok(n) = (if v.is_empty() {
                Ok(0)
            } else {
                v.parse::<i64>()
            }) else {
                return self.notify(format!("{} is a whole number.", f.label()), cx);
            };
            if f == NetField::Requests && n < 1 {
                return self.notify("Requests must be at least 1.".into(), cx);
            }
            // 0 is the default for network values: leave the file without the key.
            let val = (n > 0).then_some(n);
            self.store_op(cx, |s| {
                s.set_server_table_int(&sid, f.table(), f.key(), val)
            });
        } else if f == NetField::Window {
            if fauxfield_model::parse_window(v).is_none() {
                return self.notify("Window is like 30s, 1m or 2h.".into(), cx);
            }
            self.store_op(cx, |s| {
                s.set_server_table_str(&sid, f.table(), f.key(), Some(v))
            });
        } else {
            if !fauxfield_model::valid_rate_key(v) {
                return self.notify("Key is ip, header:<name> or query:<name>.".into(), cx);
            }
            self.store_op(cx, |s| {
                s.set_server_table_str(&sid, f.table(), f.key(), Some(v))
            });
        }
    }

    pub(crate) fn commit_fault_rate(&mut self, id: &str, text: &str, cx: &mut Context<Self>) {
        use fauxfield_store::Target as T;
        let v = text.trim();
        let id = id.to_string();
        if v.is_empty() {
            self.store_op(cx, |s| s.set_item_none(&T::Response(id), "fault_rate"));
            return;
        }
        match v.parse::<f64>() {
            Ok(r) if (0.0..=1.0).contains(&r) => {
                self.store_op(cx, |s| s.set_float(&T::Response(id), "fault_rate", Some(r)));
            }
            _ => self.notify(
                "Fault rate is a probability from 0 to 1, e.g. 0.3.".into(),
                cx,
            ),
        }
    }
}
