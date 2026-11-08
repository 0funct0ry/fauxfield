//! `ui.state`: a serde view model of the whole window, built in one function (`snapshot`) so the
//! tests can compare it with what the `Shell` holds.

use gpui::{App, Window};
use serde::Serialize;

use crate::layout::BottomTab;
use crate::shell::{PickerKind, Shell};
use crate::tabs::TabKind;
use crate::theme::Theme;
use crate::tree;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UiState {
    /// Increases when the snapshot changes (not on every redraw); see [`Tracker`].
    pub generation: u64,
    pub workspace: WorkspaceState,
    pub tabs: Vec<TabState>,
    pub active_tab: Option<String>,
    pub docks: Docks,
    pub bottom_tab: BottomTab,
    pub tree: TreeState,
    pub quick_search: PickerState,
    pub command_palette: PickerState,
    pub theme_selector: PickerState,
    pub environment_picker: PickerState,
    /// The open modal surface other than the pickers, if any (`row_menu`).
    pub modal: Option<String>,
    pub toasts: Vec<String>,
    pub traffic: TrafficState,
    pub problems: usize,
    /// The Problems list itself (validation diagnostics).
    pub problem_list: Vec<ProblemState>,
    pub logs: usize,
    /// `welcome` when no workspace is open, else `workspace`.
    pub view: &'static str,
    /// Files whose external edit collides with unsaved edits (merge banner shown when non-empty).
    pub merge_conflicts: Vec<String>,
    pub recent_workspaces: Vec<RecentState>,
    /// Open one-line prompt (rename, new item, field edit), if any.
    pub prompt: Option<String>,
    /// Inline validation message of the open prompt, if any.
    pub prompt_error: Option<String>,
    pub servers: Vec<ServerState>,
    /// Label of the button beside the toast (for example `Change port`), if any.
    pub toast_action: Option<String>,
    pub active_environment: String,
    pub window: WindowState,
    pub theme: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProblemState {
    pub severity: &'static str,
    pub code: String,
    pub message: String,
    pub file: String,
    pub path: String,
    pub target: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WorkspaceState {
    pub name: String,
    pub path: Option<String>,
    /// The open workspace folder; `None` in the Welcome view.
    pub root: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RecentState {
    pub path: String,
    pub name: String,
    pub exists: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TabState {
    pub id: String,
    pub kind: TabKind,
    pub title: String,
    pub preview: bool,
    pub pinned: bool,
    pub dirty: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Dock {
    pub open: bool,
    pub size: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Docks {
    pub left: Dock,
    pub right: Dock,
    pub bottom: Dock,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TreeState {
    pub selected: Option<String>,
    pub expanded: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PickerState {
    pub open: bool,
    pub query: String,
    pub result_count: usize,
    /// Label of the highlighted entry (for the theme selector, the theme being previewed).
    pub highlighted: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TrafficState {
    pub row_count: usize,
    pub filters: Vec<String>,
    pub selected: Option<usize>,
    /// Newest engine recording ID shown (0 when only synthetic rows or none).
    pub last_id: u64,
    pub last: Option<LastRequest>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LastRequest {
    pub method: String,
    pub path: String,
    pub status: u16,
    pub route: String,
    pub server: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ServerState {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub prefix: String,
    pub enabled: bool,
    pub unmatched_hint: bool,
    pub status: &'static str,
    /// `host:port` while running.
    pub address: Option<String>,
    /// Why the last start failed, while it is in `failed`.
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WindowState {
    pub width: f32,
    pub height: f32,
    pub scale: f32,
}

fn picker(shell: &Shell, kind: PickerKind) -> PickerState {
    match &shell.picker {
        Some(p) if p.kind == kind => PickerState {
            open: true,
            query: p.query.clone(),
            result_count: p.hits.len(),
            highlighted: p.hits.get(p.selected).map(|h| h.label.clone()),
        },
        _ => PickerState::default(),
    }
}

/// Whether the file behind a tab holds unsaved (not yet autosaved) edits.
fn tab_dirty(shell: &Shell, kind: TabKind, id: &str) -> bool {
    let Some(store) = &shell.store else {
        return false;
    };
    let file = match kind {
        TabKind::Environment => return false,
        TabKind::Server => shell.ws.find_server(id).map(|s| s.file.clone()),
        TabKind::Route => shell.ws.find_route(id).map(|(s, _)| s.file.clone()),
    };
    file.is_some_and(|f| store.is_dirty(&f))
}

/// Builds the snapshot with `generation` 0; [`Tracker::observe`] assigns the real one.
pub fn snapshot(shell: &Shell, window: &Window, cx: &App, workspace_path: Option<&str>) -> UiState {
    let rows = tree::flatten(&shell.ws, &shell.collapsed, &shell.active_env);
    let size = window.viewport_size();
    UiState {
        generation: 0,
        workspace: WorkspaceState {
            name: shell.ws.name.clone(),
            path: shell
                .store
                .as_ref()
                .map(|_| shell.ws.root.display().to_string())
                .or_else(|| workspace_path.map(str::to_string)),
            root: shell
                .store
                .as_ref()
                .map(|_| shell.ws.root.display().to_string()),
        },
        tabs: shell
            .tabs
            .tabs()
            .iter()
            .map(|t| TabState {
                id: t.id.clone(),
                kind: t.kind,
                title: t.title.clone(),
                preview: t.preview,
                pinned: !t.preview,
                dirty: tab_dirty(shell, t.kind, &t.id),
            })
            .collect(),
        active_tab: shell.tabs.active().map(|t| t.id.clone()),
        docks: Docks {
            left: Dock {
                open: shell.layout.left_open,
                size: shell.layout.left_width,
            },
            right: Dock {
                open: shell.layout.right_open,
                size: shell.layout.right_width,
            },
            bottom: Dock {
                open: shell.layout.bottom_open,
                size: shell.layout.bottom_height,
            },
        },
        bottom_tab: shell.layout.bottom_tab,
        tree: TreeState {
            selected: shell.selected.clone(),
            expanded: rows
                .iter()
                .filter(|r| r.expandable && r.expanded)
                .map(|r| r.id.clone())
                .collect(),
        },
        quick_search: picker(shell, PickerKind::Search),
        command_palette: picker(shell, PickerKind::Palette),
        theme_selector: picker(shell, PickerKind::Theme),
        environment_picker: picker(shell, PickerKind::Environment),
        modal: if shell.quit_confirm {
            Some("quit_confirm".to_string())
        } else if shell.lc.method_menu.is_some() {
            Some("method_menu".to_string())
        } else if let Some(d) = shell.dialog_name() {
            Some(d.to_string())
        } else {
            shell.menu.as_ref().map(|_| "row_menu".to_string())
        },
        toasts: shell.notice.iter().map(|(_, m)| m.clone()).collect(),
        toast_action: shell.notice_action.as_ref().map(|a| a.label.to_string()),
        traffic: TrafficState {
            row_count: shell.traffic.len(),
            filters: Vec::new(),
            selected: shell.traffic_selected,
            last_id: shell
                .traffic
                .iter()
                .rev()
                .find(|r| r.id > 0)
                .map(|r| r.id)
                .unwrap_or(0),
            last: shell
                .traffic
                .last()
                .filter(|r| r.id > 0)
                .map(|r| LastRequest {
                    method: r.method.clone(),
                    path: r.path.clone(),
                    status: r.status,
                    route: r.route.clone(),
                    server: r.server.clone(),
                }),
        },
        problems: shell.problems.len(),
        problem_list: shell
            .problems
            .iter()
            .map(|p| ProblemState {
                severity: if p.warning { "warning" } else { "error" },
                code: p.code.clone(),
                message: p.message.clone(),
                file: p.file.clone(),
                path: p.path.clone(),
                target: p.target.clone(),
            })
            .collect(),
        logs: shell.logs.len(),
        view: if shell.store.is_some() {
            "workspace"
        } else {
            "welcome"
        },
        merge_conflicts: shell.conflicts.clone(),
        recent_workspaces: shell
            .recent_entries()
            .into_iter()
            .map(|r| RecentState {
                path: r.path.display().to_string(),
                name: r.name,
                exists: r.exists,
            })
            .collect(),
        prompt: shell
            .picker
            .as_ref()
            .filter(|p| p.kind == PickerKind::Prompt)
            .and_then(|_| shell.prompt.as_ref().map(|a| a.title().to_string())),
        prompt_error: shell.lc.prompt_error.clone(),
        servers: shell
            .ws
            .servers
            .iter()
            .map(|s| {
                let info = shell
                    .host
                    .as_ref()
                    .and_then(|h| h.engine().list().into_iter().find(|i| i.id == s.id));
                let (status, address, error) = match info.map(|i| i.status) {
                    Some(fauxfield_engine::ServerStatus::Running { addr }) => {
                        ("running", Some(addr), None)
                    }
                    Some(fauxfield_engine::ServerStatus::Failed { error, .. }) => {
                        ("failed", None, Some(error))
                    }
                    Some(fauxfield_engine::ServerStatus::Stopped) => ("stopped", None, None),
                    None => (if s.running { "running" } else { "stopped" }, None, None),
                };
                ServerState {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    host: s.host.clone(),
                    port: s.port,
                    prefix: s.prefix.clone(),
                    enabled: s.enabled,
                    unmatched_hint: s.unmatched_hint,
                    status,
                    address,
                    error,
                }
            })
            .collect(),
        active_environment: shell.active_env.clone(),
        window: WindowState {
            width: f32::from(size.width),
            height: f32::from(size.height),
            scale: window.scale_factor(),
        },
        theme: cx.global::<Theme>().name.clone(),
    }
}

/// Assigns `generation`: it increases only when the snapshot differs from the previous one.
#[derive(Default)]
pub struct Tracker {
    last: Option<UiState>,
    generation: u64,
}

impl Tracker {
    pub fn observe(&mut self, mut state: UiState) -> UiState {
        state.generation = self.generation;
        if self.last.as_ref() != Some(&state) {
            self.generation += 1;
            state.generation = self.generation;
            self.last = Some(state.clone());
        }
        state
    }
}
