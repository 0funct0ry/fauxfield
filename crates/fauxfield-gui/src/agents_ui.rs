//! Agent surfaces (M16): the status-bar "attached" indicator and the Logs tab's *Agent activity*
//! source. The host (`fauxfield_control::LocalControl`) tracks sessions and calls from every
//! MCP process, so agents attached over the control socket show here too.

use fauxfield_control::{AgentEvent, AgentSession};
use gpui::{
    AnyElement, ClipboardItem, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::*, px,
};

use crate::assets::BUFFER_FONT;
use crate::combo_box::{ComboEvent, ComboItem, ComboLook, ComboSite, ComboSize, ComboWidth};
use crate::layout::BottomTab;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::ui::{eid, icon, toolbar_button};

/// Which lines the Logs tab shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LogSource {
    #[default]
    All,
    Engine,
    Agent,
}

impl LogSource {
    pub fn value(self) -> &'static str {
        match self {
            LogSource::All => "all",
            LogSource::Engine => "engine",
            LogSource::Agent => "agent",
        }
    }
    fn from_value(v: &str) -> LogSource {
        match v {
            "engine" => LogSource::Engine,
            "agent" => LogSource::Agent,
            _ => LogSource::All,
        }
    }
    fn label(self) -> &'static str {
        match self {
            LogSource::All => "All",
            LogSource::Engine => "Engine",
            LogSource::Agent => "Agent activity",
        }
    }
}

