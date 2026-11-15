//! "Agents: Set Up MCP…" (M16): choose the agent clients, see the diff of every file that would
//! change, then write. The plan is `fauxfield_store::agents::plan`, the same function the CLI
//! uses; nothing is written until *Write files*.

use fauxfield_store::agents::{self, Action, Client, Options, Plan};
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, MouseButton, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};

use crate::assets::BUFFER_FONT;
use crate::combo_box::{ComboEvent, ComboItem, ComboLook, ComboSite, ComboSize, ComboWidth};
use crate::run::{NoticeAction, ToastAction};
use crate::shell::Shell;
use crate::theme::Theme;
use crate::ui::{eid, toolbar_button};

pub const ALLOW_CHOICES: [(&str, &str); 3] = [
    ("read", "Read only"),
    ("read,write", "Read and write"),
    ("read,write,execute", "Read, write and execute"),
];

pub struct AgentsDialog {
    pub clients: Vec<(Client, bool)>,
    /// Comma list of tool classes, one of [`ALLOW_CHOICES`].
    pub allow: String,
    /// Index into the plan's files shown in the diff view.
    pub selected: usize,
    pub plan: Result<Plan, String>,
}

impl AgentsDialog {
    fn options(&self) -> Options {
        Options {
            clients: self
                .clients
                .iter()
                .filter(|(_, on)| *on)
                .map(|(c, _)| *c)
                .collect(),
            allow: self.allow.split(',').map(str::to_owned).collect(),
            command: "fauxfield".into(),
        }
    }

    pub fn replan(&mut self, root: &std::path::Path) {
        self.plan = agents::plan(root, &self.options()).map_err(|e| e.to_string());
        if let Ok(p) = &self.plan
            && self.selected >= p.files.len()
        {
            self.selected = 0;
        }
    }

    pub fn enabled_clients(&self) -> Vec<Client> {
        self.options().clients
    }
}

fn action_word(a: Action) -> &'static str {
    match a {
        Action::Create => "new file",
        Action::Merge => "update",
        Action::Append => "append",
        Action::Unchanged => "unchanged",
    }
}

impl Shell {
    pub fn open_agents_dialog(&mut self, cx: &mut Context<Self>) {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        self.open_agents_dialog_with_home(home, cx);
    }

    /// `home` is where `~/.codex` is looked for when preselecting clients.
    pub fn open_agents_dialog_with_home(
        &mut self,
        home: Option<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if self.store.is_none() {
            self.notify("Open a workspace first.".into(), cx);
            return;
        }
        let root = self.ws.root.clone();
        let mut detected = agents::detect_clients(&root, home.as_deref());
        if detected.is_empty() {
            detected.push(Client::Claude);
        }
        let mut d = AgentsDialog {
            clients: Client::ALL
                .iter()
                .map(|c| (*c, detected.contains(c)))
                .collect(),
            allow: ALLOW_CHOICES[2].0.into(),
            selected: 0,
            plan: Err(String::new()),
        };
        d.replan(&root);
        self.agents_dialog = Some(d);
        cx.notify();
    }

    /// Cancel: writes nothing.
    pub fn close_agents_dialog(&mut self, cx: &mut Context<Self>) {
        self.agents_dialog = None;
        cx.notify();
    }

    pub fn agents_toggle_client(&mut self, id: &str, cx: &mut Context<Self>) {
        let root = self.ws.root.clone();
        if let Some(d) = self.agents_dialog.as_mut() {
            if let Some(c) = d.clients.iter_mut().find(|(c, _)| c.id() == id) {
                c.1 = !c.1;
            }
            d.replan(&root);
        }
        cx.notify();
    }

    pub fn agents_set_allow(&mut self, allow: &str, cx: &mut Context<Self>) {
        let root = self.ws.root.clone();
        if let Some(d) = self.agents_dialog.as_mut() {
            d.allow = allow.to_owned();
            d.replan(&root);
        }
        cx.notify();
    }

    pub fn agents_select_file(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(d) = self.agents_dialog.as_mut() {
            d.selected = i;
        }
        cx.notify();
    }

    /// Write every changed file (all or nothing), then toast how to reach the agent.
    pub fn agents_write(&mut self, cx: &mut Context<Self>) {
        let root = self.ws.root.clone();
        let Some(d) = self.agents_dialog.as_ref() else {
            return;
        };
        let clients = d.enabled_clients();
        let plan = match &d.plan {
            Ok(p) if !p.is_noop() => p.clone(),
            Ok(_) => {
                self.agents_dialog = None;
                self.notify("Already configured".into(), cx);
                return;
            }
            Err(_) => return,
        };
        match agents::apply(&root, &plan) {
            Ok(_) => {
                self.agents_dialog = None;
                let who = clients
                    .iter()
                    .map(|c| c.label())
                    .collect::<Vec<_>>()
                    .join(", ");
                let who = if who.is_empty() {
                    "AGENTS.md".into()
                } else {
                    who
                };
                self.notify_action(
                    format!("MCP configured for {who}. Restart it, then run /mcp."),
                    Some(NoticeAction {
                        label: "Copy command",
                        action: ToastAction::CopyText("/mcp".into()),
                    }),
                    cx,
                );
            }
            Err(e) => self.notify(format!("Nothing was written: {e}"), cx),
        }
    }

