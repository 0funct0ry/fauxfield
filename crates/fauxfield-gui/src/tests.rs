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
    cx.run_until_parked(); // the palette runs its command after two frames
    cx.run_until_parked();
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
    // New Route is greyed without a current server, so its key does nothing (SPEC §9.11).
    cx.simulate_keystrokes("secondary-n");
    assert!(read(&view, cx, |s| s.notice.is_none() && s.prompt.is_none()));
    view.update(cx, |s, cx| {
        s.selected = Some("srv_payments".into());
        cx.notify();
    });
    cx.run_until_parked();
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
    cx.run_until_parked();
    assert_eq!(read(&view, cx, |s| s.ws.running_count()), 1);

    // A row of a hidden sidebar tab has no menu.
    view.update(cx, |s, cx| s.open_menu("ds:users", 0.0, 0.0, cx));
    assert!(read(&view, cx, |s| s.menu.is_none()));
    // Delete removes the route from the workspace files.
    view.update_in(cx, |s, w, cx| {
        s.open_menu("rt_health", 0.0, 0.0, cx);
        s.run_menu(MenuAct::Delete, w, cx);
    });
    assert!(read(&view, cx, |s| s.ws.find_route("rt_health").is_none()));

    view.update_in(cx, |s, w, cx| {
        s.set_sidebar_tab(crate::tree::SidebarTab::Environments, cx);
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
    // The status is an inline field now: type over the selected value and commit.
    view.update_in(cx, |s, w, cx| {
        s.inline.as_mut().unwrap().buf = crate::editor::TextBuf::new("418");
        s.commit_inline(crate::response_pane::Nav::Stay, w, cx);
    });
    assert_eq!(
        read(&view, cx, |s| s.ws.find_route(&rid).unwrap().1.responses[0]
            .status),
        418
    );
    cx.run_until_parked(); // renders, so the Edit menu knows there is something to undo
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
    // The native dialog cannot run in the test platform; the typed-path action is its fallback.
    view.update_in(cx, |s, w, cx| {
        s.run_action_id("create_workspace_by_path", w, cx)
    });
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

/// Wait (real time) until `done` holds, letting the poll loop and the engine make progress.
fn settle(cx: &mut VisualTestContext, view: &Entity<Shell>, done: impl Fn(&Shell) -> bool) {
    cx.executor().allow_parking();
    for _ in 0..100 {
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        if read(view, cx, |s| done(s)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("condition never held");
}

#[gpui::test]
fn starting_a_server_serves_requests_and_fills_traffic(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, cx| {
        s.clear_traffic(cx);
        s.open_tab("rt_users_get", TabKind::Route, "Get user", false);
    });
    let index = read(&view, cx, |s| {
        s.ws.servers
            .iter()
            .position(|x| x.all_routes().any(|r| r.id == "rt_users_get"))
            .unwrap()
    });
    // Sending before the server runs explains what to do instead of failing silently.
    view.update(cx, |s, cx| s.send_test(cx));
    assert!(read(&view, cx, |s| s.notice.clone().unwrap().1).contains("Start"));

    view.update(cx, |s, cx| s.toggle_server_at(index, cx));
    settle(cx, &view, |s| s.ws.servers[index].running);

    view.update(cx, |s, cx| s.send_test(cx));
    settle(cx, &view, |s| s.traffic.iter().any(|r| r.id > 0));
    let (route, source, selected) = read(&view, cx, |s| {
        let r = s.traffic.last().unwrap();
        (
            r.route.clone(),
            r.rec.as_ref().unwrap().source.clone(),
            s.traffic_selected,
        )
    });
    assert_eq!(route, "rt_users_get");
    assert_eq!(source, "test");
    assert_eq!(
        selected,
        Some(0),
        "Send test selects its row for the Inspector"
    );
    assert!(read(&view, cx, |s| s
        .logs
        .iter()
        .any(|l| l.contains("listening on"))));
    cx.run_until_parked(); // the Inspector detail renders

    // Quitting with a server running asks first; Cancel keeps the app open.
    view.update(cx, |s, cx| s.request_quit(cx));
    assert!(read(&view, cx, |s| s.quit_confirm));
    cx.run_until_parked(); // the modal renders
    view.update(cx, |s, cx| s.cancel_quit(cx));
    assert!(!read(&view, cx, |s| s.quit_confirm));

    view.update(cx, |s, cx| s.toggle_server_at(index, cx));
    settle(cx, &view, |s| !s.ws.servers[index].running);
}

#[gpui::test]
fn a_port_in_use_becomes_a_toast_with_a_change_port_action(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    let (index, port) = read(&view, cx, |s| (0, s.ws.servers[0].port + s.port_offset));
    let _blocker = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
    view.update(cx, |s, cx| s.toggle_server_at(index, cx));
    settle(cx, &view, |s| s.notice_action.is_some());
    let (msg, action) = read(&view, cx, |s| {
        (
            s.notice.clone().unwrap().1,
            s.notice_action.clone().unwrap(),
        )
    });
    assert!(msg.contains("already in use"), "{msg}");
    assert_eq!(action.label, "Change port");
    cx.run_until_parked(); // the toast button renders
    assert!(!read(&view, cx, |s| s.ws.servers[index].running));

    // Change port opens the Server tab and prompts with the current port.
    let crate::run::ToastAction::ChangePort(server_id) = action.action.clone() else {
        panic!("expected ChangePort, got {:?}", action.action);
    };
    view.update_in(cx, |s, w, cx| s.change_port(&server_id, w, cx));
    assert_eq!(
        read(&view, cx, |s| s.picker.as_ref().map(|p| p.kind)),
        Some(PickerKind::Prompt)
    );
}

// ---- M3.1: workspace lifecycle and server management ---------------------------------------

fn setup_recent(
    cx: &mut TestAppContext,
) -> (
    Entity<Shell>,
    &mut VisualTestContext,
    tempfile::TempDir,
    std::path::PathBuf,
) {
    let tmp = tempfile::tempdir().unwrap();
    let recent = tmp.path().join("recent.json");
    let r2 = recent.clone();
    let (view, cx) = cx.add_window_view(move |w, cx| {
        Shell::new(
            ShellOptions {
                recent_path: Some(r2),
                ..crate::view::test_options()
            },
            w,
            cx,
        )
    });
    cx.run_until_parked();
    (view, cx, tmp, recent)
}

fn notice(view: &Entity<Shell>, cx: &mut VisualTestContext) -> String {
    read(view, cx, |s| {
        s.notice.clone().map(|n| n.1).unwrap_or_default()
    })
}

fn start_first(view: &Entity<Shell>, cx: &mut VisualTestContext) {
    view.update(cx, |s, cx| s.toggle_server_at(0, cx));
    settle(cx, view, |s| s.ws.servers[0].running);
}

#[gpui::test]
fn closing_releases_the_host_and_returns_to_welcome(cx: &mut TestAppContext) {
    let (view, cx, _tmp, recent) = setup_recent(cx);
    let root = read(&view, cx, |s| s.ws.root.clone());
    let host_json = root.join(".fauxfield/run/host.json");
    assert!(host_json.is_file(), "the open workspace is hosted");
    let sock = read(&view, cx, |s| {
        fauxfield_control::read_host(&s.ws.root).map(|h| h.socket)
    })
    .expect("live host");
    view.update(cx, |s, cx| s.guard(crate::lifecycle::Intent::Close, cx));
    assert!(read(&view, cx, |s| s.store.is_none() && s.host.is_none()));
    assert!(!host_json.exists(), "host.json is released");
    assert!(!sock.exists(), "the control socket is released");
    assert_eq!(fauxfield_store::load_recent(&recent)[0], root);
    assert!(read(&view, cx, |s| s.tabs.tabs().is_empty()
        && s.traffic.is_empty()
        && s.problems.is_empty()));
    assert!(root.join(".fauxfield/ui.json").is_file());
    // The window title follows.
    assert_eq!(read(&view, cx, |s| s.window_title_text()), "Fauxfield");
    // Reopening from Recent works in one call.
    view.update(cx, |s, cx| s.open_recent(&root, cx));
    assert!(read(&view, cx, |s| s.store.is_some()));
    assert!(host_json.is_file());
}

#[gpui::test]
fn closing_with_running_servers_confirms_then_stops(cx: &mut TestAppContext) {
    let (view, cx, _tmp, _r) = setup_recent(cx);
    start_first(&view, cx);
    let port = read(&view, cx, |s| s.ws.servers[0].port + s.port_offset);
    view.update(cx, |s, cx| s.guard(crate::lifecycle::Intent::Close, cx));
    assert_eq!(read(&view, cx, |s| s.dialog_name()), Some("close_confirm"));
    assert!(read(&view, cx, |s| s.store.is_some()), "nothing closed yet");
    cx.run_until_parked(); // the modal renders
    view.update(cx, |s, cx| s.cancel_dialog(cx));
    assert!(read(&view, cx, |s| s.store.is_some()
        && s.dialog_name().is_none()));
    view.update(cx, |s, cx| s.guard(crate::lifecycle::Intent::Close, cx));
    view.update(cx, |s, cx| s.confirm_dialog(cx));
    assert!(read(&view, cx, |s| s.store.is_none()));
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
}

#[gpui::test]
fn a_merge_conflict_blocks_closing_and_switching(cx: &mut TestAppContext) {
    let (view, cx, _tmp, _r) = setup_recent(cx);
    let (root, store) = read(&view, cx, |s| (s.ws.root.clone(), s.store.clone().unwrap()));
    store
        .set_str(
            &fauxfield_store::Target::Server("srv_users".into()),
            "docs",
            Some("mine"),
        )
        .unwrap();
    let p = root.join("servers/01-users-api.toml");
    let t = std::fs::read_to_string(&p).unwrap();
    std::fs::write(&p, t.replace("4010", "4011")).unwrap();
    store.reconcile();
    assert!(!store.conflicts().is_empty());
    view.update(cx, |s, cx| s.guard(crate::lifecycle::Intent::Close, cx));
    assert!(read(&view, cx, |s| s.store.is_some()));
    assert_eq!(notice(&view, cx), "Resolve the merge banner first.");
    let other = tempfile::tempdir().unwrap();
    view.update(cx, |s, cx| {
        s.guard(
            crate::lifecycle::Intent::Switch {
                path: other.path().to_path_buf(),
                create: true,
            },
            cx,
        )
    });
    assert_eq!(read(&view, cx, |s| s.ws.root.clone()), root);
}

#[gpui::test]
fn open_and_create_folder_rules_toast_and_confirm(cx: &mut TestAppContext) {
    let (view, cx, _tmp, _r) = setup_recent(cx);
    let root = read(&view, cx, |s| s.ws.root.clone());
    let plain = tempfile::tempdir().unwrap();
    // Open on a folder without fauxfield.toml explains what to do.
    view.update(cx, |s, cx| {
        s.open_folder_chosen(plain.path().to_path_buf(), cx)
    });
    assert!(notice(&view, cx).starts_with("No fauxfield.toml in "));
    let action = read(&view, cx, |s| s.notice_action.clone().unwrap());
    assert_eq!(action.label, "Create workspace here");
    assert_eq!(read(&view, cx, |s| s.ws.root.clone()), root);
    // Create on an existing workspace: toast with "Open it", nothing written.
    view.update(cx, |s, cx| s.create_folder_chosen(root.clone(), cx));
    assert!(notice(&view, cx).ends_with("is already a workspace."));
    assert_eq!(
        read(&view, cx, |s| s.notice_action.clone().unwrap().label),
        "Open it"
    );
    // Create on a non-empty folder asks first and never overwrites.
    std::fs::write(plain.path().join("notes.txt"), "keep").unwrap();
    std::fs::create_dir_all(plain.path().join("environments")).unwrap();
    std::fs::write(
        plain.path().join("environments/local.toml"),
        "name = \"mine\"\n",
    )
    .unwrap();
    view.update(cx, |s, cx| {
        s.create_folder_chosen(plain.path().to_path_buf(), cx)
    });
    assert_eq!(read(&view, cx, |s| s.dialog_name()), Some("create_confirm"));
    assert!(
        !plain.path().join("fauxfield.toml").exists(),
        "nothing written before confirming"
    );
    cx.run_until_parked();
    view.update(cx, |s, cx| s.confirm_dialog(cx));
    assert!(plain.path().join("fauxfield.toml").is_file());
    assert_eq!(
        std::fs::read_to_string(plain.path().join("notes.txt")).unwrap(),
        "keep"
    );
    assert_eq!(
        std::fs::read_to_string(plain.path().join("environments/local.toml")).unwrap(),
        "name = \"mine\"\n"
    );
    assert_eq!(
        read(&view, cx, |s| s.ws.root.canonicalize().unwrap()),
        plain.path().canonicalize().unwrap()
    );
}

#[gpui::test]
fn recent_workspaces_list_remove_clear_and_missing(cx: &mut TestAppContext) {
    let (view, cx, tmp, recent) = setup_recent(cx);
    let root = read(&view, cx, |s| s.ws.root.clone());
    let gone = tmp.path().join("gone");
    fauxfield_store::push_recent(&recent, &gone).unwrap();
    fauxfield_store::push_recent(&recent, &root).unwrap();
    view.update(cx, |s, cx| {
        s.remove_recent(std::path::Path::new("/nope"), cx)
    });
    let entries = read(&view, cx, |s| s.recent_entries());
    assert_eq!(entries[0].path, root);
    assert!(entries[0].exists);
    assert_eq!(entries[1].path, gone);
    assert!(!entries[1].exists);
    // A missing entry cannot be opened.
    view.update(cx, |s, cx| s.open_recent(&gone, cx));
    assert!(notice(&view, cx).ends_with("no longer exists"));
    assert_eq!(read(&view, cx, |s| s.ws.root.clone()), root);
    // Remove edits recent.json only.
    view.update(cx, |s, cx| s.remove_recent(&gone, cx));
    assert!(!fauxfield_store::load_recent(&recent).contains(&gone));
    assert!(root.join("fauxfield.toml").is_file());
    view.update(cx, |s, cx| s.clear_recent(cx));
    assert!(fauxfield_store::load_recent(&recent).is_empty());
}

#[gpui::test]
fn renaming_the_workspace_validates_inline_and_can_be_undone(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    let root = read(&view, cx, |s| s.ws.root.clone());
    let original = read(&view, cx, |s| s.ws.name.clone());
    view.update_in(cx, |s, w, cx| s.run_action_id("rename_workspace", w, cx));
    cx.run_until_parked();
    assert_eq!(
        read(&view, cx, |s| s.picker.as_ref().map(|p| p.query.clone())),
        Some(original.clone()),
        "prefilled with the current name"
    );
    // Invalid input keeps the prompt open with an inline message, not a toast.
    for _ in 0..original.chars().count() {
        view.update(cx, |s, cx| s.picker_backspace(cx));
    }
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s.picker.is_some()));
    assert!(read(&view, cx, |s| s.lc.prompt_error.clone()).is_some());
    assert_eq!(notice(&view, cx), "");
    type_prompt(&view, cx, "My Mocks");
    assert_eq!(read(&view, cx, |s| s.ws.name.clone()), "My Mocks");
    assert_eq!(
        read(&view, cx, |s| s.window_title_text()),
        "Fauxfield — My Mocks"
    );
    let text = view_text(&view, cx);
    assert!(text.contains("name = \"My Mocks\""));
    // The folder on disk is untouched.
    assert!(root.is_dir());
    // Undo restores the old name, redo applies it again.
    view.update_in(cx, |s, w, cx| s.run_action_id("undo", w, cx));
    cx.run_until_parked();
    assert_eq!(read(&view, cx, |s| s.ws.name.clone()), original);
}

fn view_text(view: &Entity<Shell>, cx: &mut VisualTestContext) -> String {
    read(view, cx, |s| {
        s.store.as_ref().unwrap().text("fauxfield.toml").unwrap()
    })
}

#[gpui::test]
fn server_fields_host_prefix_enabled_and_hint_are_editable(cx: &mut TestAppContext) {
    use crate::run::ToastAction;
    let (view, cx) = setup(cx, None);
    let id = "srv_users".to_string();
    // Host: invalid input stays in the prompt; 0.0.0.0 toasts once.
    view.update_in(cx, |s, w, cx| s.change_host(&id, w, cx));
    cx.run_until_parked();
    for _ in 0..9 {
        view.update(cx, |s, cx| s.picker_backspace(cx));
    }
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s.lc.prompt_error.clone().unwrap()).contains("empty"));
    type_prompt(&view, cx, "0.0.0.0");
    assert_eq!(
        read(&view, cx, |s| s.ws.find_server(&id).unwrap().host.clone()),
        "0.0.0.0"
    );
    assert_eq!(notice(&view, cx), "Reachable from your network.");
    // Prefix: validation text, set, then empty removes the key.
    view.update_in(cx, |s, w, cx| s.change_prefix(&id, w, cx));
    cx.run_until_parked();
    for _ in 0..30 {
        view.update(cx, |s, cx| s.picker_backspace(cx));
    }
    type_prompt(&view, cx, "api");
    assert!(read(&view, cx, |s| s.lc.prompt_error.clone().unwrap()).contains("start with"));
    for _ in 0..3 {
        view.update(cx, |s, cx| s.picker_backspace(cx));
    }
    type_prompt(&view, cx, "/v1/{{ vars.x }}");
    assert_eq!(
        read(&view, cx, |s| s.ws.find_server(&id).unwrap().prefix.clone()),
        "/v1/{{ vars.x }}"
    );
    view.update_in(cx, |s, w, cx| s.change_prefix(&id, w, cx));
    cx.run_until_parked();
    for _ in 0..30 {
        view.update(cx, |s, cx| s.picker_backspace(cx));
    }
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        read(&view, cx, |s| s.ws.find_server(&id).unwrap().prefix.clone()),
        ""
    );
    // 404 hint toggles in the file.
    view.update(cx, |s, cx| s.toggle_unmatched_hint(&id, cx));
    assert!(!read(&view, cx, |s| s
        .ws
        .find_server(&id)
        .unwrap()
        .unmatched_hint));
    view.update(cx, |s, cx| s.toggle_unmatched_hint(&id, cx));
    assert!(read(&view, cx, |s| s
        .ws
        .find_server(&id)
        .unwrap()
        .unmatched_hint));
    // Enabled: disabled servers are skipped by Run all and refuse an explicit start.
    view.update(cx, |s, cx| s.toggle_enabled(&id, cx));
    assert!(!read(&view, cx, |s| s.ws.find_server(&id).unwrap().enabled));
    let ix = read(&view, cx, |s| {
        s.ws.servers.iter().position(|x| x.id == id).unwrap()
    });
    view.update(cx, |s, cx| s.run_all(cx));
    settle(cx, &view, |s| s.ws.servers.iter().any(|x| x.running));
    assert!(
        !read(&view, cx, |s| s.ws.servers[ix].running),
        "Run all skips it"
    );
    view.update(cx, |s, cx| s.toggle_server_at(ix, cx));
    settle(cx, &view, |s| s.notice_action.is_some());
    assert!(notice(&view, cx).ends_with("is disabled. Enable it first."));
    assert_eq!(
        read(&view, cx, |s| s.notice_action.clone().unwrap().action),
        ToastAction::EnableAndStart(id.clone())
    );
    view.update_in(cx, |s, w, cx| s.run_toast_action(w, cx));
    settle(cx, &view, |s| s.ws.servers[ix].running);
    assert!(read(&view, cx, |s| s.ws.servers[ix].enabled));
    // Disabling a running server stops it.
    view.update(cx, |s, cx| s.toggle_enabled(&id, cx));
    assert!(notice(&view, cx).ends_with("disabled and stopped"));
    assert!(!read(&view, cx, |s| s.ws.servers[ix].running));
}

