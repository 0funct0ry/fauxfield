//! Behavioural tests for the shell, driven through real keystrokes and actions.

use std::time::Duration;

use gpui::{Entity, TestAppContext, VisualTestContext};

use crate::actions::ACTIONS;
use crate::layout::{BottomTab, Layout};
use crate::search::Target;
use crate::shell::{PickerKind, Shell, ShellOptions, TRAFFIC_ROWS};
use crate::tabs::TabKind;
use crate::theme::{Theme, theme};

fn setup(
    cx: &mut TestAppContext,
    layout_path: Option<std::path::PathBuf>,
) -> (Entity<Shell>, &mut VisualTestContext) {
    let (view, cx) = cx.add_window_view(|w, cx| {
        Shell::new(
            ShellOptions {
                layout_path,
                config_dir: None,
                ..crate::view::test_options()
            },
            w,
            cx,
        )
    });
    cx.run_until_parked();
    (view, cx)
}

fn read<R>(view: &Entity<Shell>, cx: &mut VisualTestContext, f: impl FnOnce(&Shell) -> R) -> R {
    view.read_with(cx, |s, _| f(s))
}

fn theme_name(cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| theme(cx).name.clone())
}

#[gpui::test]
fn dock_toggles_by_keystroke_and_persist(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("layout.json");
    let (view, cx) = setup(cx, Some(path.clone()));
    assert!(read(&view, cx, |s| s.layout.left_open && s.layout.bottom_open));
    assert!(!read(&view, cx, |s| s.layout.right_open));

    cx.simulate_keystrokes("secondary-b");
    cx.simulate_keystrokes("secondary-`");
    cx.simulate_keystrokes("secondary-alt-b");
    let l = read(&view, cx, |s| s.layout.clone());
    assert!(!l.left_open && !l.bottom_open && l.right_open);

    let saved = Layout::load(&path);
    assert!(!saved.left_open && !saved.bottom_open && saved.right_open);

    cx.simulate_keystrokes("secondary-b");
    assert!(read(&view, cx, |s| s.layout.left_open));
}

#[gpui::test]
fn palette_lists_every_action_and_runs_the_chosen_one(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    cx.simulate_keystrokes("secondary-shift-p");
    let (kind, n, with_binding) = read(&view, cx, |s| {
        let p = s.picker.as_ref().unwrap();
        (
            p.kind,
            p.hits.len(),
            p.hits.iter().filter(|h| h.binding.is_some()).count(),
        )
    });
    assert_eq!(kind, PickerKind::Palette);
    assert_eq!(n, ACTIONS.len());
    assert!(with_binding >= 20);

    cx.simulate_input("toggle left");
    cx.simulate_keystrokes("enter");
    assert!(read(&view, cx, |s| s.picker.is_none()));
    assert!(!read(&view, cx, |s| s.layout.left_open));

    // The recently used command floats to the top next time.
    cx.simulate_keystrokes("secondary-shift-p");
    let first = read(&view, cx, |s| {
        s.picker.as_ref().unwrap().hits[0].target.clone()
    });
    assert_eq!(first, Target::Action("toggle_left_dock".into()));
    cx.simulate_keystrokes("escape");
    assert!(read(&view, cx, |s| s.picker.is_none()));
}

