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
                settings_path: None,
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
    assert_eq!(theme_name(cx), "Ayu Dark", "highlight previews the theme");
    cx.simulate_keystrokes("escape");
    assert_eq!(theme_name(cx), "One Dark", "Esc reverts");
    assert!(read(&view, cx, |s| s.picker.is_none()));

    // Enter commits.
    cx.simulate_keystrokes("secondary-k secondary-t");
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("enter");
    assert_eq!(theme_name(cx), "Ayu Dark");
    assert_eq!(
        read(&view, cx, |s| s.theme_setting.dark.clone()),
        "Ayu Dark"
    );

    // The OS flipping to light follows only in system mode.
    view.update(cx, |s, cx| {
        s.on_appearance_changed(false, cx);
    });
    assert_eq!(theme_name(cx), "One Light");
    view.update(cx, |s, cx| {
        s.on_appearance_changed(true, cx);
    });
    assert_eq!(theme_name(cx), "Ayu Dark");
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
    assert!(read(&view, cx, |s| s.server_dialog.is_some()));
    let set = |view: &Entity<Shell>, cx: &mut VisualTestContext, vals: [&str; 3]| {
        view.update_in(cx, |s, _, cx| {
            let ins = s.server_dialog.as_ref().unwrap().inputs.clone();
            for (inp, v) in ins.iter().zip(vals) {
                inp.update(cx, |t, cx| t.set_value(v, cx));
            }
        });
    };
    // Host and port arrive prefilled; an empty name is refused and keeps the dialog open.
    let (host, port0) = view.update_in(cx, |s, _, cx| {
        let d = s.server_dialog.as_ref().unwrap();
        (
            d.inputs[1].read(cx).value().to_string(),
            d.inputs[2].read(cx).value().to_string(),
        )
    });
    assert_eq!(host, "127.0.0.1");
    assert!(!port0.is_empty());
    view.update_in(cx, |s, w, cx| s.submit_server_dialog(w, cx));
    assert!(read(&view, cx, |s| s.server_dialog.is_some()));
    // A port already used on the same host is refused.
    let (used, uhost) = read(&view, cx, |s| {
        (
            s.ws.servers[0].port.to_string(),
            s.ws.servers[0].host.clone(),
        )
    });
    set(&view, cx, ["Dup", &uhost, &used]);
    view.update_in(cx, |s, w, cx| s.submit_server_dialog(w, cx));
    assert!(read(&view, cx, |s| s.server_dialog.is_some()));
    set(&view, cx, ["Billing API", "127.0.0.1", "4021"]);
    view.update_in(cx, |s, w, cx| s.submit_server_dialog(w, cx));
    assert!(read(&view, cx, |s| s.server_dialog.is_none()));
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
        s.set_inline_text("418", cx);
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
    view.update(cx, |s, cx| s.toggle_server_at(index, cx));
    settle(cx, &view, |s| s.ws.servers[index].running);

    // A request over loopback fills Traffic.
    view.update(cx, |s, _| {
        let (srv, route) = s.ws.find_route("rt_users_get").unwrap();
        let req = crate::sample::Sample::default().request(srv, route);
        let host = s.host.as_ref().unwrap();
        let engine = host.engine().clone();
        let id = srv.id.clone();
        host.handle().spawn(async move {
            let _ = engine.send_test(&id, &req).await;
        });
    });
    settle(cx, &view, |s| s.traffic.iter().any(|r| r.id > 0));
    let (route, source) = read(&view, cx, |s| {
        let r = s.traffic.last().unwrap();
        (r.route.clone(), r.rec.as_ref().unwrap().source.clone())
    });
    assert_eq!(route, "rt_users_get");
    assert_eq!(source, "test");
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
    assert!(notice(&view, cx).contains("The Server is disabled in its file"));
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
    assert!(read(&view, cx, |s| s.any_combo_open()));
    cx.run_until_parked(); // the list renders
    view.update_in(cx, |s, w, cx| {
        assert!(s.combo_pick("route-method-rt_users_get", "PATCH", w, cx))
    });
    cx.run_until_parked();
    assert!(read(&view, cx, |s| !s.any_combo_open()));
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
            "https://0funct0ry.github.io/fauxfield/docs/".to_string(),
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
            s.set_inline_text(text, cx);
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
        s.set_inline_text("bodies/user.json", cx);
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
        s.run_action_id("picker_open_pinned", w, cx)
    });
    cx.run_until_parked();
    assert!(
        read(&view, cx, |s| s.body_edit.is_some()),
        "Cmd+Enter must not save"
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
    // The field's input is driven directly: the header cell may not be on screen in this test,
    // so there is no input handler for simulated typing to reach.
    let key = |view: &Entity<Shell>, cx: &mut VisualTestContext, k: &str, ch: Option<&str>| {
        let mut ks = gpui::Keystroke::parse(k).unwrap();
        ks.key_char = ch.map(String::from);
        let ev = gpui::KeyDownEvent {
            keystroke: ks,
            is_held: false,
        };
        let Some(input) = read(view, cx, |s| s.inline.as_ref().map(|i| i.input.clone())) else {
            return;
        };
        match ch {
            Some(c) => input.update(cx, |t, cx| t.insert_text(c, cx)),
            None => {
                input.update_in(cx, |t, w, cx| t.handle_key(&ev, w, cx));
            }
        }
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
        read(&view, cx, |s| s.inline_suggestions()[0].label.clone()),
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
    view.update(cx, |s, cx| s.set_rule_op(&first, 0, "regex", cx));
    assert_eq!(rules(&view, cx)[0].op, "regex");
    view.update(cx, |s, cx| {
        s.edit_rule(&first, 0, cx, |r| {
            r.value = Some(toml::Value::String("(".into()))
        })
    });
    assert!(fauxfield_model::rules::check_rule(&rules(&view, cx)[0]).is_err());
    view.update(cx, |s, cx| s.edit_rule(&first, 0, cx, |r| r.negate = true));
    assert!(rules(&view, cx)[0].negate);

    // Dataset actions (M10.06): add, edit, reorder-safe delete.
    let ds = read(&view, cx, |s| s.response_datasets(&first));
    if let Some(name) = ds.first() {
        let acts = |view: &Entity<Shell>, cx: &mut VisualTestContext| {
            read(view, cx, |s| s.find_response(&first).unwrap().2.actions)
        };
        view.update(cx, |s, cx| s.add_dataset_action(&first, cx));
        assert_eq!(acts(&view, cx).len(), 1);
        assert_eq!(&acts(&view, cx)[0].dataset, name);
        view.update(cx, |s, cx| {
            s.edit_action(&first, 0, cx, |a| {
                a.op = "increment".into();
                a.field = Some("stock".into());
            })
        });
        assert_eq!(acts(&view, cx)[0].op, "increment");
        view.update(cx, |s, cx| {
            s.store_op(cx, |st| st.delete_action(&first, 0));
        });
        assert!(acts(&view, cx).is_empty());
    }

    // Response mode, disable, reorder, preset.
    view.update(cx, |s, cx| {
        s.set_response_mode_value("rt_users_get", "sequential", cx)
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
        s.set_inline_text("Only for admins", cx);
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
fn templating_preview_route_headers_seed_and_dice(cx: &mut TestAppContext) {
    use crate::header_catalog::Col;
    use crate::response_pane::InlineTarget as I;
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, cx| {
        s.open_tab("rt_users_get", TabKind::Route, "Get user", false);
        cx.notify();
    });
    cx.run_until_parked();
    // The field's input is driven directly: the header cell may not be on screen in this test,
    // so there is no input handler for simulated typing to reach.
    let key = |view: &Entity<Shell>, cx: &mut VisualTestContext, k: &str, ch: Option<&str>| {
        let mut ks = gpui::Keystroke::parse(k).unwrap();
        ks.key_char = ch.map(String::from);
        let ev = gpui::KeyDownEvent {
            keystroke: ks,
            is_held: false,
        };
        let Some(input) = read(view, cx, |s| s.inline.as_ref().map(|i| i.input.clone())) else {
            return;
        };
        match ch {
            Some(c) => input.update(cx, |t, cx| t.insert_text(c, cx)),
            None => {
                input.update_in(cx, |t, w, cx| t.handle_key(&ev, w, cx));
            }
        }
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
    typed(&view, cx, "x-mine");
    key(&view, cx, "tab", None);
    assert_eq!(
        read(&view, cx, |s| s.inline.as_ref().map(|i| i.target.clone())),
        Some(I::RouteHeaderValue {
            route: "rt_users_get".into(),
            row: 0
        })
    );
    // A Value inside `{{ }}` completes template functions; Tab inserts and keeps editing.
    typed(&view, cx, "{{ uui");
    assert!(read(&view, cx, |s| s
        .inline_suggestions()
        .iter()
        .any(|x| x.label == "uuid")));
    key(&view, cx, "tab", None);
    assert!(read(&view, cx, |s| s
        .inline
        .as_ref()
        .unwrap()
        .text
        .starts_with("{{ uuid")));
    assert!(matches!(
        read(&view, cx, |s| s.inline.as_ref().map(|i| i.target.clone())),
        Some(I::RouteHeaderValue { .. })
    ));
    view.update(cx, |s, cx| s.set_inline_text("{{ uuid() }}", cx));
    key(&view, cx, "enter", None);
    assert_eq!(
        read(&view, cx, |s| s
            .ws
            .find_route("rt_users_get")
            .unwrap()
            .1
            .headers
            .clone()),
        vec![("x-mine".to_string(), "{{ uuid() }}".to_string())]
    );
    // Inline problems: request data the sample lacks is reported with advice; the sample has
    // the common headers, and `default` makes a missing one fine.
    read(&view, cx, |s| {
        let p = |v: &str| s.header_value_problem("rt_users_get", v, true);
        assert!(
            p("{{ request.headers[\"x-tenant\"] }}")
                .unwrap()
                .contains("default(value")
        );
        assert!(p("{{ request.headers[\"x-tenant\"] | default(value=\"a\") }}").is_none());
        assert!(p("{{ request.headers[\"x-request-id\"] }}").is_none());
        assert!(p("{{ oops ").is_some());
        assert!(p("{{ request.headers[\"x-tenant\"] }}").is_some());
        assert!(
            s.header_value_problem("rt_users_get", "{{ request.headers[\"x-tenant\"] }}", false)
                .is_none()
        );
    });
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
    view.update(cx, |s, cx| {
        s.delete_dataset_rows("srv_users:users", &[0], cx)
    });
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
fn dataset_is_built_from_the_table_without_a_route(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, cx| {
        s.create_dataset_from_table(
            "srv_users",
            "products",
            "id:int, name:string, price:float",
            cx,
        )
    });
    cx.run_until_parked();
    let key = "srv_users:products";
    let d = read(&view, cx, |s| s.ws.find_dataset(key).cloned()).expect("created");
    assert_eq!(d.columns.len(), 3);
    assert_eq!(d.source, "static");
    assert!(d.unused(), "no Route is created");

    for _ in 0..5 {
        view.update(cx, |s, cx| s.add_dataset_row(key, cx));
        cx.run_until_parked();
    }
    let rows = |cx: &mut gpui::VisualTestContext| {
        read(&view, cx, |s| s.engine().unwrap())
            .dataset_rows("srv_users", "products")
            .unwrap()
    };
    assert_eq!(rows(cx).as_array().unwrap().len(), 5);
    let seed = |cx: &mut gpui::VisualTestContext| {
        read(&view, cx, |s| {
            s.store
                .as_ref()
                .unwrap()
                .read_dataset_seed("srv_users", "products")
                .unwrap()
                .unwrap()
        })
    };
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&seed(cx))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        5
    );

    // Bake 200 generated rows after the 5 manual ones, ids continuing the sequence.
    view.update(cx, |s, cx| s.generate_rows_now(key, "200 bake", cx));
    cx.run_until_parked();
    let all = rows(cx);
    assert_eq!(all.as_array().unwrap().len(), 205);
    assert_eq!(all[5]["id"], 6);
    assert!(read(&view, cx, |s| s.ws.find_dataset(key).unwrap().source
        == "static"));

    // A column added to a static seed fills the existing rows.
    view.update(cx, |s, cx| {
        s.add_column_now(key, "email:string=fake:internet.email", cx)
    });
    cx.run_until_parked();
    assert!(
        rows(cx)[0]["email"]
            .as_str()
            .is_some_and(|e| e.contains('@'))
    );
    view.update(cx, |s, cx| s.rename_column_now(key, "email", "mail", cx));
    cx.run_until_parked();
    assert!(rows(cx)[0].get("mail").is_some() && rows(cx)[0].get("email").is_none());
    view.update(cx, |s, cx| s.remove_column_now(key, "mail", cx));
    cx.run_until_parked();
    assert!(rows(cx)[0].get("mail").is_none());

    // Keep as generator writes the block and clears the seed.
    view.update(cx, |s, cx| s.generate_rows_now(key, "7 keep", cx));
    cx.run_until_parked();
    assert_eq!(
        read(&view, cx, |s| s
            .ws
            .find_dataset(key)
            .unwrap()
            .source
            .clone()),
        "generator"
    );
    assert_eq!(rows(cx).as_array().unwrap().len(), 7);
}

#[gpui::test]
fn import_creates_then_appends_with_preview_and_warnings(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("items.csv");
    std::fs::write(&csv, "id,name\n1,a\n2,b\n3\n").unwrap();
    let path = csv.to_string_lossy().to_string();
    let rows = |cx: &mut gpui::VisualTestContext| {
        read(&view, cx, |s| s.engine().unwrap())
            .dataset_rows("srv_users", "items")
            .unwrap()
    };
    view.update_in(cx, |s, w, cx| {
        s.show_import_options("srv_users", "items", false, &path, w, cx)
    });
    cx.run_until_parked();
    let pv = read(&view, cx, |s| s.import_preview.clone()).unwrap();
    assert!(
        pv.contains("csv") && pv.contains("3 row(s)") && pv.contains("ragged"),
        "{pv}"
    );
    view.update(cx, |s, cx| {
        s.import_commit("srv_users", "items", false, &path, "replace", cx)
    });
    cx.run_until_parked();
    assert_eq!(rows(cx).as_array().unwrap().len(), 3);
    assert_eq!(rows(cx)[0]["id"], 1);
    view.update(cx, |s, cx| {
        s.import_commit("srv_users", "items", true, &path, "append", cx)
    });
    cx.run_until_parked();
    assert_eq!(rows(cx).as_array().unwrap().len(), 6);
    // A bad option leaves the rows alone.
    view.update(cx, |s, cx| {
        s.import_commit("srv_users", "items", true, &path, "bogus", cx)
    });
    cx.run_until_parked();
    assert_eq!(rows(cx).as_array().unwrap().len(), 6);
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

/// M10.01: empty Datasets, rename following a crud Route, Route delete keeps the Dataset,
/// delete blocked behind a confirmation that lists references.
#[gpui::test]
fn dataset_lifecycle_without_routes(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, cx| s.create_dataset_now("srv_users", "orders", cx));
    cx.run_until_parked();
    let d = read(&view, cx, |s| {
        s.ws.find_dataset("srv_users:orders").cloned()
    });
    let d = d.expect("created");
    assert_eq!(d.rows, 0);
    assert!(d.unused());
    assert!(read(&view, cx, |s| s
        .ws
        .find_server("srv_users")
        .unwrap()
        .all_routes()
        .all(|r| r.dataset.as_deref() != Some("orders"))));

    // A crud Route referencing it follows a rename.
    view.update(cx, |s, cx| {
        s.create_crud_resource("srv_users", "orders", "/orders", "name:name", cx)
    });
    cx.run_until_parked();
    view.update(cx, |s, cx| {
        s.rename_dataset_now("srv_users:orders", "purchases", cx)
    });
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s
        .ws
        .find_dataset("srv_users:purchases")
        .is_some()));
    assert!(read(&view, cx, |s| {
        s.ws.find_server("srv_users")
            .unwrap()
            .all_routes()
            .any(|r| r.dataset.as_deref() == Some("purchases"))
    }));

    // Delete is behind a confirmation listing the Route.
    view.update(cx, |s, cx| {
        s.request_delete_dataset("srv_users:purchases", cx)
    });
    assert_eq!(
        read(&view, cx, |s| s.dialog_name()),
        Some("delete_dataset_confirm")
    );
    assert_eq!(
        read(&view, cx, |s| s
            .lc
            .dataset_delete
            .as_ref()
            .unwrap()
            .refs
            .len()),
        1
    );
    view.update(cx, |s, cx| s.cancel_dialog(cx));
    assert!(read(&view, cx, |s| s
        .ws
        .find_dataset("srv_users:purchases")
        .is_some()));
    view.update(cx, |s, cx| {
        s.request_delete_dataset("srv_users:purchases", cx);
        s.confirm_dialog(cx);
    });
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s
        .ws
        .find_dataset("srv_users:purchases")
        .is_none()));
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
            s.set_inline_text(text, cx);
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
    view.update(cx, |s, cx| s.set_response_fault(&r, "timeout", cx));
    assert_eq!(resp(&view, cx).fault, "timeout");
    type_field(&view, cx, I::FaultRate(r.clone()), "0.3");
    assert_eq!(resp(&view, cx).fault_rate, "0.3");
    type_field(&view, cx, I::FaultRate(r.clone()), "7");
    assert_eq!(resp(&view, cx).fault_rate, "0.3");
    view.update(cx, |s, cx| s.set_response_fault(&r, "none", cx));
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