#[gpui::test]
fn deleting_a_server_asks_first_and_duplicates_stay_stopped(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    let id = "srv_users".to_string();
    start_first(&view, cx);
    // Duplicate while running does not start the copy.
    view.update(cx, |s, cx| s.run_menu_for("duplicate", &id, cx));
    let copy = read(&view, cx, |s| {
        s.ws.servers
            .iter()
            .find(|x| x.name.ends_with(" copy"))
            .cloned()
    })
    .expect("a copy exists");
    assert!(!copy.running);
    assert_ne!(copy.id, id);
    // Delete asks, naming the server; Cancel keeps it.
    view.update(cx, |s, cx| s.run_menu_for("delete", &id, cx));
    assert_eq!(read(&view, cx, |s| s.dialog_name()), Some("delete_confirm"));
    cx.run_until_parked();
    view.update(cx, |s, cx| s.cancel_dialog(cx));
    assert!(read(&view, cx, |s| s.ws.find_server(&id).is_some()));
    view.update(cx, |s, cx| s.run_menu_for("delete", &id, cx));
    view.update(cx, |s, cx| s.confirm_dialog(cx));
    assert!(read(&view, cx, |s| s.ws.find_server(&id).is_none()));
}

#[gpui::test]
fn the_route_method_dropdown_changes_the_verb(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, _cx| {
        s.open_tab("rt_users_get", TabKind::Route, "Get user", false)
    });
    assert_eq!(
        read(&view, cx, |s| s
            .ws
            .find_route("rt_users_get")
            .unwrap()
            .1
            .method
            .clone()),
        "GET"
    );
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked(); // render, so the menu gating sees the open tab
    view.update_in(cx, |s, w, cx| s.run_action_id("edit_route_method", w, cx));
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s.lc.method_menu.is_some()));
    cx.run_until_parked(); // the dropdown renders
    view.update(cx, |s, cx| s.set_route_method("rt_users_get", "patch", cx));
    assert!(read(&view, cx, |s| s.lc.method_menu.is_none()));
    assert_eq!(
        read(&view, cx, |s| s
            .ws
            .find_route("rt_users_get")
            .unwrap()
            .1
            .method
            .clone()),
        "PATCH"
    );
}

