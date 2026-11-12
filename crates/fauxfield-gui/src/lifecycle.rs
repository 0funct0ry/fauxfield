//! Workspace lifecycle (M3.1): the native folder picker, the shared close/switch guard, closing,
//! recent workspaces, renaming, server field edits, and the confirmation modals. The workspace
//! files stay authoritative; everything here is a thin layer over `fauxfield-store` and the
//! host owned by [`Shell`].

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, PathPromptOptions,
    StatefulInteractiveElement, Styled, Window, div, prelude::*, px,
};

use crate::run::{NoticeAction, ToastAction};
use crate::shell::{PromptAction, Shell};
use crate::tabs::TabSet;
use crate::theme::Theme;
use crate::ui::toolbar_button;
use crate::view::Workspace;

/// What the guard is protecting.
#[derive(Clone, Debug, PartialEq)]
pub enum Intent {
    Close,
    /// Replace the open workspace by `path` (creating it first when `create`).
    Switch {
        path: PathBuf,
        create: bool,
    },
}

/// A Recent workspaces entry as shown in Welcome and the workspace menu.
#[derive(Clone, Debug, PartialEq)]
pub struct RecentEntry {
    pub path: PathBuf,
    pub name: String,
    pub exists: bool,
    pub opened: String,
}

/// A Dataset delete waiting on the confirmation modal (M10.01).
#[derive(Clone, Debug, PartialEq)]
pub struct DatasetDelete {
    pub key: String,
    pub name: String,
    pub server_id: String,
    /// Route labels and template locations that still reference the Dataset.
    pub refs: Vec<String>,
    pub remove_seed: bool,
    pub remove_state: bool,
}

/// State added to [`Shell`] by M3.1.
#[derive(Default)]
pub struct Lifecycle {
    pub close_confirm: Option<Intent>,
    /// Server id awaiting a delete confirmation.
    pub delete_confirm: Option<String>,
    /// Dataset delete awaiting confirmation (M10.01).
    pub dataset_delete: Option<DatasetDelete>,
    /// Folder (and its item count) awaiting a "create in a non-empty folder" confirmation.
    pub create_confirm: Option<(PathBuf, usize)>,
    /// Inline message shown in the open prompt instead of a toast.
    pub prompt_error: Option<String>,
    /// File the next Undo/Redo applies to when it is not a server file (workspace rename).
    pub undo_file: Option<String>,
    /// Generation of the store poll loop; a stale loop stops itself.
    pub poll_gen: u64,
    pub window_title: String,
    /// When each workspace was opened in this session.
    pub opened_at: std::collections::HashMap<PathBuf, SystemTime>,
}

fn relative(t: SystemTime) -> String {
    let secs = SystemTime::now()
        .duration_since(t)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

/// The `name` of a folder's fauxfield.toml, else the folder name.
fn workspace_name_at(path: &Path) -> String {
    std::fs::read_to_string(path.join("fauxfield.toml"))
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok())
        .and_then(|t| t.get("name").and_then(|n| n.as_str().map(str::to_string)))
        .filter(|n| !n.is_empty())
        .or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| path.display().to_string())
}

impl Shell {
    // ---- titles ----------------------------------------------------------------------------

    /// "Fauxfield — <name>" while a workspace is open, "Fauxfield" otherwise.
    pub fn window_title_text(&self) -> String {
        if self.store.is_some() {
            format!("Fauxfield — {}", self.ws.name)
        } else {
            "Fauxfield".to_string()
        }
    }

    pub(crate) fn sync_window_title(&mut self, window: &mut Window) {
        let title = self.window_title_text();
        if self.lc.window_title != title {
            window.set_window_title(&title);
            self.lc.window_title = title;
        }
    }

    // ---- native folder picker --------------------------------------------------------------