/// M15 and M17.06: Docs editors and the OpenAPI fields write the files.
#[gpui::test]
fn docs_and_openapi_fields_are_editable(cx: &mut TestAppContext) {
    use crate::docs_edit::OpenapiField;
    let (view, cx) = setup(cx, None);
    let (sid, rid, resp) = read(&view, cx, |s| {
        let sv = &s.ws.servers[0];
        let r = sv.all_routes().next().unwrap();
        (sv.id.clone(), r.id.clone(), r.responses[0].id.clone())
    });
    // Markdown docs of a Route and of the Server.
    for key in [format!("route:{rid}"), format!("server:{sid}")] {
        view.update(cx, |s, cx| {
            let ed = s.docs_editor(&key, "", cx);
            ed.update(cx, |e, cx| e.set_text("# Hello\n\nSome *docs*.", cx));
            s.commit_docs(&key, cx);
        });
    }
    assert_eq!(
        read(&view, cx, |s| s.ws.find_route(&rid).unwrap().1.docs.clone()),
        "# Hello\n\nSome *docs*."
    );
    assert!(read(&view, cx, |s| s.ws.servers[0]
        .docs
        .starts_with("# Hello")));
    // An empty text removes the key.
    view.update(cx, |s, cx| {
        let key = format!("route:{rid}");
        let ed = s.docs_editor(&key, "# Hello\n\nSome *docs*.", cx);
        ed.update(cx, |e, cx| e.set_text("", cx));
        s.commit_docs(&key, cx);
    });
    assert_eq!(
        read(&view, cx, |s| s.ws.find_route(&rid).unwrap().1.docs.clone()),
        ""
    );
    // OpenAPI overrides.
    view.update(cx, |s, cx| {
        s.commit_openapi_field(&rid, OpenapiField::Summary, "List things", cx);
        s.commit_openapi_field(&rid, OpenapiField::Tags, "Orders, Billing", cx);
        s.commit_openapi_field(&rid, OpenapiField::OperationId, "list_things", cx);
        s.commit_openapi_field(&rid, OpenapiField::OperationId, "has space", cx); // refused
        s.commit_openapi_field(&rid, OpenapiField::RequestSchema, "../x.json", cx); // refused
        s.commit_openapi_field(&rid, OpenapiField::RequestSchema, "schemas/req.json", cx);
        s.toggle_openapi_hidden(&rid, cx);
        s.commit_response_schema(&resp, "schemas/resp.json", cx);
        s.commit_response_schema(&resp, "/abs.json", cx); // refused
    });
    let o = read(&view, cx, |s| {
        s.ws.find_route(&rid).unwrap().1.openapi.clone()
    });
    assert_eq!(o.summary.as_deref(), Some("List things"));
    assert_eq!(o.tags, vec!["Orders".to_string(), "Billing".to_string()]);
    assert_eq!(o.operation_id.as_deref(), Some("list_things"));
    assert_eq!(o.request_schema.as_deref(), Some("schemas/req.json"));
    assert!(o.hidden);
    assert_eq!(
        read(&view, cx, |s| s.ws.find_route(&rid).unwrap().1.responses[0]
            .schema
            .clone())
        .as_deref(),
        Some("schemas/resp.json")
    );
    // Clearing every key removes the table.
    view.update(cx, |s, cx| {
        for f in OpenapiField::ALL {
            s.commit_openapi_field(&rid, f, "", cx);
        }
        s.toggle_openapi_hidden(&rid, cx);
    });
    let o = read(&view, cx, |s| {
        s.ws.find_route(&rid).unwrap().1.openapi.clone()
    });
    assert_eq!(o, fauxfield_model::RouteOpenapi::default());
}