#[gpui::test]
fn a_vanished_active_environment_falls_back(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, _cx| s.active_env = "deleted-env".into());
    view.update(cx, |s, cx| s.reload(cx));
    let env = read(&view, cx, |s| s.active_env.clone());
    assert!(!env.is_empty() && env != "deleted-env", "{env}");
}

// ---- M3.2: application menu, About box, Help links ----------------------------------------

#[derive(Clone, Default)]
struct Recorder(std::rc::Rc<std::cell::RefCell<Vec<String>>>);

impl crate::menu_shell::UrlOpener for Recorder {
    fn open(&self, url: &str, _: &mut gpui::App) {
        self.0.borrow_mut().push(url.to_string());
    }
}

struct CountingSink(std::rc::Rc<std::cell::Cell<usize>>);

impl crate::menu_shell::MenuSink for CountingSink {
    fn apply(&mut self, _: &[crate::menu::MenuDef], _: &mut gpui::App) {
        self.0.set(self.0.get() + 1);
    }
}

#[gpui::test]
fn help_items_open_exact_urls_through_the_seam(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    let rec = Recorder::default();
    view.update(cx, |s, _| s.set_url_opener(Box::new(rec.clone())));
    for id in ["open_documentation", "open_release_notes", "report_issue"] {
        view.update_in(cx, |s, w, cx| s.run_action_id(id, w, cx));
    }
    let repo = crate::menu::links::REPOSITORY;
    assert_eq!(
        *rec.0.borrow(),
        vec![
            format!("{repo}#readme"),
            format!("{repo}/blob/main/CHANGELOG.md"),
            format!("{repo}/issues/new"),
        ]
    );
}

#[gpui::test]
fn about_opens_copies_and_closes_with_esc_or_enter(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    view.update_in(cx, |s, w, cx| s.run_action_id("show_about", w, cx));
    cx.run_until_parked();
    assert_eq!(read(&view, cx, |s| s.dialog_name()), Some("about"));
    // A second About refocuses, it does not stack.
    view.update_in(cx, |s, w, cx| s.run_action_id("show_about", w, cx));
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s.about_open));

    view.update(cx, |s, cx| s.copy_version_info(cx));
    let text = cx.read_from_clipboard().and_then(|c| c.text()).unwrap();
    assert_eq!(text, Shell::build_info().copy_text());
    assert!(text.starts_with("Fauxfield "));
    let toast = read(&view, cx, |s| s.notice.as_ref().map(|(_, m)| m.clone()));
    assert_eq!(toast.as_deref(), Some("Version info copied"));
    assert!(read(&view, cx, |s| s.about_open), "Copy keeps the box open");

    cx.simulate_keystrokes("escape");
    assert!(!read(&view, cx, |s| s.about_open));
    view.update(cx, |s, cx| s.show_about(cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    assert!(!read(&view, cx, |s| s.about_open));
}

#[gpui::test]
fn open_recent_with_a_missing_folder_toasts(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    cx.update(|w, cx| {
        w.dispatch_action(
            Box::new(crate::actions::OpenRecent {
                path: "/definitely/not/here".into(),
            }),
            cx,
        )
    });
    cx.run_until_parked();
    let toast = read(&view, cx, |s| s.notice.as_ref().map(|(_, m)| m.clone()));
    assert_eq!(
        toast.as_deref(),
        Some("/definitely/not/here no longer exists")
    );
}

#[gpui::test]
fn identical_contexts_apply_the_native_menu_once(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    let n = std::rc::Rc::new(std::cell::Cell::new(0));
    view.update(cx, |s, cx| {
        s.menu_style = crate::menu::MenuStyle::Native;
        s.set_menu_sink(Box::new(CountingSink(n.clone())));
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(n.get(), 1);
    for _ in 0..3 {
        view.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
    }
    assert_eq!(
        n.get(),
        1,
        "an unchanged model must not call set_menus again"
    );
    view.update(cx, |s, cx| s.toggle_left(cx));
    cx.run_until_parked();
    assert_eq!(n.get(), 2, "the dock label changed");
}

// ---- M3.2: in-window menu bar ---------------------------------------------------------------

fn welcome(cx: &mut TestAppContext) -> (Entity<Shell>, &mut VisualTestContext) {
    let (view, cx) = cx.add_window_view(|w, cx| Shell::new(ShellOptions::default(), w, cx));
    cx.run_until_parked();
    (view, cx)
}

fn in_window(view: &Entity<Shell>, cx: &mut VisualTestContext) {
    view.update(cx, |s, cx| {
        s.menu_style = crate::menu::MenuStyle::InWindow;
        cx.notify();
    });
    cx.run_until_parked();
}

#[gpui::test]
fn alt_mnemonic_opens_arrows_skip_disabled_and_esc_layers(cx: &mut TestAppContext) {
    let (view, cx) = welcome(cx);
    in_window(&view, cx);
    cx.simulate_keystrokes("alt-f");
    assert_eq!(read(&view, cx, |s| s.bar.open), Some(0));
    // Welcome: New Server/Route/Folder are disabled, so Down lands on Create Workspace.
    cx.simulate_keystrokes("down");
    let first = read(&view, cx, |s| s.bar.hi[0]).unwrap();
    let entries = read(&view, cx, |s| s.bar_entries(0));
    assert!(matches!(
        &entries[first],
        crate::menu::MenuEntry::Action {
            id: "create_workspace",
            ..
        }
    ));
    // Right switches to Edit, Left comes back.
    cx.simulate_keystrokes("right");
    assert_eq!(read(&view, cx, |s| s.bar.open), Some(1));
    cx.simulate_keystrokes("left");
    assert_eq!(read(&view, cx, |s| s.bar.open), Some(0));
    // Open Recent is a submenu: type-ahead to it, Right opens, Esc closes one level at a time.
    cx.simulate_keystrokes("o o");
    cx.simulate_keystrokes("right");
    assert!(read(&view, cx, |s| s.bar.sub.is_some()));
    cx.simulate_keystrokes("escape");
    assert!(read(&view, cx, |s| s.bar.sub.is_none() && s.bar.open.is_some()));
    cx.simulate_keystrokes("escape");
    assert_eq!(read(&view, cx, |s| s.bar.open), None);
}

#[gpui::test]
fn enter_runs_the_highlighted_item_and_closes(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    in_window(&view, cx);
    let n = read(&view, cx, |s| s.bar_menus().len());
    assert_eq!(n, 7, "no Fauxfield menu in the in-window bar");
    view.update(cx, |s, cx| s.bar_open_menu(2, cx)); // View
    cx.simulate_keystrokes("down down enter"); // Quick Search…, Command Palette…
    assert_eq!(read(&view, cx, |s| s.bar.open), None);
    cx.run_until_parked();
    assert_eq!(
        read(&view, cx, |s| s.picker.as_ref().map(|p| p.kind)),
        Some(PickerKind::Palette)
    );
}

#[gpui::test]
fn hovering_another_title_switches_the_open_menu(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    in_window(&view, cx);
    view.update(cx, |s, cx| s.bar_open_menu(0, cx));
    view.update(cx, |s, cx| s.bar_open_menu(3, cx)); // what the hover handler calls
    assert_eq!(read(&view, cx, |s| s.bar.open), Some(3));
}

#[gpui::test]
fn disabled_items_refuse_and_say_why(cx: &mut TestAppContext) {
    let (view, cx) = welcome(cx);
    cx.run_until_parked();
    let err = view
        .update_in(cx, |s, w, cx| {
            s.run_menu_path(&["File".into(), "Close Workspace".into()], w, cx)
        })
        .unwrap_err();
    assert_eq!(
        err,
        "menu item disabled: Close Workspace (no workspace open)"
    );
    let err = view
        .update_in(cx, |s, w, cx| {
            s.run_menu_path(&["File".into(), "Open Recent".into(), "Nope".into()], w, cx)
        })
        .unwrap_err();
    assert_eq!(err, "no menu item File/Open Recent/Nope");
}

#[gpui::test]
fn narrow_windows_collapse_to_the_hamburger(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    in_window(&view, cx);
    cx.simulate_resize(gpui::size(gpui::px(900.), gpui::px(700.)));
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s.bar_collapsed()));
    let top = read(&view, cx, |s| s.bar_entries(0));
    assert_eq!(top.len(), 7);
    assert!(matches!(&top[0], crate::menu::MenuEntry::Submenu { label, .. } if label == "File"));
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(700.)));
    cx.run_until_parked();
    assert!(!read(&view, cx, |s| s.bar_collapsed()));
}