    /// Ask the OS for a folder (gpui `prompt_for_paths`, directories only, single selection).
    /// Cancelling does nothing; a failed dialog falls back to the typed-path prompt.
    pub fn pick_folder(&mut self, create: bool, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(
                if create {
                    "Create workspace here"
                } else {
                    "Open workspace folder"
                }
                .into(),
            ),
        });
        cx.spawn_in(window, async move |this, cx| match rx.await {
            Ok(Ok(Some(paths))) => {
                if let Some(p) = paths.into_iter().next() {
                    this.update(cx, |this, cx| {
                        if create {
                            this.create_folder_chosen(p, cx)
                        } else {
                            this.open_folder_chosen(p, cx)
                        }
                    })
                    .ok();
                }
            }
            // Cancelled: nothing happens, no toast.
            Ok(Ok(None)) => {}
            // No dialog available (a Linux session without a portal): type the path instead.
            _ => {
                this.update_in(cx, |this, w, cx| {
                    let action = if create {
                        PromptAction::CreateWorkspace
                    } else {
                        PromptAction::OpenFolder
                    };
                    this.begin_prompt(action, "", w, cx)
                })
                .ok();
            }
        })
        .detach();
    }

    /// A folder was chosen to open (dialog, typed path, or toast action).
    pub fn open_folder_chosen(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !path.join("fauxfield.toml").is_file() {
            return self.notify_action(
                format!(
                    "No fauxfield.toml in {}. Use Create workspace to start one here.",
                    path.display()
                ),
                Some(NoticeAction {
                    label: "Create workspace here",
                    action: ToastAction::CreateWorkspaceAt(path),
                }),
                cx,
            );
        }
        self.guard(
            Intent::Switch {
                path,
                create: false,
            },
            cx,
        );
    }

    /// A folder was chosen to hold a new workspace.
    pub fn create_folder_chosen(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if path.join("fauxfield.toml").exists() {
            return self.notify_action(
                format!("{} is already a workspace.", path.display()),
                Some(NoticeAction {
                    label: "Open it",
                    action: ToastAction::OpenWorkspaceAt(path),
                }),
                cx,
            );
        }
        let n = std::fs::read_dir(&path).map(|d| d.count()).unwrap_or(0);
        if n > 0 {
            self.lc.create_confirm = Some((path, n));
            cx.notify();
            return;
        }
        self.guard(Intent::Switch { path, create: true }, cx);
    }

    // ---- the guard ---------------------------------------------------------------------------

    /// Shared by Close, Open folder, Create workspace and Recent whenever a workspace is open:
    /// refuse on merge conflicts, flush autosave, confirm when servers are running.
    pub fn guard(&mut self, intent: Intent, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            if intent != Intent::Close {
                self.perform(intent, cx);
            }
            return;
        };
        if !store.conflicts().is_empty() {
            return self.notify("Resolve the merge banner first.".into(), cx);
        }
        if let Err(e) = store.flush() {
            return self.notify(format!("Could not save: {e}"), cx);
        }
        if self.servers_running() > 0 {
            self.lc.close_confirm = Some(intent);
            cx.notify();
            return;
        }
        self.perform(intent, cx);
    }

    fn perform(&mut self, intent: Intent, cx: &mut Context<Self>) {
        match intent {
            Intent::Close => self.close_workspace_now(cx),
            Intent::Switch { path, create } => {
                let r = if create {
                    self.create_workspace(&path, cx)
                } else {
                    self.open_workspace(&path, cx)
                };
                if let Err(e) = r {
                    self.notify(e, cx);
                }
            }
        }
    }

    /// Stop everything, release the host, forget the workspace and show Welcome.
    pub fn close_workspace_now(&mut self, cx: &mut Context<Self>) {
        if self.store.is_none() {
            return;
        }
        let root = self.ws.root.clone();
        self.teardown();
        if let Some(p) = &self.recent_path {
            let _ = fauxfield_store::push_recent(p, &root);
            self.recents = fauxfield_store::load_recent(p);
        }
        cx.notify();
    }

    /// Release the open workspace's resources. Does not touch Recent.
    pub(crate) fn teardown(&mut self) {
        self.engine_events = None;
        if let Some(h) = &self.host {
            h.engine().stop_all();
        }
        // Dropping the host releases `.fauxfield/run/host.json` and the control socket.
        self.host = None;
        #[cfg(feature = "dev-mcp")]
        {
            crate::dev_mcp::host::publish_control(None);
            crate::dev_mcp::host::publish_store(None);
        }
        if let Some(store) = &self.store {
            let _ = store.flush();
            // Merge, so keys owned elsewhere (sample_requests) survive.
            let mut state = fauxfield_store::load_ui_state(&store.root());
            if !state.is_object() {
                state = serde_json::json!({});
            }
            state["tabs"] = serde_json::json!(self.tabs.tabs());
            state["active_tab"] = serde_json::json!(self.tabs.active().map(|t| t.id.clone()));
            let _ = fauxfield_store::save_ui_state(&store.root(), &state);
        }
        self.store = None;
        self.store_events = None;
        self._background = None;
        self.lc.poll_gen += 1;
        self.ws = Workspace::empty();
        self.tabs = TabSet::from_saved(vec![], None);
        self.selected = None;
        self.traffic.clear();
        self.traffic_selected = None;
        self.logs.clear();
        self.problems.clear();
        self.conflicts.clear();
        self.notice = None;
        self.notice_action = None;
        self.menu = None;
        self.prompt = None;
        self.picker = None;
        self.quit_confirm = false;
        self.resp_sel.clear();
        self.route_sub.clear();
        self.server_section.clear();
        self.collapsed.clear();
        self.active_env = String::new();
        self.lc.close_confirm = None;
        self.lc.delete_confirm = None;
        self.lc.dataset_delete = None;
        self.lc.create_confirm = None;
        self.lc.prompt_error = None;
        self.lc.undo_file = None;
        self.save_layout();
    }

    // ---- modals ------------------------------------------------------------------------------

    pub fn confirm_dialog(&mut self, cx: &mut Context<Self>) {
        if let Some(intent) = self.lc.close_confirm.take() {
            self.perform(intent, cx);
        } else if let Some(id) = self.lc.delete_confirm.take() {
            self.delete_server_now(&id, cx);
        } else if let Some(dd) = self.lc.dataset_delete.take() {
            self.delete_dataset_now(&dd, cx);
        } else if let Some((path, _)) = self.lc.create_confirm.take() {
            self.guard(Intent::Switch { path, create: true }, cx);
        }
        cx.notify();
    }

    pub fn cancel_dialog(&mut self, cx: &mut Context<Self>) {
        self.lc.close_confirm = None;
        self.lc.delete_confirm = None;
        self.lc.dataset_delete = None;
        self.lc.create_confirm = None;
        cx.notify();
    }

    pub(crate) fn any_dialog_open(&self) -> bool {
        self.lc.close_confirm.is_some()
            || self.lc.delete_confirm.is_some()
            || self.lc.dataset_delete.is_some()
            || self.lc.create_confirm.is_some()
    }

    /// Which confirmation modal is open, for `ui.state.modal`.
    pub fn dialog_name(&self) -> Option<&'static str> {
        if self.lc.close_confirm.is_some() {
            Some("close_confirm")
        } else if self.lc.delete_confirm.is_some() {
            Some("delete_confirm")
        } else if self.lc.dataset_delete.is_some() {
            Some("delete_dataset_confirm")
        } else if self.lc.create_confirm.is_some() {
            Some("create_confirm")
        } else if self.about_open {
            Some("about")
        } else if self.sample_dialog.is_some() {
            Some("sample_request")
        } else {
            None
        }
    }

    pub(crate) fn render_dialogs(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        if let Some(intent) = &self.lc.close_confirm {
            let n = self.servers_running();
            let plural = if n == 1 { "" } else { "s" };
            let (verb, title) = match intent {
                Intent::Close => (
                    "Close",
                    format!("Stop {n} server{plural} and close {}?", self.ws.name),
                ),
                Intent::Switch { path, .. } => (
                    "Switch",
                    format!(
                        "Stop {n} server{plural} and switch to {}?",
                        path.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    ),
                ),
            };
            return self
                .confirm_modal(t, "close-confirm", title, vec![], verb, cx)
                .into_any_element();
        }
        if let Some(id) = &self.lc.delete_confirm
            && let Some(s) = self.ws.find_server(id)
        {
            let mut body = vec![
                format!("File: {}", s.file),
                format!(
                    "{} route{}",
                    s.all_routes().count(),
                    if s.all_routes().count() == 1 { "" } else { "s" }
                ),
            ];
            if s.running {
                body.push("It will be stopped.".into());
            }
            return self
                .confirm_modal(
                    t,
                    "delete-confirm",
                    format!("Delete {}?", s.name),
                    body,
                    "Delete",
                    cx,
                )
                .into_any_element();
        }
        if let Some(dd) = &self.lc.dataset_delete {
            let mut body = if dd.refs.is_empty() {
                vec!["Nothing uses this Dataset.".to_string()]
            } else {
                let mut b = vec![format!(
                    "Still used by {} reference{}:",
                    dd.refs.len(),
                    if dd.refs.len() == 1 { "" } else { "s" }
                )];
                b.extend(dd.refs.iter().map(|r| format!("  {r}")));
                b.push("Routes that use it will report E072 until repointed.".into());
                b
            };
            body.push("Seed file and persisted state are kept unless ticked below.".into());
            let toggle =
                |id: &'static str, label: &'static str, on: bool, cx: &mut Context<Self>| {
                    crate::dev_id!(
                        toolbar_button(t, id)
                            .h(px(24.))
                            .px(px(8.))
                            .when(on, |d| d.bg(t.element_selected))
                            .child(format!("{} {label}", if on { "[x]" } else { "[ ]" }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(dd) = this.lc.dataset_delete.as_mut() {
                                    if id == "delete-dataset-seed" {
                                        dd.remove_seed = !dd.remove_seed;
                                    } else {
                                        dd.remove_state = !dd.remove_state;
                                    }
                                }
                                cx.notify();
                            })),
                        id,
                        Button,
                        label,
                        None
                    )
                    .into_any_element()
                };
            let extra = vec![
                toggle(
                    "delete-dataset-seed",
                    "Also delete the seed file",
                    dd.remove_seed,
                    cx,
                ),
                toggle(
                    "delete-dataset-state",
                    "Also delete the persisted state",
                    dd.remove_state,
                    cx,
                ),
            ];
            return self
                .confirm_modal_with(
                    t,
                    "delete-dataset-confirm",
                    format!("Delete dataset {}?", dd.name),
                    body,
                    extra,
                    "Delete",
                    cx,
                )
                .into_any_element();
        }
        if let Some((path, n)) = &self.lc.create_confirm {
            return self
                .confirm_modal(
                    t,
                    "create-confirm",
                    format!(
                        "This folder has {n} item{}. Create a workspace in it?",
                        if *n == 1 { "" } else { "s" }
                    ),
                    vec![
                        path.display().to_string(),
                        "Existing files are never overwritten.".into(),
                    ],
                    "Create",
                    cx,
                )
                .into_any_element();
        }
        div().into_any_element()
    }

    fn confirm_modal(
        &self,
        t: &Theme,
        key: &'static str,
        title: String,
        body: Vec<String>,
        confirm: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.confirm_modal_with(t, key, title, body, vec![], confirm, cx)
    }

    #[allow(clippy::too_many_arguments)]
    fn confirm_modal_with(
        &self,
        t: &Theme,
        key: &'static str,
        title: String,
        body: Vec<String>,
        extra: Vec<gpui::AnyElement>,
        confirm: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let button = |id: String, label: &'static str| {
            toolbar_button(t, crate::ui::eid(id))
                .h(px(26.))
                .px(px(12.))
                .child(label)
        };
        #[cfg(feature = "dev-mcp")]
        let label = title.clone();
        crate::dev_id_z!(
            div()
                .id(crate::ui::eid(key))
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
                        .child(div().text_size(px(14.)).child(title))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .children(body.into_iter().map(|l| {
                                    div().text_color(t.text_muted).text_size(px(12.)).child(l)
                                },))
                        )
                        .children(extra)
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .justify_end()
                                .gap(px(8.))
                                .child(crate::dev_id!(
                                    button(format!("{key}-cancel"), "Cancel").on_click(
                                        cx.listener(|this, _, _, cx| this.cancel_dialog(cx))
                                    ),
                                    format!("{key}-cancel"),
                                    Button,
                                    "Cancel",
                                    Action("cancel_dialog")
                                ))
                                .child(crate::dev_id!(
                                    button(format!("{key}-yes"), confirm).on_click(
                                        cx.listener(|this, _, _, cx| this.confirm_dialog(cx))
                                    ),
                                    format!("{key}-yes"),
                                    Button,
                                    confirm,
                                    Action("confirm_dialog")
                                )),
                        ),
                ),
            10,
            key,
            Modal,
            label,
            None
        )
    }

    // ---- toast actions -----------------------------------------------------------------------

    pub fn run_toast_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(a) = self.notice_action.take() else {
            return;
        };
        match a.action {
            ToastAction::ChangePort(id) => self.change_port(&id, window, cx),
            ToastAction::CreateWorkspaceAt(p) => self.create_folder_chosen(p, cx),
            ToastAction::OpenWorkspaceAt(p) => self.open_folder_chosen(p, cx),
            ToastAction::EnableAndStart(id) => {
                use fauxfield_store::Target as T;
                if self
                    .store_op(cx, |s| s.set_bool(&T::Server(id.clone()), "enabled", true))
                    .is_some()
                {
                    // The engine hot-swaps the definition on the store event; start after it lands.
                    self.start_server_after_reload(&id, cx);
                }
            }
        }
    }

    // ---- recent workspaces ---------------------------------------------------------------------

    pub fn recent_entries(&self) -> Vec<RecentEntry> {
        self.recents
            .iter()
            .take(10)
            .map(|p| {
                let exists = p.join("fauxfield.toml").is_file();
                let opened = self
                    .lc
                    .opened_at
                    .get(p)
                    .copied()
                    .or_else(|| {
                        std::fs::metadata(p.join(".fauxfield/ui.json"))
                            .and_then(|m| m.modified())
                            .ok()
                    })
                    .map(relative)
                    .unwrap_or_else(|| "earlier".into());
                RecentEntry {
                    name: if exists {
                        workspace_name_at(p)
                    } else {
                        p.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    },
                    path: p.clone(),
                    exists,
                    opened,
                }
            })
            .collect()
    }

    pub fn open_recent(&mut self, path: &Path, cx: &mut Context<Self>) {
        if !path.join("fauxfield.toml").is_file() {
            return self.notify(format!("{} no longer exists", path.display()), cx);
        }
        self.open_folder_chosen(path.to_path_buf(), cx);
    }

    /// Edits recent.json only; the folder is never touched.
    pub fn remove_recent(&mut self, path: &Path, cx: &mut Context<Self>) {
        if let Some(p) = &self.recent_path {
            let _ = fauxfield_store::remove_recent(p, path);
            self.recents = fauxfield_store::load_recent(p);
        }
        cx.notify();
    }

    /// Dev helper: put `path` first in Recent even when it does not exist.
    #[cfg(feature = "dev-mcp")]
    pub fn add_recent(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if let Some(p) = &self.recent_path {
            let _ = fauxfield_store::push_recent(p, &path);
            self.recents = fauxfield_store::load_recent(p);
        }
        cx.notify();
    }

    pub fn clear_recent(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = &self.recent_path {
            let _ = fauxfield_store::clear_recent(p);
            self.recents.clear();
        }
        cx.notify();
    }

    // ---- rename workspace ------------------------------------------------------------------------

    pub fn rename_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.store.is_none() {
            return self.notify("Open a workspace first.".into(), cx);
        }
        let name = self.ws.name.clone();
        self.begin_prompt(PromptAction::RenameWorkspace, &name, window, cx);
    }

    // ---- server field edits ----------------------------------------------------------------------

    fn open_server_for_edit(&mut self, id: &str) -> Option<crate::view::Server> {
        let s = self.ws.find_server(id).cloned()?;
        self.open_tab(&s.id, crate::tabs::TabKind::Server, &s.name, false);
        Some(s)
    }

    pub fn change_host(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = self.open_server_for_edit(id) {
            self.begin_prompt(
                PromptAction::ServerHost { server: s.id },
                &s.host,
                window,
                cx,
            );
        }
    }

    pub fn change_prefix(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = self.open_server_for_edit(id) {
            self.begin_prompt(
                PromptAction::ServerPrefix { server: s.id },
                &s.prefix,
                window,
                cx,
            );
        }
    }

    pub fn toggle_enabled(&mut self, id: &str, cx: &mut Context<Self>) {
        use fauxfield_store::Target as T;
        let Some(s) = self.ws.find_server(id).cloned() else {
            return;
        };
        let enable = !s.enabled;
        let sid = s.id.clone();
        if self
            .store_op(cx, |st| st.set_bool(&T::Server(sid), "enabled", enable))
            .is_none()
        {
            return;
        }
        if enable {
            self.notify(format!("{} enabled", s.name), cx);
        } else if s.running {
            if let Some(h) = &self.host {
                let _ = h.engine().stop(id);
            }
            self.sync_running();
            self.notify(format!("{} disabled and stopped", s.name), cx);
        } else {
            self.notify(format!("{} disabled", s.name), cx);
        }
    }

    pub fn toggle_unmatched_hint(&mut self, id: &str, cx: &mut Context<Self>) {
        use fauxfield_store::Target as T;
        let Some(s) = self.ws.find_server(id).cloned() else {
            return;
        };
        let on = !s.unmatched_hint;
        let sid = s.id.clone();
        self.store_op(cx, |st| st.set_bool(&T::Server(sid), "unmatched_hint", on));
    }

    /// Validate a prompt value before it closes the prompt; the message shows inline.
    pub(crate) fn validate_prompt(action: &PromptAction, value: &str) -> Result<(), String> {
        match action {
            PromptAction::RenameWorkspace => {
                fauxfield_model::validate_workspace_name(value).map(|_| ())
            }
            PromptAction::ServerHost { .. } => fauxfield_model::validate_host(value).map(|_| ()),
            PromptAction::ServerPrefix { .. } => {
                fauxfield_model::validate_prefix(value).map(|_| ())
            }
            PromptAction::NewDataset { .. }
            | PromptAction::RenameDataset { .. }
            | PromptAction::NewDatasetTableName { .. }
            | PromptAction::NewCrudName { .. } => {
                let ok = !value.is_empty()
                    && value
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '-');
                ok.then_some(())
                    .ok_or_else(|| "Use letters, digits, `_` and `-`.".to_string())
            }
            PromptAction::NewDatasetColumns { .. } => {
                crate::dataset_edit::parse_columns(value).map(|_| ())
            }
            PromptAction::GenerateRows { .. } => {
                crate::dataset_edit::parse_generate(value).map(|_| ())
            }
            PromptAction::NewCrudPath { .. } => value
                .starts_with('/')
                .then_some(())
                .ok_or_else(|| "Path must start with `/`.".to_string()),
            PromptAction::NewCrudFields { .. } => {
                crate::dataset_ui::parse_fields(value).map(|_| ())
            }
            PromptAction::AddRow { .. } => match serde_json::from_str::<serde_json::Value>(value) {
                Ok(serde_json::Value::Object(_)) => Ok(()),
                _ => Err("A row must be a JSON object.".into()),
            },
            _ => Ok(()),
        }
    }

    // ---- delete with confirmation ------------------------------------------------------------------

    pub fn request_delete_server(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.ws.find_server(id).is_some() {
            self.lc.delete_confirm = Some(id.to_string());
            cx.notify();
        }
    }

    pub(crate) fn delete_server_now(&mut self, id: &str, cx: &mut Context<Self>) {
        use fauxfield_store::Target as T;
        let name = self
            .ws
            .find_server(id)
            .map(|s| s.name.clone())
            .unwrap_or_default();
        if let Some(h) = &self.host {
            let _ = h.engine().stop(id);
        }
        let t = T::Server(id.to_string());
        if self.store_op(cx, |s| s.delete(&t)).is_some() {
            self.selected = None;
            self.notify(format!("Deleted {name} (undo with Edit: Undo)"), cx);
        }
    }
}