/// M17.06: the API documentation section's fields, toggle and the docs state.
#[gpui::test]
fn api_documentation_controls(cx: &mut TestAppContext) {
    use crate::https_ui::HttpsField;
    use crate::response_pane::{InlineTarget as I, Nav};
    let (view, cx) = setup(cx, None);
    let sid = read(&view, cx, |s| s.ws.servers[0].id.clone());
    let type_field =
        |view: &Entity<Shell>, cx: &mut VisualTestContext, f: HttpsField, text: &str| {
            let t = I::Https {
                server: sid.clone(),
                field: f,
            };
            view.update_in(cx, |s, w, cx| {
                s.begin_inline(t, "", false, w, cx);
                s.set_inline_text(text, cx);
                s.commit_inline(Nav::Stay, w, cx);
            });
        };
    assert!(read(&view, cx, |s| s.ws.servers[0].docs_site.is_none()));
    view.update(cx, |s, cx| s.toggle_docs(&sid, cx));
    let d = read(&view, cx, |s| s.ws.servers[0].docs_site.clone().unwrap());
    assert!(d.enabled);
    type_field(&view, cx, HttpsField::DocsPath, "/spec/");
    type_field(&view, cx, HttpsField::DocsPath, "/spec"); // the first is rejected (E113)
    type_field(&view, cx, HttpsField::DocsTitle, "My API");
    type_field(&view, cx, HttpsField::DocsVersion, "2.0.0");
    let d = read(&view, cx, |s| s.ws.servers[0].docs_site.clone().unwrap());
    assert_eq!(
        (d.path.as_deref(), d.title.as_deref(), d.version.as_deref()),
        (Some("/spec"), Some("My API"), Some("2.0.0"))
    );
    view.update(cx, |s, cx| s.toggle_docs(&sid, cx));
    assert!(!read(&view, cx, |s| s.ws.servers[0]
        .docs_site
        .clone()
        .unwrap()
        .enabled));
    assert!(crate::docs_ui::docs_path_ok("/__docs"));
    for bad in ["", "/", "/a/", "a", "/a:b", "/a*", "/a{b}"] {
        assert!(!crate::docs_ui::docs_path_ok(bad), "{bad}");
    }
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
            s.set_inline_text(text, cx);
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
    // WebSocket Route: created from the Server menu action and shown as a WS Route.
    view.update(cx, |s, cx| s.begin_new_websocket(cx));
    let ws_route = read(&view, cx, |s| {
        s.ws.servers[0]
            .all_routes()
            .find(|r| r.is_websocket())
            .map(|r| (r.path.clone(), r.method.clone(), r.ws_mode.clone()))
    });
    assert_eq!(
        ws_route,
        Some(("/ws".to_string(), "WS".to_string(), "echo".to_string()))
    );
    // Messages table: adding a message switches echo to script, opens its editor, and the body
    // is saved by Apply.
    let wsid = read(&view, cx, |s| {
        s.ws.servers[0]
            .all_routes()
            .find(|r| r.is_websocket())
            .map(|r| r.id.clone())
    })
    .expect("websocket route");
    view.update(cx, |s, cx| s.ws_op("add-interval", &wsid, None, cx));
    let (mode, n, every, open) = read(&view, cx, |s| {
        let r = s.ws.find_route(&wsid).unwrap().1;
        (
            r.ws_mode.clone(),
            r.ws_messages.len(),
            r.ws_messages[0].every.clone(),
            s.ws_body_ed.is_some(),
        )
    });
    assert_eq!(
        (mode.as_str(), n, every.as_str(), open),
        ("script", 1, "5s", true)
    );
    let ed = read(&view, cx, |s| s.ws_body_ed.as_ref().unwrap().2.clone());
    view.update(cx, |_, cx| {
        ed.update(cx, |e, cx| e.set_text("{\"type\":\"tick\"}", cx))
    });
    view.update(cx, |s, cx| s.ws_op("apply", &wsid, Some(0), cx));
    let body = read(&view, cx, |s| {
        s.ws.find_route(&wsid).unwrap().1.ws_messages[0]
            .body
            .clone()
    });
    assert_eq!(body, "{\"type\":\"tick\"}");
    view.update(cx, |s, cx| s.set_ws_format(&wsid, 0, "pretty", cx));
    assert_eq!(
        read(&view, cx, |s| s.ws.find_route(&wsid).unwrap().1.ws_messages
            [0]
        .format
        .clone()),
        "pretty"
    );
    view.update(cx, |s, cx| s.set_ws_format(&wsid, 0, "compact", cx));
    assert_eq!(
        read(&view, cx, |s| s.ws.find_route(&wsid).unwrap().1.ws_messages
            [0]
        .format
        .clone()),
        "compact"
    );
    view.update(cx, |s, cx| s.ws_op("mode-broadcast", &wsid, None, cx));
    assert_eq!(
        read(&view, cx, |s| s
            .ws
            .find_route(&wsid)
            .unwrap()
            .1
            .ws_mode
            .clone()),
        "broadcast"
    );
    view.update(cx, |s, cx| s.ws_op("del", &wsid, Some(0), cx));
    assert_eq!(
        read(&view, cx, |s| s
            .ws
            .find_route(&wsid)
            .unwrap()
            .1
            .ws_messages
            .len()),
        0
    );
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
fn proxy_controls_and_route_flags(cx: &mut TestAppContext) {
    use crate::proxy_ui::ProxyField;
    use crate::response_pane::{InlineTarget as I, Nav};
    let (view, cx) = setup(cx, None);
    let sid = read(&view, cx, |s| s.ws.servers[0].id.clone());
    let type_field =
        |view: &Entity<Shell>, cx: &mut VisualTestContext, f: ProxyField, text: &str| {
            let t = I::Proxy {
                server: sid.clone(),
                field: f,
            };
            view.update_in(cx, |s, w, cx| {
                s.begin_inline(t, "", false, w, cx);
                s.set_inline_text(text, cx);
                s.commit_inline(Nav::Stay, w, cx);
            });
        };
    let proxy = |view: &Entity<Shell>, cx: &mut VisualTestContext| {
        read(view, cx, |s| {
            s.ws.servers[0].proxy.clone().unwrap_or_default()
        })
    };
    view.update(cx, |s, cx| s.toggle_proxy(&sid, "enabled", cx));
    assert!(proxy(&view, cx).enabled);
    type_field(&view, cx, ProxyField::Upstream, "ftp://nope"); // rejected
    assert_eq!(proxy(&view, cx).upstream, None);
    type_field(&view, cx, ProxyField::Upstream, "https://api.example.com");
    type_field(&view, cx, ProxyField::Timeout, "abc"); // rejected
    type_field(&view, cx, ProxyField::Timeout, "1500");
    type_field(&view, cx, ProxyField::RequestHeader, "no colon here"); // rejected
    type_field(&view, cx, ProxyField::RequestHeader, "X-Env: staging");
    type_field(
        &view,
        cx,
        ProxyField::ResponseHeader,
        "x-served-by: fauxfield",
    );
    view.update(cx, |s, cx| s.set_proxy_mode(&sid, "passthrough", cx));
    view.update(cx, |s, cx| s.toggle_proxy(&sid, "insecure", cx));
    let p = proxy(&view, cx);
    assert_eq!(p.upstream.as_deref(), Some("https://api.example.com"));
    assert_eq!(p.timeout_ms, Some(1500));
    assert_eq!(
        p.request_headers.get("X-Env").map(String::as_str),
        Some("staging")
    );
    assert_eq!(p.response_headers.len(), 1);
    assert_eq!((p.mode_name(), p.insecure), ("passthrough", true));
    assert_eq!(
        read(&view, cx, |s| s.upstream_preview(&s.ws.servers[0])),
        Ok("https://api.example.com".into())
    );
    view.update(cx, |s, cx| {
        s.remove_proxy_header(&sid, "request_headers", "X-Env", cx)
    });
    assert!(proxy(&view, cx).request_headers.is_empty());
    view.update(cx, |s, cx| s.set_proxy_mode(&sid, "fallback", cx));
    assert_eq!(proxy(&view, cx).mode, None);

    // Route flags and the tree icon source.
    let rid = read(&view, cx, |s| {
        s.ws.servers[0].all_routes().next().unwrap().id.clone()
    });
    view.update(cx, |s, cx| s.toggle_route_flag(&rid, "proxy", cx));
    let (fwd, src) = read(&view, cx, |s| {
        let r = s.ws.find_route(&rid).unwrap().1;
        (r.forward, crate::tree::route_source(&s.ws.servers[0], r))
    });
    assert!(fwd && src == Some(crate::tree::RouteSource::Proxied));
    view.update(cx, |s, cx| s.toggle_route_flag(&rid, "pinned", cx));
    let src = read(&view, cx, |s| {
        crate::tree::route_source(&s.ws.servers[0], s.ws.find_route(&rid).unwrap().1)
    });
    assert_eq!(src, Some(crate::tree::RouteSource::Pinned));
}

#[gpui::test]
fn comboboxes_are_keyboard_operable(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, _cx| {
        s.open_tab("rt_users_get", TabKind::Route, "Get user", false)
    });
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let method = |v: &Entity<Shell>, cx: &mut VisualTestContext| {
        read(v, cx, |s| {
            s.ws.find_route("rt_users_get").unwrap().1.method.clone()
        })
    };
    // Enter on the focused trigger opens the list; Down moves, Enter picks.
    view.update_in(cx, |s, w, cx| {
        assert!(s.combo_open("route-method-rt_users_get", w, cx))
    });
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s.any_combo_open()));
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    assert_eq!(method(&view, cx), "POST");
    assert!(read(&view, cx, |s| !s.any_combo_open()));
    // Esc closes without a change; the "Custom…" entry runs its palette action.
    view.update_in(cx, |s, w, cx| {
        s.combo_open("route-method-rt_users_get", w, cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    assert!(read(&view, cx, |s| !s.any_combo_open()));
    assert_eq!(method(&view, cx), "POST");
    view.update_in(cx, |s, w, cx| {
        s.combo_open("route-method-rt_users_get", w, cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("end enter");
    cx.run_until_parked();
    cx.run_until_parked();
    assert_eq!(
        read(&view, cx, |s| s.picker.as_ref().map(|p| p.kind)),
        Some(PickerKind::Prompt)
    );
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

#[gpui::test]
fn route_tab_panes_resize_by_drag_and_clamp(cx: &mut TestAppContext) {
    use crate::shell::Resize;
    let (view, cx) = setup(cx, None);
    let mv = |x: f32, y: f32| gpui::MouseMoveEvent {
        position: gpui::point(gpui::px(x), gpui::px(y)),
        pressed_button: Some(gpui::MouseButton::Left),
        modifiers: gpui::Modifiers::default(),
    };
    // Dragging the Responses list edge right by 60 px widens it by 60 px.
    let w0 = read(&view, cx, |s| s.layout.resp_list_width);
    view.update_in(cx, |s, w, cx| {
        s.begin_resize(Resize::RespList, (500., 300.), cx);
        s.on_resize_move(&mv(560., 300.), w, cx);
    });
    assert_eq!(read(&view, cx, |s| s.layout.resp_list_width), w0 + 60.);
    view.update_in(cx, |s, w, cx| s.on_resize_move(&mv(-9000., 300.), w, cx));
    assert_eq!(
        read(&view, cx, |s| s.layout.resp_list_width),
        crate::layout::RESP_LIST.0
    );
    // Dragging the Preview edge up grows the preview.
    let h0 = read(&view, cx, |s| s.layout.preview_height);
    view.update_in(cx, |s, w, cx| {
        s.begin_resize(Resize::Preview, (500., 700.), cx);
        s.on_resize_move(&mv(500., 650.), w, cx);
    });
    assert_eq!(read(&view, cx, |s| s.layout.preview_height), h0 + 50.);
}

#[gpui::test]
fn schema_designer_creates_a_dataset_with_constraints_and_no_route(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    let routes_before = read(&view, cx, |s| {
        s.ws.servers.iter().map(|x| x.routes.len()).sum::<usize>()
    });
    view.update_in(cx, |s, w, cx| s.begin_new_dataset(w, cx));
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s.schema_dialog.is_some()));
    // An empty name keeps the dialog open.
    view.update_in(cx, |s, w, cx| s.submit_schema_dialog(w, cx));
    assert!(read(&view, cx, |s| s.schema_dialog.is_some()));
    view.update_in(cx, |s, w, cx| {
        let d = s.schema_dialog.as_ref().unwrap();
        d.name
            .clone()
            .update(cx, |t, cx| t.set_value("widgets", cx));
        s.schema_op("starter", "Products", w, cx);
        s.schema_op("required", "2", w, cx);
    });
    view.update_in(cx, |s, w, cx| s.submit_schema_dialog(w, cx));
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s.schema_dialog.is_none()));
    let (cols, routes) = read(&view, cx, |s| {
        let d = s.ws.datasets.iter().find(|d| d.name == "widgets").unwrap();
        (
            d.column_defs.clone(),
            s.ws.servers.iter().map(|x| x.routes.len()).sum::<usize>(),
        )
    });
    assert_eq!(cols.len(), 5);
    assert!(cols[0].primary_key && cols[2].unique && cols[2].required);
    assert_eq!(routes, routes_before, "creating a Dataset adds no Route");
}

#[gpui::test]
fn selection_actions_duplicate_copy_delete_with_confirm_and_undo(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, cx| s.click_row("ds:srv_users:users", 2, cx));
    cx.run_until_parked();
    let key = "srv_users:users";
    let table = read(&view, cx, |s| s.ds_tables.borrow().get(key).cloned()).expect("table drawn");
    let rows = |view: &Entity<Shell>, cx: &mut VisualTestContext| {
        read(view, cx, |s| {
            s.engine()
                .unwrap()
                .dataset_rows("srv_users", "users")
                .unwrap()
                .as_array()
                .unwrap()
                .len()
        })
    };
    assert_eq!(rows(&view, cx), 3);
    table.update(cx, |t, _| {
        let d = t.delegate_mut();
        d.selection.set = [0usize, 1].into_iter().collect();
    });
    view.update(cx, |s, cx| s.duplicate_selected_rows(cx));
    cx.run_until_parked();
    assert_eq!(rows(&view, cx), 5);
    // The copies are selected; deleting two rows asks first.
    assert_eq!(table.read_with(cx, |t, _| t.delegate().selected_count()), 2);
    view.update(cx, |s, cx| s.delete_selected_rows(cx));
    assert_eq!(read(&view, cx, |s| s.dialog_name()), Some("data_confirm"));
    view.update(cx, |s, cx| s.confirm_dialog(cx));
    cx.run_until_parked();
    assert_eq!(rows(&view, cx), 3);
    // Undo restores them.
    view.update(cx, |s, cx| s.undo_row_delete(cx));
    cx.run_until_parked();
    assert_eq!(rows(&view, cx), 5);
    // Clear data asks, empties the live rows and leaves the seed alone.
    view.update(cx, |s, cx| s.begin_clear_data(cx));
    assert_eq!(read(&view, cx, |s| s.dialog_name()), Some("data_confirm"));
    view.update(cx, |s, cx| s.confirm_dialog(cx));
    cx.run_until_parked();
    assert_eq!(rows(&view, cx), 0);
}

#[gpui::test]
fn export_modal_writes_selected_columns_and_seed_modal_generates(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, cx| s.click_row("ds:srv_users:users", 2, cx));
    cx.run_until_parked();
    let key = "srv_users:users";
    view.update_in(cx, |s, w, cx| s.begin_export(key, w, cx));
    cx.run_until_parked();
    assert_eq!(read(&view, cx, |s| s.dialog_name()), Some("export_dataset"));
    let root = read(&view, cx, |s| s.ws.root.clone());
    let out = root.join("picked.csv");
    view.update_in(cx, |s, w, cx| {
        s.export_op("format", "csv", w, cx);
        let d = s.export_dialog.as_mut().unwrap();
        d.path
            .update(cx, |t, cx| t.set_value(&out.to_string_lossy(), cx));
        // Keep only the first column.
        for c in d.cols.iter_mut().skip(1) {
            c.1 = false;
        }
        d.scope = crate::dataset_dialogs::Scope::First;
        d.first_n.update(cx, |t, cx| t.set_value("2", cx));
    });
    view.update_in(cx, |s, w, cx| s.export_op("go", "", w, cx));
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s.export_dialog.is_none()));
    let text = std::fs::read_to_string(&out).unwrap();
    assert_eq!(text.lines().count(), 3, "header plus two rows: {text}");

    // Seed data: replace the rows with 7 generated ones.
    view.update_in(cx, |s, w, cx| s.begin_generate_rows(w, cx));
    cx.run_until_parked();
    assert_eq!(read(&view, cx, |s| s.dialog_name()), Some("seed_data"));
    view.update_in(cx, |s, w, cx| {
        let d = s.seed_dialog.as_mut().unwrap();
        d.rows.update(cx, |t, cx| t.set_value("7", cx));
        d.replace = true;
        s.seed_op("go", "", w, cx);
    });
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s.seed_dialog.is_none()));
}