#[gpui::test]
fn sidebar_tabs_switch_reveal_and_guard_hidden_rows(cx: &mut TestAppContext) {
    use crate::tree::SidebarTab;
    let (view, cx) = setup(cx, None);
    assert_eq!(read(&view, cx, |s| s.sidebar_tab), SidebarTab::Servers);

    // The shipped binding switches tabs and reopens a hidden dock.
    view.update(cx, |s, cx| s.toggle_left(cx));
    cx.simulate_keystrokes("secondary-alt-2");
    assert_eq!(read(&view, cx, |s| s.sidebar_tab), SidebarTab::Environments);
    assert!(read(&view, cx, |s| s.layout.left_open));
    assert_eq!(
        read(&view, cx, |s| s.layout.sidebar_tab.clone()),
        "environments"
    );

    // Reveal moves the tab to the row's owner.
    view.update(cx, |s, _| s.reveal_row("rt_health"));
    assert_eq!(read(&view, cx, |s| s.sidebar_tab), SidebarTab::Servers);
    view.update(cx, |s, _| s.reveal_row("ds:users"));
    assert_eq!(read(&view, cx, |s| s.sidebar_tab), SidebarTab::Datasets);

    // A selected row on a hidden tab is not actionable.
    view.update(cx, |s, _| s.reveal_row("rt_health"));
    view.update(cx, |s, cx| s.set_sidebar_tab(SidebarTab::Datasets, cx));
    assert_eq!(read(&view, cx, |s| s.selected_visible()), None);
    view.update_in(cx, |s, w, cx| s.run_action_id("delete", w, cx));
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s.ws.find_route("rt_health").is_some()));

    // Editor-tab activation does not change the sidebar tab.
    view.update(cx, |s, cx| {
        s.open_tab("rt_users_get", TabKind::Route, "Get user", false);
        s.set_sidebar_tab(SidebarTab::Environments, cx);
        s.activate_tab(0, cx);
    });
    assert_eq!(read(&view, cx, |s| s.sidebar_tab), SidebarTab::Environments);
}

#[gpui::test]
fn moves_drops_and_folder_flags_edit_the_files(cx: &mut TestAppContext) {
    use crate::tree::MenuAct;
    let (view, cx) = setup(cx, None);
    let order = |view: &Entity<Shell>, cx: &mut VisualTestContext| {
        read(view, cx, |s| {
            let u = s.ws.find_server("srv_users").unwrap();
            (
                u.folders[0]
                    .routes
                    .iter()
                    .map(|r| r.id.clone())
                    .collect::<Vec<_>>(),
                u.routes.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
            )
        })
    };
    let (inside, _) = order(&view, cx);
    assert!(inside.len() >= 2);
    // Alt+Down moves the first route after the second.
    view.update(cx, |s, cx| s.move_row(&inside[0], 1, cx));
    let (after, _) = order(&view, cx);
    assert_eq!((&after[0], &after[1]), (&inside[1], &inside[0]));
    // Dropping a route on its Server row moves it to the root.
    view.update(cx, |s, cx| s.drop_row(&inside[0], "srv_users", cx));
    let (folder, root) = order(&view, cx);
    assert!(!folder.contains(&inside[0]) && root.last() == Some(&inside[0]));
    // Dropping it on the Folder row puts it back; cross-server drops do nothing.
    view.update(cx, |s, cx| s.drop_row(&inside[0], "fld_users", cx));
    assert!(order(&view, cx).0.contains(&inside[0]));
    view.update(cx, |s, cx| s.drop_row(&inside[0], "srv_payments", cx));
    assert!(order(&view, cx).0.contains(&inside[0]));
    // Folder flags: disable all routes and start collapsed are written, defaults are removed.
    view.update_in(cx, |s, w, cx| {
        s.open_menu("fld_users", 0.0, 0.0, cx);
        s.run_menu(MenuAct::FolderToggleEnabled, w, cx);
        s.open_menu("fld_users", 0.0, 0.0, cx);
        s.run_menu(MenuAct::FolderToggleCollapsed, w, cx);
    });
    let (en, col) = read(&view, cx, |s| {
        let f = &s.ws.find_server("srv_users").unwrap().folders[0];
        (f.enabled, f.collapsed)
    });
    assert!(!en && col);
    // Delete with routes removes them.
    view.update_in(cx, |s, w, cx| {
        s.open_menu("fld_users", 0.0, 0.0, cx);
        s.run_menu(MenuAct::DeleteWithRoutes, w, cx);
    });
    assert!(read(&view, cx, |s| {
        s.ws.find_server("srv_users").unwrap().folders.is_empty()
            && s.ws.find_route(&inside[0]).is_none()
    }));
}

