//! Every user action, registered with GPUI and listed once in `ACTIONS` so the keymap, the
//! Command Palette and (from M1.3) agent tooling read one table (SPEC §9.7, standing rule 5).

use gpui::{Action, App, KeyBinding, Keystroke, actions};

use crate::keymap::Keymap;
use crate::search::CommandEntry;

actions!(
    fauxfield,
    [
        QuickSearch,
        CommandPalette,
        SelectTheme,
        SwitchEnvironment,
        SwitchNetworkProfile,
        ToggleChaos,
        ToggleLeftDock,
        ToggleBottomDock,
        ToggleRightDock,
        ShowSidebarServers,
        ShowSidebarEnvironments,
        ShowSidebarDatasets,
        ShowTraffic,
        ShowProblems,
        ShowLogs,
        CloseTab,
        PinTab,
        ActivateTab1,
        ActivateTab2,
        ActivateTab3,
        ActivateTab4,
        ActivateTab5,
        ActivateTab6,
        ActivateTab7,
        ActivateTab8,
        ActivateTab9,
        RunAll,
        ToggleServer,
        NewServer,
        NewEnvironment,
        NewRoute,
        SendTest,
        AddDatasetAction,
        Save,
        Duplicate,
        ResetState,
        CopyDatasetCell,
        CopyDatasetRow,
        DeleteDatasetRow,
        NewDatasetFromTable,
        ImportDatasetFile,
        ImportIntoDataset,
        AddDatasetRow,
        AddDatasetColumn,
        RenameDatasetColumn,
        RemoveDatasetColumn,
        GenerateDatasetRows,
        PasteDatasetRows,
        EditDatasetCell,
        SaveDatasetSeed,
        ReloadDatasetFromSeed,
        SaveRowsToSeed,
        NewCrudResource,
        EditDatasetSchema,
        CopyCrudOpCurl,
        AddOperationResponse,
        RemoveOperationResponse,
        ApplyDatasetJson,
        RevertDatasetJson,
        ToggleDatasetJsonFormat,
        UndoDatasetEdit,
        RedoDatasetEdit,
        DuplicateDatasetRows,
        SelectAllDatasetRows,
        ClearDatasetSelection,
        CreateCrudRoutes,
        ExportDataset,
        ClearDatasetData,
        ToggleDatasetView,
        OpenDatasetSeed,
        NewDataset,
        NewStaticRoute,
        NewWebsocketRoute,
        SendWsMessage,
        TrustDevCa,
        NewFolder,
        NewResponse,
        Rename,
        Delete,
        Undo,
        Redo,
        OpenFolder,
        CreateWorkspace,
        OpenFolderByPath,
        CreateWorkspaceByPath,
        CloseWorkspace,
        RenameWorkspace,
        ClearRecent,
        ConfirmDialog,
        CancelDialog,
        ToastAction,
        EditServerHost,
        EditServerPrefix,
        ToggleServerEnabled,
        ToggleUnmatchedHint,
        MergeKeepMine,
        MergeLoadDisk,
        EditRouteMethod,
        EditRouteMethodCustom,
        EditRoutePath,
        MoveUp,
        MoveDown,
        MoveToRoot,
        EditRoutePriority,
        EditRouteMatch,
        RouteTestPath,
        ToggleStrictSlash,
        EditResponseStatus,
        EditResponseName,
        EditResponseContentType,
        EditResponseDelay,
        EditResponseHeader,
        EditResponseBody,
        SetDefaultResponse,
        RestartServer,
        ClearTraffic,
        CopyAsCurl,
        MockThisRoute,
        RefireCallback,
        ToggleServerProxy,
        ToggleApiDocs,
        OpenApiDocs,
        CopyApiDocsUrl,
        ExportOpenapi,
        ToggleRouteForward,
        ToggleRoutePinned,
        EditServerPort,
        QuitAnyway,
        CancelQuit,
        Quit,
        ShowAbout,
        AgentsSetup,
        OpenSettings,
        SettingsFocusSearch,
        SettingsOpenFile,
        SettingsResetSection,
        KeymapOpenFile,
        ListNext,
        ListPrev,
        ListFirst,
        ListLast,
        ListCollapse,
        ListExpand,
        ListFilter,
        ListOpen,
        ListLeave,
        HideApp,
        HideOthers,
        ShowAllApps,
        MinimizeWindow,
        ZoomWindow,
        ToggleFullscreen,
        RevealWorkspace,
        OpenDocumentation,
        OpenReleaseNotes,
        ReportIssue,
        CheckForUpdates,
        EditCut,
        EditCopy,
        EditPaste,
        EditSelectAll,
    ]
);