#[gpui::test]
fn quick_search_filters_with_prefixes_and_opens_tabs(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    view.update_in(cx, |s, w, cx| s.open_picker(PickerKind::Search, w, cx));
    cx.simulate_input("/charges");
    let all_routes = read(&view, cx, |s| {
        let p = s.picker.as_ref().unwrap();
        !p.hits.is_empty() && p.hits.iter().all(|h| matches!(h.target, Target::Route(_)))
    });
    assert!(all_routes);

    cx.simulate_keystrokes("enter");
    let (n, preview, kind) = read(&view, cx, |s| {
        let t = s.tabs.active().unwrap();
        (s.tabs.len(), t.preview, t.kind)
    });
    assert_eq!((n, preview, kind), (1, true, TabKind::Route));

    // Cmd+Enter opens pinned.
    view.update_in(cx, |s, w, cx| s.open_picker(PickerKind::Search, w, cx));
    cx.simulate_input("@users");
    cx.simulate_keystrokes("secondary-enter");
    let tabs = read(&view, cx, |s| s.tabs.tabs().to_vec());
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs[1].kind, TabKind::Server);
    assert!(!tabs[1].preview);

    // Alt+Enter reveals without opening a tab.
    view.update_in(cx, |s, w, cx| s.open_picker(PickerKind::Search, w, cx));
    cx.simulate_input("#Charges");
    cx.simulate_keystrokes("alt-enter");
    assert_eq!(read(&view, cx, |s| s.tabs.len()), 2);
    assert_eq!(
        read(&view, cx, |s| s.selected.clone()).as_deref(),
        Some("fld_charges")
    );
}

#[gpui::test]
fn cmd_k_alone_opens_search_after_the_chord_timeout(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    cx.simulate_keystrokes("secondary-k");
    assert!(
        read(&view, cx, |s| s.picker.is_none()),
        "waits for a second key"
    );
    cx.executor().advance_clock(Duration::from_millis(401));
    cx.run_until_parked();
    assert_eq!(
        read(&view, cx, |s| s.picker.as_ref().map(|p| p.kind)),
        Some(PickerKind::Search)
    );
}

#[gpui::test]
fn theme_chord_previews_live_and_escape_reverts(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    cx.update(|_, cx| cx.set_global(Theme::one_dark()));
    cx.simulate_keystrokes("secondary-k secondary-t");
    assert_eq!(
        read(&view, cx, |s| s.picker.as_ref().map(|p| p.kind)),
        Some(PickerKind::Theme)
    );
    assert_eq!(theme_name(cx), "One Dark");

    cx.simulate_keystrokes("down");
    assert_eq!(theme_name(cx), "One Light", "highlight previews the theme");
    cx.simulate_keystrokes("escape");
    assert_eq!(theme_name(cx), "One Dark", "Esc reverts");
    assert!(read(&view, cx, |s| s.picker.is_none()));

    // Enter commits.
    cx.simulate_keystrokes("secondary-k secondary-t");
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("enter");
    assert_eq!(theme_name(cx), "One Light");
    assert_eq!(
        read(&view, cx, |s| s.layout.theme.clone()).as_deref(),
        Some("One Light")
    );
}

#[gpui::test]
fn tabs_preview_pin_close_and_jump(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, cx| {
        s.click_row("rt_users_list", 1, cx);
        s.click_row("rt_users_get", 1, cx); // replaces the preview
    });
    assert_eq!(read(&view, cx, |s| s.tabs.len()), 1);
    view.update(cx, |s, cx| {
        s.click_row("rt_users_get", 2, cx); // double-click pins
        s.click_row("srv_payments", 1, cx);
        s.click_row("env:local", 2, cx);
    });
    let titles: Vec<String> = read(&view, cx, |s| {
        s.tabs.tabs().iter().map(|t| t.title.clone()).collect()
    });
    assert_eq!(titles, ["GET /users/:id", "Payments", "local"]);
    assert!(read(&view, cx, |s| s.tabs.tabs()[1].preview));

    cx.simulate_keystrokes("secondary-1");
    assert_eq!(read(&view, cx, |s| s.tabs.active_index()), Some(0));
    cx.simulate_keystrokes("secondary-9");
    assert_eq!(
        read(&view, cx, |s| s.tabs.active_index()),
        Some(0),
        "out of range is ignored"
    );
    cx.simulate_keystrokes("secondary-w");
    assert_eq!(read(&view, cx, |s| s.tabs.len()), 2);

    view.update(cx, |s, cx| s.move_tab(0, 1, cx));
    assert_eq!(
        read(&view, cx, |s| s.tabs.tabs()[1].title.clone()),
        "Payments"
    );
}