#[gpui::test]
fn response_menu_body_editor_and_headers(cx: &mut TestAppContext) {
    use crate::tree::MenuAct;
    let (view, cx) = setup(cx, None);
    let resp = |view: &Entity<Shell>, cx: &mut VisualTestContext| {
        read(view, cx, |s| {
            s.ws.find_route("rt_users_get")
                .unwrap()
                .1
                .responses
                .iter()
                .map(|r| (r.id.clone(), r.default, r.name.clone()))
                .collect::<Vec<_>>()
        })
    };
    let before = resp(&view, cx);
    assert!(before[0].1 && !before[1].1);
    // Set as default, rename through the menu's prompt, then delete.
    view.update_in(cx, |s, w, cx| {
        s.open_response_menu("rsp_users_get_2", 0.0, 0.0, cx);
        s.run_menu(MenuAct::SetDefaultResponse, w, cx);
    });
    assert!(!resp(&view, cx)[0].1 && resp(&view, cx)[1].1);
    view.update_in(cx, |s, w, cx| {
        s.open_response_menu("rsp_users_get_2", 0.0, 0.0, cx);
        s.run_menu(MenuAct::Rename, w, cx);
    });
    view.update(cx, |s, cx| s.edit_select_all(cx));
    type_prompt(&view, cx, "Gone");
    assert_eq!(resp(&view, cx)[1].2, "Gone");
    view.update_in(cx, |s, w, cx| {
        s.open_response_menu("rsp_users_get_1", 0.0, 0.0, cx);
        s.run_menu(MenuAct::Delete, w, cx);
    });
    assert_eq!(resp(&view, cx).len(), 1);

    // Inline fields: status code and delay.
    use crate::response_pane::{InlineTarget as I, Nav};
    let r = "rsp_users_get_2".to_string();
    let type_field = |view: &Entity<Shell>, cx: &mut VisualTestContext, t: I, text: &str| {
        view.update_in(cx, |s, w, cx| {
            s.begin_inline(t, "", false, w, cx);
            s.inline.as_mut().unwrap().buf = crate::editor::TextBuf::new(text);
            s.commit_inline(Nav::Stay, w, cx);
        });
    };
    type_field(&view, cx, I::Status(r.clone()), "503");
    type_field(&view, cx, I::Delay(r.clone()), "250");
    type_field(&view, cx, I::Status(r.clone()), "42"); // rejected
    let x = read(&view, cx, |s| s.find_response(&r).unwrap().2);
    assert_eq!((x.status, x.delay.as_str()), (503, "250"));
    // Body from a file, then back to inline.
    view.update_in(cx, |s, w, cx| s.set_body_file_mode(&r, true, w, cx));
    view.update_in(cx, |s, w, cx| {
        s.inline.as_mut().unwrap().buf = crate::editor::TextBuf::new("bodies/user.json");
        s.commit_inline(Nav::Stay, w, cx);
    });
    assert_eq!(
        read(&view, cx, |s| s.find_response(&r).unwrap().2.body_file),
        Some("bodies/user.json".to_string())
    );
    view.update_in(cx, |s, w, cx| s.set_body_file_mode(&r, false, w, cx));
    assert_eq!(
        read(&view, cx, |s| s.find_response(&r).unwrap().2.body_file),
        None
    );

    // Body editor: edit, commit, JSON auto content type, format.
    let new = view.update(cx, |s, cx| {
        let id = s
            .store
            .as_ref()
            .unwrap()
            .create_response("rt_users_get")
            .unwrap();
        s.reload(cx);
        id
    });
    view.update_in(cx, |s, w, cx| {
        s.begin_body_edit(&new, w, cx);
        let ed = s.body_editors.get(&new).unwrap().clone();
        ed.update(cx, |e, cx| {
            e.replace_all("{\"a\":[1,2]}", cx);
        });
    });
    // Cmd+S saves the body; Cmd+Enter does not.
    // The test window draws a different Route, so the editor is not in the focus tree.
    view.update_in(cx, |s, w, cx| {
        w.focus(&s.focus);
        s.run_action_id("send_test", w, cx)
    });
    cx.run_until_parked();
    assert!(
        read(&view, cx, |s| s.body_edit.is_some()),
        "Send Test must not save"
    );
    view.update_in(cx, |s, w, cx| s.run_action_id("save", w, cx));
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s.body_edit.is_none()), "Cmd+S saves");
    let (body, ct) = read(&view, cx, |s| {
        let x = s.find_response(&new).unwrap().2;
        (x.body, x.content_type)
    });
    assert_eq!(
        (body.as_str(), ct.as_str()),
        ("{\"a\":[1,2]}", "application/json")
    );
    view.update(cx, |s, cx| s.format_body(&new, cx));
    assert!(read(&view, cx, |s| s
        .find_response(&new)
        .unwrap()
        .2
        .body
        .contains("\n  \"a\"")));
}

#[gpui::test]
fn prompt_input_supports_caret_selection_and_clipboard(cx: &mut TestAppContext) {
    use crate::shell::PromptAction;
    let (view, cx) = setup(cx, None);
    let q = |view: &Entity<Shell>, cx: &mut VisualTestContext| {
        read(view, cx, |s| s.picker.as_ref().map(|p| p.query.clone())).unwrap()
    };
    view.update_in(cx, |s, w, cx| {
        s.begin_prompt(PromptAction::RenameWorkspace, "", w, cx)
    });
    cx.run_until_parked();
    cx.simulate_input("abc");
    cx.simulate_keystrokes("left left");
    cx.simulate_input("x");
    assert_eq!(q(&view, cx), "axbc");
    cx.simulate_keystrokes("shift-end");
    cx.simulate_input("Z");
    assert_eq!(q(&view, cx), "axZ");
    cx.simulate_keystrokes("home delete");
    assert_eq!(q(&view, cx), "xZ");
    cx.simulate_keystrokes("shift-right");
    cx.simulate_keystrokes("backspace");
    assert_eq!(q(&view, cx), "Z");
    // Select all, copy, cut, paste.
    cx.simulate_keystrokes("secondary-a secondary-c secondary-x");
    assert_eq!(q(&view, cx), "");
    cx.simulate_keystrokes("secondary-v secondary-v");
    assert_eq!(q(&view, cx), "ZZ");
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("q");
    assert_eq!(q(&view, cx), "q");
}

#[gpui::test]
fn body_editor_helps_typing_and_formats_on_paste_and_save(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    let new = view.update(cx, |s, cx| {
        let id = s
            .store
            .as_ref()
            .unwrap()
            .create_response("rt_users_get")
            .unwrap();
        s.reload(cx);
        id
    });
    let text = |view: &Entity<Shell>, cx: &mut VisualTestContext| {
        let ed = read(view, cx, |s| s.body_editors.get(&new).unwrap().clone());
        ed.read_with(cx, |e, _| e.text().to_string())
    };
    // Show the new Response's Body in its Route tab so the editor is part of the window.
    view.update(cx, |s, cx| {
        s.open_tab("rt_users_get", TabKind::Route, "Get user", false);
        let n = s.ws.find_route("rt_users_get").unwrap().1.responses.len();
        s.resp_sel.insert("rt_users_get".into(), n - 1);
        s.resp_sub.insert(new.clone(), 0);
        cx.notify();
    });
    cx.run_until_parked();
    view.update_in(cx, |s, w, cx| s.begin_body_edit(&new, w, cx));
    cx.run_until_parked();
    // `{` Enter `"a"`: brackets and quotes close, Enter indents.
    cx.simulate_input("{");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("\"a\"");
    assert_eq!(text(&view, cx), "{\n  \"a\"\n}");
    // Pasting a valid JSON document over a fully selected body formats it.
    cx.update(|_, cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("{\"k\":[1,2]}".into()));
    });
    cx.simulate_keystrokes("secondary-a secondary-v");
    assert_eq!(text(&view, cx), "{\n  \"k\": [\n    1,\n    2\n  ]\n}");
    // Format on save rewrites a compact body when the body is committed.
    view.update(cx, |s, cx| {
        s.layout.format_on_save = true;
        let ed = s.body_editors.get(&new).unwrap().clone();
        ed.update(cx, |e, cx| {
            e.replace_all("{\"x\":1}", cx);
        });
        s.commit_body_edit(cx);
    });
    assert_eq!(
        read(&view, cx, |s| s.find_response(&new).unwrap().2.body),
        "{\n  \"x\": 1\n}"
    );
}

