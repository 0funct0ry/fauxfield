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
    pub sidebar: SidebarState,
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
    /// The application menu exactly as built (native menus cannot be seen or screenshotted).
    pub menus: MenusState,
    /// Every CodeEditor on screen (SPEC §20.18).
    pub editors: Vec<EditorState>,
    /// The open Route's responses, rules and mode (M5); `None` without a Route tab.
    pub route: Option<RouteState>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RouteState {
    pub id: String,
    pub response_mode: String,
    pub sub_tab: usize,
    pub selected_response: usize,
    pub sample_dialog: bool,
    /// Route headers in file order (M6).
    pub headers: Vec<(String, String)>,
    pub responses: Vec<ResponseState>,
    /// Preview of the selected Response (M6); absent until it has rendered.
    pub preview: Option<PreviewState>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PreviewState {
    pub response_id: String,
    pub status: u16,
    pub render_ms: f64,
    pub error: Option<String>,
    pub json_error: Option<String>,
    /// No render error and no JSON error.
    pub ok: bool,
    pub nondeterministic: bool,
    /// Length of the rendered body (the text is in the `<id>#preview` editor).
    pub body_len: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResponseState {
    pub id: String,
    pub name: String,
    pub description: String,
    pub status: u16,
    pub default: bool,
    pub disabled: bool,
    pub rules_logic: String,
    pub rules: usize,
    /// Rules that fail validation (shown inline in the Rules sub-tab).
    pub invalid_rules: usize,
    /// Rendered with Tera (M6).
    pub template: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EditorState {
    pub id: String,
    pub language: &'static str,
    pub revision: u64,
    pub text_len: usize,
    /// Zero-based caret line and column, plus the byte offset.
    pub cursor: CursorState,
    pub selection: [usize; 2],
    pub line_count: usize,
    pub soft_wrap: bool,
    pub read_only: bool,
    pub scroll_row: f32,
    pub undo_depth: usize,
    /// `Some(true/false)` when the language is validated (JSON), else absent.
    pub valid: Option<bool>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CursorState {
    pub line: usize,
    pub col: usize,
    pub offset: usize,
}

fn editor_state(
    id: String,
    e: &gpui::Entity<crate::code_editor::CodeEditor>,
    cx: &App,
) -> EditorState {
    let e = e.read(cx);
    let (line, col) = e.line_col();
    let sel = e.selection();
    let verdict = e.validity();
    EditorState {
        id,
        language: e.lang().name(),
        revision: e.revision(),
        text_len: e.text().len(),
        cursor: CursorState {
            line,
            col,
            offset: e.cursor(),
        },
        selection: [sel.start, sel.end],
        line_count: e.line_count(),
        soft_wrap: e.is_soft_wrap(),
        read_only: e.is_read_only(),
        scroll_row: e.scroll_row(),
        undo_depth: e.undo_depth(),
        valid: verdict.as_ref().map(Result::is_ok),
        error: verdict.and_then(Result::err).map(|e| e.to_string()),
    }
}

fn editors_state(shell: &Shell, cx: &App) -> Vec<EditorState> {
    let mut out = Vec::new();
    if let Some(rid) = shell.current_response_id() {
        if let Some(e) = shell.body_editors.get(&rid) {
            out.push(editor_state(format!("body:{rid}"), e, cx));
        }
        if let Some(e) = shell.body_editors.get(&format!("{rid}#file")) {
            out.push(editor_state(format!("file:{rid}"), e, cx));
        }
    }
    if let Some(eds) = &shell.traffic_editors
        && shell.traffic_selected.is_some()
    {
        out.push(editor_state("traffic:request".into(), &eds[0], cx));
        out.push(editor_state("traffic:response".into(), &eds[1], cx));
    }
    out
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MenusState {
    pub style: &'static str,
    /// Label path of the open in-window menu level, e.g. `["File", "Open Recent"]`.
    pub open: Option<Vec<String>>,
    pub menus: Vec<MenuState>,
    pub last_invoked: Option<InvokedState>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InvokedState {
    pub seq: u64,
    pub path: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MenuState {
    pub title: String,
    pub items: Vec<MenuItemState>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MenuItemState {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    pub binding: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub checked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<MenuItemState>>,
}

fn menu_items(
    entries: &[crate::menu::MenuEntry],
    ctx: &crate::menu::MenuContext,
) -> Vec<MenuItemState> {
    use crate::menu::{MenuEntry, entry_enabled};
    let mut recent = 0;
    entries
        .iter()
        .filter_map(|e| {
            let enabled = entry_enabled(e, ctx);
            Some(match e {
                MenuEntry::Separator | MenuEntry::Services => return None,
                MenuEntry::Action { id, label, checked } => MenuItemState {
                    id: id.to_string(),
                    label: label.clone(),
                    enabled,
                    checked: *checked,
                    binding: ctx.binding(id).map(str::to_string),
                    items: None,
                },
                MenuEntry::Recent { label, .. } => {
                    recent += 1;
                    MenuItemState {
                        id: format!("recent-{}", recent - 1),
                        label: label.clone(),
                        enabled,
                        binding: None,
                        checked: false,
                        items: None,
                    }
                }
                MenuEntry::Disabled { label } => MenuItemState {
                    id: "none".into(),
                    label: label.clone(),
                    enabled: false,
                    binding: None,
                    checked: false,
                    items: None,
                },
                MenuEntry::Submenu {
                    key,
                    label,
                    entries,
                    ..
                } => MenuItemState {
                    id: key.to_string(),
                    label: label.clone(),
                    enabled,
                    binding: None,
                    checked: false,
                    items: Some(menu_items(entries, ctx)),
                },
            })
        })
        .collect()
}

pub(crate) fn menus_state(shell: &Shell) -> MenusState {
    use crate::menu::MenuStyle;
    let in_window = shell.menu_style == MenuStyle::InWindow;
    let (menus, open) = match &shell.menu_ctx {
        Some(ctx) => {
            let defs = crate::menu::build_menus(ctx);
            let defs: Vec<_> = if in_window {
                defs.into_iter()
                    .filter(|m| m.title != "Fauxfield")
                    .collect()
            } else {
                defs
            };
            let open = shell.bar.open.map(|i| {
                let d = if shell.bar_collapsed() {
                    None
                } else {
                    defs.get(i)
                };
                let mut p = vec![
                    d.map(|d| d.title.to_string())
                        .unwrap_or_else(|| "Menu".into()),
                ];
                if let (Some(s), Some(d)) = (shell.bar.sub, d)
                    && let Some(crate::menu::MenuEntry::Submenu { label, .. }) = d.entries.get(s)
                {
                    p.push(label.clone());
                }
                p
            });
            (
                defs.iter()
                    .map(|d| MenuState {
                        title: d.title.to_string(),
                        items: menu_items(&d.entries, ctx),
                    })
                    .collect(),
                open,
            )
        }
        None => (Vec::new(), None),
    };
    MenusState {
        style: if in_window { "in_window" } else { "native" },
        open,
        menus,
        last_invoked: shell.menu_invoked.as_ref().map(|(seq, path)| InvokedState {
            seq: *seq,
            path: path.clone(),
        }),
    }
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
    /// Row ids currently drawn (the active sidebar tab).
    pub visible_rows: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SidebarTabState {
    pub id: &'static str,
    pub label: &'static str,
    pub count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SidebarState {
    pub tab: &'static str,
    pub tabs: Vec<SidebarTabState>,
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
    /// The Response that answered, and why (M5 rule trace).
    pub response_id: String,
    pub selection_reason: String,
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
    /// Seed for fake data; absent means random output (M6).
    pub fake_seed: Option<u64>,
    /// The dice icon is shown: no seed and a template uses a random function (M6).
    pub random_output: bool,
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
        TabKind::Folder => shell.ws.find_folder(id).map(|(s, _)| s.file.clone()),
    };
    file.is_some_and(|f| store.is_dirty(&f))
}

/// Builds the snapshot with `generation` 0; [`Tracker::observe`] assigns the real one.
pub fn snapshot(shell: &Shell, window: &Window, cx: &App, workspace_path: Option<&str>) -> UiState {
    let visible = tree::flatten(
        &shell.ws,
        &shell.collapsed,
        &shell.active_env,
        shell.sidebar_tab,
    );
    // `expanded` describes the whole workspace, not just the visible tab.
    let rows: Vec<_> = tree::SIDEBAR_TABS
        .into_iter()
        .flat_map(|t| tree::flatten(&shell.ws, &shell.collapsed, &shell.active_env, t))
        .collect();
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
            visible_rows: visible.iter().map(|r| r.id.clone()).collect(),
        },
        sidebar: SidebarState {
            tab: shell.sidebar_tab.id(),
            tabs: tree::SIDEBAR_TABS
                .into_iter()
                .map(|t| SidebarTabState {
                    id: t.id(),
                    label: t.label(),
                    count: shell.sidebar_count(t),
                })
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
                    response_id: r
                        .rec
                        .as_ref()
                        .map(|x| x.response_id.clone())
                        .unwrap_or_default(),
                    selection_reason: r
                        .rec
                        .as_ref()
                        .and_then(|x| x.trace.as_ref())
                        .map(|t| t.reason.clone())
                        .unwrap_or_default(),
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
                    fake_seed: s.fake_seed,
                    random_output: s.random_output(),
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
        menus: menus_state(shell),
        editors: editors_state(shell, cx),
        route: route_state(shell),
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

fn route_state(shell: &Shell) -> Option<RouteState> {
    let id = shell.current_route_id()?;
    let (_, r) = shell.ws.find_route(&id)?;
    Some(RouteState {
        id: r.id.clone(),
        response_mode: r.response_mode.clone(),
        sub_tab: shell.route_sub.get(&r.id).copied().unwrap_or(0),
        selected_response: shell.resp_sel.get(&r.id).copied().unwrap_or(0),
        sample_dialog: shell.sample_dialog.is_some(),
        headers: r.headers.clone(),
        preview: shell.preview.as_ref().map(|(id, p)| PreviewState {
            response_id: id.clone(),
            status: p.status,
            render_ms: p.render_ms,
            error: p.error.clone(),
            json_error: p.json_error.clone(),
            ok: p.error.is_none() && p.json_error.is_none(),
            nondeterministic: p.nondeterministic,
            body_len: p.body.len(),
        }),
        responses: r
            .responses
            .iter()
            .map(|x| ResponseState {
                id: x.id.clone(),
                name: x.name.clone(),
                description: x.description.clone(),
                status: x.status,
                default: x.default,
                disabled: x.disabled,
                rules_logic: x.rules_logic.clone(),
                rules: x.rules.len(),
                template: x.template,
                invalid_rules: x
                    .rules
                    .iter()
                    .filter(|r| fauxfield_model::rules::check_rule(r).is_err())
                    .count(),
            })
            .collect(),
    })
}