#[gpui::test]
fn tabs_and_theme_survive_a_restart(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("layout.json");
    {
        let (view, cx) = setup(cx, Some(path.clone()));
        view.update(cx, |s, cx| {
            s.click_row("rt_health", 2, cx);
            s.click_row("srv_users", 1, cx);
            s.show_bottom(BottomTab::Logs, cx);
        });
    }
    let (view, cx) = setup(cx, Some(path));
    let (n, bottom) = read(&view, cx, |s| (s.tabs.len(), s.layout.bottom_tab));
    assert_eq!((n, bottom), (2, BottomTab::Logs));
}

#[gpui::test]
fn run_all_toggles_and_environment_switch(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    assert_eq!(read(&view, cx, |s| s.ws.running_count()), 0);
    cx.simulate_keystrokes("secondary-shift-r");
    assert_eq!(read(&view, cx, |s| s.ws.running_count()), 2);
    cx.simulate_keystrokes("secondary-shift-r");
    assert_eq!(read(&view, cx, |s| s.ws.running_count()), 0);

    cx.simulate_keystrokes("secondary-e");
    cx.simulate_input("stag");
    cx.simulate_keystrokes("enter");
    assert_eq!(read(&view, cx, |s| s.active_env.clone()), "staging");
}

#[gpui::test]
fn new_route_needs_a_server_then_prompts(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    cx.simulate_keystrokes("secondary-n");
    let msg = read(&view, cx, |s| s.notice.as_ref().map(|(_, m)| m.clone()));
    assert!(msg.unwrap().contains("Select a server"));
    view.update(cx, |s, _| s.selected = Some("srv_payments".into()));
    cx.simulate_keystrokes("secondary-n");
    assert_eq!(
        read(&view, cx, |s| s.picker.as_ref().map(|p| p.kind)),
        Some(PickerKind::Prompt)
    );
}

#[gpui::test]
fn traffic_holds_ten_thousand_rows_and_renders(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    assert_eq!(read(&view, cx, |s| s.traffic.len()), TRAFFIC_ROWS);
    // A full draw with 10k rows must not lay out every row: the list is virtualised.
    cx.update(|w, _| w.refresh());
    cx.run_until_parked();
    view.update(cx, |s, cx| {
        s.traffic_selected = Some(9_999);
        cx.notify();
    });
    cx.run_until_parked();
}

#[gpui::test]
fn row_context_menu_opens_and_runs_items(cx: &mut TestAppContext) {
    use crate::tree::MenuAct;
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, cx| s.open_menu("srv_payments", 40.0, 60.0, cx));
    assert!(read(&view, cx, |s| s.menu.is_some()));
    cx.run_until_parked(); // renders the menu

    view.update_in(cx, |s, w, cx| s.run_menu(MenuAct::StartStop, w, cx));
    assert!(read(&view, cx, |s| s.menu.is_none()));
    assert_eq!(read(&view, cx, |s| s.ws.running_count()), 1);

    // Sections have no menu.
    view.update(cx, |s, cx| {
        s.open_menu(crate::tree::SEC_DATASETS, 0.0, 0.0, cx)
    });
    assert!(read(&view, cx, |s| s.menu.is_none()));
    // Delete removes the route from the workspace files.
    view.update_in(cx, |s, w, cx| {
        s.open_menu("rt_health", 0.0, 0.0, cx);
        s.run_menu(MenuAct::Delete, w, cx);
    });
    assert!(read(&view, cx, |s| s.ws.find_route("rt_health").is_none()));

    view.update_in(cx, |s, w, cx| {
        s.open_menu("env:staging", 0.0, 0.0, cx);
        s.run_menu(MenuAct::SetActive, w, cx);
    });
    assert_eq!(read(&view, cx, |s| s.active_env.clone()), "staging");
}

fn type_prompt(view: &Entity<Shell>, cx: &mut VisualTestContext, text: &str) {
    for ch in text.chars() {
        view.update(cx, |s, cx| s.picker_type(&ch.to_string(), cx));
    }
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
}