#[gpui::test]
fn headers_table_edits_completes_and_navigates(cx: &mut TestAppContext) {
    use crate::header_catalog::Col;
    use crate::response_pane::InlineTarget as I;
    let (view, cx) = setup(cx, None);
    let r = view.update(cx, |s, cx| {
        let id = s
            .store
            .as_ref()
            .unwrap()
            .create_response("rt_users_get")
            .unwrap();
        s.reload(cx);
        id
    });
    let key = |view: &Entity<Shell>, cx: &mut VisualTestContext, k: &str, ch: Option<&str>| {
        let mut ks = gpui::Keystroke::parse(k).unwrap();
        ks.key_char = ch.map(String::from);
        let ev = gpui::KeyDownEvent {
            keystroke: ks,
            is_held: false,
        };
        view.update_in(cx, |s, w, cx| s.on_field_key(&ev, w, cx));
    };
    let typed = |view: &Entity<Shell>, cx: &mut VisualTestContext, text: &str| {
        for c in text.chars() {
            let c = c.to_string();
            key(view, cx, &c, Some(&c));
        }
    };
    let headers = |view: &Entity<Shell>, cx: &mut VisualTestContext| {
        read(view, cx, |s| {
            let x = s.find_response(&r).unwrap().2;
            (x.content_type, x.headers)
        })
    };
    let active = |view: &Entity<Shell>, cx: &mut VisualTestContext| {
        read(view, cx, |s| s.inline.as_ref().map(|i| i.target.clone()))
    };
    let hdr = |k: &str, v: &str| (k.to_string(), v.to_string());

    // Key completion: "content-ty" offers Content-Type first; Enter accepts it and moves to
    // the value, whose completions are the MIME types; "json" + Enter accepts application/json.
    view.update_in(cx, |s, w, cx| s.begin_header_cell(&r, 0, Col::Key, w, cx));
    typed(&view, cx, "content-ty");
    assert_eq!(
        read(&view, cx, |s| s.inline_suggestions()[0].label),
        "Content-Type"
    );
    key(&view, cx, "enter", None);
    assert_eq!(
        active(&view, cx),
        Some(I::HeaderValue {
            response: r.clone(),
            row: 0
        })
    );
    assert!(read(&view, cx, |s| s.pending_header.is_some()));
    assert!(read(&view, cx, |s| {
        s.inline_suggestions()
            .iter()
            .any(|x| x.label == "text/html")
    }));
    typed(&view, cx, "json");
    // "json" is only a substring of the types: Down chooses one explicitly.
    key(&view, cx, "down", None);
    key(&view, cx, "enter", None);
    assert_eq!(headers(&view, cx).0, "application/json");
    assert_eq!(active(&view, cx), None);

    // Tab walks Key -> Value -> next row's Key and opens new rows as it goes.
    view.update_in(cx, |s, w, cx| s.begin_header_cell(&r, 1, Col::Key, w, cx));
    typed(&view, cx, "x-a");
    key(&view, cx, "tab", None);
    typed(&view, cx, "1");
    key(&view, cx, "tab", None);
    assert_eq!(
        active(&view, cx),
        Some(I::HeaderKey {
            response: r.clone(),
            row: 2
        })
    );
    typed(&view, cx, "x-b");
    key(&view, cx, "tab", None);
    typed(&view, cx, "2");
    key(&view, cx, "shift-tab", None);
    assert_eq!(
        active(&view, cx),
        Some(I::HeaderKey {
            response: r.clone(),
            row: 2
        })
    );
    key(&view, cx, "escape", None);
    assert_eq!(
        headers(&view, cx),
        (
            "application/json".into(),
            vec![hdr("x-a", "1"), hdr("x-b", "2")].into_iter().collect()
        )
    );

    // Renaming keeps the row in place; a duplicate name is refused.
    view.update_in(cx, |s, w, cx| s.begin_header_cell(&r, 1, Col::Key, w, cx));
    typed(&view, cx, "x-first");
    key(&view, cx, "enter", None);
    key(&view, cx, "escape", None);
    let order = |view: &Entity<Shell>, cx: &mut VisualTestContext| {
        headers(view, cx).1.into_iter().collect::<Vec<_>>()
    };
    assert_eq!(order(&view, cx), vec![hdr("x-first", "1"), hdr("x-b", "2")]);
    view.update_in(cx, |s, w, cx| s.begin_header_cell(&r, 2, Col::Key, w, cx));
    typed(&view, cx, "X-FIRST");
    key(&view, cx, "enter", None);
    assert_eq!(order(&view, cx), vec![hdr("x-first", "1"), hdr("x-b", "2")]);

    // Escape closes the popup first and only then cancels; Backspace edits the text.
    view.update_in(cx, |s, w, cx| s.begin_header_cell(&r, 3, Col::Key, w, cx));
    typed(&view, cx, "cac");
    assert!(read(&view, cx, |s| !s.inline_suggestions().is_empty()));
    key(&view, cx, "escape", None);
    assert!(read(&view, cx, |s| s.inline.is_some()
        && s.inline_suggestions().is_empty()));
    // Editing the text reopens the popup, so it takes two Escapes again.
    key(&view, cx, "backspace", None);
    assert!(read(&view, cx, |s| !s.inline_suggestions().is_empty()));
    key(&view, cx, "escape", None);
    key(&view, cx, "escape", None);
    assert_eq!(active(&view, cx), None);
    assert_eq!(order(&view, cx).len(), 2);

    // Removing rows: Content-Type clears the field, others the header.
    view.update(cx, |s, cx| s.remove_header_row(&r, 0, cx));
    assert_eq!(headers(&view, cx).0, "");
    view.update(cx, |s, cx| s.remove_header_row(&r, 0, cx));
    assert_eq!(order(&view, cx), vec![hdr("x-b", "2")]);
}

#[gpui::test]
fn m5_rules_modes_reorder_presets_and_sample(cx: &mut TestAppContext) {
    use crate::tree::MenuAct;
    let (view, cx) = setup(cx, None);
    let ids = |view: &Entity<Shell>, cx: &mut VisualTestContext| {
        read(view, cx, |s| {
            s.ws.find_route("rt_users_get")
                .unwrap()
                .1
                .responses
                .iter()
                .map(|r| r.id.clone())
                .collect::<Vec<_>>()
        })
    };
    let before = ids(&view, cx);
    assert!(before.len() >= 2);
    let first = before[0].clone();

    // Rules: add, edit through the pickers, validate, remove.
    view.update(cx, |s, cx| s.add_rule(&first, cx));
    let rules = |view: &Entity<Shell>, cx: &mut VisualTestContext| {
        read(view, cx, |s| s.find_response(&first).unwrap().2.rules)
    };
    assert_eq!(rules(&view, cx).len(), 1);
    view.update_in(cx, |s, w, cx| {
        s.open_picker_menu(
            &format!("{first}#0"),
            crate::tree::RowKind::RuleOp,
            0.,
            0.,
            cx,
        );
        let i = fauxfield_model::rules::OPS
            .iter()
            .position(|o| *o == "regex")
            .unwrap();
        s.run_menu(MenuAct::SetRuleOp(i), w, cx);
    });
    assert_eq!(rules(&view, cx)[0].op, "regex");
    view.update(cx, |s, cx| {
        s.edit_rule(&first, 0, cx, |r| {
            r.value = Some(toml::Value::String("(".into()))
        })
    });
    assert!(fauxfield_model::rules::check_rule(&rules(&view, cx)[0]).is_err());
    view.update(cx, |s, cx| s.edit_rule(&first, 0, cx, |r| r.negate = true));
    assert!(rules(&view, cx)[0].negate);

    // Response mode, disable, reorder, preset.
    view.update_in(cx, |s, w, cx| {
        s.open_picker_menu(
            "rt_users_get",
            crate::tree::RowKind::ResponseMode,
            0.,
            0.,
            cx,
        );
        s.run_menu(MenuAct::SetResponseMode(1), w, cx);
    });
    assert_eq!(
        read(&view, cx, |s| s
            .ws
            .find_route("rt_users_get")
            .unwrap()
            .1
            .response_mode
            .clone()),
        "sequential"
    );
    view.update(cx, |s, cx| s.toggle_response_disabled(&first, cx));
    assert!(read(&view, cx, |s| s
        .find_response(&first)
        .unwrap()
        .2
        .disabled));
    view.update(cx, |s, cx| s.move_row(&first, 1, cx));
    assert_eq!(ids(&view, cx)[1], first);
    view.update(cx, |s, cx| s.drop_response(&first, &before[1], cx));
    assert_eq!(ids(&view, cx)[0], first);
    view.update_in(cx, |s, w, cx| {
        s.open_picker_menu(
            "rt_users_get",
            crate::tree::RowKind::ErrorPresets,
            0.,
            0.,
            cx,
        );
        s.run_menu(MenuAct::ErrorPreset(429), w, cx);
    });
    let last = ids(&view, cx).last().unwrap().clone();
    let r = read(&view, cx, |s| s.find_response(&last).unwrap().2);
    assert_eq!(
        (r.status, r.content_type.as_str()),
        (429, "application/problem+json")
    );

    // Description edit.
    use crate::response_pane::{InlineTarget as I, Nav};
    view.update_in(cx, |s, w, cx| {
        s.begin_inline(I::Description(first.clone()), "", false, w, cx);
        s.inline.as_mut().unwrap().buf = crate::editor::TextBuf::new("Only for admins");
        s.commit_inline(Nav::Stay, w, cx);
    });
    assert_eq!(
        read(&view, cx, |s| s
            .find_response(&first)
            .unwrap()
            .2
            .description),
        "Only for admins"
    );

    // Sample request round trip through ui.json.
    view.update(cx, |s, cx| {
        s.open_tab("rt_users_get", TabKind::Route, "Get user", false);
        s.open_sample_dialog(cx);
        s.sample_dialog.as_mut().unwrap().sample.query = "role=admin".into();
    });
    view.update_in(cx, |s, w, cx| s.save_sample(false, w, cx));
    let q = read(&view, cx, |s| s.sample_of("rt_users_get").query);
    assert_eq!(q, "role=admin");
    assert!(read(&view, cx, |s| s.sample_dialog.is_none()));
}

#[test]
fn sample_builds_the_test_request() {
    let s = crate::sample::Sample {
        method: "post".into(),
        query: "?role=admin".into(),
        headers: "X-A: 1\nbad line\nAuthorization: Bearer t".into(),
        cookies: "sid=abc".into(),
        body: "{}".into(),
    };
    let ws = crate::view::workspace();
    let srv = ws
        .servers
        .iter()
        .find(|s| s.all_routes().any(|r| r.id == "rt_users_get"))
        .unwrap();
    let route = ws.find_route("rt_users_get").unwrap().1;
    let req = s.request(srv, route);
    assert_eq!(req.method, "POST");
    assert!(req.path.ends_with("?role=admin"));
    assert_eq!(req.headers.len(), 3);
    assert!(
        req.headers
            .iter()
            .any(|(k, v)| k == "cookie" && v == "sid=abc")
    );
}

#[gpui::test]
fn edit_sample_request_action_opens_the_dialog(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, cx| {
        s.open_tab("rt_users_get", TabKind::Route, "Get user", false);
        cx.notify();
    });
    cx.run_until_parked();
    view.update_in(cx, |s, w, cx| s.run_action_id("edit_sample_request", w, cx));
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s.sample_dialog.is_some()));
}