#[gpui::test]
fn cell_edits_enforce_constraints_move_with_tab_and_undo(cx: &mut TestAppContext) {
    use crate::dataset_ui::EditMove;
    let (view, cx) = setup(cx, None);
    view.update_in(cx, |s, w, cx| s.begin_new_dataset(w, cx));
    cx.run_until_parked();
    view.update_in(cx, |s, w, cx| {
        let d = s.schema_dialog.as_ref().unwrap();
        d.name
            .clone()
            .update(cx, |t, cx| t.set_value("gadgets", cx));
        s.schema_op("starter", "Products", w, cx);
        s.schema_op("required", "1", w, cx); // name
        s.schema_op("unique", "1", w, cx);
        s.submit_schema_dialog(w, cx);
    });
    cx.run_until_parked();
    let key = "srv_users:gadgets";
    view.update(cx, |s, cx| s.click_row(&format!("ds:{key}"), 2, cx));
    cx.run_until_parked();
    // Two rows: the first named, the second empty.
    view.update(cx, |s, cx| s.add_dataset_row(key, cx));
    view.update(cx, |s, cx| s.add_dataset_row(key, cx));
    cx.run_until_parked();
    let table = read(&view, cx, |s| s.ds_tables.borrow().get(key).cloned()).expect("table");
    // Row 0, column 2 is `name`; set it, Tab moves on.
    let edit = |cx: &mut VisualTestContext, row: usize, col: usize, text: &str, mv: EditMove| {
        table.update(cx, |t, cx| {
            t.delegate_mut().begin_edit(row, col, cx);
            let input = t.delegate().editing.as_ref().unwrap().input.clone();
            input.update(cx, |i, cx| i.set_value(text, cx));
            crate::dataset_ui::DatasetTable::finish_edit(t, true, false, mv, cx);
        });
    };
    edit(cx, 0, 2, "Widget", EditMove::Next);
    assert!(
        table.read_with(cx, |t, _| t.delegate().editing.is_some()),
        "Tab opened the next cell"
    );
    table.update(cx, |t, cx| {
        crate::dataset_ui::DatasetTable::finish_edit(t, false, false, EditMove::Stay, cx)
    });
    // A duplicate name on row 1 is refused with a reason and the editor stays open.
    edit(cx, 1, 2, "Widget", EditMove::Stay);
    assert!(table.read_with(cx, |t, _| t.delegate().editing.is_some()));
    let err = table.read_with(cx, |t, _| t.delegate().shared.edit_error.borrow().clone());
    assert!(err.is_some_and(|e| e.contains("unique")), "reason is shown");
    table.update(cx, |t, cx| {
        crate::dataset_ui::DatasetTable::finish_edit(t, false, false, EditMove::Stay, cx)
    });
    // Undo and redo the accepted edit.
    let name_of = |view: &Entity<Shell>, cx: &mut VisualTestContext| {
        read(view, cx, |s| {
            s.engine()
                .unwrap()
                .dataset_rows("srv_users", "gadgets")
                .unwrap()[0]["name"]
                .clone()
        })
    };
    assert_eq!(name_of(&view, cx), "Widget");
    view.update(cx, |s, cx| s.step_cell_history(false, cx));
    assert_ne!(name_of(&view, cx), "Widget");
    view.update(cx, |s, cx| s.step_cell_history(true, cx));
    assert_eq!(name_of(&view, cx), "Widget");
}

