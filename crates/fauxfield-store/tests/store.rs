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
    assert!(
        snap.diagnostics.iter().all(|d| d.code == "W051"),
        "{:?}",
        snap.diagnostics
    );
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

#[test]
fn workspace_rename_keeps_comments_undoes_and_reloads_externally() {
    let dir = tempfile::tempdir().unwrap();
    init_workspace(dir.path()).unwrap();
    let manifest = dir.path().join("fauxfield.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(
        &manifest,
        format!(
            "# top comment\n{}",
            text.replace("[variables]", "# vars\n[variables]")
        ),
    )
    .unwrap();
    let s = Store::open(dir.path()).unwrap();
    s.set_workspace_name("  Renamed  ").unwrap();
    assert_eq!(s.snapshot().manifest.name, "Renamed");
    let t = s.text("fauxfield.toml").unwrap();
    assert!(t.starts_with("# top comment\n") && t.contains("# vars\n[variables]"));
    assert!(t.find("schema").unwrap() < t.find("name").unwrap());
    assert!(s.set_workspace_name("").is_err());
    assert!(s.set_workspace_name("a\nb").is_err());
    assert!(s.undo("fauxfield.toml"));
    assert_ne!(s.snapshot().manifest.name, "Renamed");
    assert!(s.redo("fauxfield.toml"));
    s.flush().unwrap();
    assert!(
        std::fs::read_to_string(&manifest)
            .unwrap()
            .contains("Renamed")
    );

    let _bg = s.start_background(Duration::from_secs(60)).unwrap();
    let ext = std::fs::read_to_string(&manifest)
        .unwrap()
        .replace("Renamed", "External");
    std::fs::write(&manifest, ext).unwrap();
    assert!(wait(|| s.snapshot().manifest.name == "External"));
}

#[test]
fn init_never_overwrites_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("environments")).unwrap();
    std::fs::write(
        dir.path().join("environments/local.toml"),
        "name = \"mine\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join(".gitignore"), "keep\n").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "hi").unwrap();
    init_workspace(dir.path()).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("environments/local.toml")).unwrap(),
        "name = \"mine\"\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".gitignore")).unwrap(),
        "keep\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
        "hi"
    );
    assert!(dir.path().join("fauxfield.toml").is_file());
}

#[test]
fn duplicate_gets_new_id_max_port_and_unique_names() {
    let (_d, s) = ws();
    let id = s.snapshot().servers[0].1.id.clone();
    s.create_server("Other", 4000).unwrap();
    let c1 = s.duplicate_server(&id).unwrap();
    let c2 = s.duplicate_server(&id).unwrap();
    let snap = s.snapshot();
    let get = |i: &str| {
        snap.servers
            .iter()
            .find(|(_, x)| x.id == i)
            .unwrap()
            .1
            .clone()
    };
    assert_ne!(c1, id);
    assert_eq!(get(&c1).name, "Example API copy");
    assert_eq!(get(&c2).name, "Example API copy 2");
    assert_eq!(get(&c1).port, 4001);
    assert_eq!(get(&c2).port, 4002);
}

#[test]
fn problems_fixture_has_exactly_three_errors() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/problems-workspace");
    let s = Store::open(&root).unwrap();
    let errors = s
        .snapshot()
        .diagnostics
        .iter()
        .filter(|d| d.severity == fauxfield_model::Severity::Error)
        .count();
    assert_eq!(errors, 3);
}

const FOLDERS: &str = r#"id = "srv_f"
name = "F"
port = 4100

[[folders]]
id = "fld_a"
name = "A"

[[folders]]
id = "fld_b"
name = "B"

# first
[[routes]]
id = "r1"
folder = "fld_a"
path = "/one"

[[routes]]
id = "r2"
folder = "fld_a"
path = "/two"

[[routes]]
id = "r3"
path = "/three"
"#;

fn ids(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| {
            l.strip_prefix("id = \"r")
                .map(|x| format!("r{}", x.trim_end_matches('"')))
        })
        .collect()
}

#[test]
fn route_and_folder_moves_keep_comments_and_defaults() {
    let (dir, s) = ws();
    let f = dir.path().join("servers/f.toml");
    std::fs::write(&f, FOLDERS).unwrap();
    let s2 = Store::open(dir.path()).unwrap();
    drop(s);
    s2.move_route("r1", Some("fld_b"), None).unwrap();
    s2.move_route("r3", Some("fld_a"), Some("r2")).unwrap();
    s2.flush().unwrap();
    let out = std::fs::read_to_string(&f).unwrap();
    assert!(out.contains("# first"), "{out}");
    assert_eq!(ids(&out), ["r3", "r2", "r1"].map(String::from), "{out}");
    // no-op move writes nothing
    assert!(!s2.is_dirty("servers/f.toml"));
    s2.move_route("r3", Some("fld_a"), Some("r2")).unwrap();
    assert!(!s2.is_dirty("servers/f.toml"));
    s2.move_folder("fld_b", Some("fld_a")).unwrap();
    s2.set_route_priority("r2", 5).unwrap();
    s2.set_route_priority("r2", 0).unwrap();
    s2.set_folder_enabled("fld_a", false).unwrap();
    s2.set_folder_collapsed("fld_a", true).unwrap();
    s2.set_route_match("r2", Some("regex")).unwrap();
    assert!(s2.set_route_match("r2", Some("bogus")).is_err());
    s2.set_route_match("r2", None).unwrap();
    s2.flush().unwrap();
    let out = std::fs::read_to_string(&f).unwrap();
    assert!(out.find("fld_b").unwrap() < out.find("fld_a").unwrap());
    assert!(!out.contains("priority") && !out.contains("match"));
    assert!(out.contains("enabled = false") && out.contains("collapsed = true"));
    s2.set_folder_enabled("fld_a", true).unwrap();
    s2.flush().unwrap();
    assert!(!std::fs::read_to_string(&f).unwrap().contains("enabled"));
    s2.delete_folder("fld_a", true).unwrap();
    s2.flush().unwrap();
    let out = std::fs::read_to_string(&f).unwrap();
    assert_eq!(ids(&out), ["r1"].map(String::from), "{out}");
}