#[gpui::test]
fn templating_preview_route_headers_seed_and_dice(cx: &mut TestAppContext) {
    use crate::header_catalog::Col;
    use crate::response_pane::InlineTarget as I;
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, cx| {
        s.open_tab("rt_users_get", TabKind::Route, "Get user", false);
        cx.notify();
    });
    cx.run_until_parked();
    let key = |view: &Entity<Shell>, cx: &mut VisualTestContext, k: &str, ch: Option<&str>| {
        let mut ks = gpui::Keystroke::parse(k).unwrap();
        ks.key_char = ch.map(String::from);
        let ev = gpui::KeyDownEvent {
            keystroke: ks,
            is_held: false,
        };
        view.update_in(cx, |s, w, cx| s.on_field_key(&ev, w, cx));
    };
    let typed = |view: &Entity<Shell>, cx: &mut VisualTestContext, text: &str| {
        for c in text.chars() {
            let c = c.to_string();
            key(view, cx, &c, Some(&c));
        }
    };

    // The example body uses fake() with no seed: the dice shows. The preview renders it.
    let (rid, random) = read(&view, cx, |s| {
        (
            s.current_response_id().unwrap(),
            s.ws.servers[0].random_output(),
        )
    });
    assert!(random);
    view.update(cx, |s, cx| s.compute_preview(&rid, cx));
    let (status, err, jerr) = read(&view, cx, |s| {
        let p = &s.preview.as_ref().unwrap().1;
        (p.status, p.error.clone(), p.json_error.clone())
    });
    assert_eq!((status, err, jerr), (200, None, None));
    assert!(read(&view, cx, |s| s
        .body_editors
        .contains_key(&format!("{rid}#preview"))));

    // Template off: the Response is verbatim, so the dice goes away.
    view.update(cx, |s, cx| {
        s.store_op(cx, |st| st.set_response_template(&rid, false));
    });
    assert!(!read(&view, cx, |s| s.ws.servers[0].random_output()));
    view.update(cx, |s, cx| {
        s.store_op(cx, |st| st.set_response_template(&rid, true));
    });
    assert!(read(&view, cx, |s| s.ws.servers[0].random_output()));

    // Route headers: add one through the cells; a bad template is reported inline.
    view.update_in(cx, |s, w, cx| {
        s.begin_route_header_cell("rt_users_get", 0, Col::Key, w, cx)
    });
    typed(&view, cx, "x-req");
    key(&view, cx, "tab", None);
    assert_eq!(
        read(&view, cx, |s| s.inline.as_ref().map(|i| i.target.clone())),
        Some(I::RouteHeaderValue {
            route: "rt_users_get".into(),
            row: 0
        })
    );
    typed(&view, cx, "{{ uuid() }}");
    key(&view, cx, "enter", None);
    assert_eq!(
        read(&view, cx, |s| s
            .ws
            .find_route("rt_users_get")
            .unwrap()
            .1
            .headers
            .clone()),
        vec![("x-req".to_string(), "{{ uuid() }}".to_string())]
    );
    view.update(cx, |s, cx| s.remove_route_header_row("rt_users_get", 0, cx));
    assert!(read(&view, cx, |s| s
        .ws
        .find_route("rt_users_get")
        .unwrap()
        .1
        .headers
        .is_empty()));

    // Seed: a number in the Inspector field sets fake_seed, which removes the dice.
    let sid = read(&view, cx, |s| s.ws.servers[0].id.clone());
    view.update_in(cx, |s, w, cx| {
        s.begin_inline(I::Seed(sid.clone()), "", true, w, cx)
    });
    typed(&view, cx, "42");
    key(&view, cx, "enter", None);
    assert_eq!(read(&view, cx, |s| s.ws.servers[0].fake_seed), Some(42));
    assert!(!read(&view, cx, |s| s.ws.servers[0].random_output()));
}

#[gpui::test]
fn dataset_tab_opens_edits_live_rows_and_resets(cx: &mut TestAppContext) {
    use crate::tree::SidebarTab;
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, cx| s.set_sidebar_tab(SidebarTab::Datasets, cx));
    let labels = read(&view, cx, |s| {
        crate::tree::flatten(&s.ws, &s.collapsed, &s.active_env, SidebarTab::Datasets)
            .into_iter()
            .map(|r| r.label)
            .collect::<Vec<_>>()
    });
    assert_eq!(labels, ["users · 3 rows", "charges · 2 rows"]);

    // A double-click on the row opens a pinned Dataset tab.
    view.update(cx, |s, cx| s.click_row("ds:srv_users:users", 2, cx));
    assert_eq!(
        read(&view, cx, |s| s.tabs.active().map(|t| (
            t.kind,
            t.id.clone(),
            t.title.clone()
        ))),
        Some((TabKind::Dataset, "srv_users:users".into(), "users".into()))
    );
    assert_eq!(
        read(&view, cx, |s| s.selected.clone()).as_deref(),
        Some("ds:srv_users:users")
    );

    // Live edits go through the engine and show in the row count.
    view.update(cx, |s, cx| {
        s.edit_dataset("srv_users:users", cx, |rows| {
            rows.as_array_mut()
                .unwrap()
                .push(serde_json::json!({"id": 4}));
            Ok(())
        })
    });
    assert_eq!(
        read(&view, cx, |s| s
            .ws
            .find_dataset("srv_users:users")
            .unwrap()
            .rows),
        4
    );
    view.update(cx, |s, cx| s.delete_dataset_row("srv_users:users", 0, cx));
    assert_eq!(
        read(&view, cx, |s| s
            .ws
            .find_dataset("srv_users:users")
            .unwrap()
            .rows),
        3
    );

    // Reset (the action, with the Dataset tab active) restores the seed.
    view.update_in(cx, |s, w, cx| s.run_action_id("reset_state", w, cx));
    cx.run_until_parked();
    let e = read(&view, cx, |s| s.engine().unwrap());
    let rows = e.dataset_rows("srv_users", "users").unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 3);
    assert_eq!(rows[0]["id"], 1);
}

#[gpui::test]
fn crud_wizard_prompts_chain_and_create_the_resource(cx: &mut TestAppContext) {
    use crate::shell::PromptAction as P;
    let (view, cx) = setup(cx, None);
    assert!(
        P::NewCrudName { server: "s".into() }
            .title()
            .contains("CRUD")
    );
    assert!(crate::dataset_ui::parse_fields("name:name, price:float, ok:bool").is_ok());
    assert!(crate::dataset_ui::parse_fields("x:nope").is_err());
    // Creating through the shell writes the files and opens the Dataset tab.
    view.update(cx, |s, cx| {
        s.create_crud_resource(
            "srv_users",
            "products",
            "/products",
            "name:name, price:float",
            cx,
        )
    });
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s
        .ws
        .find_dataset("srv_users:products")
        .is_some()));
    assert!(read(&view, cx, |s| {
        s.ws.find_server("srv_users")
            .unwrap()
            .all_routes()
            .any(|r| r.is_crud() && r.path == "/products")
    }));
    assert_eq!(
        read(&view, cx, |s| s.tabs.active().map(|t| t.kind)),
        Some(TabKind::Dataset)
    );
}

