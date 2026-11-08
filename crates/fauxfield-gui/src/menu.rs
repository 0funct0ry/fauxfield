//! The application menu model (M3.2, SPEC §9.11): pure data, no GPUI. One model feeds two
//! renderers, the native macOS menu bar and the in-window bar (Windows, Linux, dev override).
//! `enabled` is the single source of truth for which items are greyed.

use crate::keymap::Platform;

/// Where the menu is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuStyle {
    Native,
    InWindow,
}

/// `Native` on macOS, `InWindow` elsewhere (gpui 0.2.2 draws no native menu there).
pub fn menu_style() -> MenuStyle {
    if Platform::current() == Platform::Macos {
        MenuStyle::Native
    } else {
        MenuStyle::InWindow
    }
}

/// Every URL the Help menu and the About box open, in one place.
pub mod links {
    pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
    /// TODO(M18): point at the published docs site.
    pub fn documentation() -> String {
        format!("{REPOSITORY}#readme")
    }
    /// TODO(M19): point at the releases page.
    pub fn release_notes() -> String {
        format!("{REPOSITORY}/blob/main/CHANGELOG.md")
    }
    pub fn report_issue() -> String {
        format!("{REPOSITORY}/issues/new")
    }
    pub fn license() -> String {
        format!("{REPOSITORY}/blob/main/LICENSE")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SelectionKind {
    #[default]
    None,
    Server,
    Folder,
    Route,
    Response,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recent {
    pub path: String,
    pub name: String,
    pub exists: bool,
}

/// Everything the menu depends on, as plain data (filled by `Shell::menu_context`).
#[derive(Clone, Debug, PartialEq)]
pub struct MenuContext {
    pub platform: Platform,
    pub workspace_open: bool,
    pub modal_open: bool,
    pub text_input_focused: bool,
    pub current_server: Option<String>,
    pub current_server_enabled: bool,
    pub current_route: Option<String>,
    pub current_response: Option<String>,
    pub selection_kind: SelectionKind,
    pub servers_total: usize,
    pub servers_enabled: usize,
    pub servers_running: usize,
    pub current_server_running: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub active_tab_is_preview: bool,
    pub tab_titles: Vec<String>,
    pub left_dock: bool,
    pub bottom_dock: bool,
    pub right_dock: bool,
    pub traffic_rows: usize,
    pub traffic_row_selected: bool,
    pub recents: Vec<Recent>,
    pub fullscreen: bool,
    pub maximized: bool,
    /// Action id to display string (`⌘O`), resolved from the live keymap by the caller.
    pub bindings: Vec<(String, String)>,
}

impl MenuContext {
    /// A fresh Welcome view: nothing open, nothing selected.
    pub fn welcome(platform: Platform) -> Self {
        MenuContext {
            platform,
            workspace_open: false,
            modal_open: false,
            text_input_focused: false,
            current_server: None,
            current_server_enabled: false,
            current_route: None,
            current_response: None,
            selection_kind: SelectionKind::None,
            servers_total: 0,
            servers_enabled: 0,
            servers_running: 0,
            current_server_running: false,
            can_undo: false,
            can_redo: false,
            active_tab_is_preview: false,
            tab_titles: Vec::new(),
            left_dock: true,
            bottom_dock: true,
            right_dock: false,
            traffic_rows: 0,
            traffic_row_selected: false,
            recents: Vec::new(),
            fullscreen: false,
            maximized: false,
            bindings: Vec::new(),
        }
    }

    pub fn binding(&self, id: &str) -> Option<&str> {
        self.bindings
            .iter()
            .find(|(a, _)| a == id)
            .map(|(_, k)| k.as_str())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuEntry {
    Action {
        id: &'static str,
        label: String,
    },
    /// An Open Recent entry; runs `OpenRecent { path }`.
    Recent {
        path: String,
        label: String,
        enabled: bool,
    },
    /// A permanently disabled informational row ("No Recent Workspaces").
    Disabled {
        label: String,
    },
    Separator,
    Submenu {
        /// Stable key for dev ids (`menu-file-recent`).
        key: &'static str,
        label: String,
        enabled: bool,
        entries: Vec<MenuEntry>,
    },
    /// macOS Services system submenu (native renderer only).
    Services,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuDef {
    pub title: &'static str,
    pub mnemonic: Option<char>,
    pub entries: Vec<MenuEntry>,
}

/// Actions that are deliberately not in the menu, each with the reason.
pub const NOT_IN_MENU: &[(&str, &str)] = &[
    ("cancel_quit", "button of the quit confirmation modal"),
    ("quit_anyway", "button of the quit confirmation modal"),
    ("merge_keep_mine", "button of the merge banner"),
    ("merge_load_disk", "button of the merge banner"),
    ("confirm_dialog", "button of dialogs"),
    ("cancel_dialog", "button of dialogs"),
    ("toast_action", "button of a toast"),
    ("open_folder_by_path", "prompt follow-up of Open Folder"),
    (
        "create_workspace_by_path",
        "prompt follow-up of Create Workspace",
    ),
    (
        "edit_server_host",
        "context-menu only; not in the spec'd Server menu",
    ),
    (
        "edit_server_prefix",
        "context-menu only; not in the spec'd Server menu",
    ),
    (
        "toggle_server_enabled",
        "context-menu only; not in the spec'd Server menu",
    ),
    (
        "toggle_unmatched_hint",
        "context-menu only; not in the spec'd Server menu",
    ),
    ("edit_route_method_custom", "follow-up of Edit Method…"),
    ("reset_state", "placeholder until M8 (dataset state)"),
];

fn act(id: &'static str, label: &str) -> MenuEntry {
    MenuEntry::Action {
        id,
        label: label.to_string(),
    }
}

fn sub(key: &'static str, label: &str, entries: Vec<MenuEntry>) -> MenuEntry {
    MenuEntry::Submenu {
        key,
        label: label.to_string(),
        enabled: true,
        entries,
    }
}

fn truncate_mid(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let keep = max.saturating_sub(1);
    let head = keep / 2 + keep % 2;
    let tail = keep / 2;
    let h: String = s.chars().take(head).collect();
    let t: String = s.chars().skip(n - tail).collect();
    format!("{h}…{t}")
}

fn truncate_end(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max - 1).collect();
        format!("{t}…")
    }
}

/// Shortens `$HOME` to `~`.
fn tilde(path: &str) -> String {
    if let Some(home) = std::env::var_os("HOME").and_then(|h| h.into_string().ok())
        && !home.is_empty()
        && let Some(rest) = path.strip_prefix(&home)
        && (rest.is_empty() || rest.starts_with('/'))
    {
        return format!("~{rest}");
    }
    path.to_string()
}

fn recent_entries(ctx: &MenuContext) -> Vec<MenuEntry> {
    if ctx.recents.is_empty() {
        return vec![MenuEntry::Disabled {
            label: "No Recent Workspaces".into(),
        }];
    }
    let mut v: Vec<MenuEntry> = ctx
        .recents
        .iter()
        .take(10)
        .map(|r| {
            let mut label = format!("{} — {}", r.name, truncate_mid(&tilde(&r.path), 60));
            if !r.exists {
                label.push_str(" (not found)");
            }
            MenuEntry::Recent {
                path: r.path.clone(),
                label,
                enabled: r.exists,
            }
        })
        .collect();
    v.push(MenuEntry::Separator);
    v.push(act("clear_recent", "Clear Recent"));
    v
}

fn file_manager(platform: Platform) -> &'static str {
    match platform {
        Platform::Macos => "Finder",
        Platform::Windows => "Explorer",
        Platform::Linux => "File Manager",
    }
}

pub fn build_menus(ctx: &MenuContext) -> Vec<MenuDef> {
    let mac = ctx.platform == Platform::Macos;
    let mut menus = Vec::new();

    if mac {
        menus.push(MenuDef {
            title: "Fauxfield",
            mnemonic: None,
            entries: vec![
                act("show_about", "About Fauxfield"),
                MenuEntry::Separator,
                MenuEntry::Services,
                MenuEntry::Separator,
                act("hide_app", "Hide Fauxfield"),
                act("hide_others", "Hide Others"),
                act("show_all_apps", "Show All"),
                MenuEntry::Separator,
                act("quit", "Quit Fauxfield"),
            ],
        });
    }

    let mut file = vec![
        act("new_server", "New Server…"),
        act("new_route", "New Route…"),
        act("new_folder", "New Folder…"),
        MenuEntry::Separator,
        act("create_workspace", "Create Workspace…"),
        act("open_folder", "Open Folder…"),
        sub("recent", "Open Recent", recent_entries(ctx)),
        MenuEntry::Separator,
        act("close_tab", "Close Tab"),
        act("pin_tab", "Pin Tab"),
        act("close_workspace", "Close Workspace"),
        act("rename_workspace", "Rename Workspace…"),
        act(
            "reveal_workspace",
            &format!("Reveal Workspace in {}", file_manager(ctx.platform)),
        ),
    ];
    if !mac {
        file.push(MenuEntry::Separator);
        file.push(act("quit", "Quit"));
    }
    menus.push(MenuDef {
        title: "File",
        mnemonic: Some('F'),
        entries: file,
    });

    menus.push(MenuDef {
        title: "Edit",
        mnemonic: Some('E'),
        entries: vec![
            act("undo", "Undo"),
            act("redo", "Redo"),
            MenuEntry::Separator,
            act("edit_cut", "Cut"),
            act("edit_copy", "Copy"),
            act("edit_paste", "Paste"),
            act("edit_select_all", "Select All"),
            MenuEntry::Separator,
            act("duplicate", "Duplicate"),
            act("rename", "Rename…"),
            act("delete", "Delete"),
        ],
    });

    let dock =
        |visible: bool, name: &str| format!("{} {name}", if visible { "Hide" } else { "Show" });
    menus.push(MenuDef {
        title: "View",
        mnemonic: Some('V'),
        entries: vec![
            act("quick_search", "Quick Search…"),
            act("command_palette", "Command Palette…"),
            MenuEntry::Separator,
            act("toggle_left_dock", &dock(ctx.left_dock, "Left Dock")),
            act("toggle_bottom_dock", &dock(ctx.bottom_dock, "Bottom Dock")),
            act("toggle_right_dock", &dock(ctx.right_dock, "Inspector")),
            MenuEntry::Separator,
            act("show_traffic", "Show Traffic"),
            act("show_problems", "Show Problems"),
            act("show_logs", "Show Logs"),
            MenuEntry::Separator,
            act("select_theme", "Theme…"),
            MenuEntry::Separator,
            act(
                "toggle_fullscreen",
                if ctx.fullscreen {
                    "Exit Full Screen"
                } else {
                    "Enter Full Screen"
                },
            ),
        ],
    });

    let all_running = ctx.servers_enabled > 0 && ctx.servers_running == ctx.servers_enabled;
    menus.push(MenuDef {
        title: "Server",
        mnemonic: Some('S'),
        entries: vec![
            act(
                "run_all",
                if all_running {
                    "Stop All Servers"
                } else {
                    "Run All Servers"
                },
            ),
            act(
                "toggle_server",
                if ctx.current_server_running {
                    "Stop Server"
                } else {
                    "Start Server"
                },
            ),
            act("restart_server", "Restart Server"),
            act("edit_server_port", "Change Port…"),
            MenuEntry::Separator,
            act("switch_environment", "Switch Environment…"),
            MenuEntry::Separator,
            sub(
                "traffic",
                "Traffic",
                vec![
                    act("clear_traffic", "Clear Traffic"),
                    act("copy_as_curl", "Copy as cURL"),
                ],
            ),
        ],
    });

    menus.push(MenuDef {
        title: "Route",
        mnemonic: Some('R'),
        entries: vec![
            act("send_test", "Send Test Request"),
            MenuEntry::Separator,
            act("edit_route_method", "Edit Method…"),
            act("edit_route_path", "Edit Path…"),
            MenuEntry::Separator,
            act("new_response", "New Response"),
            act("set_default_response", "Make Default Response"),
            sub(
                "response",
                "Response",
                vec![
                    act("edit_response_status", "Edit Status…"),
                    act("edit_response_name", "Edit Name…"),
                    act("edit_response_content_type", "Edit Content Type…"),
                    act("edit_response_delay", "Edit Delay…"),
                    act("edit_response_header", "Set Header…"),
                    act("edit_response_body", "Edit Body…"),
                ],
            ),
        ],
    });

    const TAB_IDS: [&str; 9] = [
        "activate_tab_1",
        "activate_tab_2",
        "activate_tab_3",
        "activate_tab_4",
        "activate_tab_5",
        "activate_tab_6",
        "activate_tab_7",
        "activate_tab_8",
        "activate_tab_9",
    ];
    let tabs: Vec<MenuEntry> = ctx
        .tab_titles
        .iter()
        .take(9)
        .enumerate()
        .map(|(i, t)| act(TAB_IDS[i], &truncate_end(t, 40)))
        .collect();
    let zoom = if mac {
        "Zoom"
    } else if ctx.maximized {
        "Restore"
    } else {
        "Maximize"
    };
    menus.push(MenuDef {
        title: "Window",
        mnemonic: Some('W'),
        entries: vec![
            act("minimize_window", "Minimize"),
            act("zoom_window", zoom),
            MenuEntry::Separator,
            MenuEntry::Submenu {
                key: "tabs",
                label: "Go to Tab".into(),
                enabled: !tabs.is_empty(),
                entries: tabs,
            },
        ],
    });

    let mut help = vec![
        act("open_documentation", "Documentation"),
        act("open_release_notes", "Release Notes"),
        act("report_issue", "Report an Issue…"),
    ];
    if !mac {
        help.push(MenuEntry::Separator);
        help.push(act("show_about", "About Fauxfield"));
    }
    menus.push(MenuDef {
        title: "Help",
        mnemonic: Some('H'),
        entries: help,
    });
    menus
}

/// Whether the item `id` is enabled in `ctx`. The whole truth about greying (SPEC §9.11).
pub fn enabled(id: &str, ctx: &MenuContext) -> bool {
    match id {
        "quit" | "hide_app" | "hide_others" | "show_all_apps" | "minimize_window"
        | "zoom_window" | "toggle_fullscreen" | "open_documentation" | "open_release_notes"
        | "report_issue" | "show_about" => return true,
        "edit_cut" | "edit_copy" | "edit_paste" | "edit_select_all" => {
            return ctx.text_input_focused;
        }
        _ => {}
    }
    if ctx.modal_open {
        return false;
    }
    match id {
        "new_server" | "close_workspace" | "rename_workspace" | "reveal_workspace"
        | "switch_environment" => ctx.workspace_open,
        "new_route" | "new_folder" => ctx.current_server.is_some(),
        "create_workspace" | "open_folder" | "clear_recent" => true,
        "close_tab" => !ctx.tab_titles.is_empty(),
        "pin_tab" => ctx.active_tab_is_preview,
        "undo" => ctx.can_undo,
        "redo" => ctx.can_redo,
        "duplicate" | "rename" | "delete" => ctx.selection_kind != SelectionKind::None,
        "run_all" => ctx.servers_enabled > 0,
        // A disabled server stays startable on purpose: starting it explains itself with the
        // M3.1 "Enable and start" toast (SPEC §20.14).
        "toggle_server" => ctx.current_server.is_some(),
        "restart_server" => ctx.current_server_running,
        "edit_server_port" => ctx.current_server.is_some(),
        "clear_traffic" => ctx.traffic_rows > 0,
        "copy_as_curl" => ctx.traffic_row_selected,
        "send_test" | "edit_route_method" | "edit_route_path" | "new_response" => {
            ctx.current_route.is_some()
        }
        "set_default_response"
        | "edit_response_status"
        | "edit_response_name"
        | "edit_response_content_type"
        | "edit_response_delay"
        | "edit_response_header"
        | "edit_response_body" => ctx.current_response.is_some(),
        "quick_search" | "command_palette" | "toggle_left_dock" | "toggle_bottom_dock"
        | "toggle_right_dock" | "show_traffic" | "show_problems" | "show_logs" | "select_theme" => {
            true
        }
        _ => {
            if let Some(n) = id
                .strip_prefix("activate_tab_")
                .and_then(|n| n.parse::<usize>().ok())
            {
                return n >= 1 && n <= ctx.tab_titles.len();
            }
            false
        }
    }
}

/// Label path (`["File", "Open Recent", "x"]`) of action `id`, for events.
pub fn find_path(menus: &[MenuDef], id: &str) -> Option<Vec<String>> {
    fn walk(es: &[MenuEntry], id: &str, path: &mut Vec<String>) -> bool {
        for e in es {
            match e {
                MenuEntry::Action { id: i, label } if *i == id => {
                    path.push(label.clone());
                    return true;
                }
                MenuEntry::Submenu { label, entries, .. } => {
                    path.push(label.clone());
                    if walk(entries, id, path) {
                        return true;
                    }
                    path.pop();
                }
                _ => {}
            }
        }
        false
    }
    for m in menus {
        let mut p = vec![m.title.to_string()];
        if walk(&m.entries, id, &mut p) {
            return Some(p);
        }
    }
    None
}

/// Why `id` is disabled in `ctx`, for `ui.menu` errors.
pub fn why_disabled(id: &str, ctx: &MenuContext) -> &'static str {
    if ctx.modal_open {
        "a dialog is open"
    } else if matches!(
        id,
        "edit_cut" | "edit_copy" | "edit_paste" | "edit_select_all"
    ) {
        "no text input focused"
    } else if matches!(
        id,
        "new_server"
            | "close_workspace"
            | "rename_workspace"
            | "reveal_workspace"
            | "switch_environment"
    ) {
        "no workspace open"
    } else if matches!(
        id,
        "new_route" | "new_folder" | "edit_server_port" | "toggle_server"
    ) {
        "no server selected"
    } else if id == "undo" || id == "redo" {
        "nothing to undo or redo"
    } else {
        "not available now"
    }
}

/// Enabled state of any entry, for renderers.
pub fn entry_enabled(e: &MenuEntry, ctx: &MenuContext) -> bool {
    match e {
        MenuEntry::Action { id, .. } => enabled(id, ctx),
        MenuEntry::Recent { enabled: ok, .. } => *ok && !ctx.modal_open,
        MenuEntry::Disabled { .. } | MenuEntry::Separator | MenuEntry::Services => false,
        MenuEntry::Submenu { enabled, .. } => *enabled,
    }
}

/// Every action id reachable in some context (used by coverage tests).
pub fn menu_ids(menus: &[MenuDef]) -> Vec<&'static str> {
    fn walk(es: &[MenuEntry], out: &mut Vec<&'static str>) {
        for e in es {
            match e {
                MenuEntry::Action { id, .. } => out.push(id),
                MenuEntry::Submenu { entries, .. } => walk(entries, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for m in menus {
        walk(&m.entries, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::ACTIONS;

    fn ws(p: Platform) -> MenuContext {
        let mut c = MenuContext::welcome(p);
        c.workspace_open = true;
        c
    }

    fn all(p: Platform) -> Vec<&'static str> {
        let mut ids = Vec::new();
        let mut c = ws(p);
        c.tab_titles = (0..9).map(|i| format!("t{i}")).collect();
        c.recents = vec![Recent {
            path: "/r".into(),
            name: "r".into(),
            exists: true,
        }];
        ids.extend(menu_ids(&build_menus(&c)));
        ids
    }

    fn titles(p: Platform) -> Vec<&'static str> {
        build_menus(&MenuContext::welcome(p))
            .iter()
            .map(|m| m.title)
            .collect()
    }

    #[test]
    fn structure_per_platform() {
        assert_eq!(
            titles(Platform::Macos),
            [
                "Fauxfield",
                "File",
                "Edit",
                "View",
                "Server",
                "Route",
                "Window",
                "Help"
            ]
        );
        for p in [Platform::Windows, Platform::Linux] {
            assert_eq!(
                titles(p),
                ["File", "Edit", "View", "Server", "Route", "Window", "Help"]
            );
            let ids = all(p);
            assert!(ids.contains(&"quit") && ids.contains(&"show_about"));
            let m = build_menus(&MenuContext::welcome(p));
            let file = m.iter().find(|m| m.title == "File").unwrap();
            assert!(matches!(
                file.entries.last(),
                Some(MenuEntry::Action { id: "quit", .. })
            ));
            let help = m.iter().find(|m| m.title == "Help").unwrap();
            assert!(matches!(
                help.entries.last(),
                Some(MenuEntry::Action {
                    id: "show_about",
                    ..
                })
            ));
        }
        let mac = build_menus(&MenuContext::welcome(Platform::Macos));
        assert!(matches!(mac[0].entries[2], MenuEntry::Services));
        let file = mac.iter().find(|m| m.title == "File").unwrap();
        assert!(!menu_ids(std::slice::from_ref(file)).contains(&"quit"));
    }

    #[test]
    fn every_menu_id_exists_in_actions_and_every_action_is_covered() {
        let known: Vec<&str> = ACTIONS.iter().map(|a| a.id).collect();
        let mut seen: Vec<&str> = Vec::new();
        for p in [Platform::Macos, Platform::Windows, Platform::Linux] {
            for id in all(p) {
                let mac_only = matches!(id, "hide_app" | "hide_others" | "show_all_apps");
                assert!(
                    known.contains(&id) || (mac_only && !cfg!(target_os = "macos")),
                    "menu id {id} not in ACTIONS"
                );
                seen.push(id);
            }
        }
        for a in ACTIONS {
            let skipped = NOT_IN_MENU.iter().any(|(id, _)| *id == a.id);
            assert!(
                seen.contains(&a.id) || skipped,
                "action {} is neither in the menu nor in NOT_IN_MENU",
                a.id
            );
            if skipped {
                assert!(!seen.contains(&a.id), "{} is both", a.id);
            }
        }
        for (id, reason) in NOT_IN_MENU {
            assert!(known.contains(id), "NOT_IN_MENU names unknown {id}");
            assert!(!reason.is_empty());
        }
    }

    #[test]
    fn welcome_enabled_set_is_exact() {
        let c = MenuContext::welcome(Platform::Macos);
        let mut on: Vec<&str> = menu_ids(&build_menus(&c))
            .into_iter()
            .filter(|id| enabled(id, &c))
            .collect();
        on.sort_unstable();
        let mut want = vec![
            "create_workspace",
            "open_folder",
            "quit",
            "show_about",
            "hide_app",
            "hide_others",
            "show_all_apps",
            "toggle_left_dock",
            "toggle_bottom_dock",
            "toggle_right_dock",
            "show_traffic",
            "show_problems",
            "show_logs",
            "select_theme",
            "quick_search",
            "command_palette",
            "minimize_window",
            "zoom_window",
            "toggle_fullscreen",
            "open_documentation",
            "open_release_notes",
            "report_issue",
        ];
        want.sort_unstable();
        // Clear Recent is disabled-by-absence with no recents (the submenu has one dead row).
        assert_eq!(on, want);
    }

    #[test]
    fn modal_blocks_everything_but_the_always_set() {
        let mut c = ws(Platform::Macos);
        c.modal_open = true;
        c.current_server = Some("s".into());
        c.current_server_enabled = true;
        c.servers_enabled = 1;
        c.selection_kind = SelectionKind::Server;
        for id in all(Platform::Macos) {
            let always = [
                "quit",
                "hide_app",
                "hide_others",
                "show_all_apps",
                "minimize_window",
                "zoom_window",
                "toggle_fullscreen",
                "open_documentation",
                "open_release_notes",
                "report_issue",
                "show_about",
            ];
            if id.starts_with("edit_c") || id == "edit_paste" || id == "edit_select_all" {
                assert!(!enabled(id, &c), "{id} needs a focused input");
            } else {
                assert_eq!(enabled(id, &c), always.contains(&id), "{id}");
            }
        }
        c.text_input_focused = true;
        assert!(enabled("edit_copy", &c) && enabled("edit_paste", &c));
    }

    #[test]
    fn enable_rules_one_per_row() {
        let mut c = ws(Platform::Linux);
        // workspace_open rows
        for id in [
            "new_server",
            "close_workspace",
            "rename_workspace",
            "reveal_workspace",
            "switch_environment",
        ] {
            assert!(enabled(id, &c), "{id}");
            assert!(!enabled(id, &MenuContext::welcome(Platform::Linux)), "{id}");
        }
        // server-scoped
        for id in ["new_route", "new_folder", "edit_server_port"] {
            assert!(!enabled(id, &c));
        }
        c.current_server = Some("api".into());
        for id in ["new_route", "new_folder", "edit_server_port"] {
            assert!(enabled(id, &c));
        }
        assert!(
            enabled("toggle_server", &c),
            "a disabled server stays startable (M3.1 toast)"
        );
        c.current_server_enabled = true;
        assert!(!enabled("restart_server", &c));
        c.current_server_running = true;
        assert!(enabled("restart_server", &c));
        // tabs
        assert!(
            !enabled("close_tab", &c) && !enabled("pin_tab", &c) && !enabled("activate_tab_1", &c)
        );
        c.tab_titles = vec!["a".into(), "b".into()];
        c.active_tab_is_preview = true;
        assert!(enabled("close_tab", &c) && enabled("pin_tab", &c));
        assert!(enabled("activate_tab_2", &c) && !enabled("activate_tab_3", &c));
        // undo / redo
        assert!(!enabled("undo", &c) && !enabled("redo", &c));
        c.can_undo = true;
        c.can_redo = true;
        assert!(enabled("undo", &c) && enabled("redo", &c));
        // selection
        for id in ["duplicate", "rename", "delete"] {
            assert!(!enabled(id, &c));
        }
        c.selection_kind = SelectionKind::Route;
        for id in ["duplicate", "rename", "delete"] {
            assert!(enabled(id, &c));
        }
        // run all
        assert!(!enabled("run_all", &c));
        c.servers_enabled = 2;
        assert!(enabled("run_all", &c));
        // traffic
        assert!(!enabled("clear_traffic", &c) && !enabled("copy_as_curl", &c));
        c.traffic_rows = 3;
        c.traffic_row_selected = true;
        assert!(enabled("clear_traffic", &c) && enabled("copy_as_curl", &c));
        // route / response
        for id in [
            "send_test",
            "edit_route_method",
            "edit_route_path",
            "new_response",
        ] {
            assert!(!enabled(id, &c));
        }
        c.current_route = Some("r".into());
        for id in [
            "send_test",
            "edit_route_method",
            "edit_route_path",
            "new_response",
        ] {
            assert!(enabled(id, &c));
        }
        for id in [
            "set_default_response",
            "edit_response_status",
            "edit_response_body",
        ] {
            assert!(!enabled(id, &c));
        }
        c.current_response = Some("x".into());
        for id in [
            "set_default_response",
            "edit_response_status",
            "edit_response_body",
        ] {
            assert!(enabled(id, &c));
        }
        // text input
        assert!(!enabled("edit_cut", &c));
        c.text_input_focused = true;
        assert!(enabled("edit_cut", &c) && enabled("edit_select_all", &c));
    }

    fn label_of(menus: &[MenuDef], id: &str) -> String {
        fn walk(es: &[MenuEntry], id: &str) -> Option<String> {
            for e in es {
                match e {
                    MenuEntry::Action { id: i, label } if *i == id => return Some(label.clone()),
                    MenuEntry::Submenu { entries, .. } => {
                        if let Some(l) = walk(entries, id) {
                            return Some(l);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        menus.iter().find_map(|m| walk(&m.entries, id)).unwrap()
    }

    #[test]
    fn dynamic_labels() {
        let mut c = ws(Platform::Linux);
        let l = |c: &MenuContext, id: &str| label_of(&build_menus(c), id);
        assert_eq!(l(&c, "toggle_left_dock"), "Hide Left Dock");
        c.left_dock = false;
        assert_eq!(l(&c, "toggle_left_dock"), "Show Left Dock");
        assert_eq!(l(&c, "toggle_right_dock"), "Show Inspector");
        assert_eq!(l(&c, "run_all"), "Run All Servers");
        c.servers_enabled = 2;
        c.servers_running = 1;
        assert_eq!(l(&c, "run_all"), "Run All Servers");
        c.servers_running = 2;
        assert_eq!(l(&c, "run_all"), "Stop All Servers");
        assert_eq!(l(&c, "toggle_server"), "Start Server");
        c.current_server_running = true;
        assert_eq!(l(&c, "toggle_server"), "Stop Server");
        assert_eq!(l(&c, "toggle_fullscreen"), "Enter Full Screen");
        c.fullscreen = true;
        assert_eq!(l(&c, "toggle_fullscreen"), "Exit Full Screen");
        assert_eq!(l(&c, "zoom_window"), "Maximize");
        c.maximized = true;
        assert_eq!(l(&c, "zoom_window"), "Restore");
        let mac = ws(Platform::Macos);
        assert_eq!(l(&mac, "zoom_window"), "Zoom");
        assert_eq!(l(&mac, "reveal_workspace"), "Reveal Workspace in Finder");
        assert_eq!(
            l(&c, "reveal_workspace"),
            "Reveal Workspace in File Manager"
        );
        assert_eq!(
            label_of(&build_menus(&ws(Platform::Windows)), "reveal_workspace"),
            "Reveal Workspace in Explorer"
        );
    }

    #[test]
    fn go_to_tab_labels_and_truncation() {
        let mut c = ws(Platform::Linux);
        let long = "x".repeat(60);
        c.tab_titles = vec!["one".into(), long];
        let m = build_menus(&c);
        let w = m.iter().find(|m| m.title == "Window").unwrap();
        let MenuEntry::Submenu {
            entries, enabled, ..
        } = &w.entries[3]
        else {
            panic!()
        };
        assert!(*enabled);
        assert_eq!(entries.len(), 2);
        let MenuEntry::Action { label, .. } = &entries[1] else {
            panic!()
        };
        assert_eq!(label.chars().count(), 40);
        assert!(label.ends_with('…'));
        let none = build_menus(&ws(Platform::Linux));
        let w = none.iter().find(|m| m.title == "Window").unwrap();
        let MenuEntry::Submenu { enabled, .. } = &w.entries[3] else {
            panic!()
        };
        assert!(!*enabled);
    }

    #[test]
    fn recent_states() {
        let mut c = MenuContext::welcome(Platform::Linux);
        let recent = |c: &MenuContext| {
            let m = build_menus(c);
            let f = m.iter().find(|m| m.title == "File").unwrap();
            f.entries
                .iter()
                .find_map(|e| match e {
                    MenuEntry::Submenu {
                        key: "recent",
                        entries,
                        ..
                    } => Some(entries.clone()),
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(
            recent(&c),
            vec![MenuEntry::Disabled {
                label: "No Recent Workspaces".into()
            }]
        );
        c.recents = vec![
            Recent {
                path: "/a/demo".into(),
                name: "demo".into(),
                exists: true,
            },
            Recent {
                path: "/gone".into(),
                name: "gone".into(),
                exists: false,
            },
        ];
        let r = recent(&c);
        assert_eq!(r.len(), 4);
        assert!(
            matches!(&r[0], MenuEntry::Recent { label, enabled: true, .. } if label == "demo — /a/demo")
        );
        assert!(
            matches!(&r[1], MenuEntry::Recent { label, enabled: false, .. } if label.ends_with(" (not found)"))
        );
        assert_eq!(r[2], MenuEntry::Separator);
        assert!(matches!(
            &r[3],
            MenuEntry::Action {
                id: "clear_recent",
                ..
            }
        ));
        assert!(enabled("clear_recent", &c));
        c.recents = (0..15)
            .map(|i| Recent {
                path: format!("/p{i}"),
                name: format!("n{i}"),
                exists: true,
            })
            .collect();
        assert_eq!(recent(&c).len(), 12);
        c.modal_open = true;
        assert!(!entry_enabled(&recent(&c)[0], &c));
    }

    #[test]
    fn long_paths_truncate_in_the_middle() {
        let p = format!("/{}", "a".repeat(100));
        let t = truncate_mid(&p, 60);
        assert_eq!(t.chars().count(), 60);
        assert!(t.contains('…') && t.starts_with("/aaa") && t.ends_with("aaa"));
    }

    #[test]
    fn help_links_are_exact() {
        let repo = "https://github.com/0funct0ry/fauxfield";
        assert_eq!(links::REPOSITORY, repo);
        assert_eq!(links::documentation(), format!("{repo}#readme"));
        assert_eq!(
            links::release_notes(),
            format!("{repo}/blob/main/CHANGELOG.md")
        );
        assert_eq!(links::report_issue(), format!("{repo}/issues/new"));
        assert_eq!(links::license(), format!("{repo}/blob/main/LICENSE"));
    }

    #[test]
    fn no_placeholder_ids_in_menu() {
        for id in all(Platform::Macos) {
            assert_ne!(id, "reset_state");
        }
    }
}
