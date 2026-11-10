//! Running servers from the shell (M3): the host (engine, control socket), start/stop/restart,
//! Send test, live Traffic and Logs, Copy as cURL, and the quit confirmation. The workspace
//! files stay authoritative; run state and traffic are runtime-only.

use fauxfield_control::{Host, HostError, HostOptions};
use fauxfield_engine::{EngineError, EngineEvent, ServerStatus, TestRequest, to_curl};
use gpui::{
    ClipboardItem, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};
use tokio::sync::broadcast::error::TryRecvError;

use crate::shell::Shell;
use crate::theme::Theme;
use crate::traffic::TrafficRow;
use crate::ui::toolbar_button;

/// Rows kept in the Traffic panel (SPEC §9: `max_rows`).
pub const TRAFFIC_CAP: usize = 100_000;
const LOG_CAP: usize = 2_000;

/// What a toast's action button does.
#[derive(Clone, Debug, PartialEq)]
pub enum ToastAction {
    ChangePort(String),
    CreateWorkspaceAt(std::path::PathBuf),
    OpenWorkspaceAt(std::path::PathBuf),
    EnableAndStart(String),
}

/// A button shown beside the status-bar message.
#[derive(Clone, Debug, PartialEq)]
pub struct NoticeAction {
    pub label: &'static str,
    pub action: ToastAction,
}

#[derive(Clone, Debug)]
enum Job {
    Start(String),
    Stop(String),
    Restart(String),
    StartAll,
    StopAll,
}

