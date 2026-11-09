//! Flattens the workspace into the rows of the left dock tree. Pure; row ids are semantic
//! (server id, folder id, route id, `env:<name>`), never loop indices.

use std::collections::HashSet;

use crate::view::Workspace;

/// Row id of the workspace header (and its hamburger menu).
pub const WORKSPACE_ROW: &str = "ws:workspace";

/// A tab of the left dock's sidebar (never an editor tab).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SidebarTab {
    #[default]
    Servers,
    Environments,
    Datasets,
}

/// Every sidebar tab in strip order; a later milestone adds a tab by adding one variant.
pub const SIDEBAR_TABS: [SidebarTab; 3] = [
    SidebarTab::Servers,
    SidebarTab::Environments,
    SidebarTab::Datasets,
];

impl SidebarTab {
    pub fn id(self) -> &'static str {
        match self {
            SidebarTab::Servers => "servers",
            SidebarTab::Environments => "environments",
            SidebarTab::Datasets => "datasets",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            SidebarTab::Servers => "Servers",
            SidebarTab::Environments => "Environments",
            SidebarTab::Datasets => "Datasets",
        }
    }
    pub fn icon(self) -> &'static str {
        match self {
            SidebarTab::Servers => "server",
            SidebarTab::Environments => "layers",
            SidebarTab::Datasets => "database",
        }
    }
    pub fn action_id(self) -> &'static str {
        match self {
            SidebarTab::Servers => "show_sidebar_servers",
            SidebarTab::Environments => "show_sidebar_environments",
            SidebarTab::Datasets => "show_sidebar_datasets",
        }
    }
    pub fn empty_text(self) -> &'static str {
        match self {
            SidebarTab::Servers => "No servers yet.",
            SidebarTab::Environments => "No environments.",
            SidebarTab::Datasets => "No datasets. Create a CRUD resource to add one.",
        }
    }
    /// Parses a persisted id; anything unknown falls back to Servers.
    pub fn from_id(id: &str) -> SidebarTab {
        SIDEBAR_TABS
            .into_iter()
            .find(|t| t.id() == id)
            .unwrap_or_default()
    }
    /// The tab that owns a row id.
    pub fn of_row(id: &str) -> SidebarTab {
        if id.starts_with("env:") {
            SidebarTab::Environments
        } else if id.starts_with("ds:") {
            SidebarTab::Datasets
        } else {
            SidebarTab::Servers
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowKind {
    Server {
        port: u16,
        running: bool,
        enabled: bool,
        /// Chaos switch is on (runtime only).
        chaos: bool,
        /// Serves HTTPS (lock icon, M10).
        https: bool,
    },
    /// The workspace root header (rendered above the tree, not part of `flatten`).
    Workspace,
    Folder {
        enabled: bool,
        routes: usize,
        start_collapsed: bool,
    },
    Route {
        method: String,
        /// Shadow or pattern problem shown as a trailing warn icon (tooltip text).
        warn: Option<RouteWarn>,
        /// Route sits in a disabled Folder.
        disabled: bool,
    },
    Environment {
        active: bool,
    },
    Dataset {
        rows: usize,
    },
    /// One generated operation of a crud Route (child row of the Route node; opens the Route).
    CrudOp {
        method: String,
    },
    /// A Response in the Route tab's list (menu only; never a tree row).
    Response {
        default: bool,
        disabled: bool,
    },
    /// "Add error preset" picker; the row id is the Route.
    ErrorPresets,
    /// Rule target picker; the row id is `<response>#<rule index>`.
    RuleTarget,
    RuleOp,
    /// Response mode picker; the row id is the Route.
    ResponseMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteWarn {
    /// `true`: invalid pattern (error colour); `false`: shadowed (warning colour).
    pub error: bool,
    pub message: String,
}

fn route_warn(r: &crate::view::Route) -> Option<RouteWarn> {
    if let Some(e) = &r.pattern_error {
        return Some(RouteWarn {
            error: true,
            message: e.clone(),
        });
    }
    r.shadowed_by.as_ref().map(|by| RouteWarn {
        error: false,
        message: format!("Never reached: shadowed by {by}"),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    pub id: String,
    pub depth: u8,
    pub label: String,
    pub kind: RowKind,
    pub expandable: bool,
    pub expanded: bool,
}

pub fn env_row_id(name: &str) -> String {
    format!("env:{name}")
}

/// `key` is `Dataset::key()` (`<server id>:<name>`).
pub fn dataset_row_id(key: &str) -> String {
    format!("ds:{key}")
}

/// Child row of a crud Route node: `<route id>#<n>`; `crud_route_of` maps it back.
pub fn crud_op_row_id(route: &str, n: usize) -> String {
    format!("{route}#{n}")
}

pub fn crud_route_of(row_id: &str) -> Option<&str> {
    row_id.split_once('#').map(|(r, _)| r)
}

/// `(method, path suffix)` of the six operations a crud Route generates (SPEC §6.2).
pub const CRUD_OPS: [(&str, &str); 6] = [
    ("GET", ""),
    ("POST", ""),
    ("GET", "/:id"),
    ("PUT", "/:id"),
    ("PATCH", "/:id"),
    ("DELETE", "/:id"),
];

/// A Route row; a crud Route is an expandable node whose children are its six operations.
fn route_rows(
    rows: &mut Vec<TreeRow>,
    collapsed: &HashSet<String>,
    r: &crate::view::Route,
    depth: u8,
    disabled: bool,
) {
    let crud = r.is_crud();
    let open = crud && !collapsed.contains(&r.id);
    rows.push(TreeRow {
        id: r.id.clone(),
        depth,
        label: r.path.clone(),
        kind: RowKind::Route {
            method: r.method.clone(),
            warn: route_warn(r),
            disabled,
        },
        expandable: crud,
        expanded: open,
    });
    if open {
        for (i, (m, suffix)) in CRUD_OPS.iter().enumerate() {
            rows.push(TreeRow {
                id: crud_op_row_id(&r.id, i),
                depth: depth + 1,
                label: format!("{}{}", r.path.trim_end_matches('/'), suffix),
                kind: RowKind::CrudOp {
                    method: (*m).to_string(),
                },
                expandable: false,
                expanded: false,
            });
        }
    }
}

pub fn flatten(
    ws: &Workspace,
    collapsed: &HashSet<String>,
    active_env: &str,
    tab: SidebarTab,
) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    let node = |rows: &mut Vec<TreeRow>, id: &str, depth, label: &str, kind, expandable| {
        rows.push(TreeRow {
            id: id.to_string(),
            depth,
            label: label.to_string(),
            kind,
            expandable,
            expanded: expandable && !collapsed.contains(id),
        });
        expandable && !collapsed.contains(id)
    };
    if tab == SidebarTab::Servers {
        for s in &ws.servers {
            let open = node(
                &mut rows,
                &s.id,
                0,
                &s.name,
                RowKind::Server {
                    port: s.port,
                    running: s.running,
                    enabled: s.enabled,
                    chaos: s.chaos.is_some(),
                    https: s.https(),
                },
                true,
            );
            if !open {
                continue;
            }
            for f in &s.folders {
                let open = node(
                    &mut rows,
                    &f.id,
                    1,
                    &f.name,
                    RowKind::Folder {
                        enabled: f.enabled,
                        routes: f.routes.len(),
                        start_collapsed: f.collapsed,
                    },
                    true,
                );
                if open {
                    for r in &f.routes {
                        route_rows(&mut rows, collapsed, r, 2, !f.enabled);
                    }
                }
            }
            for r in &s.routes {
                route_rows(&mut rows, collapsed, r, 1, false);
            }
        }
    }
    match tab {
        SidebarTab::Servers => {}
        SidebarTab::Environments => {
            for e in &ws.environments {
                node(
                    &mut rows,
                    &env_row_id(&e.name),
                    0,
                    &e.name,
                    RowKind::Environment {
                        active: e.name == active_env,
                    },
                    false,
                );
            }
        }
        SidebarTab::Datasets => {
            for d in &ws.datasets {
                node(
                    &mut rows,
                    &dataset_row_id(&d.key()),
                    0,
                    &d.label(),
                    RowKind::Dataset { rows: d.rows },
                    false,
                );
            }
        }
    }
    rows
}

/// What a context-menu item does. Mutating items are stubs until M2/M8 (SPEC §9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAct {
    Open,
    OpenPinned,
    StartStop,
    SetActive,
    NewRoute,
    NewFolder,
    NewResponse,
    Rename,
    Duplicate,
    Delete,
    /// Folder: delete it together with its Routes (SPEC §20.17).
    DeleteWithRoutes,
    FolderToggleEnabled,
    FolderToggleCollapsed,
    MoveUp,
    MoveDown,
    MoveToRoot,
    SetDefaultResponse,
    /// Response: toggle `disabled`.
    ToggleResponseDisabled,
    /// Add the RFC 9457 error preset for this status to the Route.
    ErrorPreset(u16),
    /// Add the fault preset at this index of `fauxfield_store::FAULT_PRESETS` (M9).
    FaultPreset(usize),
    /// Server: switch the chaos fault injection on or off (runtime only).
    ToggleChaos,
    /// Index into `fauxfield_model::rules::TARGETS` / `OPS` / `RESPONSE_MODES`.
    SetRuleTarget(usize),
    SetRuleOp(usize),
    SetResponseMode(usize),
    ResetDataset,
    ExportDataset,
    // Server menu (M3.1).
    Restart,
    ToggleEnabled,
    ChangePort,
    ChangeHost,
    ChangePrefix,
    // Workspace menu (M3.1).
    NewServer,
    RenameWorkspace,
    OpenFolder,
    CreateWorkspace,
    CloseWorkspace,
    ClearRecent,
    OpenRecent(usize),
}

impl MenuAct {
    /// Stable dev-id key for a menu item.
    pub fn key(self) -> String {
        use MenuAct::*;
        match self {
            Open => "open".into(),
            OpenPinned => "open-pinned".into(),
            StartStop => "start-stop".into(),
            SetActive => "set-active".into(),
            NewRoute => "new-route".into(),
            NewFolder => "new-folder".into(),
            NewResponse => "new-response".into(),
            Rename => "rename".into(),
            Duplicate => "duplicate".into(),
            Delete => "delete".into(),
            DeleteWithRoutes => "delete-with-routes".into(),
            FolderToggleEnabled => "folder-toggle-enabled".into(),
            FolderToggleCollapsed => "folder-toggle-collapsed".into(),
            MoveUp => "move-up".into(),
            MoveDown => "move-down".into(),
            MoveToRoot => "move-to-root".into(),
            SetDefaultResponse => "set-default-response".into(),
            ToggleResponseDisabled => "toggle-response-disabled".into(),
            ErrorPreset(c) => format!("error-preset-{c}"),
            FaultPreset(i) => format!("fault-preset-{}", fauxfield_store::FAULT_PRESETS[i].id),
            ToggleChaos => "toggle-chaos".into(),
            SetRuleTarget(i) => format!("rule-target-{i}"),
            SetRuleOp(i) => format!("rule-op-{i}"),
            SetResponseMode(i) => format!("response-mode-{i}"),
            ResetDataset => "reset-dataset".into(),
            ExportDataset => "export-dataset".into(),
            Restart => "restart".into(),
            ToggleEnabled => "toggle-enabled".into(),
            ChangePort => "change-port".into(),
            ChangeHost => "change-host".into(),
            ChangePrefix => "change-prefix".into(),
            NewServer => "new-server".into(),
            RenameWorkspace => "rename-workspace".into(),
            OpenFolder => "open-folder".into(),
            CreateWorkspace => "create-workspace".into(),
            CloseWorkspace => "close-workspace".into(),
            ClearRecent => "clear-recent".into(),
            OpenRecent(i) => format!("recent-{i}"),
        }
    }
}

/// Items of the workspace hamburger menu: lifecycle, new server, then recents.
pub fn workspace_menu_items(recents: &[std::path::PathBuf]) -> Vec<(String, MenuAct)> {
    let mut v: Vec<(String, MenuAct)> = vec![
        ("New server…".into(), MenuAct::NewServer),
        ("Rename workspace…".into(), MenuAct::RenameWorkspace),
        ("Open folder…".into(), MenuAct::OpenFolder),
        ("Create workspace…".into(), MenuAct::CreateWorkspace),
        ("Close workspace".into(), MenuAct::CloseWorkspace),
    ];
    for (i, p) in recents.iter().take(10).enumerate() {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.display().to_string());
        v.push((format!("Recent: {name}"), MenuAct::OpenRecent(i)));
    }
    if !recents.is_empty() {
        v.push(("Clear recent".into(), MenuAct::ClearRecent));
    }
    v
}

/// Items of the "⋯" / right-click menu of a tree row, in display order.
/// "Add error preset" entries (SPEC §5.6), in status order.
pub const ERROR_PRESET_ITEMS: [(&str, MenuAct); 11] = [
    ("400 Bad Request", MenuAct::ErrorPreset(400)),
    ("401 Unauthorized", MenuAct::ErrorPreset(401)),
    ("403 Forbidden", MenuAct::ErrorPreset(403)),
    ("404 Not Found", MenuAct::ErrorPreset(404)),
    ("409 Conflict", MenuAct::ErrorPreset(409)),
    ("422 Unprocessable Entity", MenuAct::ErrorPreset(422)),
    ("429 Too Many Requests", MenuAct::ErrorPreset(429)),
    ("500 Internal Server Error", MenuAct::ErrorPreset(500)),
    ("502 Bad Gateway", MenuAct::ErrorPreset(502)),
    ("503 Service Unavailable", MenuAct::ErrorPreset(503)),
    ("504 Gateway Timeout", MenuAct::ErrorPreset(504)),
];

/// Error presets followed by the fault variants ("502 after 10 s", "Connection reset"…).
pub fn error_preset_items() -> Vec<(&'static str, MenuAct)> {
    let mut v = ERROR_PRESET_ITEMS.to_vec();
    v.extend(
        fauxfield_store::FAULT_PRESETS
            .iter()
            .enumerate()
            .map(|(i, p)| (p.label, MenuAct::FaultPreset(i))),
    );
    v
}

pub fn menu_items(kind: &RowKind, server_running: bool) -> Vec<(&'static str, MenuAct)> {
    use MenuAct::*;
    match kind {
        RowKind::Server { enabled, chaos, .. } => {
            let mut v = vec![(if server_running { "Stop" } else { "Start" }, StartStop)];
            if server_running {
                v.push(("Restart", Restart));
            }
            v.extend([
                ("New route…", NewRoute),
                ("New folder…", NewFolder),
                (if *enabled { "Disable" } else { "Enable" }, ToggleEnabled),
                ("Change Port…", ChangePort),
                ("Change Host…", ChangeHost),
                ("Change Prefix…", ChangePrefix),
                (if *chaos { "Chaos off" } else { "Chaos on" }, ToggleChaos),
                ("Rename…", Rename),
                ("Duplicate", Duplicate),
                ("Delete", Delete),
            ]);
            v
        }
        RowKind::Workspace => vec![],
        RowKind::Folder {
            enabled,
            start_collapsed,
            ..
        } => vec![
            ("New route…", NewRoute),
            ("Rename…", Rename),
            ("Duplicate", Duplicate),
            (
                if *enabled {
                    "Disable all routes"
                } else {
                    "Enable all routes"
                },
                FolderToggleEnabled,
            ),
            (
                if *start_collapsed {
                    "✓ Start collapsed"
                } else {
                    "Start collapsed"
                },
                FolderToggleCollapsed,
            ),
            ("Move up", MoveUp),
            ("Move down", MoveDown),
            ("Delete folder, keep routes", Delete),
            ("Delete folder and its routes", DeleteWithRoutes),
        ],
        RowKind::Route { .. } => vec![
            ("Open", Open),
            ("Open pinned", OpenPinned),
            ("New response", NewResponse),
            ("Rename…", Rename),
            ("Duplicate", Duplicate),
            ("Move up", MoveUp),
            ("Move down", MoveDown),
            ("Move to server root", MoveToRoot),
            ("Delete", Delete),
        ],
        RowKind::Environment { active } => {
            let mut v = vec![("Open", Open), ("Open pinned", OpenPinned)];
            if !active {
                v.push(("Set active", SetActive));
            }
            v.extend([
                ("Rename…", Rename),
                ("Duplicate", Duplicate),
                ("Delete", Delete),
            ]);
            v
        }
        RowKind::Dataset { .. } => vec![
            ("Open", Open),
            ("Open pinned", OpenPinned),
            ("Reset to seed", ResetDataset),
            ("Export JSON…", ExportDataset),
        ],
        RowKind::CrudOp { .. } => vec![("Open route", Open)],
        RowKind::Response { default, disabled } => {
            let mut v = Vec::new();
            if !default {
                v.push(("Set as default", SetDefaultResponse));
            }
            v.push((
                if *disabled { "Enable" } else { "Disable" },
                ToggleResponseDisabled,
            ));
            v.extend([
                ("Rename…", Rename),
                ("Duplicate", Duplicate),
                ("Move up", MoveUp),
                ("Move down", MoveDown),
                ("Delete", Delete),
            ]);
            v
        }
        RowKind::ErrorPresets => error_preset_items(),
        RowKind::RuleTarget => fauxfield_model::rules::TARGETS
            .iter()
            .enumerate()
            .map(|(i, t)| (*t, SetRuleTarget(i)))
            .collect(),
        RowKind::RuleOp => fauxfield_model::rules::OPS
            .iter()
            .enumerate()
            .map(|(i, t)| (*t, SetRuleOp(i)))
            .collect(),
        RowKind::ResponseMode => fauxfield_model::rules::RESPONSE_MODES
            .iter()
            .enumerate()
            .map(|(i, t)| (*t, SetResponseMode(i)))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::workspace;

    fn ids(rows: &[TreeRow]) -> Vec<&str> {
        rows.iter().map(|r| r.id.as_str()).collect()
    }

    #[test]
    fn expanded_tree_lists_everything_in_order() {
        let ws = workspace();
        let rows = flatten(&ws, &HashSet::new(), "local", SidebarTab::Servers);
        assert_eq!(rows[0].id, "srv_users");
        assert_eq!(rows[1].id, "fld_users");
        assert_eq!(rows[2].id, "rt_users_list");
        assert_eq!(rows[2].depth, 2);
        let pos = |id: &str| rows.iter().position(|r| r.id == id).unwrap();
        assert!(pos("rt_health") > pos("rt_users_delete"));
        assert!(pos("srv_payments") > pos("rt_health"));
        assert!(rows.iter().all(|r| {
            !matches!(
                r.kind,
                RowKind::Environment { .. } | RowKind::Dataset { .. }
            )
        }));
    }

    #[test]
    fn each_tab_lists_only_its_rows() {
        let ws = workspace();
        let c = HashSet::new();
        let env = flatten(&ws, &c, "local", SidebarTab::Environments);
        assert!(!env.is_empty() && env.iter().all(|r| r.id.starts_with("env:") && r.depth == 0));
        let ds = flatten(&ws, &c, "local", SidebarTab::Datasets);
        assert!(
            ds.iter()
                .any(|r| r.id == "ds:srv_users:users" && r.label.starts_with("users · "))
        );
        assert!(ds.iter().all(|r| r.id.starts_with("ds:")));
        // Collapsing a server does not affect other tabs.
        let c: HashSet<String> = ["srv_users".to_string()].into();
        assert_eq!(flatten(&ws, &c, "local", SidebarTab::Environments), env);
    }

    #[test]
    fn sidebar_tab_ids_are_unique_and_parse() {
        let ids: HashSet<_> = SIDEBAR_TABS.iter().map(|t| t.id()).collect();
        assert_eq!(ids.len(), SIDEBAR_TABS.len());
        for t in SIDEBAR_TABS {
            assert_eq!(SidebarTab::from_id(t.id()), t);
        }
        assert_eq!(SidebarTab::from_id("history"), SidebarTab::Servers);
        assert_eq!(SidebarTab::from_id(""), SidebarTab::Servers);
        assert_eq!(SidebarTab::of_row("env:x"), SidebarTab::Environments);
        assert_eq!(SidebarTab::of_row("ds:x"), SidebarTab::Datasets);
        assert_eq!(SidebarTab::of_row("rt_x"), SidebarTab::Servers);
    }

    #[test]
    fn collapsing_hides_descendants() {
        let ws = workspace();
        let c: HashSet<String> = ["fld_users".to_string(), "srv_payments".to_string()].into();
        let rows = flatten(&ws, &c, "local", SidebarTab::Servers);
        let ids = ids(&rows);
        assert!(ids.contains(&"fld_users"));
        assert!(!ids.contains(&"rt_users_get"));
        assert!(ids.contains(&"rt_health"));
        assert!(ids.contains(&"srv_payments"));
        assert!(!ids.contains(&"fld_charges"));
        let folder = rows.iter().find(|r| r.id == "fld_users").unwrap();
        assert!(folder.expandable && !folder.expanded);
    }

    #[test]
    fn active_environment_is_marked() {
        let ws = workspace();
        let rows = flatten(&ws, &HashSet::new(), "staging", SidebarTab::Environments);
        let active: Vec<&str> = rows
            .iter()
            .filter(|r| matches!(r.kind, RowKind::Environment { active: true }))
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(active, ["env:staging"]);
    }

    #[test]
    fn menus_depend_on_the_row_kind() {
        let labels = |k: &RowKind, r| -> Vec<&'static str> {
            menu_items(k, r).into_iter().map(|(l, _)| l).collect()
        };
        let srv = RowKind::Server {
            port: 1,
            running: true,
            enabled: true,
            chaos: false,
            https: false,
        };
        assert_eq!(
            labels(&srv, true),
            [
                "Stop",
                "Restart",
                "New route…",
                "New folder…",
                "Disable",
                "Change Port…",
                "Change Host…",
                "Change Prefix…",
                "Chaos on",
                "Rename…",
                "Duplicate",
                "Delete"
            ]
        );
        // Restart is hidden while stopped; a disabled server offers Enable.
        let off = RowKind::Server {
            port: 1,
            running: false,
            enabled: false,
            chaos: false,
            https: false,
        };
        let l = labels(&off, false);
        assert_eq!((l[0], l[1], l[2]), ("Start", "New route…", "New folder…"));
        assert_eq!(l[3], "Enable");
        assert!(!l.contains(&"Restart"));
        assert!(
            labels(&RowKind::Environment { active: true }, false)
                .iter()
                .all(|l| *l != "Set active")
        );
        assert!(labels(&RowKind::Environment { active: false }, false).contains(&"Set active"));
        assert_eq!(
            labels(&RowKind::Dataset { rows: 1 }, false),
            ["Open", "Open pinned", "Reset to seed", "Export JSON…"]
        );
    }
}

/// A Response row being dragged in the Route tab's list (runtime-only).
#[derive(Clone, Debug)]
pub struct DraggedResponse {
    pub id: String,
    pub label: String,
}

/// A Route or Folder row being dragged in the tree (runtime-only).
#[derive(Clone, Debug)]
pub struct DraggedRow {
    pub id: String,
    pub label: String,
}
