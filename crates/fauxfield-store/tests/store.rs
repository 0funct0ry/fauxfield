use std::time::{Duration, Instant};

use fauxfield_store::{Event, Store, Target, init_workspace, slugify};

fn ws() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    init_workspace(dir.path()).unwrap();
    let s = Store::open(dir.path()).unwrap();
    (dir, s)
}

const COMMENTED: &str = r#"# Hand-written server
schema = 1
id = "srv_hand"
name = "Hand"   # trailing comment
port = 4000

# routes below
[[routes]]
id = "rte_a"
path = "/a"   # keep me
future_field = 7

  [[routes.responses]]
  id = "rsp_a"
  status = 200
"#;

#[test]
fn init_makes_a_valid_workspace() {
    let (dir, s) = ws();
    assert!(dir.path().join(".gitignore").is_file());
    let snap = s.snapshot();
    assert_eq!(snap.servers.len(), 1);
    assert!(snap.diagnostics.is_empty(), "{:?}", snap.diagnostics);
    assert!(init_workspace(dir.path()).is_err());
}

#[test]
fn edits_preserve_comments_order_and_unknown_fields() {
    let (dir, s) = ws();
    std::fs::write(dir.path().join("servers/hand.toml"), COMMENTED).unwrap();
    let s2 = Store::open(dir.path()).unwrap();
    drop(s);
    s2.set_str(&Target::Route("rte_a".into()), "name", Some("Alpha"))
        .unwrap();
    s2.set_int(&Target::Response("rsp_a".into()), "status", Some(201))
        .unwrap();
    s2.set_header("rsp_a", "x-test", Some("1")).unwrap();
    s2.flush().unwrap();
    let out = std::fs::read_to_string(dir.path().join("servers/hand.toml")).unwrap();
    for keep in [
        "# Hand-written server",
        "# trailing comment",
        "# routes below",
        "# keep me",
        "future_field = 7",
    ] {
        assert!(out.contains(keep), "lost `{keep}` in:\n{out}");
    }
    assert!(
        out.contains("status = 201") && out.contains("name = \"Alpha\"") && out.contains("x-test")
    );
    assert!(out.find("id = \"srv_hand\"").unwrap() < out.find("port = 4000").unwrap());
}

#[test]
fn crud_undo_and_ids() {
    let (_d, s) = ws();
    let srv = s.create_server("Payments API", 3002).unwrap();
    let fld = s.create_folder(&srv, "Charges").unwrap();
    let rte = s
        .create_route(&srv, Some(&fld), "post", "/charges")
        .unwrap();
    let rsp = s.create_response(&rte).unwrap();
    let dup = s.duplicate(&Target::Route(rte.clone())).unwrap();
    assert_ne!(dup, rte);
    let snap = s.snapshot();
    let (file, p) = snap.servers.iter().find(|(_, x)| x.id == srv).unwrap();
    assert_eq!(file, "servers/payments-api.toml");
    assert_eq!(p.routes.len(), 2);
    assert_eq!(p.routes[0].method, "POST");
    assert!(snap.diagnostics.is_empty(), "{:?}", snap.diagnostics);
    s.delete(&Target::Response(rsp)).unwrap();
    assert!(s.undo(file));
    assert!(
        s.snapshot()
            .servers
            .iter()
            .any(|(_, x)| x.routes[0].responses.len() == 2)
    );
    assert!(s.redo(file));
    s.delete(&Target::Folder(fld)).unwrap();
    assert!(
        s.snapshot()
            .servers
            .iter()
            .find(|(_, x)| x.id == srv)
            .unwrap()
            .1
            .routes
            .iter()
            .all(|r| r.folder.is_none())
    );
}

#[test]
fn rename_moves_the_file_but_keeps_the_id() {
    let (dir, s) = ws();
    let id = s.snapshot().servers[0].1.id.clone();
    s.rename_server(&id, "Renamed Thing").unwrap();
    s.flush().unwrap();
    assert!(dir.path().join("servers/renamed-thing.toml").is_file());
    assert!(!dir.path().join("servers/example-api.toml").exists());
    assert_eq!(s.snapshot().servers[0].1.id, id);
    assert_eq!(slugify("  A / b  "), "a-b");
}

#[test]
fn validate_catches_duplicate_ports() {
    let (_d, s) = ws();
    s.create_server("Other", 3001).unwrap();
    assert!(s.snapshot().diagnostics.iter().any(|d| d.code == "E020"));
}

fn wait(mut f: impl FnMut() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    false
}

#[test]
fn autosave_is_debounced_and_ignored_by_the_watcher() {
    let (dir, s) = ws();
    let rx = s.subscribe();
    let _bg = s.start_background(Duration::from_millis(100)).unwrap();
    let id = s.snapshot().servers[0].1.id.clone();
    s.rename_server(&id, "Example API").unwrap();
    s.set_int(&Target::Server(id.clone()), "port", Some(3999))
        .unwrap();
    assert!(wait(|| std::fs::read_to_string(
        dir.path().join("servers/example-api.toml")
    )
    .unwrap()
    .contains("3999")));
    std::thread::sleep(Duration::from_millis(300));
    assert!(!s.is_dirty("servers/example-api.toml"));
    while rx.try_recv().is_ok() {}
    std::thread::sleep(Duration::from_millis(300));
    assert!(rx.try_recv().is_err(), "own write must not reload");
}

#[test]
fn external_edit_reloads_when_clean_and_conflicts_when_dirty() {
    let (dir, s) = ws();
    let rx = s.subscribe();
    let _bg = s.start_background(Duration::from_secs(60)).unwrap();
    let path = dir.path().join("servers/example-api.toml");
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("3001", "4242");
    std::fs::write(&path, &text).unwrap();
    assert!(
        wait(|| s.snapshot().servers[0].1.port == 4242),
        "silent reload"
    );
    // Now an unsaved GUI edit collides with another external edit.
    let id = s.snapshot().servers[0].1.id.clone();
    s.set_str(&Target::Server(id), "docs", Some("mine"))
        .unwrap();
    std::fs::write(&path, text.replace("4242", "5555")).unwrap();
    let mut saw = false;
    assert!(wait(|| {
        while let Ok(e) = rx.try_recv() {
            if matches!(e, Event::MergeConflict { .. }) {
                saw = true;
            }
        }
        saw
    }));
    assert_eq!(s.conflicts(), vec!["servers/example-api.toml".to_string()]);
    s.resolve_conflict("servers/example-api.toml", false)
        .unwrap();
    assert_eq!(s.snapshot().servers[0].1.port, 5555);
    assert!(s.conflicts().is_empty());
}