/// M9: delay ranges, faults, the Server's Network and Rate limit fields, the network profile
/// switcher and the chaos switch.
#[gpui::test]
fn network_and_fault_controls(cx: &mut TestAppContext) {
    use crate::network_ui::NetField;
    use crate::response_pane::{InlineTarget as I, Nav};
    use crate::tree::MenuAct;
    let (view, cx) = setup(cx, None);
    let r = "rsp_users_get_2".to_string();
    let sid = read(&view, cx, |s| s.ws.servers[0].id.clone());
    let type_field = |view: &Entity<Shell>, cx: &mut VisualTestContext, t: I, text: &str| {
        view.update_in(cx, |s, w, cx| {
            s.begin_inline(t, "", false, w, cx);
            s.inline.as_mut().unwrap().buf = crate::editor::TextBuf::new(text);
            s.commit_inline(Nav::Stay, w, cx);
        });
    };
    let resp = |view: &Entity<Shell>, cx: &mut VisualTestContext| {
        read(view, cx, |s| s.find_response("rsp_users_get_2").unwrap().2)
    };

    // Delay accepts a range; garbage and inverted ranges are rejected.
    type_field(&view, cx, I::Delay(r.clone()), "200..800");
    assert_eq!(resp(&view, cx).delay, "200..800");
    type_field(&view, cx, I::Delay(r.clone()), "800..200");
    assert_eq!(resp(&view, cx).delay, "200..800");
    type_field(&view, cx, I::Delay(r.clone()), "0");
    assert_eq!(resp(&view, cx).delay, "");

    // Fault is picked from a menu (index 1 = timeout); the rate is validated.
    view.update_in(cx, |s, w, cx| {
        s.open_picker_menu(&r, crate::tree::RowKind::ResponseFault, 0., 0., cx);
        s.run_menu(MenuAct::SetResponseFault(1), w, cx);
    });
    assert_eq!(resp(&view, cx).fault, "timeout");
    type_field(&view, cx, I::FaultRate(r.clone()), "0.3");
    assert_eq!(resp(&view, cx).fault_rate, "0.3");
    type_field(&view, cx, I::FaultRate(r.clone()), "7");
    assert_eq!(resp(&view, cx).fault_rate, "0.3");
    view.update_in(cx, |s, w, cx| {
        s.open_picker_menu(&r, crate::tree::RowKind::ResponseFault, 0., 0., cx);
        s.run_menu(MenuAct::SetResponseFault(0), w, cx);
    });
    assert_eq!(resp(&view, cx).fault, "");
    assert_eq!(resp(&view, cx).fault_rate, "");

    // Fault preset adds a Response with the fault set.
    view.update_in(cx, |s, w, cx| {
        s.open_picker_menu(
            "rt_users_get",
            crate::tree::RowKind::ErrorPresets,
            0.,
            0.,
            cx,
        );
        s.run_menu(MenuAct::FaultPreset(0), w, cx);
    });
    let (st, d) = read(&view, cx, |s| {
        let x =
            s.ws.find_route("rt_users_get")
                .unwrap()
                .1
                .responses
                .last()
                .unwrap()
                .clone();
        (x.status, x.delay)
    });
    assert_eq!((st, d.as_str()), (502, "10000"));

    // Server tab fields.
    let net = |f| I::Net {
        server: sid.clone(),
        field: f,
    };
    type_field(&view, cx, net(NetField::Latency), "120");
    type_field(&view, cx, net(NetField::Bandwidth), "400");
    type_field(&view, cx, net(NetField::Window), "30s");
    type_field(&view, cx, net(NetField::Window), "soon"); // rejected
    type_field(&view, cx, net(NetField::Key), "header:x-api-key");
    view.update(cx, |s, cx| s.toggle_rate_limit(&sid, cx));
    let (n, rl) = read(&view, cx, |s| {
        let x = &s.ws.servers[0];
        (x.network.clone(), x.rate_limit.clone().unwrap())
    });
    assert_eq!((n.latency_ms, n.bandwidth_kbps), (120, 400));
    assert!(rl.enabled);
    assert_eq!(
        (rl.window.as_str(), rl.key.as_str()),
        ("30s", "header:x-api-key")
    );
    type_field(&view, cx, net(NetField::Latency), "");
    assert_eq!(read(&view, cx, |s| s.ws.servers[0].network.latency_ms), 0);

    // Status bar profile and chaos switch (no host in tests: state only).
    view.update(cx, |s, cx| {
        s.set_network_profile(fauxfield_engine::NetworkProfile::Slow3g, cx)
    });
    assert_eq!(
        read(&view, cx, |s| s.network_profile),
        fauxfield_engine::NetworkProfile::Slow3g
    );
    view.update(cx, |s, cx| s.toggle_chaos_at(0, cx));
    assert_eq!(read(&view, cx, |s| s.ws.servers[0].chaos), Some(0.2));
    view.update(cx, |s, cx| s.toggle_chaos_at(0, cx));
    assert_eq!(read(&view, cx, |s| s.ws.servers[0].chaos), None);
}

/// M10: TLS and CORS fields of the Server tab, the CORS presets and static Route editing.
#[gpui::test]
fn tls_cors_and_static_controls(cx: &mut TestAppContext) {
    use crate::https_ui::{HttpsField, StaticField};
    use crate::response_pane::{InlineTarget as I, Nav};
    let (view, cx) = setup(cx, None);
    let sid = read(&view, cx, |s| s.ws.servers[0].id.clone());
    let type_field = |view: &Entity<Shell>, cx: &mut VisualTestContext, t: I, text: &str| {
        view.update_in(cx, |s, w, cx| {
            s.begin_inline(t, "", false, w, cx);
            s.inline.as_mut().unwrap().buf = crate::editor::TextBuf::new(text);
            s.commit_inline(Nav::Stay, w, cx);
        });
    };
    let https = |f| I::Https {
        server: sid.clone(),
        field: f,
    };
    // TLS: toggle, mode, also-HTTP port (the server's own port is rejected).
    assert!(!read(&view, cx, |s| s.ws.servers[0].https()));
    view.update(cx, |s, cx| s.toggle_tls(&sid, cx));
    assert!(read(&view, cx, |s| s.ws.servers[0].https()));
    view.update(cx, |s, cx| s.cycle_tls_mode(&sid, cx));
    type_field(&view, cx, https(HttpsField::Cert), "certs/s.pem");
    type_field(&view, cx, https(HttpsField::Cert), "../escape.pem"); // rejected
    let port = read(&view, cx, |s| s.ws.servers[0].port);
    type_field(&view, cx, https(HttpsField::AlsoHttp), &port.to_string()); // rejected
    type_field(&view, cx, https(HttpsField::AlsoHttp), "8080");
    let t = read(&view, cx, |s| s.ws.servers[0].tls.clone().unwrap());
    assert_eq!(t.mode_name(), "files");
    assert_eq!(t.cert.as_deref(), Some("certs/s.pem"));
    assert_eq!(t.also_http_port, Some(8080));

    // CORS: presets then edits.
    view.update(cx, |s, cx| s.apply_cors_preset(&sid, true, cx));
    let c = read(&view, cx, |s| s.ws.servers[0].cors.clone().unwrap());
    assert!(c.enabled && c.credentials && c.origins.contains(&"http://localhost:*".to_string()));
    view.update(cx, |s, cx| s.apply_cors_preset(&sid, false, cx));
    let c = read(&view, cx, |s| s.ws.servers[0].cors.clone().unwrap());
    assert_eq!(c.origins, vec!["*".to_string()]);
    assert!(!c.credentials);
    type_field(&view, cx, https(HttpsField::Methods), "get, post");
    type_field(&view, cx, https(HttpsField::MaxAge), "600");
    view.update(cx, |s, cx| s.toggle_cors(&sid, "enabled", cx));
    let c = read(&view, cx, |s| s.ws.servers[0].cors.clone().unwrap());
    assert_eq!(
        (c.methods, c.max_age, c.enabled),
        (vec!["GET".into(), "POST".into()], Some(600), false)
    );

    // Static Route: created in the workspace, then edited.
    let dir = read(&view, cx, |s| s.store.as_ref().unwrap().root());
    std::fs::create_dir_all(dir.join("public")).unwrap();
    std::fs::write(dir.join("public/index.html.tera"), "x").unwrap();
    view.update(cx, |s, cx| {
        s.new_static_chosen(&sid, dir.join("public"), cx)
    });
    let rid = read(&view, cx, |s| {
        s.ws.servers[0]
            .all_routes()
            .find(|r| r.is_static())
            .map(|r| r.id.clone())
    })
    .expect("static route created");
    let (path, d, idx) = read(&view, cx, |s| {
        let r = s.ws.find_route(&rid).unwrap().1;
        (r.path.clone(), r.dir.clone(), r.index.clone())
    });
    assert_eq!(
        (path.as_str(), d.as_deref(), idx.as_deref()),
        ("/public/*", Some("public"), Some("index.html"))
    );
    let st = |f| I::Static {
        route: rid.clone(),
        field: f,
    };
    type_field(&view, cx, st(StaticField::TemplateExt), "tera"); // rejected: no dot
    type_field(&view, cx, st(StaticField::TemplateExt), ".tera, .tpl");
    type_field(&view, cx, st(StaticField::Index), "home.html");
    let r = read(&view, cx, |s| s.ws.find_route(&rid).unwrap().1.clone());
    assert_eq!(
        r.template_extensions,
        vec![".tera".to_string(), ".tpl".to_string()]
    );
    assert_eq!(r.index.as_deref(), Some("home.html"));
}

#[gpui::test]
fn menus_are_keyboard_navigable(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    let r = "rsp_users_get_2".to_string();
    view.update(cx, |s, cx| {
        s.open_picker_menu(&r, crate::tree::RowKind::ResponseFault, 0., 0., cx)
    });
    let sel = |v: &Entity<Shell>, cx: &mut VisualTestContext| read(v, cx, |s| s.menu_sel);
    assert_eq!(sel(&view, cx), 0);
    cx.simulate_keystrokes("down down");
    assert_eq!(sel(&view, cx), 2);
    cx.simulate_keystrokes("up up up");
    assert_eq!(sel(&view, cx), fauxfield_model::FAULTS.len() - 1, "wraps");
    cx.simulate_keystrokes("r");
    assert_eq!(fauxfield_model::FAULTS[sel(&view, cx)], "reset");
    cx.simulate_keystrokes("enter");
    assert!(read(&view, cx, |s| s.menu.is_none()));
    assert_eq!(
        read(&view, cx, |s| s.find_response(&r).unwrap().2.fault),
        "reset"
    );
    view.update(cx, |s, cx| {
        s.open_picker_menu(&r, crate::tree::RowKind::ResponseFault, 0., 0., cx)
    });
    cx.simulate_keystrokes("escape");
    assert!(read(&view, cx, |s| s.menu.is_none()));
}

#[gpui::test]
fn copy_rule_curl_puts_a_matching_curl_on_the_clipboard(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    let r = "rsp_users_get_2".to_string();
    view.update(cx, |s, cx| s.add_rule(&r, cx));
    view.update(cx, |s, cx| {
        s.edit_rule(&r, 0, cx, |x| {
            x.key = Some("foo".into());
            x.value = Some(toml::Value::String("1".into()));
        })
    });
    view.update(cx, |s, cx| s.copy_rule_curl(&r, 0, cx));
    let text = cx
        .read_from_clipboard()
        .and_then(|c| c.text())
        .unwrap_or_default();
    assert!(text.starts_with("curl -X "), "{text}");
    assert!(text.contains("?foo=1"), "{text}");
}