/// Parameterised action used by the Open Recent submenu (SPEC §20.14). It has no JSON form
/// (`no_json`), so it never appears in `ACTIONS` or the palette; the menu builds it directly.
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = fauxfield, no_json)]
pub struct OpenRecent {
    pub path: String,
}

pub struct ActionInfo {
    /// Stable snake_case id used in keymap TOML and by the palette.
    pub id: &'static str,
    /// Palette label.
    pub label: &'static str,
    pub build: fn() -> Box<dyn Action>,
    bind: fn(&str, Option<&str>) -> KeyBinding,
}

macro_rules! row {
    ($id:literal, $label:literal, $ty:path) => {
        ActionInfo {
            id: $id,
            label: $label,
            build: || Box::new($ty),
            bind: |keys, ctx| KeyBinding::new(keys, $ty, ctx),
        }
    };
}

pub static ACTIONS: &[ActionInfo] = &[
    row!("quick_search", "Quick Search: Open", QuickSearch),
    row!("command_palette", "Command Palette: Toggle", CommandPalette),
    row!("select_theme", "Theme Selector: Toggle", SelectTheme),
    row!(
        "switch_environment",
        "Environment: Switch…",
        SwitchEnvironment
    ),
    row!(
        "switch_network_profile",
        "Network: Switch Profile…",
        SwitchNetworkProfile
    ),
    row!("toggle_chaos", "Server: Toggle Chaos", ToggleChaos),
    row!("toggle_left_dock", "View: Toggle Left Dock", ToggleLeftDock),
    row!(
        "show_sidebar_servers",
        "Sidebar: Show Servers",
        ShowSidebarServers
    ),
    row!(
        "show_sidebar_environments",
        "Sidebar: Show Environments",
        ShowSidebarEnvironments
    ),
    row!(
        "show_sidebar_datasets",
        "Sidebar: Show Datasets",
        ShowSidebarDatasets
    ),
    row!(
        "toggle_bottom_dock",
        "View: Toggle Bottom Dock",
        ToggleBottomDock
    ),
    row!(
        "toggle_right_dock",
        "View: Toggle Right Dock (Inspector)",
        ToggleRightDock
    ),
    row!("show_traffic", "View: Show Traffic", ShowTraffic),
    row!("show_problems", "View: Show Problems", ShowProblems),
    row!("show_logs", "View: Show Logs", ShowLogs),
    row!("close_tab", "Tab: Close", CloseTab),
    row!("pin_tab", "Tab: Pin", PinTab),
    row!("activate_tab_1", "Tab: Go to Tab 1", ActivateTab1),
    row!("activate_tab_2", "Tab: Go to Tab 2", ActivateTab2),
    row!("activate_tab_3", "Tab: Go to Tab 3", ActivateTab3),
    row!("activate_tab_4", "Tab: Go to Tab 4", ActivateTab4),
    row!("activate_tab_5", "Tab: Go to Tab 5", ActivateTab5),
    row!("activate_tab_6", "Tab: Go to Tab 6", ActivateTab6),
    row!("activate_tab_7", "Tab: Go to Tab 7", ActivateTab7),
    row!("activate_tab_8", "Tab: Go to Tab 8", ActivateTab8),
    row!("activate_tab_9", "Tab: Go to Tab 9", ActivateTab9),
    row!("run_all", "Server: Run All / Stop All", RunAll),
    row!(
        "toggle_server",
        "Server: Start / Stop Current",
        ToggleServer
    ),
    row!("restart_server", "Server: Restart Current", RestartServer),
    row!("edit_server_port", "Server: Change Port…", EditServerPort),
    row!("clear_traffic", "Traffic: Clear", ClearTraffic),
    row!("copy_as_curl", "Traffic: Copy as cURL", CopyAsCurl),
    row!("mock_this_route", "Traffic: Mock This Route", MockThisRoute),
    row!(
        "refire_callback",
        "Traffic: Re-fire Callback",
        RefireCallback
    ),
    row!("toggle_proxy", "Server: Toggle Proxy", ToggleServerProxy),
    row!(
        "toggle_api_docs",
        "Server: Toggle API Documentation",
        ToggleApiDocs
    ),
    row!(
        "open_api_docs",
        "Server: Open API Documentation",
        OpenApiDocs
    ),
    row!(
        "copy_api_docs_url",
        "Server: Copy API Documentation URL",
        CopyApiDocsUrl
    ),
    row!(
        "export_openapi",
        "Server: Export OpenAPI Document…",
        ExportOpenapi
    ),
    row!(
        "toggle_route_forward",
        "Route: Toggle Forward to Upstream",
        ToggleRouteForward
    ),
    row!(
        "toggle_route_pinned",
        "Route: Toggle Pin Mock",
        ToggleRoutePinned
    ),
    row!("quit_anyway", "Quit: Stop Servers and Quit", QuitAnyway),
    row!("cancel_quit", "Quit: Cancel", CancelQuit),
    row!("new_server", "Server: New…", NewServer),
    row!("new_environment", "Environment: New…", NewEnvironment),
    row!("new_route", "Route: New…", NewRoute),
    row!("send_test", "Route: Send Test Request", SendTest),
    row!(
        "add_dataset_action",
        "Response: Add Dataset Action",
        AddDatasetAction
    ),
    row!("save", "File: Save", Save),
    row!("duplicate", "Edit: Duplicate Response or Route", Duplicate),
    row!("reset_state", "Dataset: Reset State", ResetState),
    row!("copy_dataset_cell", "Dataset: Copy Cell", CopyDatasetCell),
    row!(
        "copy_dataset_row",
        "Dataset: Copy Row as JSON",
        CopyDatasetRow
    ),
    row!(
        "delete_dataset_row",
        "Dataset: Delete Selected Rows",
        DeleteDatasetRow
    ),
    row!("new_dataset", "Dataset: New…", NewDataset),
    row!(
        "new_dataset_from_table",
        "Dataset: New From Table…",
        NewDatasetFromTable
    ),
    row!(
        "import_dataset_file",
        "Dataset: Import File…",
        ImportDatasetFile
    ),
    row!(
        "import_into_dataset",
        "Dataset: Import Into…",
        ImportIntoDataset
    ),
    row!("add_dataset_row", "Dataset: Add Row", AddDatasetRow),
    row!(
        "add_dataset_column",
        "Dataset: Add Column…",
        AddDatasetColumn
    ),
    row!(
        "rename_dataset_column",
        "Dataset: Rename Column…",
        RenameDatasetColumn
    ),
    row!(
        "remove_dataset_column",
        "Dataset: Remove Column…",
        RemoveDatasetColumn
    ),
    row!(
        "generate_dataset_rows",
        "Dataset: Seed Data…",
        GenerateDatasetRows
    ),
    row!(
        "paste_dataset_rows",
        "Dataset: Paste Rows",
        PasteDatasetRows
    ),
    row!("edit_dataset_cell", "Dataset: Edit Cell", EditDatasetCell),
    row!("save_dataset_seed", "Dataset: Save Seed", SaveDatasetSeed),
    row!(
        "reload_dataset_from_seed",
        "Dataset: Reload From Seed",
        ReloadDatasetFromSeed
    ),
    row!(
        "save_rows_to_seed",
        "Dataset: Save Rows to Seed",
        SaveRowsToSeed
    ),
    row!(
        "duplicate_dataset_rows",
        "Dataset: Duplicate Selected Rows",
        DuplicateDatasetRows
    ),
    row!(
        "select_all_dataset_rows",
        "Dataset: Select All Rows",
        SelectAllDatasetRows
    ),
    row!(
        "clear_dataset_selection",
        "Dataset: Clear Row Selection",
        ClearDatasetSelection
    ),
    row!(
        "undo_dataset_edit",
        "Dataset: Undo Cell Edit",
        UndoDatasetEdit
    ),
    row!(
        "redo_dataset_edit",
        "Dataset: Redo Cell Edit",
        RedoDatasetEdit
    ),
    row!(
        "apply_dataset_json",
        "Dataset: Apply JSON Edits",
        ApplyDatasetJson
    ),
    row!(
        "revert_dataset_json",
        "Dataset: Revert JSON Edits",
        RevertDatasetJson
    ),
    row!(
        "toggle_dataset_json_format",
        "Dataset: Toggle Pretty/Compact JSON",
        ToggleDatasetJsonFormat
    ),
    row!(
        "copy_crud_op_curl",
        "Route: Copy CRUD Operation as cURL",
        CopyCrudOpCurl
    ),
    row!(
        "edit_dataset_schema",
        "Dataset: Edit Schema…",
        EditDatasetSchema
    ),
    row!(
        "create_crud_routes",
        "Dataset: Create CRUD Routes…",
        CreateCrudRoutes
    ),
    row!("export_dataset", "Dataset: Export…", ExportDataset),
    row!(
        "clear_dataset_data",
        "Dataset: Clear Data…",
        ClearDatasetData
    ),
    row!(
        "toggle_dataset_view",
        "Dataset: Switch Table/JSON View",
        ToggleDatasetView
    ),
    row!("open_dataset_seed", "Dataset: Open Seed", OpenDatasetSeed),
    row!(
        "new_crud_resource",
        "Dataset: New CRUD Resource…",
        NewCrudResource
    ),
    row!(
        "new_static_route",
        "Route: New Static Files…",
        NewStaticRoute
    ),
    row!(
        "new_websocket_route",
        "Route: New WebSocket",
        NewWebsocketRoute
    ),
    row!(
        "send_ws_message",
        "WebSocket: Send Draft Message to All Clients",
        SendWsMessage
    ),
    row!("trust_dev_ca", "HTTPS: Trust Dev CA…", TrustDevCa),
    row!("new_folder", "Folder: New…", NewFolder),
    row!("new_response", "Response: New", NewResponse),
    row!(
        "add_operation_response",
        "Route: Add Operation Response",
        AddOperationResponse
    ),
    row!(
        "remove_operation_response",
        "Route: Remove Operation Response",
        RemoveOperationResponse
    ),
    row!("rename", "Edit: Rename…", Rename),
    row!("delete", "Edit: Delete", Delete),
    row!("undo", "Edit: Undo", Undo),
    row!("redo", "Edit: Redo", Redo),
    row!("open_folder", "Workspace: Open Folder…", OpenFolder),
    row!("create_workspace", "Workspace: Create…", CreateWorkspace),
    row!(
        "open_folder_by_path",
        "Workspace: Open Folder by Path…",
        OpenFolderByPath
    ),
    row!(
        "create_workspace_by_path",
        "Workspace: Create by Path…",
        CreateWorkspaceByPath
    ),
    row!("close_workspace", "Workspace: Close", CloseWorkspace),
    row!("rename_workspace", "Workspace: Rename…", RenameWorkspace),
    row!("clear_recent", "Workspace: Clear Recent", ClearRecent),
    row!("confirm_dialog", "Dialog: Confirm", ConfirmDialog),
    row!("cancel_dialog", "Dialog: Cancel", CancelDialog),
    row!("toast_action", "Toast: Run Action", ToastAction),
    row!("edit_server_host", "Server: Change Host…", EditServerHost),
    row!(
        "edit_server_prefix",
        "Server: Change Prefix…",
        EditServerPrefix
    ),
    row!(
        "toggle_server_enabled",
        "Server: Enable / Disable",
        ToggleServerEnabled
    ),
    row!(
        "toggle_unmatched_hint",
        "Server: Toggle 404 Closest-Routes Hint",
        ToggleUnmatchedHint
    ),
    row!("merge_keep_mine", "Merge: Keep Mine", MergeKeepMine),
    row!("merge_load_disk", "Merge: Load From Disk", MergeLoadDisk),
    row!(
        "edit_route_method",
        "Route: Change Method…",
        EditRouteMethod
    ),
    row!(
        "edit_route_method_custom",
        "Route: Type Custom Method…",
        EditRouteMethodCustom
    ),
    row!("edit_route_path", "Route: Edit Path…", EditRoutePath),
    row!("move_up", "Tree: Move Route or Folder Up", MoveUp),
    row!("move_down", "Tree: Move Route or Folder Down", MoveDown),
    row!(
        "route_move_to_root",
        "Route: Move to Server Root",
        MoveToRoot
    ),
    row!(
        "edit_route_priority",
        "Route: Set Priority…",
        EditRoutePriority
    ),
    row!("edit_route_match", "Route: Set Match Mode…", EditRouteMatch),
    row!("route_test_path", "Route: Test a Path…", RouteTestPath),
    row!(
        "toggle_strict_slash",
        "Server: Toggle Strict Trailing Slash",
        ToggleStrictSlash
    ),
    row!(
        "edit_response_status",
        "Response: Edit Status…",
        EditResponseStatus
    ),
    row!(
        "edit_response_name",
        "Response: Edit Name…",
        EditResponseName
    ),
    row!(
        "edit_response_content_type",
        "Response: Edit Content Type…",
        EditResponseContentType
    ),
    row!(
        "edit_response_delay",
        "Response: Edit Delay…",
        EditResponseDelay
    ),
    row!(
        "edit_response_header",
        "Response: Set Header…",
        EditResponseHeader
    ),
    row!(
        "edit_response_body",
        "Response: Edit Body…",
        EditResponseBody
    ),
    row!(
        "set_default_response",
        "Response: Make Default",
        SetDefaultResponse
    ),
    row!("quit", "Application: Quit", Quit),
    row!("show_about", "Application: About Fauxfield", ShowAbout),
    row!("agents_setup", "Agents: Set Up MCP…", AgentsSetup),
    row!("open_settings", "Open Settings", OpenSettings),
    row!(
        "settings_focus_search",
        "Settings: Search",
        SettingsFocusSearch
    ),
    row!(
        "settings_open_file",
        "Settings: Open settings.toml",
        SettingsOpenFile
    ),
    row!(
        "settings_reset_section",
        "Settings: Reset Section",
        SettingsResetSection
    ),
    row!(
        "keymap_open_file",
        "Keymap: Open keymap.toml",
        KeymapOpenFile
    ),
    row!("list_next", "List: Next Row", ListNext),
    row!("list_prev", "List: Previous Row", ListPrev),
    row!("list_first", "List: First Row", ListFirst),
    row!("list_last", "List: Last Row", ListLast),
    row!("list_collapse", "List: Collapse Row", ListCollapse),
    row!("list_expand", "List: Expand Row", ListExpand),
    row!("list_filter", "List: Filter", ListFilter),
    row!("list_open", "List: Open Row", ListOpen),
    row!("list_leave", "List: Leave List", ListLeave),
    #[cfg(target_os = "macos")]
    row!("hide_app", "Application: Hide Fauxfield", HideApp),
    #[cfg(target_os = "macos")]
    row!("hide_others", "Application: Hide Others", HideOthers),
    #[cfg(target_os = "macos")]
    row!("show_all_apps", "Application: Show All", ShowAllApps),
    row!("minimize_window", "Window: Minimize", MinimizeWindow),
    row!("zoom_window", "Window: Zoom / Maximize", ZoomWindow),
    row!(
        "toggle_fullscreen",
        "Window: Toggle Full Screen",
        ToggleFullscreen
    ),
    row!(
        "reveal_workspace",
        "Workspace: Reveal in File Manager",
        RevealWorkspace
    ),
    row!(
        "open_documentation",
        "Help: Documentation",
        OpenDocumentation
    ),
    row!(
        "open_release_notes",
        "Help: Release Notes",
        OpenReleaseNotes
    ),
    row!("report_issue", "Help: Report an Issue…", ReportIssue),
    row!(
        "check_for_updates",
        "Help: Check for Updates…",
        CheckForUpdates
    ),
    row!("edit_cut", "Edit: Cut", EditCut),
    row!("edit_copy", "Edit: Copy", EditCopy),
    row!("edit_paste", "Edit: Paste", EditPaste),
    row!("edit_select_all", "Edit: Select All", EditSelectAll),
];