impl Shell {
    /// (Re)create the host for the open workspace: engine, hot reload, control socket.
    pub(crate) fn attach_engine(&mut self, cx: &mut Context<Self>) {
        // Stop the previous workspace's servers and release its lock first.
        self.engine_events = None;
        self.host = None;
        let Some(store) = self.store.clone() else {
            return;
        };
        let rt = self.rt.get_or_insert_with(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .thread_name("fauxfield-engine")
                .build()
                .expect("tokio runtime")
        });
        let handle = rt.handle().clone();
        let publish = HostOptions {
            publish: true,
            watch: false,
            port_offset: self.port_offset,
            ..Default::default()
        };
        let host = match Host::with_store(store.clone(), handle.clone(), publish.clone()) {
            Ok(h) => Some(h),
            Err(e) => {
                let why = match &e {
                    HostError::Already(info) => format!(
                        "Workspace is already hosted by pid {}; servers started here are separate.",
                        info.pid
                    ),
                    HostError::Other(m) => format!("Control socket unavailable: {m}"),
                };
                self.notify(why, cx);
                Host::with_store(
                    store,
                    handle,
                    HostOptions {
                        publish: false,
                        ..publish
                    },
                )
                .ok()
            }
        };
        #[cfg(feature = "dev-mcp")]
        crate::dev_mcp::host::publish_control(
            host.as_ref()
                .map(|h| h.control() as std::sync::Arc<dyn fauxfield_control::ControlApi>),
        );
        self.engine_events = host.as_ref().map(|h| h.engine().subscribe());
        if let Some(h) = &host {
            let a = h.engine().var_context().active_env;
            if !a.is_empty() {
                self.active_env = a;
            }
        }
        self.host = host;
        self.sync_running();
    }

    /// Mirror the engine's statuses into the view's running flags.
    pub(crate) fn sync_running(&mut self) {
        self.sync_dataset_counts();
        let Some(h) = &self.host else { return };
        let list = h.engine().list();
        self.network_profile = h.engine().network_profile();
        for s in &mut self.ws.servers {
            s.chaos = list.iter().find(|x| x.id == s.id).and_then(|x| x.chaos);
            s.cert = list
                .iter()
                .find(|x| x.id == s.id)
                .and_then(|x| x.cert.clone());
            let st = list.iter().find(|x| x.id == s.id).map(|x| &x.status);
            s.running = matches!(st, Some(ServerStatus::Running { .. }));
            s.listen = match st {
                Some(ServerStatus::Running { addr }) => Some(addr.to_string()),
                _ => None,
            };
            s.error = match st {
                Some(ServerStatus::Failed { error, .. }) => Some(error.clone()),
                _ => None,
            };
        }
    }

    pub(crate) fn push_log(&mut self, line: String) {
        self.logs.push(line);
        if self.logs.len() > LOG_CAP {
            let extra = self.logs.len() - LOG_CAP;
            self.logs.drain(..extra);
        }
    }

    /// Drain engine events into Traffic, Logs and run state (called from the 60 ms poll).
    pub(crate) fn poll_engine(&mut self, cx: &mut Context<Self>) {
        let Some(rx) = self.engine_events.as_mut() else {
            return;
        };
        let mut events = vec![];
        loop {
            match rx.try_recv() {
                Ok(ev) => events.push(ev),
                Err(TryRecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
        if events.is_empty() {
            return;
        }
        for ev in events {
            match ev {
                EngineEvent::Traffic(rec) => {
                    self.push_log(format!(
                        "{} {} {} -> {} ({}ms) [{}]",
                        rec.ts, rec.method, rec.url, rec.status, rec.timing.total_ms, rec.route
                    ));
                    let is_test = rec.source == "test";
                    self.traffic.push(TrafficRow::from_recording(&rec));
                    if self.traffic.len() > TRAFFIC_CAP {
                        let extra = self.traffic.len() - TRAFFIC_CAP;
                        self.traffic.drain(..extra);
                        self.traffic_selected =
                            self.traffic_selected.and_then(|i| i.checked_sub(extra));
                    }
                    if is_test && self.select_next_test {
                        self.select_next_test = false;
                        self.traffic_selected = Some(self.traffic.len() - 1);
                        self.layout.right_open = true;
                    }
                }
                EngineEvent::Dataset { .. } => {}
                EngineEvent::Network { profile } => {
                    self.network_profile = profile;
                    self.push_log(format!("Network profile: {}", profile.label()));
                }
                EngineEvent::Chaos { .. } => {}
                EngineEvent::Status { server_id, status } => {
                    let name = self
                        .ws
                        .find_server(&server_id)
                        .map(|s| s.name.clone())
                        .unwrap_or(server_id);
                    let line = match status {
                        ServerStatus::Running { addr } => {
                            format!("{name} listening on http://{addr}")
                        }
                        ServerStatus::Stopped => format!("{name} stopped"),
                        ServerStatus::Failed { error, .. } => format!("{name} failed: {error}"),
                    };
                    self.push_log(line);
                }
            }
        }
        self.sync_running();
        cx.notify();
    }

    // ---- start / stop --------------------------------------------------------------------

    pub fn toggle_server_at(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(s) = self.ws.servers.get(i).cloned() else {
            return;
        };
        if self.host.is_none() {
            // No workspace is hosted (Welcome view and shell tests): flag only.
            if let Some(s) = self.ws.servers.get_mut(i) {
                s.running = !s.running;
            }
            return;
        }
        self.run_job(
            if s.running {
                Job::Stop(s.id)
            } else {
                Job::Start(s.id)
            },
            cx,
        );
    }

    pub fn restart_current(&mut self, cx: &mut Context<Self>) {
        match self
            .current_server()
            .and_then(|i| self.ws.servers.get(i))
            .map(|s| s.id.clone())
        {
            Some(id) if self.host.is_some() => self.run_job(Job::Restart(id), cx),
            Some(_) => {}
            None => self.notify("Select a server or route first.".into(), cx),
        }
    }

    /// Restart a server by id (tree menu).
    pub fn restart_server_id(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.host.is_some() {
            self.run_job(Job::Restart(id.to_string()), cx);
        }
    }

    /// Start `id` once the engine has taken the freshly saved (enabled) definition.
    pub(crate) fn start_server_after_reload(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(host) = &self.host else { return };
        let engine = host.engine().clone();
        let id = id.to_string();
        cx.spawn(async move |this, cx| {
            let eid = id.clone();
            cx.background_executor()
                .spawn(async move {
                    for _ in 0..50 {
                        if engine.list().iter().any(|s| s.id == eid && s.enabled) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                })
                .await;
            this.update(cx, |this, cx| this.run_job(Job::Start(id), cx))
                .ok();
        })
        .detach();
    }

    pub fn run_all(&mut self, cx: &mut Context<Self>) {
        let start = self.ws.running_count() < self.ws.enabled_count();
        if self.host.is_none() {
            for s in &mut self.ws.servers {
                s.running = start;
            }
            return;
        }
        self.run_job(if start { Job::StartAll } else { Job::StopAll }, cx);
    }

    fn run_job(&mut self, job: Job, cx: &mut Context<Self>) {
        let Some(host) = &self.host else { return };
        let engine = host.engine().clone();
        cx.spawn(async move |this, cx| {
            // Binding and draining block briefly, so keep them off the UI thread.
            let (msg, errors) = cx
                .background_executor()
                .spawn(async move {
                    let mut errors: Vec<(String, EngineError)> = vec![];
                    let name_of = |id: &str| {
                        engine
                            .list()
                            .into_iter()
                            .find(|s| s.id == id)
                            .map(|s| s.name)
                            .unwrap_or_else(|| id.to_string())
                    };
                    let msg = match &job {
                        Job::Start(id) | Job::Restart(id) => {
                            let r = if matches!(job, Job::Start(_)) {
                                engine.start(id)
                            } else {
                                engine.restart(id)
                            };
                            match r {
                                Ok(addr) => format!("{} listening on http://{addr}", name_of(id)),
                                Err(e) => {
                                    errors.push((id.clone(), e));
                                    String::new()
                                }
                            }
                        }
                        Job::Stop(id) => {
                            let name = name_of(id);
                            let _ = engine.stop(id);
                            format!("{name} stopped")
                        }
                        Job::StartAll => {
                            errors = engine.start_all();
                            format!("{} servers running", engine.running_count())
                        }
                        Job::StopAll => {
                            engine.stop_all();
                            "All servers stopped".to_string()
                        }
                    };
                    (msg, errors)
                })
                .await;
            this.update(cx, |this, cx| {
                this.sync_running();
                if errors.is_empty() {
                    this.notify(msg, cx);
                }
                for (id, e) in errors {
                    match e {
                        EngineError::PortInUse { server, port } => this.notify_action(
                            format!("Port {port} is already in use ({server})."),
                            Some(NoticeAction {
                                label: "Change port",
                                action: ToastAction::ChangePort(id),
                            }),
                            cx,
                        ),
                        EngineError::Disabled(_) => this.notify_action(
                            "{} is disabled. Enable it first."
                                .replace("{}", &name_of_server(this, &id)),
                            Some(NoticeAction {
                                label: "Enable and start",
                                action: ToastAction::EnableAndStart(id),
                            }),
                            cx,
                        ),
                        other => this.notify(other.to_string(), cx),
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ---- send test, cURL, clear ------------------------------------------------------------

    /// Fire the open route's request at its running server; the result lands in Traffic.
    pub fn send_test(&mut self, cx: &mut Context<Self>) {
        let Some(host) = &self.host else {
            return self.notify("Open a workspace first.".into(), cx);
        };
        let Some((server, route)) = self
            .current_route_id()
            .and_then(|id| self.ws.find_route(&id).map(|(s, r)| (s.clone(), r.clone())))
        else {
            return self.notify("Open a route first.".into(), cx);
        };
        if !server.running {
            return self.notify(format!("Start {} first (Cmd/Ctrl+R).", server.name), cx);
        }
        let req: TestRequest = self.sample_of(&route.id).request(&server, &route);
        let engine = host.engine().clone();
        let id = server.id.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        host.handle().spawn(async move {
            let _ = tx.send(engine.send_test(&id, &req).await);
        });
        self.select_next_test = true;
        cx.spawn(async move |this, cx| {
            if let Ok(Err(e)) = rx.await {
                this.update(cx, |this, cx| {
                    this.select_next_test = false;
                    this.notify(format!("Send test failed: {e}"), cx);
                })
                .ok();
            }
        })
        .detach();
    }

    pub fn copy_curl(&mut self, cx: &mut Context<Self>) {
        let Some(rec) = self
            .traffic_selected
            .and_then(|i| self.traffic.get(i))
            .and_then(|r| r.rec.clone())
        else {
            return self.notify("Select a request in Traffic first.".into(), cx);
        };
        let base = self
            .host
            .as_ref()
            .and_then(|h| h.engine().base_url(&rec.server_id))
            .unwrap_or_else(|| "http://127.0.0.1".to_string());
        cx.write_to_clipboard(ClipboardItem::new_string(to_curl(&rec, &base)));
        self.notify("Copied cURL command".into(), cx);
    }

    /// Rules tab: copy a `curl` that triggers rule `idx` of Response `resp` (all rules of the
    /// Response when they are combined with AND, since each must hold).
    pub fn copy_rule_curl(&mut self, resp: &str, idx: usize, cx: &mut Context<Self>) {
        let Some(route_id) = self.ws.servers.iter().find_map(|s| {
            s.all_routes()
                .find(|r| r.responses.iter().any(|x| x.id == resp))
                .map(|r| r.id.clone())
        }) else {
            return;
        };
        let Some((server, route)) = self.ws.find_route(&route_id) else {
            return;
        };
        let Some(response) = route.responses.iter().find(|x| x.id == resp) else {
            return;
        };
        let Some(rule) = response.rules.get(idx) else {
            return;
        };
        let rules: Vec<&fauxfield_model::Rule> = if response.rules_logic == "or" {
            vec![rule]
        } else {
            response.rules.iter().collect()
        };
        let base = self
            .host
            .as_ref()
            .and_then(|h| h.engine().base_url(&server.id))
            .unwrap_or_else(|| {
                let host = match server.host.as_str() {
                    "" | "0.0.0.0" | "::" => "127.0.0.1",
                    h => h,
                };
                let scheme = if server.https() { "https" } else { "http" };
                format!("{scheme}://{host}:{}", server.port)
            });
        let curl =
            crate::rule_curl::build(&base, &server.prefix, &route.method, &route.path, &rules);
        cx.write_to_clipboard(ClipboardItem::new_string(curl));
        self.notify("Copied cURL command".into(), cx);
    }

    pub fn clear_traffic(&mut self, cx: &mut Context<Self>) {
        self.traffic.clear();
        self.traffic_selected = None;
        if let Some(h) = &self.host {
            h.engine().clear_traffic();
        }
        cx.notify();
    }

    // ---- notices with an action ------------------------------------------------------------

    pub fn notify_action(
        &mut self,
        msg: String,
        action: Option<NoticeAction>,
        cx: &mut Context<Self>,
    ) {
        self.notify(msg, cx);
        self.notice_action = action;
    }

    /// "Change port": open the Server tab and prompt for the port.
    pub fn change_port(
        &mut self,
        server_id: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let Some(s) = self.ws.find_server(server_id).cloned() else {
            return;
        };
        self.open_tab(&s.id, crate::tabs::TabKind::Server, &s.name, false);
        self.server_section.insert(s.id.clone(), 0);
        self.begin_prompt(
            crate::shell::PromptAction::ServerPort { server: s.id },
            &s.port.to_string(),
            window,
            cx,
        );
    }

    // ---- quit confirmation -----------------------------------------------------------------

    pub(crate) fn servers_running(&self) -> usize {
        self.host
            .as_ref()
            .map(|h| h.engine().running_count())
            .unwrap_or(0)
    }

    /// Quit, asking first while servers are running.
    pub fn request_quit(&mut self, cx: &mut Context<Self>) {
        if self.servers_running() > 0 && !self.quit_confirm {
            self.quit_confirm = true;
            cx.notify();
        } else {
            self.quit_now(cx);
        }
    }

    pub fn quit_now(&mut self, cx: &mut Context<Self>) {
        self.quit_confirmed = true;
        self.engine_events = None;
        if let Some(h) = &self.host {
            h.engine().stop_all();
        }
        cx.quit();
    }

    pub fn cancel_quit(&mut self, cx: &mut Context<Self>) {
        self.quit_confirm = false;
        cx.notify();
    }

    /// Window close button: allow it unless servers are running and not yet confirmed.
    pub(crate) fn should_close(&mut self, cx: &mut Context<Self>) -> bool {
        if self.quit_confirmed || self.servers_running() == 0 {
            return true;
        }
        self.quit_confirm = true;
        cx.notify();
        false
    }

    pub(crate) fn render_quit_confirm(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let n = self.servers_running();
        let button = |id: &'static str, label: &'static str| {
            toolbar_button(t, id).h(px(26.)).px(px(12.)).child(label)
        };
        crate::dev_id_z!(
            div()
                .id("quit-confirm")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(t.background.opacity(0.6))
                .child(
                    div()
                        .w(px(380.))
                        .p(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(t.border)
                        .bg(t.elevated)
                        .child(div().text_size(px(14.)).child("Quit Fauxfield?"))
                        .child(div().text_color(t.text_muted).child(format!(
                            "{n} server{} will stop.",
                            if n == 1 { "" } else { "s" }
                        )))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .justify_end()
                                .gap(px(8.))
                                .child(crate::dev_id!(
                                    button("quit-cancel", "Cancel").on_click(
                                        cx.listener(|this, _, _, cx| this.cancel_quit(cx))
                                    ),
                                    "quit-cancel",
                                    Button,
                                    "Cancel",
                                    Action("cancel_quit")
                                ))
                                .child(crate::dev_id!(
                                    button("quit-confirm-yes", "Quit")
                                        .on_click(cx.listener(|this, _, _, cx| this.quit_now(cx))),
                                    "quit-confirm-yes",
                                    Button,
                                    "Quit",
                                    Action("quit_anyway")
                                )),
                        ),
                ),
            10,
            "quit-confirm",
            Modal,
            "Quit Fauxfield?",
            None
        )
    }
}

fn name_of_server(shell: &Shell, id: &str) -> String {
    shell
        .ws
        .find_server(id)
        .map(|s| s.name.clone())
        .unwrap_or_else(|| id.to_string())
}