#[test]
fn headers_keep_file_order_and_rename_in_place() {
    let (dir, s) = ws();
    let f = dir.path().join("servers/h.toml");
    std::fs::write(
        &f,
        r#"id = "srv_h"
name = "H"
port = 4200

[[routes]]
id = "rt"
path = "/h"

  [[routes.responses]]
  id = "rs"
  status = 200
  [routes.responses.headers]
  zeta = "1"
  # keep
  alpha = "2"
  mid = "3"
"#,
    )
    .unwrap();
    let s2 = Store::open(dir.path()).unwrap();
    drop(s);
    let order = |s: &Store| {
        s.snapshot()
            .servers
            .iter()
            .find(|(_, x)| x.id == "srv_h")
            .unwrap()
            .1
            .routes[0]
            .responses[0]
            .headers
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(order(&s2), ["zeta", "alpha", "mid"]);
    s2.rename_header("rs", "alpha", "beta").unwrap();
    assert_eq!(order(&s2), ["zeta", "beta", "mid"]);
    assert!(s2.rename_header("rs", "beta", "MID").is_err());
    s2.set_header("rs", "new", Some("")).unwrap();
    assert_eq!(order(&s2), ["zeta", "beta", "mid", "new"]);
    s2.flush().unwrap();
    let out = std::fs::read_to_string(&f).unwrap();
    assert!(
        out.contains("# keep") && out.contains("beta = \"2\""),
        "{out}"
    );
}

#[test]
fn rules_reorder_and_presets_round_trip() {
    let (_d, s) = ws();
    let srv = s.create_server("Rules", 3050).unwrap();
    let rte = s.create_route(&srv, None, "get", "/r").unwrap();
    let a = s.create_response(&rte).unwrap();
    let b = s.create_response(&rte).unwrap();
    let rule = |k: &str, v: &str| fauxfield_model::Rule {
        target: "query".into(),
        key: Some(k.into()),
        op: "eq".into(),
        value: Some(toml::Value::String(v.into())),
        negate: false,
        extra: Default::default(),
    };
    s.set_rule(&a, None, &rule("role", "admin")).unwrap();
    s.set_rule(&a, None, &rule("x", "1")).unwrap();
    s.set_rule(&b, None, &rule("y", "2")).unwrap();
    s.move_rule(&a, 1, 0).unwrap();
    s.move_response(&a, None).unwrap(); // a after b
    let p = s.create_error_preset(&rte, 401).unwrap();
    s.set_response_disabled(&p, true).unwrap();
    s.set_response_mode(&rte, "sequential").unwrap();
    s.flush().unwrap();
    let snap = s.snapshot();
    let file = s.server_file(&srv).unwrap();
    let text = s.text(&file).unwrap();
    let parsed = fauxfield_model::parse_server(&text).unwrap();
    let route = &parsed.routes[0];
    assert_eq!(route.response_mode.as_deref(), Some("sequential"));
    assert_eq!(route.responses[1].id, b);
    assert_eq!(route.responses[1].rules[0].key.as_deref(), Some("y"));
    assert_eq!(route.responses[2].id, a);
    assert_eq!(route.responses[2].rules[0].key.as_deref(), Some("x"));
    assert_eq!(route.responses[2].rules.len(), 2);
    assert!(route.responses[3].disabled);
    assert_eq!(route.responses[3].status, 401);
    assert!(fauxfield_model::validate_servers(&[(file, parsed)]).is_empty());
    let _ = snap;
    s.delete_rule(&a, 0).unwrap();
    assert!(s.set_response_mode(&rte, "bogus").is_err());
}

#[test]
fn route_headers_seed_and_template_round_trip() {
    let (_d, s) = ws();
    let srv = s.create_server("Tpl API", 3010).unwrap();
    let rte = s.create_route(&srv, None, "get", "/t").unwrap();
    let rsp = s.create_response(&rte).unwrap();
    s.set_route_header(&rte, "X-Id", Some("{{ uuid() }}"))
        .unwrap();
    s.set_route_header(&rte, "X-Tmp", Some("1")).unwrap();
    s.set_route_header(&rte, "X-Tmp", None).unwrap();
    s.set_fake_seed(&srv, Some(42)).unwrap();
    s.set_response_template(&rsp, false).unwrap();
    let snap = s.snapshot();
    let sv = snap
        .servers
        .iter()
        .find(|(_, x)| x.id == srv)
        .unwrap()
        .1
        .clone();
    assert_eq!(sv.fake_seed, Some(42));
    let r = &sv.routes[0];
    assert_eq!(r.headers.len(), 1);
    assert_eq!(r.headers["X-Id"], "{{ uuid() }}");
    assert_eq!(
        r.responses.iter().find(|x| x.id == rsp).unwrap().template,
        Some(false)
    );
    s.set_fake_seed(&srv, None).unwrap();
    s.set_response_template(&rsp, true).unwrap();
    let sv = s
        .snapshot()
        .servers
        .iter()
        .find(|(_, x)| x.id == srv)
        .unwrap()
        .1
        .clone();
    assert_eq!(sv.fake_seed, None);
    assert_eq!(
        sv.routes[0]
            .responses
            .iter()
            .find(|x| x.id == rsp)
            .unwrap()
            .template,
        None
    );
}