#[gpui::test]
fn add_column_modal_validates_and_adds_a_constrained_column(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, cx| s.click_row("ds:srv_users:users", 2, cx));
    cx.run_until_parked();
    view.update_in(cx, |s, w, cx| s.begin_add_column(w, cx));
    cx.run_until_parked();
    assert_eq!(read(&view, cx, |s| s.dialog_name()), Some("add_column"));
    // An empty name keeps the modal open.
    view.update_in(cx, |s, w, cx| s.column_op("add", w, cx));
    assert!(read(&view, cx, |s| s.column_dialog.is_some()));
    // A default that does not fit the type is refused.
    view.update_in(cx, |s, w, cx| {
        let d = s.column_dialog.as_mut().unwrap();
        d.name.update(cx, |t, cx| t.set_value("visits", cx));
        d.default.update(cx, |t, cx| t.set_value("abc", cx));
        d.ty = "int".into();
        s.column_op("add", w, cx);
    });
    assert!(read(&view, cx, |s| s.column_dialog.is_some()));
    view.update_in(cx, |s, w, cx| {
        let d = s.column_dialog.as_mut().unwrap();
        d.default.update(cx, |t, cx| t.set_value("18", cx));
        s.column_op("required", w, cx);
        s.column_op("add", w, cx);
    });
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s.column_dialog.is_none()));
    let col = read(&view, cx, |s| {
        s.ws.find_dataset("srv_users:users")
            .unwrap()
            .column_defs
            .iter()
            .find(|c| c.name == "visits")
            .cloned()
    })
    .unwrap_or_else(|| panic!("column added: {:?}", read(&view, cx, |s| s.notice.clone())));
    assert!(col.required && col.ty == "int");
    assert_eq!(col.default, Some(toml::Value::Integer(18)));
}

#[gpui::test]
fn seed_dialog_takes_generator_options(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, cx| s.click_row("ds:srv_users:users", 2, cx));
    cx.run_until_parked();
    view.update_in(cx, |s, w, cx| s.begin_generate_rows(w, cx));
    cx.run_until_parked();
    let i = read(&view, cx, |s| {
        s.seed_dialog
            .as_ref()
            .unwrap()
            .cols
            .iter()
            .position(|c| c.name == "name")
            .unwrap()
    });
    // one_of with no values is refused, with the reason on the column.
    view.update(cx, |s, cx| s.set_seed_generator(i, "one_of", cx));
    view.update_in(cx, |s, w, cx| {
        let d = s.seed_dialog.as_mut().unwrap();
        assert!(d.cols[i].spec(cx).is_err(), "values are required");
        let input = d.cols[i]
            .args
            .iter()
            .find(|(k, _)| k == "values")
            .unwrap()
            .1
            .clone();
        input.update(cx, |t, cx| t.set_value("Ann, Bob", cx));
        assert!(d.cols[i].spec(cx).is_ok());
        d.rows.update(cx, |t, cx| t.set_value("6", cx));
        d.replace = true;
        s.seed_op("go", "", w, cx);
    });
    cx.run_until_parked();
    let rows = read(&view, cx, |s| {
        s.engine()
            .unwrap()
            .dataset_rows("srv_users", "users")
            .unwrap()
    });
    let names: Vec<_> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].clone())
        .collect();
    assert_eq!(names.len(), 6);
    assert!(names.iter().all(|n| n == "Ann" || n == "Bob"), "{names:?}");
}

#[gpui::test]
fn seed_dialog_date_pickers_bound_datetime_and_preview_is_a_table(cx: &mut TestAppContext) {
    use chrono::NaiveDate;
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, cx| s.click_row("ds:srv_users:users", 2, cx));
    cx.run_until_parked();
    view.update_in(cx, |s, w, cx| s.begin_generate_rows(w, cx));
    cx.run_until_parked();
    let i = read(&view, cx, |s| {
        s.seed_dialog
            .as_ref()
            .unwrap()
            .cols
            .iter()
            .position(|c| c.name == "name")
            .unwrap()
    });
    view.update(cx, |s, cx| s.set_seed_generator(i, "datetime", cx));
    let pickers = read(&view, cx, |s| {
        s.seed_dialog.as_ref().unwrap().cols[i].dates.clone()
    });
    assert_eq!(pickers.len(), 2, "from and to are DatePickers");
    let day = |d: u32| NaiveDate::from_ymd_opt(2026, 3, d).unwrap();
    pickers[0].1.update(cx, |p, cx| p.pick(day(10), cx));
    pickers[1].1.update(cx, |p, cx| p.pick(day(12), cx));
    let rows = view
        .update(cx, |s, cx| s.seed_preview(cx))
        .expect("preview");
    assert_eq!(rows.len(), 5);
    for r in &rows {
        let at = r["name"].as_str().unwrap();
        assert!(("2026-03-10".."2026-03-13").contains(&&at[..10]), "{at}");
    }
    // A reversed range is refused.
    pickers[0].1.update(cx, |p, cx| p.pick(day(20), cx));
    assert!(view.update(cx, |s, cx| s.seed_preview(cx)).is_err());
}

#[gpui::test]
fn export_file_starts_empty_and_browse_fills_it(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, cx| s.click_row("ds:srv_users:users", 2, cx));
    cx.run_until_parked();
    view.update_in(cx, |s, w, cx| s.begin_export("srv_users:users", w, cx));
    cx.run_until_parked();
    let path = |view: &Entity<Shell>, cx: &mut VisualTestContext| {
        view.update(cx, |s, cx| {
            s.export_dialog
                .as_ref()
                .unwrap()
                .path
                .read(cx)
                .value()
                .to_string()
        })
    };
    assert_eq!(path(&view, cx), "");
    // Export with no destination is refused and keeps the modal open.
    view.update_in(cx, |s, w, cx| s.export_op("go", "", w, cx));
    assert!(read(&view, cx, |s| s
        .export_dialog
        .as_ref()
        .unwrap()
        .error
        .is_some()));
    // The folder dialog's answer fills the field; the extension follows the format.
    let dir = read(&view, cx, |s| s.ws.root.clone());
    view.update(cx, |s, cx| {
        s.set_export_folder(&dir, "users.export.json", cx)
    });
    assert_eq!(
        path(&view, cx),
        dir.join("users.export.json").to_string_lossy()
    );
    view.update_in(cx, |s, w, cx| s.export_op("format", "csv", w, cx));
    assert!(path(&view, cx).ends_with("users.export.csv"));
    view.update_in(cx, |s, w, cx| s.export_op("go", "", w, cx));
    cx.run_until_parked();
    assert!(dir.join("users.export.csv").is_file());
}