/// `HH:MM:SS` (UTC) of a millisecond timestamp.
pub fn clock(ts_ms: u64) -> String {
    let s = ts_ms / 1000 % 86_400;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// The status-bar label: one client by name, several by count.
pub fn attached_label(sessions: &[AgentSession]) -> Option<String> {
    match sessions {
        [] => None,
        [one] => Some(format!("{} attached", one.name)),
        many => Some(format!("{} agents attached", many.len())),
    }
}

impl Shell {
    /// Refresh attached agents and the activity log from the host (called from the 60 ms poll).
    pub(crate) fn poll_agents(&mut self, cx: &mut Context<Self>) {
        let Some(host) = &self.host else {
            if !self.agents_attached.is_empty() || !self.agent_log.is_empty() {
                self.agents_attached.clear();
                self.agent_log.clear();
                cx.notify();
            }
            return;
        };
        let control = host.control();
        let sessions = control.agents().sessions();
        let log = control.agents().log(None);
        let mut changed = false;
        if sessions != self.agents_attached {
            self.agents_attached = sessions;
            changed = true;
        }
        if log != self.agent_log {
            self.flash_new_edits(&log);
            self.agent_log = log;
            changed = true;
        }
        // Expire finished flashes and keep repainting while one runs.
        let hold = self.flash_hold();
        let before = self.agent_flash.len();
        self.agent_flash.retain(|(_, at)| at.elapsed() < hold);
        if before != self.agent_flash.len() || !self.agent_flash.is_empty() {
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    /// How long a changed row keeps its mark: 1.2 s, or a static 3 s tag under reduced motion.
    pub(crate) fn flash_hold(&self) -> std::time::Duration {
        std::time::Duration::from_millis(if self.reduced_motion { 3000 } else { 1200 })
    }

    /// The row id an agent write touched (Route, Server, Environment or Dataset), if any.
    pub(crate) fn flash_target(&self, target: &str) -> Option<String> {
        if target.is_empty() {
            return None;
        }
        for s in &self.ws.servers {
            let in_folder = s.folders.iter().flat_map(|f| f.routes.iter());
            if s.routes.iter().chain(in_folder).any(|r| r.id == target) {
                return Some(target.to_owned());
            }
        }
        if let Some(s) = self
            .ws
            .servers
            .iter()
            .find(|s| s.id == target || s.name == target)
        {
            return Some(s.id.clone());
        }
        if self.ws.environments.iter().any(|e| e.name == target) {
            return Some(crate::tree::env_row_id(target));
        }
        self.ws
            .datasets
            .iter()
            .find(|d| d.name == target)
            .map(|d| crate::tree::dataset_row_id(&d.key()))
    }

    fn flash_new_edits(&mut self, log: &[AgentEvent]) {
        let seen = self.agent_seen_ts;
        for e in log.iter().filter(|e| e.ts_ms > seen) {
            if matches!(e.outcome.as_str(), "created" | "updated")
                && let Some(id) = self.flash_target(&e.target)
            {
                self.agent_flash.retain(|(i, _)| *i != id);
                self.agent_flash.push((id, std::time::Instant::now()));
            }
        }
        if let Some(last) = log.last() {
            self.agent_seen_ts = self.agent_seen_ts.max(last.ts_ms);
        }
    }

    /// Row ids to mark now: each flashed entity, or its nearest visible ancestor when its row
    /// is collapsed out of sight (Route, then its Folder, then its Server).
    pub(crate) fn flashing_rows(
        &self,
        rows: &[crate::tree::TreeRow],
    ) -> std::collections::HashSet<String> {
        let visible = |id: &str| rows.iter().any(|r| r.id == id);
        let mut out = std::collections::HashSet::new();
        for (id, _) in &self.agent_flash {
            if visible(id) {
                out.insert(id.clone());
                continue;
            }
            for s in &self.ws.servers {
                let mut chain: Vec<&str> = vec![];
                if let Some(f) = s
                    .folders
                    .iter()
                    .find(|f| f.routes.iter().any(|r| r.id == *id))
                {
                    chain.push(&f.id);
                }
                if s.routes.iter().any(|r| r.id == *id) || !chain.is_empty() {
                    chain.push(&s.id);
                }
                if let Some(v) = chain.into_iter().find(|c| visible(c)) {
                    out.insert(v.to_owned());
                    break;
                }
            }
        }
        out
    }

    /// Status-bar click: Logs, filtered to Agent activity.
    pub fn show_agent_activity(&mut self, cx: &mut Context<Self>) {
        self.log_source = LogSource::Agent;
        self.show_bottom(BottomTab::Logs, cx);
    }

    pub(crate) fn render_agent_status(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let label = attached_label(&self.agents_attached)?;
        let tip = self
            .agents_attached
            .iter()
            .map(|s| format!("{} since {} UTC", s.name, clock(s.since_ms)))
            .collect::<Vec<_>>()
            .join("\n");
        Some(
            crate::dev_id!(
                toolbar_button(t, "status-agent")
                    .child(div().size(px(7.)).rounded_full().bg(t.created))
                    .child(div().text_color(t.accent).child(label.clone()))
                    .tooltip(crate::ui::tooltip(tip))
                    .on_click(cx.listener(|this, _, _, cx| this.show_agent_activity(cx))),
                "status.agent",
                Button,
                label,
                Action("show_logs")
            )
            .into_any_element(),
        )
    }

    fn visible_agent_events(&self) -> Vec<&AgentEvent> {
        self.agent_log
            .iter()
            .filter(|e| self.log_session.as_ref().is_none_or(|c| e.client == *c))
            .collect()
    }

    /// "Copy as Markdown" of the visible agent activity (never holds secret values).
    pub fn copy_agent_log_markdown(&mut self, cx: &mut Context<Self>) {
        let mut md = String::from(
            "| Time (UTC) | Client | Tool | Target | Outcome |\n|---|---|---|---|---|\n",
        );
        for e in self.visible_agent_events() {
            md.push_str(&format!(
                "| {} | {} | `{}` | {} | {} |\n",
                clock(e.ts_ms),
                e.client,
                e.tool,
                e.target.replace('|', "\\|"),
                e.outcome
            ));
        }
        cx.write_to_clipboard(ClipboardItem::new_string(md));
        self.notify("Agent activity copied as Markdown".into(), cx);
    }

    pub(crate) fn render_logs(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let source = self.log_source;
        let source_combo = self.combo_field(
            cx,
            ComboSite {
                key: "logs-source".into(),
                items: [LogSource::All, LogSource::Engine, LogSource::Agent]
                    .iter()
                    .map(|s| ComboItem::new(s.value(), s.label()))
                    .collect(),
                value: Some(source.value().into()),
                fallback: Some(source.value().into()),
                color: None,
                tip: None,
                width: ComboWidth::Fixed(150.),
                look: ComboLook::Field,
                size: ComboSize::Small,
                on_event: std::rc::Rc::new(|this, ev, cx| {
                    if let ComboEvent::Change(v) = ev {
                        this.log_source = LogSource::from_value(v.as_ref());
                        cx.notify();
                    }
                }),
            },
        );
        let mut clients: Vec<String> = Vec::new();
        for e in &self.agent_log {
            if !clients.contains(&e.client) {
                clients.push(e.client.clone());
            }
        }
        let session_combo = (source != LogSource::Engine).then(|| {
            let mut items = vec![ComboItem::new("", "All sessions")];
            items.extend(clients.iter().map(|c| ComboItem::new(c.clone(), c.clone())));
            let cur = self.log_session.clone().unwrap_or_default();
            self.combo_field(
                cx,
                ComboSite {
                    key: "logs-session".into(),
                    items,
                    value: Some(cur.clone().into()),
                    fallback: Some(cur.into()),
                    color: None,
                    tip: None,
                    width: ComboWidth::Fixed(160.),
                    look: ComboLook::Field,
                    size: ComboSize::Small,
                    on_event: std::rc::Rc::new(|this, ev, cx| {
                        if let ComboEvent::Change(v) = ev {
                            this.log_session = Some(v.to_string()).filter(|s| !s.is_empty());
                            cx.notify();
                        }
                    }),
                },
            )
        });

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .px(px(10.))
            .py(px(4.))
            .border_b_1()
            .border_color(t.border_variant)
            .child(crate::dev_id!(
                div().child(source_combo),
                "logs.source",
                Button,
                "Log source",
                None
            ))
            .children(session_combo)
            .child(div().flex_1())
            .when(source != LogSource::Engine, |d| {
                d.child(
                    toolbar_button(t, "logs-copy-md")
                        .child("Copy as Markdown")
                        .on_click(cx.listener(|this, _, _, cx| this.copy_agent_log_markdown(cx))),
                )
            });

        let show_engine = source != LogSource::Agent;
        let show_agent = source != LogSource::Engine;
        let events = if show_agent {
            self.visible_agent_events()
        } else {
            vec![]
        };
        let empty = (!show_engine || self.logs.is_empty()) && events.is_empty();
        let body = div()
            .id("logs")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(10.))
            .font_family(BUFFER_FONT)
            .text_size(px(11.))
            .text_color(t.text_muted)
            .when(empty, |d| {
                d.flex().flex_col().child(self.empty_state(
                    if source == LogSource::Agent {
                        "agent-activity"
                    } else {
                        "logs"
                    },
                    t,
                    cx,
                ))
            })
            .when(show_engine, |d| {
                d.children(self.logs.iter().map(|l| div().child(l.clone())))
            })
            .children(events.into_iter().enumerate().map(|(i, e)| {
                let color = match e.outcome.as_str() {
                    "denied" => t.modified,
                    "error" => t.deleted,
                    _ => t.text_muted,
                };
                let line = format!(
                    "{}  {}  {}  {}  {}",
                    clock(e.ts_ms),
                    e.client,
                    e.tool,
                    e.target,
                    e.outcome
                );
                crate::dev_id!(
                    div()
                        .id(eid(format!("logs-agent-row-{i}")))
                        .flex()
                        .flex_row()
                        .gap(px(6.))
                        .text_color(color)
                        .child(icon("bot", 11., color))
                        .child(line.clone()),
                    format!("logs.agent.row.{i}"),
                    Row,
                    line,
                    None
                )
            }));
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(header)
            .child(body)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str) -> AgentSession {
        AgentSession {
            id: name.into(),
            name: name.into(),
            since_ms: 0,
        }
    }

    #[test]
    fn label_names_one_client_and_counts_many() {
        assert_eq!(attached_label(&[]), None);
        assert_eq!(
            attached_label(&[s("Claude Code")]).as_deref(),
            Some("Claude Code attached")
        );
        assert_eq!(
            attached_label(&[s("a"), s("b")]).as_deref(),
            Some("2 agents attached")
        );
    }

    #[test]
    fn clock_wraps_the_day() {
        assert_eq!(clock(0), "00:00:00");
        assert_eq!(clock(3_661_000), "01:01:01");
        assert_eq!(clock(86_400_000 + 5_000), "00:00:05");
    }
}