/// Whether `id` is an action of the app, the code editor or the data table (the keymap editor
/// lists all of them).
pub fn is_known(id: &str) -> bool {
    find(id).is_some()
        || crate::code_editor::actions::binder(id).is_some()
        || crate::data_table::actions::binder(id).is_some()
}

/// Every action id the keymap editor lists: app actions, editor keys and data table keys.
pub fn all_ids() -> Vec<&'static str> {
    ACTIONS
        .iter()
        .map(|a| a.id)
        .chain(crate::code_editor::actions::IDS.iter().copied())
        .chain(crate::data_table::actions::IDS.iter().copied())
        .collect()
}

/// A readable label for any action id (palette label, else the id).
pub fn label_of(id: &str) -> String {
    find(id).map_or_else(|| id.to_string(), |a| a.label.to_string())
}

/// The words of an action name, lowercased and sorted, without punctuation: "Server: New…" and
/// "New Server…" are the same name (SPEC §20.34).
pub fn name_words(label: &str) -> Vec<String> {
    let mut w: Vec<String> = label
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    w.sort();
    w
}

/// The tooltip of an action's button: its palette label with the live binding, e.g.
/// `Server: New… (Cmd+N)`.
pub fn tip(id: &str, keymap: &Keymap, mac: bool) -> String {
    let label = label_of(id);
    match keymap.binding_for(id) {
        Some(k) => format!("{label} ({})", crate::keymap::display_keys(k, mac)),
        None => label,
    }
}