#[gpui::test]
fn hamburger_is_a_dropdown_menu_that_runs_actions(cx: &mut TestAppContext) {
    use crate::dropdown_menu::MenuItem;
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, cx| s.click_row("ds:srv_users:users", 2, cx));
    cx.run_until_parked();
    let menu = read(&view, cx, |s| s.dropdowns.borrow().get("ds-menu").cloned()).expect("drawn");
    let (labels, seps) = menu.read_with(cx, |m, _| {
        let it = m.items();
        (
            it.iter()
                .filter_map(|i| match i {
                    MenuItem::Item { label, icon, .. } => {
                        assert!(icon.is_some(), "every entry has an icon");
                        Some(label.to_string())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>(),
            it.iter().filter(|i| **i == MenuItem::Separator).count(),
        )
    });
    assert_eq!(labels[0], "Switch to JSON view");
    assert_eq!(seps, 3);
    view.update_in(cx, |s, w, cx| {
        s.dropdown_press("dropdown", "ds-menu", None, w, cx)
    })
    .unwrap();
    assert!(read(&view, cx, |s| s.any_dropdown_open()));
    assert_eq!(menu.read_with(cx, |m, _| m.highlighted()), Some(0));
    view.update_in(cx, |s, w, cx| {
        s.dropdown_press("dropdown-item", "ds-menu", Some(0), w, cx)
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!read(&view, cx, |s| s.any_dropdown_open()));
    assert!(read(&view, cx, |s| s.ds_json_mode), "the action ran");
}

#[gpui::test]
fn json_view_edits_apply_to_the_table_and_switch_compact(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    let key = "srv_users:users";
    view.update(cx, |s, cx| s.click_row("ds:srv_users:users", 2, cx));
    cx.run_until_parked();
    view.update_in(cx, |s, w, cx| s.run_action_id("toggle_dataset_view", w, cx));
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s.ds_json_mode));
    let ed = read(&view, cx, |s| {
        s.json_eds.borrow().get(key).map(|e| e.editor.clone())
    })
    .expect("the JSON view uses the code editor");
    let rows = |view: &Entity<Shell>, cx: &mut VisualTestContext| {
        read(view, cx, |s| {
            s.engine()
                .unwrap()
                .dataset_rows("srv_users", "users")
                .unwrap()
        })
    };
    // Compact puts one row per line.
    view.update(cx, |s, cx| s.set_json_pretty(key, false, cx));
    cx.run_until_parked();
    let text = ed.read_with(cx, |e, _| e.text().to_string());
    assert_eq!(text.lines().count(), 3 + 2, "{text}");
    // Invalid JSON is not applied; leaving the view is refused.
    ed.update(cx, |e, cx| e.set_text("[{\"id\": 1,", cx));
    view.update(cx, |s, cx| s.json_changed_for_test(key, cx));
    assert!(read(&view, cx, |s| s
        .json_eds
        .borrow()
        .get(key)
        .unwrap()
        .problem
        .is_some()));
    view.update(cx, |s, cx| s.toggle_dataset_view(cx));
    assert!(
        read(&view, cx, |s| s.ds_json_mode),
        "stays in the JSON view"
    );
    // A valid edit applies when leaving the view, and the table rows follow.
    let mut v = rows(&view, cx);
    v[0]["name"] = serde_json::json!("Edited in JSON");
    ed.update(cx, |e, cx| {
        e.set_text(&serde_json::to_string(&v).unwrap(), cx)
    });
    view.update(cx, |s, cx| s.toggle_dataset_view(cx));
    cx.run_until_parked();
    assert!(!read(&view, cx, |s| s.ds_json_mode));
    assert_eq!(rows(&view, cx)[0]["name"], "Edited in JSON");
}

#[gpui::test]
fn crud_operation_rows_copy_their_request_as_curl(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    let route = read(&view, cx, |s| {
        s.ws.servers
            .iter()
            .flat_map(|x| x.all_routes())
            .find(|r| r.is_crud())
            .map(|r| r.id.clone())
    })
    .expect("the fixture has a crud Route");
    // Row 3 is PUT <path>/:id: a body from the first live row, `:id` filled in.
    view.update(cx, |s, cx| s.copy_crud_op_curl(&format!("{route}#3"), cx));
    let clip = cx.read_from_clipboard().and_then(|c| c.text()).unwrap();
    assert!(clip.starts_with("curl -X PUT 'http://"), "{clip}");
    assert!(
        clip.contains("/1'") && clip.contains("--data-raw"),
        "{clip}"
    );
    view.update(cx, |s, cx| s.copy_crud_op_curl(&format!("{route}#0"), cx));
    let clip = cx.read_from_clipboard().and_then(|c| c.text()).unwrap();
    assert!(
        clip.starts_with("curl -X GET") && !clip.contains("--data-raw"),
        "{clip}"
    );
    // The row menu offers it.
    let labels: Vec<_> = crate::tree::menu_items(
        &crate::tree::RowKind::CrudOp {
            method: "PUT".into(),
            responses: 0,
        },
        false,
    )
    .into_iter()
    .map(|(l, _)| l)
    .collect();
    assert!(labels.contains(&"Copy as cURL"), "{labels:?}");
}

#[gpui::test]
fn callback_rows_nest_under_their_request(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, _| {
        s.traffic = crate::traffic::synthetic(3);
        for (i, r) in s.traffic.iter_mut().enumerate() {
            r.id = (i + 1) as u64;
        }
        s.traffic_selected = Some(2);
        let mut cb = s.traffic[0].clone();
        cb.id = 10;
        cb.parent = Some(1);
        s.push_traffic_row(cb.clone());
        cb.id = 11;
        s.push_traffic_row(cb);
    });
    let (ids, sel) = read(&view, cx, |s| {
        (
            s.traffic.iter().map(|r| r.id).collect::<Vec<_>>(),
            s.traffic_selected,
        )
    });
    assert_eq!(
        ids,
        [1, 10, 11, 2, 3],
        "callbacks follow their request in order"
    );
    assert_eq!(sel, Some(4), "the selection keeps following its row");
}

#[gpui::test]
fn callbacks_table_and_modal_edit_the_file(cx: &mut TestAppContext) {
    use crate::callback_dialog::CbMode;
    let (view, cx) = setup(cx, None);
    let resp = read(&view, cx, |s| {
        s.ws.find_route("rt_users_get").unwrap().1.responses[0]
            .id
            .clone()
    });
    let list = |view: &Entity<Shell>, cx: &mut VisualTestContext| {
        let r = resp.clone();
        read(view, cx, move |s| {
            s.find_response(&r).unwrap().2.callbacks.clone()
        })
    };
    view.update(cx, |s, _| {
        s.open_tab("rt_users_get", TabKind::Route, "Get user", false);
        s.resp_sub.insert(resp.clone(), 3);
    });
    // `+` opens the create modal; Save writes the Callback.
    view.update_in(cx, |s, w, cx| {
        s.begin_callback_dialog(&resp, None, CbMode::Create, w, cx)
    });
    cx.run_until_parked(); // the modal, its editor and preview render
    view.update_in(cx, |s, w, cx| {
        let d = s.callback_dialog.as_mut().unwrap();
        d.verb = "PUT".into();
        d.url
            .update(cx, |t, cx| t.set_value("http://127.0.0.1:1/x", cx));
        d.retries.update(cx, |t, cx| t.set_value("2", cx));
        d.name.update(cx, |t, cx| t.set_value("Settled", cx));
        s.callback_dialog_op("add_header", w, cx);
        let d = s.callback_dialog.as_mut().unwrap();
        d.headers[0].key.update(cx, |t, cx| t.set_value("x-a", cx));
        d.headers[0]
            .value
            .update(cx, |t, cx| t.set_value("{{ request.method }}", cx));
    });
    cx.run_until_parked();
    view.update_in(cx, |s, w, cx| s.callback_dialog_op("save", w, cx));
    cx.run_until_parked();
    let l = list(&view, cx);
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].method.as_deref(), Some("PUT"));
    assert_eq!(l[0].retries, Some(2));
    assert_eq!(l[0].name.as_deref(), Some("Settled"));
    assert_eq!(
        l[0].headers.get("x-a").map(String::as_str),
        Some("{{ request.method }}")
    );
    assert!(read(&view, cx, |s| s.callback_dialog.is_none()));

    // An invalid draft is not saved and the modal stays open.
    view.update_in(cx, |s, w, cx| {
        s.begin_callback_dialog(&resp, Some(0), CbMode::Edit, w, cx);
        let d = s.callback_dialog.as_mut().unwrap();
        d.url.update(cx, |t, cx| t.set_value("ftp://nope", cx));
        s.callback_dialog_op("save", w, cx);
    });
    assert!(read(&view, cx, |s| s.callback_dialog.is_some()));
    assert_eq!(list(&view, cx)[0].url, "http://127.0.0.1:1/x");
    // View mode never saves; Edit switches to edit mode.
    view.update_in(cx, |s, w, cx| {
        s.begin_callback_dialog(&resp, Some(0), CbMode::View, w, cx);
        s.callback_dialog_op("save", w, cx);
    });
    assert_eq!(
        read(&view, cx, |s| s.callback_dialog.as_ref().unwrap().mode),
        CbMode::View
    );
    view.update_in(cx, |s, w, cx| s.callback_dialog_op("edit", w, cx));
    assert_eq!(
        read(&view, cx, |s| s.callback_dialog.as_ref().unwrap().mode),
        CbMode::Edit
    );
    view.update_in(cx, |s, w, cx| s.callback_dialog_op("cancel", w, cx));

    // Row actions.
    view.update(cx, |s, cx| s.duplicate_callback(&resp, 0, cx));
    assert_eq!(list(&view, cx).len(), 2);
    view.update(cx, |s, cx| s.move_callback_row(&resp, 0, true, cx));
    let ids: Vec<_> = list(&view, cx).iter().map(|c| c.id.clone()).collect();
    assert!(ids[0].ends_with("_2") && ids[1] == "cb_1", "{ids:?}");
    view.update(cx, |s, cx| {
        s.edit_callback(&resp, 0, cx, |c| c.disabled = true)
    });
    assert!(list(&view, cx)[0].disabled);
    cx.run_until_parked(); // the table and its row menus render
    view.update(cx, |s, cx| s.delete_callback_row(&resp, 0, cx));
    view.update(cx, |s, cx| s.delete_callback_row(&resp, 0, cx));
    assert!(list(&view, cx).is_empty());
}

#[gpui::test]
fn deleting_a_callback_asks_first_and_help_popovers_toggle(cx: &mut TestAppContext) {
    use crate::callback_dialog::CbMode;
    let (view, cx) = setup(cx, None);
    let resp = read(&view, cx, |s| {
        s.ws.find_route("rt_users_get").unwrap().1.responses[0]
            .id
            .clone()
    });
    view.update(cx, |s, cx| {
        s.open_tab("rt_users_get", TabKind::Route, "Get user", false);
        let c = fauxfield_model::Callback {
            id: "cb_x".into(),
            url: "http://127.0.0.1:1/x".into(),
            ..Default::default()
        };
        let r = resp.clone();
        s.store_op(cx, |st| st.set_callback(&r, None, &c));
    });
    let count = |view: &Entity<Shell>, cx: &mut VisualTestContext| {
        let r = resp.clone();
        read(view, cx, move |s| {
            s.find_response(&r).unwrap().2.callbacks.len()
        })
    };
    assert_eq!(count(&view, cx), 1);
    view.update(cx, |s, cx| s.ask_delete_callback(&resp, 0, cx));
    assert_eq!(count(&view, cx), 1, "nothing is deleted before the answer");
    assert_eq!(
        read(&view, cx, |s| s.dialog_name()),
        Some("delete_callback_confirm")
    );
    cx.run_until_parked(); // the confirmation renders
    view.update(cx, |s, cx| s.cancel_dialog(cx));
    assert_eq!(count(&view, cx), 1);
    view.update(cx, |s, cx| {
        s.ask_delete_callback(&resp, 0, cx);
        s.confirm_dialog(cx);
    });
    assert_eq!(count(&view, cx), 0);

    // The `?` popover of a field is open state in the dialog and renders.
    view.update_in(cx, |s, w, cx| {
        s.begin_callback_dialog(&resp, None, CbMode::Create, w, cx);
        s.callback_dialog.as_mut().unwrap().help = Some("id");
    });
    cx.run_until_parked();
    assert_eq!(
        read(&view, cx, |s| s.callback_dialog.as_ref().unwrap().help),
        Some("id")
    );
}

