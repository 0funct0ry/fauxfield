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
    /// Attached agents, the Agent activity log and the setup dialog (M16).
    pub agents: AgentsState,
    /// The Settings tab and keymap editor, when the tab state exists (M17.02).
    pub settings: Option<SettingsUiState>,
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
    /// Workspace network profile slug (M9): `none`, `slow-3g`…
    pub network_profile: &'static str,
    pub network_picker: PickerState,
    pub window: WindowState,
    pub theme: String,
    /// The application menu exactly as built (native menus cannot be seen or screenshotted).
    pub menus: MenusState,
    /// Every CodeEditor on screen (SPEC §20.18).
    pub editors: Vec<EditorState>,
    /// The open Route's responses, rules and mode (M5); `None` without a Route tab.
    pub route: Option<RouteState>,
    /// Every Dataset with its live row count (M8).
    pub datasets: Vec<DatasetState>,
    /// The open Dataset tab (M8); `None` without one.
    pub dataset: Option<DatasetViewState>,
    /// The schema designer modal (M10.07), when open.
    pub schema_dialog: Option<SchemaDialogState>,
    /// The Create CRUD Routes modal (M10.07), when open.
    pub crud_dialog: Option<CrudDialogState>,
    /// The Export modal (M10.07), when open.
    pub export_dialog: Option<ExportDialogState>,
    /// The Seed data modal (M10.07), when open.
    pub seed_dialog: Option<SeedDialogState>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SchemaColumnState {
    pub name: String,
    pub ty: String,
    pub primary_key: bool,
    pub unique: bool,
    pub required: bool,
    pub default: String,
    pub generator: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SchemaDialogState {
    /// `server:name` when editing a schema, `None` for a new Dataset.
    pub editing: Option<String>,
    pub name: String,
    pub server: String,
    pub description: String,
    pub source: &'static str,
    pub rows: String,
    pub create_crud: bool,
    pub columns: Vec<SchemaColumnState>,
    /// Live validation messages (`code: message`); Create is disabled while any exists.
    pub issues: Vec<String>,
    pub create_enabled: bool,
    pub preview: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CrudDialogState {
    pub server: String,
    pub dataset: String,
    pub folder: String,
    pub path: String,
    pub ops: Vec<String>,
    pub preview: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExportDialogState {
    pub format: &'static str,
    pub scope: &'static str,
    pub columns: Vec<(String, bool)>,
    pub rows: usize,
    pub estimate_bytes: usize,
    pub clipboard: bool,
    pub path: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SeedDialogState {
    pub rows: String,
    pub fake_seed: String,
    pub replace: bool,
    pub keep: bool,
    /// Column, type, generator.
    pub columns: Vec<(String, String, String)>,
    /// Per column: why its generator options are not usable, if they are not.
    pub errors: Vec<Option<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HamburgerItemState {
    pub action: &'static str,
    pub label: String,
    pub disabled: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DatasetState {
    pub key: String,
    pub name: String,
    pub server: String,
    pub rows: usize,
    pub key_field: String,
    pub id_strategy: String,
    pub persist: bool,
    pub error: Option<String>,
    /// No crud Route uses it (M10.01).
    pub unused: bool,
    /// Route ids of the crud Routes serving it.
    pub used_by: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DatasetViewState {
    pub key: String,
    /// `table`, `json` or `seed`.
    pub view: &'static str,
    pub rows: usize,
    pub columns: Vec<String>,
    /// Declared columns `(name, type)` (M10.04).
    pub declared_columns: Vec<(String, String)>,
    /// Active seed source: `static`, `generator`, `template` or `none` (M10.04).
    pub source: String,
    /// The seed editor has unsaved edits (M10.04).
    pub dirty_seed: bool,
    /// Preview of a file being imported, while its options prompt is open (M10.05).
    pub import_preview: Option<String>,
    /// The cell being edited, `(row, column)`.
    pub editing: Option<(usize, usize)>,
    /// Why the open cell edit was refused, if it was (M10.07).
    pub edit_error: Option<String>,
    /// Selected live row indices (M10.07).
    pub selection: Vec<usize>,
    /// The single status chip row, `N rows · M columns · source` plus the used-by chip.
    pub chip: String,
    pub used_by_chip: String,
    /// Seed problem banner showing (not dismissed).
    pub banner: bool,
    /// The details popover is open.
    pub popover: bool,
    /// The hamburger entries in order, with disabled reasons.
    pub hamburger: Vec<HamburgerItemState>,
    /// JSON view: pretty (else compact), unapplied edits, and why they cannot be applied.
    pub json_pretty: bool,
    pub json_dirty: bool,
    pub json_problem: Option<String>,
    /// Cell edits that can be undone and redone.
    pub undo_depth: usize,
    pub redo_depth: usize,
    /// The DataTable view model (M10.03): order, widths, sort, selection, visible range.
    pub table: Option<crate::data_table::TableViewModel>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RouteState {
    pub id: String,
    pub response_mode: String,
    /// WebSocket Route state (M13); absent for other kinds.
    pub ws: Option<WsState>,
    /// `proxy = true` (Forward to upstream) and `pinned = true` (Pin mock), M11.
    pub forward: bool,
    pub pinned: bool,
    pub sub_tab: usize,
    pub selected_response: usize,
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
    /// Dataset action steps (M10.06) and those that fail validation.
    pub actions: usize,
    pub invalid_actions: usize,
    /// Callbacks (M14) and those switched off.
    pub callbacks: usize,
    pub disabled_callbacks: usize,
    /// The Dataset body source form is open.
    pub dataset_source: bool,
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
pub struct SettingRowState {
    pub key: String,
    pub label: String,
    pub value: String,
    pub modified: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct KeymapRowState {
    pub action: String,
    pub keys: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RecordState {
    pub action: String,
    pub keys: String,
    pub conflicts: Vec<String>,
    pub resolution: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SettingsUiState {
    pub category: String,
    pub search: String,
    pub vim_list_navigation: bool,
    pub rows: Vec<SettingRowState>,
    pub keymap: Vec<KeymapRowState>,
    pub keymap_search: String,
    pub record: Option<RecordState>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AgentsState {
    pub attached: Vec<AgentAttachedState>,
    pub activity: Vec<AgentActivityState>,
    /// Logs source filter: `all`, `engine` or `agent`.
    pub log_source: &'static str,
    /// Row ids currently flashing because an agent changed them.
    pub flashing: Vec<String>,
    pub dialog: Option<AgentsDialogState>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AgentAttachedState {
    pub name: String,
    pub since: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AgentActivityState {
    pub ts: u64,
    pub client: String,
    pub tool: String,
    pub target: String,
    pub outcome: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AgentsDialogState {
    pub clients: Vec<(String, bool)>,
    pub allow: String,
    pub files: Vec<(String, String)>,
    pub selected: usize,
    pub error: Option<String>,
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
    /// Route row id to `proxied` or `pinned` for the rows that show the M11 icon.
    pub route_sources: std::collections::BTreeMap<String, &'static str>,
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

/// The WebSocket tab (M13).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WsState {
    pub mode: String,
    pub messages: usize,
    pub triggers: Vec<String>,
    pub connections: Vec<String>,
    pub selected_connection: Option<String>,
    pub frames: usize,
    pub frames_in: usize,
    pub frames_out: usize,
    pub selected_message: Option<usize>,
    pub dir_filter: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TrafficState {
    pub row_count: usize,
    /// Rows answered by the proxy and rows that are proxied 502/504 (M11).
    pub proxied_count: usize,
    pub upstream_errors: usize,
    /// Outgoing Callback entries and those that failed (M14).
    pub callback_count: usize,
    pub callback_errors: usize,
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
    /// Answered by the proxy upstream, and which upstream (M11).
    pub proxied: bool,
    pub upstream: Option<String>,
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
    /// `[network]` latency, jitter and bandwidth (M9).
    pub latency_ms: u64,
    pub jitter_ms: u64,
    pub bandwidth_kbps: u64,
    /// `[rate_limit]` as written; absent when the table is.
    pub rate_limit: Option<RateLimitState>,
    /// Chaos fault rate while the switch is on (runtime only).
    pub chaos: Option<f64>,
    /// `[tls]` (M10): enabled, mode and the extra plain-HTTP port.
    pub https: bool,
    pub tls_mode: &'static str,
    pub also_http_port: Option<u16>,
    /// Days until the running certificate expires.
    pub cert_days_left: Option<i64>,
    /// `[cors]` as written; absent when the table is.
    pub cors: Option<CorsState>,
    /// `[proxy]` as written; absent when the table is (M11).
    pub proxy: Option<ProxyState>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProxyState {
    pub enabled: bool,
    pub mode: String,
    pub upstream: String,
    /// The upstream with templates rendered, or the template error.
    pub upstream_preview: String,
    pub preview_error: bool,
    pub preserve_host: bool,
    pub timeout_ms: u64,
    pub insecure: bool,
    pub request_headers: Vec<(String, String)>,
    pub response_headers: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CorsState {
    pub enabled: bool,
    pub origins: Vec<String>,
    pub methods: Vec<String>,
    pub headers: Vec<String>,
    pub credentials: bool,
    pub max_age: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RateLimitState {
    pub enabled: bool,
    pub requests: u64,
    pub window: String,
    pub key: String,
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
        TabKind::Environment | TabKind::Dataset | TabKind::Settings => return false,
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
            route_sources: visible
                .iter()
                .filter_map(|r| match &r.kind {
                    tree::RowKind::Route {
                        source: Some(s), ..
                    } => Some((
                        r.id.clone(),
                        match s {
                            tree::RouteSource::Proxied => "proxied",
                            tree::RouteSource::Pinned => "pinned",
                        },
                    )),
                    _ => None,
                })
                .collect(),
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
        } else if shell.any_combo_open() {
            Some("combo_menu".to_string())
        } else if shell.any_dropdown_open() {
            Some("dropdown_menu".to_string())
        } else if let Some(d) = shell.dialog_name() {
            Some(d.to_string())
        } else {
            shell.menu.as_ref().map(|_| "row_menu".to_string())
        },
        toasts: shell
            .toasts_older
            .iter()
            .chain(shell.notice.iter())
            .map(|(_, m)| m.clone())
            .collect(),
        toast_action: shell.notice_action.as_ref().map(|a| a.label.to_string()),
        traffic: TrafficState {
            row_count: shell.traffic.len(),
            proxied_count: shell.traffic.iter().filter(|r| r.proxied).count(),
            upstream_errors: shell.traffic.iter().filter(|r| r.upstream_error()).count(),
            callback_count: shell.traffic.iter().filter(|r| r.is_callback()).count(),
            callback_errors: shell
                .traffic
                .iter()
                .filter(|r| r.is_callback() && r.status == 0 || r.is_callback() && r.status >= 400)
                .count(),
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
                    proxied: r.proxied,
                    upstream: r.rec.as_ref().and_then(|x| x.upstream.clone()),
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
        settings: shell.settings_ui_state(cx),
        agents: AgentsState {
            attached: shell
                .agents_attached
                .iter()
                .map(|s| AgentAttachedState {
                    name: s.name.clone(),
                    since: s.since_ms,
                })
                .collect(),
            activity: shell
                .agent_log
                .iter()
                .map(|e| AgentActivityState {
                    ts: e.ts_ms,
                    client: e.client.clone(),
                    tool: e.tool.clone(),
                    target: e.target.clone(),
                    outcome: e.outcome.clone(),
                })
                .collect(),
            log_source: shell.log_source.value(),
            flashing: shell.agent_flash.iter().map(|(id, _)| id.clone()).collect(),
            dialog: shell.agents_dialog.as_ref().map(|d| AgentsDialogState {
                clients: d
                    .clients
                    .iter()
                    .map(|(c, on)| (c.id().to_string(), *on))
                    .collect(),
                allow: d.allow.clone(),
                files: d
                    .plan
                    .as_ref()
                    .map(|p| {
                        p.files
                            .iter()
                            .map(|f| {
                                (
                                    f.path.clone(),
                                    serde_json::to_value(f.action)
                                        .ok()
                                        .and_then(|v| v.as_str().map(str::to_owned))
                                        .unwrap_or_default(),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                selected: d.selected,
                error: d.plan.as_ref().err().filter(|e| !e.is_empty()).cloned(),
            }),
        },
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
                    latency_ms: s.network.latency_ms,
                    jitter_ms: s.network.jitter_ms,
                    bandwidth_kbps: s.network.bandwidth_kbps,
                    rate_limit: s.rate_limit.as_ref().map(|r| RateLimitState {
                        enabled: r.enabled,
                        requests: r.requests,
                        window: r.window.clone(),
                        key: r.key.clone(),
                    }),
                    chaos: s.chaos,
                    https: s.https(),
                    tls_mode: if s.tls.as_ref().is_some_and(|t| t.mode_name() == "files") {
                        "files"
                    } else {
                        "dev-ca"
                    },
                    also_http_port: s.tls.as_ref().and_then(|t| t.also_http_port),
                    cert_days_left: s.cert.as_ref().map(|c| c.days_left),
                    cors: s.cors.as_ref().map(|c| CorsState {
                        enabled: c.enabled,
                        origins: c.origins.clone(),
                        methods: c.methods.clone(),
                        headers: c.headers.clone(),
                        credentials: c.credentials,
                        max_age: c.max_age,
                    }),
                    proxy: s.proxy.as_ref().map(|p| {
                        let (preview, err) = match shell.upstream_preview(s) {
                            Ok(v) => (v, false),
                            Err(e) => (e, true),
                        };
                        ProxyState {
                            enabled: p.enabled,
                            mode: p.mode_name().to_string(),
                            upstream: p.upstream.clone().unwrap_or_default(),
                            upstream_preview: preview,
                            preview_error: err,
                            preserve_host: p.preserve_host,
                            timeout_ms: p.timeout(),
                            insecure: p.insecure,
                            request_headers: p
                                .request_headers
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect(),
                            response_headers: p
                                .response_headers
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect(),
                        }
                    }),
                }
            })
            .collect(),
        active_environment: shell.active_env.clone(),
        network_profile: shell.network_profile.slug(),
        network_picker: picker(shell, PickerKind::Network),
        window: WindowState {
            width: f32::from(size.width),
            height: f32::from(size.height),
            scale: window.scale_factor(),
        },
        theme: cx.global::<Theme>().name.clone(),
        menus: menus_state(shell),
        editors: editors_state(shell, cx),
        route: route_state(shell),
        datasets: shell
            .ws
            .datasets
            .iter()
            .map(|d| DatasetState {
                key: d.key(),
                name: d.name.clone(),
                server: d.server.clone(),
                rows: d.rows,
                key_field: d.key.clone(),
                id_strategy: d.id_strategy.clone(),
                persist: d.persist,
                error: d.error.clone(),
                unused: d.unused(),
                used_by: d.used_by.iter().map(|(id, _)| id.clone()).collect(),
            })
            .collect(),
        dataset: shell
            .tabs
            .active()
            .filter(|t| t.kind == TabKind::Dataset)
            .and_then(|t| shell.ws.find_dataset(&t.id))
            .map(|d| {
                let head = shell
                    .engine()
                    .and_then(|e| e.dataset_page(&d.server_id, &d.name, 0, 100).ok())
                    .map(|(_, r)| r)
                    .unwrap_or_default();
                DatasetViewState {
                    key: d.key(),
                    view: if shell.ds_seed_mode {
                        "seed"
                    } else if shell.ds_json_mode {
                        "json"
                    } else {
                        "table"
                    },
                    rows: d.rows,
                    columns: crate::dataset_ui::columns(&head, &d.key),
                    declared_columns: d.columns.clone(),
                    source: d.source.clone(),
                    dirty_seed: shell.seed_dirty(&d.key(), cx),
                    import_preview: shell.import_preview.clone(),
                    editing: shell.ds_tables.borrow().get(&d.key()).and_then(|e| {
                        e.read(cx)
                            .delegate()
                            .editing
                            .as_ref()
                            .map(|x| (x.row, x.col))
                    }),
                    table: shell.dataset_table_view(&d.key(), cx),
                    edit_error: shell
                        .ds_tables
                        .borrow()
                        .get(&d.key())
                        .and_then(|e| e.read(cx).delegate().shared.edit_error.borrow().clone()),
                    selection: shell
                        .ds_tables
                        .borrow()
                        .get(&d.key())
                        .map(|e| e.read(cx).delegate().selection.indices())
                        .unwrap_or_default(),
                    chip: crate::dataset_chrome::chip_text(d, d.rows),
                    used_by_chip: crate::dataset_chrome::used_by_text(d),
                    banner: d.error.is_some() && !shell.ds_banner_dismissed.contains(&d.key()),
                    popover: shell.ds_popover,
                    hamburger: crate::dataset_chrome::hamburger_items(
                        d,
                        d.rows,
                        shell.host.is_some(),
                        shell.ds_json_mode,
                        shell.ds_seed_mode,
                    )
                    .into_iter()
                    .map(|i| HamburgerItemState {
                        action: i.action,
                        label: i.label,
                        disabled: i.disabled,
                    })
                    .collect(),
                    json_pretty: shell
                        .json_eds
                        .borrow()
                        .get(&d.key())
                        .is_none_or(|e| e.pretty),
                    json_dirty: shell
                        .json_eds
                        .borrow()
                        .get(&d.key())
                        .is_some_and(|e| e.dirty(cx)),
                    json_problem: shell
                        .json_eds
                        .borrow()
                        .get(&d.key())
                        .and_then(|e| e.problem.clone()),
                    undo_depth: shell
                        .ds_tables
                        .borrow()
                        .get(&d.key())
                        .map_or(0, |e| e.read(cx).delegate().shared.undo.borrow().len()),
                    redo_depth: shell
                        .ds_tables
                        .borrow()
                        .get(&d.key())
                        .map_or(0, |e| e.read(cx).delegate().shared.redo.borrow().len()),
                }
            }),
        schema_dialog: shell.schema_dialog_state(cx),
        crud_dialog: shell.crud_dialog_state(cx),
        export_dialog: shell.export_dialog_state(cx),
        seed_dialog: shell.seed_dialog_state(cx),
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
        forward: r.forward,
        pinned: r.pinned,
        ws: r.is_websocket().then(|| {
            let mine = || shell.ws_frames.iter().filter(|f| f.route_id == r.id);
            WsState {
                mode: r.ws_mode.clone(),
                messages: r.ws_messages.len(),
                triggers: r.ws_messages.iter().map(|m| m.trigger.clone()).collect(),
                connections: shell
                    .ws_conns
                    .iter()
                    .filter(|c| c.route_id == r.id)
                    .map(|c| c.id.clone())
                    .collect(),
                selected_connection: shell.ws_sel.clone(),
                frames: mine().count(),
                frames_in: mine().filter(|f| f.dir == "in").count(),
                frames_out: mine().filter(|f| f.dir == "out").count(),
                selected_message: shell.ws_msg_sel.get(&r.id).copied(),
                dir_filter: ["all", "in", "out"][usize::from(shell.ws_dir).min(2)].to_string(),
            }
        }),
        sub_tab: shell.route_sub.get(&r.id).copied().unwrap_or(0),
        selected_response: shell.resp_sel.get(&r.id).copied().unwrap_or(0),
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
                actions: x.actions.len(),
                invalid_actions: x
                    .actions
                    .iter()
                    .filter(|a| fauxfield_model::validate_action(a).is_some())
                    .count(),
                callbacks: x.callbacks.len(),
                disabled_callbacks: x.callbacks.iter().filter(|c| c.disabled).count(),
                dataset_source: shell.ds_read.contains_key(&x.id),
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