/// Verbs offered by the route method dropdown (SPEC §5.2; ANY arrives with M4).
pub const METHODS: [&str; 7] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

impl Shell {
    /// Open the method ComboBox of the current route.
    pub fn open_method_menu(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let Some(route) = self.current_route_id() else {
            return self.notify("Open a route first.".into(), cx);
        };
        if !self.combo_open(&format!("route-method-{route}"), window, cx) {
            self.notify("Open the route tab first.".into(), cx);
        }
    }

    /// Set the route's verb from the dropdown.
    pub fn set_route_method(&mut self, route: &str, method: &str, cx: &mut Context<Self>) {
        use fauxfield_store::Target as T;
        let r = route.to_string();
        let m = method.to_ascii_uppercase();
        self.store_op(cx, |s| s.set_str(&T::Route(r), "method", Some(&m)));
    }

    /// The Route verb: a badge ComboBox with a "Custom…" entry for any other verb.
    pub(crate) fn route_method_combo(
        &self,
        t: &Theme,
        r: &crate::view::Route,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        use crate::combo_box::*;
        let current = r.method.to_ascii_uppercase();
        let mut items: Vec<ComboItem> = METHODS
            .iter()
            .map(|m| ComboItem::plain(*m).color(t.method_color(m)))
            .collect();
        items.push(ComboItem::action("custom", "Custom…").separator());
        let rid = r.id.clone();
        self.combo_field(
            cx,
            ComboSite {
                key: format!("route-method-{}", r.id),
                items,
                value: Some(current.clone().into()),
                fallback: Some(r.method.clone().into()),
                color: Some(t.method_color(&r.method)),
                tip: None,
                width: ComboWidth::Auto,
                look: ComboLook::Badge,
                size: ComboSize::Medium,
                on_event: std::rc::Rc::new(move |this, ev, cx| match ev {
                    ComboEvent::Change(m) => this.set_route_method(&rid, m, cx),
                    ComboEvent::Action(_) => this.defer_action("edit_route_method_custom", cx),
                    _ => {}
                }),
            },
        )
    }