fn dir_listing(root: &std::path::Path) -> Vec<String> {
    fn walk(d: &std::path::Path, root: &std::path::Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            out.push(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
            if p.is_dir() {
                walk(&p, root, out);
            }
        }
    }
    let mut v = vec![];
    walk(root, root, &mut v);
    v.sort();
    v
}

fn open_real_workspace(view: &Entity<Shell>, cx: &mut VisualTestContext) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fauxfield_store::init_workspace(dir.path()).unwrap();
    let store = fauxfield_store::Store::open(dir.path()).unwrap();
    let root = dir.path().to_path_buf();
    view.update(cx, |s, _| {
        s.ws.root = root;
        s.store = Some(store);
    });
    dir
}

#[gpui::test]
fn agents_dialog_cancel_writes_nothing_and_write_creates_files(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    let dir = open_real_workspace(&view, cx);
    let before = dir_listing(dir.path());

    view.update(cx, |s, cx| s.open_agents_dialog_with_home(None, cx));
    cx.run_until_parked();
    assert_eq!(read(&view, cx, |s| s.dialog_name()), Some("agents"));
    // The detected default is Claude Code; the plan shows the files it would write.
    let planned = read(&view, cx, |s| {
        let d = s.agents_dialog.as_ref().unwrap();
        d.plan
            .as_ref()
            .unwrap()
            .files
            .iter()
            .map(|f| f.path.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(planned, [".mcp.json", "AGENTS.md", ".gitignore"]);

    // Toggling a client re-plans.
    view.update(cx, |s, cx| s.agents_toggle_client("cursor", cx));
    let planned = read(&view, cx, |s| {
        s.agents_dialog
            .as_ref()
            .unwrap()
            .plan
            .as_ref()
            .unwrap()
            .files
            .len()
    });
    assert_eq!(planned, 4);

    // Esc cancels and the directory tree is unchanged.
    cx.simulate_keystrokes("escape");
    assert!(read(&view, cx, |s| s.agents_dialog.is_none()));
    assert_eq!(dir_listing(dir.path()), before);

    // Enter writes, closes and toasts.
    view.update(cx, |s, cx| s.open_agents_dialog_with_home(None, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    assert!(read(&view, cx, |s| s.agents_dialog.is_none()));
    assert!(dir.path().join(".mcp.json").is_file());
    assert!(dir.path().join("AGENTS.md").is_file());
    let toast = read(&view, cx, |s| s.notice.as_ref().map(|(_, m)| m.clone()));
    assert_eq!(
        toast.as_deref(),
        Some("MCP configured for Claude Code. Restart it, then run /mcp.")
    );
    // Opening again finds nothing to do.
    view.update(cx, |s, cx| s.open_agents_dialog_with_home(None, cx));
    let noop = read(&view, cx, |s| {
        s.agents_dialog
            .as_ref()
            .unwrap()
            .plan
            .as_ref()
            .unwrap()
            .is_noop()
    });
    assert!(noop);
}

#[gpui::test]
fn attached_agents_show_in_the_status_bar_and_activity_log(cx: &mut TestAppContext) {
    use crate::agents_ui::{LogSource, attached_label};
    let (view, cx) = setup(cx, None);
    let dir = open_real_workspace(&view, cx);
    let host = fauxfield_control::Host::start(
        dir.path(),
        fauxfield_control::HostOptions {
            watch: false,
            publish: false,
            ..Default::default()
        },
    )
    .unwrap();
    let agents = host.control();
    view.update(cx, |s, _| s.host = Some(host));

    // Nothing attached: no indicator.
    view.update(cx, |s, cx| s.poll_agents(cx));
    assert_eq!(
        read(&view, cx, |s| attached_label(&s.agents_attached)),
        None
    );

    agents.agents().attach("s1", "Claude Code");
    agents.agents().record(fauxfield_control::AgentEvent {
        ts_ms: 1_000,
        client: "Claude Code".into(),
        tool: "upsert_route".into(),
        target: "rte_1".into(),
        outcome: "updated".into(),
    });
    view.update(cx, |s, cx| s.poll_agents(cx));
    assert_eq!(
        read(&view, cx, |s| attached_label(&s.agents_attached)).as_deref(),
        Some("Claude Code attached")
    );
    assert_eq!(read(&view, cx, |s| s.agent_log.len()), 1);

    // A second client turns the label into a count.
    agents.agents().attach("s2", "codex");
    view.update(cx, |s, cx| s.poll_agents(cx));
    assert_eq!(
        read(&view, cx, |s| attached_label(&s.agents_attached)).as_deref(),
        Some("2 agents attached")
    );

    // Clicking the indicator opens Logs on Agent activity.
    view.update(cx, |s, cx| s.show_agent_activity(cx));
    assert_eq!(read(&view, cx, |s| s.log_source), LogSource::Agent);
    assert_eq!(read(&view, cx, |s| s.layout.bottom_tab), BottomTab::Logs);
    view.update(cx, |s, cx| s.copy_agent_log_markdown(cx));
    let md = cx.read_from_clipboard().and_then(|c| c.text()).unwrap();
    assert!(md.contains("`upsert_route`") && md.contains("rte_1") && md.contains("updated"));

    // Disconnect: the indicator disappears.
    agents.agents().detach("s1");
    agents.agents().detach("s2");
    view.update(cx, |s, cx| s.poll_agents(cx));
    assert_eq!(
        read(&view, cx, |s| attached_label(&s.agents_attached)),
        None
    );
}

#[gpui::test]
fn agent_edits_flash_the_row_or_its_nearest_visible_ancestor(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    let dir = open_real_workspace(&view, cx);
    let host = fauxfield_control::Host::start(
        dir.path(),
        fauxfield_control::HostOptions {
            watch: false,
            publish: false,
            ..Default::default()
        },
    )
    .unwrap();
    let agents = host.control();
    view.update(cx, |s, _| s.host = Some(host));
    let (server, route) = read(&view, cx, |s| {
        let srv = s.ws.servers.iter().find(|x| !x.routes.is_empty()).unwrap();
        (srv.id.clone(), srv.routes[0].id.clone())
    });
    let now = fauxfield_control::agents::now_ms();
    let ev = |tool: &str, target: &str, ts: u64, outcome: &str| fauxfield_control::AgentEvent {
        ts_ms: ts,
        client: "Claude Code".into(),
        tool: tool.into(),
        target: target.into(),
        outcome: outcome.into(),
    };
    // A read never flashes; an update of a Route does.
    agents
        .agents()
        .record(ev("get_route", &route, now + 5, "read"));
    view.update(cx, |s, cx| s.poll_agents(cx));
    assert!(read(&view, cx, |s| s.agent_flash.is_empty()));
    agents
        .agents()
        .record(ev("upsert_route", &route, now + 10, "updated"));
    view.update(cx, |s, cx| s.poll_agents(cx));
    let rows = |s: &Shell| crate::tree::flatten(&s.ws, &s.collapsed, &s.active_env, s.sidebar_tab);
    let flashing = read(&view, cx, |s| s.flashing_rows(&rows(s)));
    assert_eq!(flashing, std::collections::HashSet::from([route.clone()]));

    // With the Server collapsed the Route row is out of sight: its Server flashes instead.
    view.update(cx, |s, _| s.toggle_row(&server));
    let flashing = read(&view, cx, |s| s.flashing_rows(&rows(s)));
    assert_eq!(flashing, std::collections::HashSet::from([server.clone()]));

    // Reduced motion keeps a static tag for 3 s instead of the 1.2 s fade.
    assert_eq!(
        read(&view, cx, |s| s.flash_hold()),
        std::time::Duration::from_millis(1200)
    );
    view.update(cx, |s, _| s.reduced_motion = true);
    assert_eq!(
        read(&view, cx, |s| s.flash_hold()),
        std::time::Duration::from_millis(3000)
    );
}

fn setup_config(
    cx: &mut TestAppContext,
) -> (Entity<Shell>, &mut VisualTestContext, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().to_path_buf();
    let (view, cx) = cx.add_window_view(|w, cx| {
        Shell::new(
            ShellOptions {
                layout_path: None,
                config_dir: Some(d.clone()),
                settings_path: Some(d.join("settings.toml")),
                ..crate::view::test_options()
            },
            w,
            cx,
        )
    });
    cx.run_until_parked();
    (view, cx, dir)
}

#[gpui::test]
fn settings_tab_is_a_singleton_opened_by_keystroke(cx: &mut TestAppContext) {
    let (view, cx, _dir) = setup_config(cx);
    cx.simulate_keystrokes("secondary-,");
    cx.simulate_keystrokes("secondary-,");
    let (n, active) = read(&view, cx, |s| {
        (
            s.tabs
                .tabs()
                .iter()
                .filter(|t| t.kind == TabKind::Settings)
                .count(),
            s.tabs.active().map(|t| t.kind),
        )
    });
    assert_eq!((n, active), (1, Some(TabKind::Settings)));
}

#[gpui::test]
fn settings_write_keeps_comments_and_external_edits_show(cx: &mut TestAppContext) {
    let (view, cx, dir) = setup_config(cx);
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "# keep me\nfuture = 1\n").unwrap();
    view.update(cx, |s, cx| {
        s.open_settings(cx);
        s.settings_set("ui_scale", Some(toml::Value::Float(1.5)), cx);
    });
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("# keep me") && text.contains("future") && text.contains("ui_scale = 1.5")
    );
    assert_eq!(
        read(&view, cx, |s| s
            .settings
            .as_ref()
            .unwrap()
            .raw
            .get("ui_scale")
            .cloned()),
        Some(toml::Value::Float(1.5))
    );
    // An invalid value written outside the app becomes a Problem; the default stays.
    std::fs::write(&path, "ui_scale = 9\n").unwrap();
    view.update(cx, |s, cx| s.reload_settings(cx));
    let probs = read(&view, cx, |s| {
        s.problems
            .iter()
            .filter(|p| p.code == "settings.invalid")
            .map(|p| p.message.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(probs.len(), 1, "{probs:?}");
    assert!(probs[0].contains("ui_scale"));
    // Reset removes the key.
    view.update(cx, |s, cx| s.settings_set("ui_scale", None, cx));
    assert!(!std::fs::read_to_string(&path).unwrap().contains("ui_scale"));
    assert!(read(&view, cx, |s| s
        .problems
        .iter()
        .all(|p| p.code != "settings.invalid")));
}

#[gpui::test]
fn keymap_record_conflict_resolution_writes_only_user_overrides(cx: &mut TestAppContext) {
    let (view, cx, dir) = setup_config(cx);
    let km = dir.path().join("keymap.toml");
    view.update(cx, |s, cx| {
        s.keymap_start_record("run_all", cx);
        // quick_search owns secondary-k; Save is blocked until a resolution is chosen.
        s.record_stroke("secondary-k".into(), std::time::Instant::now(), cx);
        s.keymap_save_record(cx);
    });
    assert!(!km.exists(), "unresolved conflict must not write");
    let conflicts = read(&view, cx, |s| {
        s.settings
            .as_ref()
            .unwrap()
            .record
            .as_ref()
            .unwrap()
            .conflicts
            .clone()
    });
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].0, "quick_search");
    view.update(cx, |s, cx| {
        s.keymap_set_resolution(crate::settings_ui::Resolution::AllowBoth, cx);
        s.keymap_save_record(cx);
    });
    let text = std::fs::read_to_string(&km).unwrap();
    assert!(text.contains("run_all") && text.contains("secondary-k") && text.contains("context"));
    assert!(!text.contains("quick_search"), "{text}");
    let bound = read(&view, cx, |s| {
        s.keymap.binding_for("run_all").map(str::to_owned)
    });
    assert_eq!(bound.as_deref(), Some("secondary-k"));
    // Reset drops the override and the default returns.
    view.update(cx, |s, cx| s.keymap_write("run_all", &[], cx));
    assert!(
        !std::fs::read_to_string(&km)
            .map(|t| t.contains("run_all"))
            .unwrap_or(false)
    );
    assert_eq!(
        read(&view, cx, |s| s
            .keymap
            .binding_for("run_all")
            .map(str::to_owned))
        .as_deref(),
        Some("secondary-shift-r")
    );
}

#[gpui::test]
fn vim_preset_follows_the_setting_and_bad_keymap_entries_are_problems(cx: &mut TestAppContext) {
    let (view, cx, dir) = setup_config(cx);
    assert!(read(&view, cx, |s| s
        .keymap
        .binding_for("list_next")
        .is_none()));
    view.update(cx, |s, cx| {
        s.settings_set("vim_list_navigation", Some(toml::Value::Boolean(true)), cx)
    });
    assert_eq!(
        read(&view, cx, |s| s
            .keymap
            .binding_for("list_next")
            .map(str::to_owned))
        .as_deref(),
        Some("j")
    );
    std::fs::write(
        dir.path().join("keymap.toml"),
        "[[binding]]\naction='nope'\nkeys='x'\n",
    )
    .unwrap();
    view.update(cx, |s, cx| s.reload_settings(cx));
    assert!(read(&view, cx, |s| s
        .problems
        .iter()
        .any(|p| p.code == "keymap.invalid")));
}

#[test]
fn every_action_appears_in_the_keymap_editor() {
    let ids = crate::actions::all_ids();
    for a in ACTIONS {
        assert!(ids.contains(&a.id), "{}", a.id);
    }
    for id in crate::code_editor::actions::IDS
        .iter()
        .chain(crate::data_table::actions::IDS)
    {
        assert!(ids.contains(id), "{id}");
    }
    // The bundled keymap and the vim preset only name actions the editor lists.
    for src in [crate::keymap::DEFAULT_KEYMAP, crate::keymap::VIM_KEYMAP] {
        let k = crate::keymap::Keymap::parse_for(src, crate::keymap::Platform::Macos).unwrap();
        for b in k.bindings {
            assert!(ids.contains(&b.action.as_str()), "{}", b.action);
        }
    }
}

#[gpui::test]
fn toasts_stack_to_three_and_errors_persist(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    view.update(cx, |s, cx| {
        for n in 1..=4 {
            s.notify(format!("Route {n} deleted"), cx);
        }
    });
    let (older, newest) = read(&view, cx, |s| (s.toasts_older.len(), s.notice.clone()));
    assert_eq!(older, crate::copy::TOAST_MAX - 1);
    assert_eq!(newest.unwrap().1, "Route 4 deleted");

    view.update(cx, |s, cx| s.notify_error("Couldn’t start “A”.".into(), cx));
    assert_eq!(
        read(&view, cx, |s| s.notice_action.clone().unwrap().label),
        crate::copy::COPY_DETAILS
    );
    // Past the 5 s timer the plain toasts are gone, the error stays until dismissed.
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(crate::copy::TOAST_SECS + 1));
    cx.run_until_parked();
    assert!(read(&view, cx, |s| s.notice.is_some() && s.notice_is_error));
    view.update(cx, |s, cx| s.dismiss_toast(cx));
    assert!(read(&view, cx, |s| s.notice.is_none() || !s.notice_is_error));
}

#[gpui::test]
fn crud_operation_rows_show_response_counts_and_scope_the_editor(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, None);
    let route = read(&view, cx, |s| {
        s.ws.servers
            .iter()
            .flat_map(|x| x.all_routes())
            .find(|r| r.is_crud())
            .map(|r| r.id.clone())
    })
    .expect("the fixture has a crud Route");
    let op_rows = |view: &Entity<Shell>, cx: &mut VisualTestContext| {
        read(view, cx, |s| {
            let r = format!("{route}#");
            crate::tree::flatten(&s.ws, &s.collapsed, &s.active_env, s.sidebar_tab)
                .into_iter()
                .filter(|row| row.id.starts_with(&r))
                .map(|row| match row.kind {
                    crate::tree::RowKind::CrudOp { responses, .. } => (row.id, responses),
                    k => panic!("unexpected {k:?}"),
                })
                .collect::<Vec<_>>()
        })
    };
    // Six operation rows, none with Responses: they fall through to the built-in handlers.
    let before = op_rows(&view, cx);
    assert_eq!(before.len(), 6);
    assert!(before.iter().all(|(_, n)| *n == 0));
    // Selecting the `create` row (index 1) scopes the editor; Add Response adds to it.
    view.update(cx, |s, cx| s.click_row(&format!("{route}#1"), 1, cx));
    assert_eq!(
        read(&view, cx, |s| s.crud_op_sel.get(&route).cloned()).as_deref(),
        Some("create")
    );
    view.update(cx, |s, cx| {
        s.new_response_for(&format!("{route}#create"), cx)
    });
    cx.run_until_parked();
    let after = op_rows(&view, cx);
    assert_eq!(after[1].1, 1, "{after:?}");
    assert_eq!(after.iter().map(|(_, n)| n).sum::<usize>(), 1);
    let (rid, creates) = read(&view, cx, |s| {
        let (_, r) = s.ws.find_route(&route).unwrap();
        (s.current_response_id(), r.op_responses("create").len())
    });
    assert_eq!(creates, 1);
    let rid = rid.expect("the new operation Response is selected");
    // The Route's own Responses are untouched.
    assert!(read(&view, cx, |s| s
        .ws
        .find_route(&route)
        .unwrap()
        .1
        .responses
        .is_empty()));
    // The Dataset body source works on an operation Response: Generate template writes a body.
    assert!(read(&view, cx, |s| !s.response_datasets(&rid).is_empty()));
    view.update(cx, |s, cx| {
        s.toggle_dataset_source(&rid, cx);
        s.generate_dataset_body(&rid, cx);
    });
    cx.run_until_parked();
    let body = read(&view, cx, |s| s.find_response(&rid).unwrap().2.body);
    assert!(body.contains("data."), "{body}");
    // Generated reads are JSON, so the Response gets a JSON Content-Type.
    assert_eq!(
        read(&view, cx, |s| s.find_response(&rid).unwrap().2.content_type),
        "application/json"
    );
    // The row menu opens for an operation Response and offers Delete (not "Set as default").
    view.update(cx, |s, cx| s.open_response_menu(&rid, 10., 10., cx));
    let labels: Vec<&str> = read(&view, cx, |s| {
        s.menu
            .as_ref()
            .map(|m| {
                crate::tree::menu_items(&m.kind, false)
                    .into_iter()
                    .map(|(l, _)| l)
                    .collect()
            })
            .unwrap_or_default()
    });
    assert!(labels.contains(&"Delete"), "{labels:?}");
    assert!(!labels.contains(&"Set as default"), "{labels:?}");
    view.update_in(cx, |s, w, cx| {
        s.run_menu(crate::tree::MenuAct::Delete, w, cx)
    });
    // Delete from the menu removes it.
    cx.run_until_parked();
    assert!(op_rows(&view, cx).iter().all(|(_, n)| *n == 0));
    // Both palette actions exist.
    let ids: Vec<_> = ACTIONS.iter().map(|a| a.id).collect();
    assert!(ids.contains(&"add_operation_response") && ids.contains(&"remove_operation_response"));
}
