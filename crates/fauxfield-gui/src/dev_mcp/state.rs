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
    pub logs: usize,
    pub servers: Vec<ServerState>,
    pub active_environment: String,
    pub window: WindowState,
    pub theme: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WorkspaceState {
    pub name: String,
    pub path: Option<String>,
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
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ServerState {
    pub id: String,
    pub name: String,
    pub port: u16,
    pub status: &'static str,
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

/// Builds the snapshot with `generation` 0; [`Tracker::observe`] assigns the real one.
pub fn snapshot(shell: &Shell, window: &Window, cx: &App, workspace_path: Option<&str>) -> UiState {
    let rows = tree::flatten(&shell.ws, &shell.collapsed, &shell.active_env);
    let size = window.viewport_size();
    UiState {
        generation: 0,
        workspace: WorkspaceState {
            name: shell.ws.name.clone(),
            path: workspace_path.map(str::to_string),
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
                dirty: false,
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
        modal: shell.menu.as_ref().map(|_| "row_menu".to_string()),
        toasts: shell.notice.iter().map(|(_, m)| m.clone()).collect(),
        traffic: TrafficState {
            row_count: shell.traffic.len(),
            filters: Vec::new(),
            selected: shell.traffic_selected,
        },
        problems: shell.problems.len(),
        logs: shell.logs.len(),
        servers: shell
            .ws
            .servers
            .iter()
            .map(|s| ServerState {
                id: s.id.clone(),
                name: s.name.clone(),
                port: s.port,
                status: if s.running { "running" } else { "stopped" },
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