    pub(crate) fn render_agents_dialog(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let d = self.agents_dialog.as_ref().expect("dialog is open");
        let button = |id: &'static str, label: &'static str| {
            toolbar_button(t, eid(id))
                .h(px(26.))
                .px(px(12.))
                .child(label)
        };

        // Client checkboxes.
        let mut clients = div().flex().flex_row().gap(px(16.));
        for (c, on) in &d.clients {
            let id = c.id();
            clients = clients.child(crate::dev_id!(
                toolbar_button(t, eid(format!("agents-client-{id}")))
                    .h(px(24.))
                    .child(if *on { "☑" } else { "☐" })
                    .child(c.label())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.agents_toggle_client(id, cx);
                    })),
                format!("agents.client.{id}"),
                Button,
                c.label(),
                AgentsClient(id.to_string())
            ));
        }

        // Allow ComboBox.
        let allow = self.combo_field(
            cx,
            ComboSite {
                key: "agents-allow".into(),
                items: ALLOW_CHOICES
                    .iter()
                    .map(|(v, l)| ComboItem::new(*v, *l))
                    .collect(),
                value: Some(d.allow.clone().into()),
                fallback: Some(d.allow.clone().into()),
                color: None,
                tip: None,
                width: ComboWidth::Fixed(200.),
                look: ComboLook::Field,
                size: ComboSize::Small,
                on_event: std::rc::Rc::new(|this, ev, cx| {
                    if let ComboEvent::Change(v) = ev {
                        this.agents_set_allow(v.as_ref(), cx);
                    }
                }),
            },
        );

        // File tabs + diff.
        let mut tabs = div().flex().flex_row().gap(px(4.)).flex_wrap();
        let mut diff = div().flex().flex_col().p(px(8.));
        let mut can_write = false;
        match &d.plan {
            Ok(plan) => {
                can_write = !plan.is_noop();
                for (i, f) in plan.files.iter().enumerate() {
                    let sel = i == d.selected;
                    let mut b = toolbar_button(t, eid(format!("agents-file-{i}")))
                        .h(px(22.))
                        .child(format!("{}  ({})", f.path, action_word(f.action)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.agents_select_file(i, cx);
                        }));
                    if sel {
                        b = b.bg(t.element_selected);
                    }
                    tabs = tabs.child(b);
                }
                if let Some(f) = plan.files.get(d.selected) {
                    if f.diff.is_empty() {
                        diff = diff.child(
                            div()
                                .text_size(px(12.))
                                .text_color(t.text_muted)
                                .child("No changes to this file."),
                        );
                    }
                    for line in f.diff.lines() {
                        let color = if line.starts_with("+++") || line.starts_with("---") {
                            t.text_muted
                        } else if line.starts_with('+') {
                            t.created
                        } else if line.starts_with('-') {
                            t.deleted
                        } else if line.starts_with("@@") {
                            t.accent
                        } else {
                            t.text
                        };
                        diff = diff.child(
                            div()
                                .font_family(BUFFER_FONT)
                                .text_size(px(12.))
                                .text_color(color)
                                .child(if line.is_empty() {
                                    " ".into()
                                } else {
                                    line.to_string()
                                }),
                        );
                    }
                }
            }
            Err(e) => {
                diff = diff.child(div().text_color(t.deleted).child(e.clone()));
            }
        }
        let diff_box = crate::dev_id!(
            div()
                .id("agents-diff")
                .h(px(260.))
                .w_full()
                .overflow_y_scroll()
                .rounded(px(4.))
                .border_1()
                .border_color(t.border)
                .bg(t.editor)
                .child(diff),
            "agents.diff",
            Panel,
            "Diff",
            None
        );

        let write_btn = {
            let b = button("agents-write", "Write files");
            if can_write {
                b.bg(t.accent_bg())
                    .on_click(cx.listener(|this, _, _, cx| this.agents_write(cx)))
            } else {
                b.text_color(t.text_muted)
            }
        };

        let card = div()
            .id("agents-card")
            .w(px(720.))
            .p(px(20.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .rounded(px(8.))
            .border_1()
            .border_color(t.border)
            .bg(t.elevated)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().text_size(px(16.)).child("Set up MCP for coding agents"))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(t.text_muted)
                    .child("Fauxfield writes the client config and an AGENTS.md section so an agent can create Routes, start Servers and check Traffic."),
            )
            .child(clients)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(div().text_size(px(12.)).child("Allow"))
                    .child(allow),
            )
            .child(tabs)
            .child(diff_box)
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(t.text_muted)
                    .child("Nothing is written until you click Write files."),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap(px(8.))
                    .child(crate::dev_id!(
                        button("agents-cancel", "Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.close_agents_dialog(cx))),
                        "agents.cancel",
                        Button,
                        "Cancel",
                        AgentsCancel
                    ))
                    .child(crate::dev_id!(
                        write_btn,
                        "agents.write",
                        Button,
                        "Write files",
                        AgentsWrite
                    )),
            );
        crate::dev_id_z!(
            div()
                .id("agents-dialog")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(t.background.opacity(0.6))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_agents_dialog(cx)),
                )
                .child(card),
            10,
            "agents.dialog",
            Modal,
            "Set up MCP for coding agents",
            None
        )
        .into_any_element()
    }
}