    /// The Dataset a `crud` Route serves, repointable among the Server's Datasets (M10.01).
    pub(crate) fn route_dataset_combo(
        &self,
        server: &str,
        route: &str,
        current: Option<&str>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        use crate::combo_box::*;
        let (sid, rid) = (server.to_string(), route.to_string());
        self.combo_field(
            cx,
            ComboSite {
                key: format!("route-dataset-{route}"),
                items: self
                    .ws
                    .datasets
                    .iter()
                    .filter(|d| d.server_id == server)
                    .map(|d| ComboItem::plain(d.name.clone()))
                    .collect(),
                value: current.map(|c| c.to_string().into()),
                fallback: Some(current.unwrap_or("(none)").to_string().into()),
                color: None,
                tip: None,
                width: ComboWidth::Fixed(160.),
                look: ComboLook::Field,
                size: ComboSize::Small,
                on_event: std::rc::Rc::new(move |this, ev, cx| {
                    if let ComboEvent::Change(d) = ev {
                        let (sid, rid, d) = (sid.clone(), rid.clone(), d.to_string());
                        this.store_op(cx, |s| s.set_route_dataset(&sid, &rid, &d));
                    }
                }),
            },
        )
    }

    /// The Response mode of a Route.
    pub(crate) fn response_mode_combo(
        &self,
        route: &str,
        mode: &str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        use crate::combo_box::*;
        let rid = route.to_string();
        self.combo_field(
            cx,
            ComboSite {
                key: format!("route-response-mode-{route}"),
                items: fauxfield_model::rules::RESPONSE_MODES
                    .iter()
                    .map(|m| ComboItem::plain(*m))
                    .collect(),
                value: Some(mode.to_string().into()),
                fallback: Some(mode.to_string().into()),
                color: None,
                tip: None,
                width: ComboWidth::Fixed(140.),
                look: ComboLook::Field,
                size: ComboSize::Small,
                on_event: std::rc::Rc::new(move |this, ev, cx| {
                    if let ComboEvent::Change(m) = ev {
                        this.set_response_mode_value(&rid, m, cx);
                    }
                }),
            },
        )
    }
}