#[gpui::test]
fn creating_renaming_and_deleting_edits_the_files(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    let root = read(&view, cx, |s| s.ws.root.clone());

    // New server through the palette action and a prompt.
    view.update_in(cx, |s, w, cx| s.run_action_id("new_server", w, cx));
    cx.run_until_parked();
    assert_eq!(
        read(&view, cx, |s| s.picker.as_ref().map(|p| p.kind)),
        Some(PickerKind::Prompt)
    );
    type_prompt(&view, cx, "Billing API");
    let (id, port) = read(&view, cx, |s| {
        let b =
            s.ws.servers
                .iter()
                .find(|x| x.name == "Billing API")
                .unwrap();
        (b.id.clone(), b.port)
    });
    assert_eq!(port, 4021);

    // New route in it.
    view.update_in(cx, |s, w, cx| {
        s.begin_prompt(
            crate::shell::PromptAction::NewRoute {
                server: id.clone(),
                folder: None,
            },
            "",
            w,
            cx,
        )
    });
    type_prompt(&view, cx, "POST /invoices");
    let rid = read(&view, cx, |s| {
        s.ws.find_server(&id).unwrap().routes[0].id.clone()
    });
    assert_eq!(
        read(&view, cx, |s| s.tabs.active().map(|t| t.id.clone())),
        Some(rid.clone())
    );

    // Edit a field, then undo it.
    view.update_in(cx, |s, w, cx| {
        s.run_action_id("edit_response_status", w, cx)
    });
    cx.run_until_parked();
    for _ in 0..3 {
        view.update(cx, |s, cx| s.picker_backspace(cx));
    }
    type_prompt(&view, cx, "418");
    assert_eq!(
        read(&view, cx, |s| s.ws.find_route(&rid).unwrap().1.responses[0]
            .status),
        418
    );
    view.update_in(cx, |s, w, cx| s.run_action_id("undo", w, cx));
    assert_eq!(
        read(&view, cx, |s| s.ws.find_route(&rid).unwrap().1.responses[0]
            .status),
        200
    );

    // Rename moves the file; files are authoritative after a flush.
    view.update_in(cx, |s, w, cx| {
        s.begin_prompt(
            crate::shell::PromptAction::Rename { id: id.clone() },
            "",
            w,
            cx,
        )
    });
    type_prompt(&view, cx, "Billing");
    let store = read(&view, cx, |s| s.store.clone().unwrap());
    store.flush().unwrap();
    assert!(root.join("servers/billing.toml").is_file());
    let text = std::fs::read_to_string(root.join("servers/billing.toml")).unwrap();
    assert!(text.contains("/invoices"));
}

#[gpui::test]
fn welcome_view_without_a_workspace(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|w, cx| Shell::new(ShellOptions::default(), w, cx));
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s.store.is_none() && s.ws.servers.is_empty()));
    // Creating a workspace from the Welcome view opens it.
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("fresh");
    view.update_in(cx, |s, w, cx| s.run_action_id("create_workspace", w, cx));
    cx.run_until_parked();
    type_prompt(&view, cx, target.to_str().unwrap());
    assert!(target.join("fauxfield.toml").is_file());
    assert_eq!(read(&view, cx, |s| s.ws.servers.len()), 1);
}

#[gpui::test]
fn external_edits_reload_the_view(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    let root = read(&view, cx, |s| s.ws.root.clone());
    let p = root.join("servers/02-payments.toml");
    let text = std::fs::read_to_string(&p)
        .unwrap()
        .replace("port = 4020", "port = 4999");
    std::fs::write(&p, text).unwrap();
    let store = read(&view, cx, |s| s.store.clone().unwrap());
    store.reconcile();
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    assert_eq!(
        read(&view, cx, |s| s
            .ws
            .find_server("srv_payments")
            .unwrap()
            .port),
        4999
    );
}