pub fn find(id: &str) -> Option<&'static ActionInfo> {
    ACTIONS.iter().find(|a| a.id == id)
}

/// Canonical form of a keystroke string (`secondary-k` → `cmd-k` on macOS), the form GPUI
/// reports for key events, so chords compare equal.
pub fn canonical(keys: &str) -> Option<String> {
    Keystroke::parse(keys).ok().map(|k| k.unparse())
}

/// Palette entries: every action with its resolved binding.
pub fn commands(keymap: &Keymap) -> Vec<CommandEntry> {
    ACTIONS
        .iter()
        .map(|a| CommandEntry {
            id: a.id.to_string(),
            label: a.label.to_string(),
            binding: keymap.binding_for(a.id).map(str::to_string),
        })
        .collect()
}

/// Registers the keymap with GPUI. Chords, and keys that start a chord, are handled by the
/// shell's `ChordState` instead (400 ms timeout, SPEC §9.8) and are skipped here. Unknown
/// actions and unparsable keystrokes are returned as problems, never panics.
pub fn install(cx: &mut App, keymap: &Keymap) -> Vec<String> {
    let prefixes: Vec<String> = keymap
        .chords()
        .filter_map(|b| b.keys.split_whitespace().next().and_then(canonical))
        .collect();
    let mut problems = Vec::new();
    let mut bindings = Vec::new();
    for b in &keymap.bindings {
        if let Some(bind) = crate::code_editor::actions::binder(&b.action)
            .or_else(|| crate::data_table::actions::binder(&b.action))
        {
            if let Some(canon) = canonical(&b.keys) {
                if !b.is_chord() && !prefixes.contains(&canon) {
                    bindings.push(bind(&b.keys, b.context.as_deref()));
                }
            } else {
                problems.push(format!("keymap: invalid keys {:?}", b.keys));
            }
            continue;
        }
        let Some(info) = find(&b.action) else {
            problems.push(format!("keymap: unknown action {:?}", b.action));
            continue;
        };
        if b.is_chord() {
            continue;
        }
        let Some(canon) = canonical(&b.keys) else {
            problems.push(format!("keymap: invalid keys {:?}", b.keys));
            continue;
        };
        if prefixes.contains(&canon) {
            continue;
        }
        bindings.push((info.bind)(&b.keys, b.context.as_deref()));
    }
    cx.clear_key_bindings();
    cx.bind_keys(bindings);
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_labels_are_unique() {
        let mut ids: Vec<&str> = ACTIONS.iter().map(|a| a.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ACTIONS.len());
        let mut labels: Vec<&str> = ACTIONS.iter().map(|a| a.label).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), ACTIONS.len());
    }

    #[test]
    fn bundled_keymap_only_names_registered_actions() {
        for b in &Keymap::bundled().bindings {
            assert!(
                find(&b.action).is_some()
                    || crate::code_editor::actions::binder(&b.action).is_some()
                    || crate::data_table::actions::binder(&b.action).is_some(),
                "{}",
                b.action
            );
            for stroke in b.keys.split_whitespace() {
                assert!(canonical(stroke).is_some(), "{stroke}");
            }
        }
    }

    #[test]
    fn tooltip_is_the_palette_label_with_the_binding() {
        let km = Keymap::bundled();
        for a in ACTIONS {
            let t = tip(a.id, &km, true);
            assert!(t.starts_with(a.label), "{t}");
            assert_eq!(t.contains('('), km.binding_for(a.id).is_some(), "{t}");
        }
    }

    #[test]
    fn every_action_is_in_the_palette_with_its_binding() {
        let km = Keymap::bundled();
        let cmds = commands(&km);
        assert_eq!(cmds.len(), ACTIONS.len());
        let theme = cmds.iter().find(|c| c.id == "select_theme").unwrap();
        assert_eq!(theme.binding.as_deref(), Some("secondary-k secondary-t"));
        assert!(
            cmds.iter()
                .find(|c| c.id == "quit")
                .unwrap()
                .binding
                .is_some()
        );
    }
}
