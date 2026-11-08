//! Flattens the workspace into the rows of the left dock tree. Pure; row ids are semantic
//! (server id, folder id, route id, `env:<name>`), never loop indices.

use std::collections::HashSet;

use crate::view::Workspace;

/// Row id of the workspace header (and its hamburger menu).
pub const WORKSPACE_ROW: &str = "ws:workspace";
pub const SEC_ENVIRONMENTS: &str = "sec_environments";
pub const SEC_DATASETS: &str = "sec_datasets";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowKind {
    Server {
        port: u16,
        running: bool,
        enabled: bool,
    },
    /// The workspace root header (rendered above the tree, not part of `flatten`).
    Workspace,
    Folder,
    Route {
        method: String,
    },
    Section,
    Environment {
        active: bool,
    },
    Dataset,
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

pub fn dataset_row_id(name: &str) -> String {
    format!("ds:{name}")
}

pub fn flatten(ws: &Workspace, collapsed: &HashSet<String>, active_env: &str) -> Vec<TreeRow> {
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
            },
            true,
        );
        if !open {
            continue;
        }
        for f in &s.folders {
            let open = node(&mut rows, &f.id, 1, &f.name, RowKind::Folder, true);
            if open {
                for r in &f.routes {
                    node(
                        &mut rows,
                        &r.id,
                        2,
                        &r.path,
                        RowKind::Route {
                            method: r.method.clone(),
                        },
                        false,
                    );
                }
            }
        }
        for r in &s.routes {
            node(
                &mut rows,
                &r.id,
                1,
                &r.path,
                RowKind::Route {
                    method: r.method.clone(),
                },
                false,
            );
        }
    }
    if node(
        &mut rows,
        SEC_ENVIRONMENTS,
        0,
        "Environments",
        RowKind::Section,
        true,
    ) {
        for e in &ws.environments {
            node(
                &mut rows,
                &env_row_id(&e.name),
                1,
                &e.name,
                RowKind::Environment {
                    active: e.name == active_env,
                },
                false,
            );
        }
    }
    if node(
        &mut rows,
        SEC_DATASETS,
        0,
        "Datasets",
        RowKind::Section,
        true,
    ) {
        for d in &ws.datasets {
            node(&mut rows, &dataset_row_id(d), 1, d, RowKind::Dataset, false);
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
    ResetDataset,
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
            ResetDataset => "reset-dataset".into(),
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
pub fn menu_items(kind: &RowKind, server_running: bool) -> Vec<(&'static str, MenuAct)> {
    use MenuAct::*;
    match kind {
        RowKind::Server { enabled, .. } => {
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
                ("Rename…", Rename),
                ("Duplicate", Duplicate),
                ("Delete", Delete),
            ]);
            v
        }
        RowKind::Workspace => vec![],
        RowKind::Folder => vec![
            ("New route…", NewRoute),
            ("Rename…", Rename),
            ("Duplicate", Duplicate),
            ("Delete", Delete),
        ],
        RowKind::Route { .. } => vec![
            ("Open", Open),
            ("Open pinned", OpenPinned),
            ("New response", NewResponse),
            ("Rename…", Rename),
            ("Duplicate", Duplicate),
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
        RowKind::Dataset => vec![("Reset to seed", ResetDataset)],
        RowKind::Section => vec![],
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
        let rows = flatten(&ws, &HashSet::new(), "local");
        assert_eq!(rows[0].id, "srv_users");
        assert_eq!(rows[1].id, "fld_users");
        assert_eq!(rows[2].id, "rt_users_list");
        assert_eq!(rows[2].depth, 2);
        let pos = |id: &str| rows.iter().position(|r| r.id == id).unwrap();
        assert!(pos("rt_health") > pos("rt_users_delete"));
        assert!(pos("srv_payments") > pos("rt_health"));
        assert!(pos(SEC_ENVIRONMENTS) > pos("rt_refunds"));
        assert!(pos("env:local") == pos(SEC_ENVIRONMENTS) + 1);
        assert!(rows.iter().any(|r| r.id == "ds:users"));
    }

    #[test]
    fn collapsing_hides_descendants() {
        let ws = workspace();
        let c: HashSet<String> = ["fld_users".to_string(), "srv_payments".to_string()].into();
        let rows = flatten(&ws, &c, "local");
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
        let rows = flatten(&ws, &HashSet::new(), "staging");
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
        assert!(menu_items(&RowKind::Section, false).is_empty());
        assert_eq!(labels(&RowKind::Dataset, false), ["Reset to seed"]);
    }
}
