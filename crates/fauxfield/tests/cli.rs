//! CLI snapshot tests (assert_cmd + insta). `--json` output is a stable schema (standing rule 6).

use assert_cmd::Command;

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::cargo_bin("fauxfield")
        .unwrap()
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

#[test]
fn version_json_is_stable() {
    let (code, out, _) = run(&["version", "--json"]);
    assert_eq!(code, 0);
    let mut v: serde_json::Value = serde_json::from_str(&out).unwrap();
    // The version moves with every release; the schema must not.
    v["version"] = "<version>".into();
    assert!(v["gui"].is_boolean());
    v["gui"] = "<bool>".into();
    // Redact per-commit / per-host build facts.
    for k in ["commit", "commit_date", "target", "profile"] {
        assert!(v[k].is_string(), "{k}");
        v[k] = format!("<{k}>").into();
    }
    assert!(v["dirty"].is_boolean());
    v["dirty"] = "<bool>".into();
    assert!(v["gpui"].is_string());
    snap_json(Some("version_json_is_stable"), &v);
}

#[test]
fn version_plain() {
    let (code, out, _) = run(&["version"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("fauxfield "));
}

#[test]
fn completions_for_every_shell_mention_the_binary() {
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let (code, out, _) = run(&["completions", shell]);
        assert_eq!(code, 0, "{shell}");
        assert!(out.contains("fauxfield"), "{shell}");
        assert!(
            out.contains("completions") && out.contains("version"),
            "{shell}"
        );
    }
}

#[test]
fn unknown_subcommand_is_rejected_as_a_missing_folder() {
    let (code, _, err) = run(&["doctor"]);
    assert_eq!(code, 1);
    assert!(err.contains("not a folder"));
}

#[test]
fn bad_flag_is_a_usage_error() {
    let (code, _, err) = run(&["version", "--nope"]);
    assert_eq!(code, 2);
    assert!(err.contains("--nope"));
}

#[test]
fn mcp_help_is_stable() {
    // `fauxfield mcp` speaks MCP on stdout, so it has no `--json`; its help is the CLI contract.
    let (code, out, _) = run(&["mcp", "--help"]);
    assert_eq!(code, 0);
    insta::assert_snapshot!(out);
}

/// Release builds have no in-app MCP host and no `--mcp-*` flags (SPEC §11.4).
#[cfg(not(feature = "dev-mcp"))]
#[test]
fn dev_mcp_flags_are_absent_without_the_feature() {
    let (_, help, _) = run(&["--help"]);
    assert!(!help.contains("--mcp-socket") && !help.contains("--mcp-stdio"));
    let (code, _, err) = run(&["--mcp-stdio"]);
    assert_eq!(code, 2);
    assert!(err.contains("--mcp-stdio"));
}

#[cfg(feature = "dev-mcp")]
#[test]
fn dev_mcp_flags_are_listed_with_the_feature() {
    let (_, help, _) = run(&["--help"]);
    assert!(help.contains("--mcp-socket") && help.contains("--mcp-stdio"));
}

/// JSON snapshot with sorted keys: key order is not part of the schema (serde_json's
/// `preserve_order` is enabled by some feature sets).
fn snap_json(name: Option<&str>, v: &serde_json::Value) {
    insta::with_settings!({sort_maps => true}, {
        match name {
            Some(n) => insta::assert_json_snapshot!(n, v),
            None => insta::assert_json_snapshot!(v),
        }
    });
}

fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let (code, _, _) = run(&["init", dir.path().to_str().unwrap()]);
    assert_eq!(code, 0);
    dir
}

/// Replace the random IDs so snapshots are stable.
fn redact(mut s: String) -> String {
    for prefix in ["srv_", "rte_", "rsp_"] {
        while let Some(i) = s.find(prefix) {
            s.replace_range(i..i + prefix.len() + 8, &format!("<{}-id>", &prefix[..3]));
        }
    }
    s
}

#[test]
fn init_json_is_stable() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().to_str().unwrap();
    let (code, out, _) = run(&["init", p, "--json"]);
    assert_eq!(code, 0);
    let mut v: serde_json::Value = serde_json::from_str(&out).unwrap();
    v["path"] = "<path>".into();
    snap_json(Some("init_json_is_stable"), &v);
    // Refuses to overwrite an existing workspace.
    assert_eq!(run(&["init", p]).0, 1);
}

#[test]
fn validate_json_is_stable_and_exit_code_3_on_errors() {
    let dir = workspace();
    let p = dir.path().to_str().unwrap();
    let (code, out, _) = run(&["validate", "-w", p, "--json"]);
    assert_eq!(code, 0);
    snap_json(Some("validate_clean"), &serde_json::from_str(&out).unwrap());
    // A second enabled server on the same port is a validation error.
    let dup = std::fs::read_to_string(dir.path().join("servers/example-api.toml")).unwrap();
    let dup = dup
        .replace("srv_", "srv_x")
        .replace("rte_", "rte_x")
        .replace("rsp_", "rsp_x");
    std::fs::write(dir.path().join("servers/dup.toml"), dup).unwrap();
    let (code, out, _) = run(&["validate", "-w", p, "--json"]);
    assert_eq!(code, 3);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["ok"], false);
    assert!(
        v["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "E020")
    );
}

#[test]
fn server_and_route_list_json_are_stable() {
    let dir = workspace();
    let p = dir.path().to_str().unwrap();
    let (code, out, _) = run(&["server", "list", "-w", p, "--json"]);
    assert_eq!(code, 0);
    snap_json(
        Some("server_list"),
        &serde_json::from_str(&redact(out)).unwrap(),
    );
    let (code, out, _) = run(&[
        "route",
        "list",
        "-w",
        p,
        "--server",
        "example-api",
        "--json",
    ]);
    assert_eq!(code, 0);
    snap_json(
        Some("route_list"),
        &serde_json::from_str(&redact(out)).unwrap(),
    );
    let (_, out, _) = run(&["route", "list", "-w", p, "--server", "nope", "--json"]);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&out).unwrap()["routes"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn not_a_workspace_is_a_runtime_error() {
    let dir = tempfile::tempdir().unwrap();
    let (code, _, err) = run(&["validate", "-w", dir.path().to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(err.contains("not a workspace"));
}

#[test]
fn workspace_info_and_rename_are_stable() {
    let dir = workspace();
    let d = dir.path().to_str().unwrap();
    let (code, out, _) = run(&["workspace", "info", "-w", d, "--json"]);
    assert_eq!(code, 0);
    let mut v: serde_json::Value = serde_json::from_str(&out).unwrap();
    v["root"] = "<root>".into();
    v["name"] = "<name>".into();
    snap_json(Some("workspace_info"), &v);

    let (code, out, _) = run(&["workspace", "rename", "My Mocks", "-w", d, "--json"]);
    assert_eq!(code, 0);
    let mut v: serde_json::Value = serde_json::from_str(&out).unwrap();
    v["root"] = "<root>".into();
    snap_json(Some("workspace_rename"), &v);
    let text = std::fs::read_to_string(dir.path().join("fauxfield.toml")).unwrap();
    assert!(text.contains("name = \"My Mocks\""), "{text}");

    // Invalid names exit 3 and change nothing.
    for bad in ["", "   ", "a\nb"] {
        let (code, _, err) = run(&["workspace", "rename", bad, "-w", d]);
        assert_eq!(code, 3, "{bad:?}");
        assert!(err.starts_with("error:"));
    }
    assert!(
        std::fs::read_to_string(dir.path().join("fauxfield.toml"))
            .unwrap()
            .contains("My Mocks")
    );
}

#[test]
fn route_add_variants_are_stable() {
    let dir = workspace();
    let p = dir.path().to_str().unwrap();
    let add = |args: &[&str]| {
        let mut a = vec!["route", "add", "example-api"];
        a.extend_from_slice(args);
        a.extend_from_slice(&["-w", p, "--json"]);
        run(&a)
    };
    let (code, out, _) = add(&["get", "/users/:id", "--name", "User"]);
    assert_eq!(code, 0, "{out}");
    let mut v: serde_json::Value = serde_json::from_str(&out).unwrap();
    v["id"] = "<id>".into();
    v["server"] = "<srv>".into();
    snap_json(Some("route_add_param"), &v);
    let (code, out, _) = add(&[
        "any",
        "^/v[0-9]+/x$",
        "--match",
        "regex",
        "--priority",
        "-2",
    ]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["match"], "regex");
    assert_eq!(v["priority"], -2);
    let (code, _, err) = add(&["get", "/x", "--folder", "nope"]);
    assert_eq!((code, err.contains("unknown folder")), (2, true));
    let (code, _, err) = add(&["get", "/a/*x/b"]);
    assert_eq!((code, err.contains("last segment")), (3, true));
    // duplicate pattern: succeeds with a shadow warning
    let (code, out, _) = add(&["get", "/users/:id"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["warnings"][0]["code"], "W051");
}

#[test]
fn response_add_and_set_default_are_stable() {
    let dir = workspace();
    let p = dir.path().to_str().unwrap();
    let (code, out, _) = run(&[
        "route",
        "add",
        "example-api",
        "get",
        "/r",
        "-w",
        p,
        "--json",
    ]);
    assert_eq!(code, 0, "{out}");
    let route = serde_json::from_str::<serde_json::Value>(&out).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (code, out, _) = run(&[
        "response", "add", &route, "--name", "Teapot", "--status", "418", "--body", "short", "-w",
        p, "--json",
    ]);
    assert_eq!(code, 0, "{out}");
    let mut v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let rsp = v["id"].as_str().unwrap().to_string();
    v["id"] = "<id>".into();
    v["route"] = "<route>".into();
    snap_json(Some("response_add"), &v);
    let (code, out, _) = run(&["response", "set-default", &rsp, "-w", p, "--json"]);
    assert_eq!(code, 0, "{out}");
    let mut v: serde_json::Value = serde_json::from_str(&out).unwrap();
    v["id"] = "<id>".into();
    snap_json(Some("response_set_default"), &v);
    let (code, out, _) = run(&[
        "response", "add", &route, "--preset", "401", "-w", p, "--json",
    ]);
    assert_eq!(code, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["status"], 401);
    let (code, _, _) = run(&["response", "set-default", "rsp_nope", "-w", p]);
    assert_eq!(code, 2);
}

#[test]
fn render_is_stable_and_seed_deterministic() {
    let dir = workspace();
    let p = dir.path().to_str().unwrap();
    let (_, out, _) = run(&[
        "route",
        "add",
        "example-api",
        "get",
        "/r",
        "-w",
        p,
        "--json",
    ]);
    let route = serde_json::from_str::<serde_json::Value>(&out).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (code, out, _) = run(&[
        "response",
        "add",
        &route,
        "--status",
        "200",
        "--body",
        "{{ request.method }} {{ fake(kind='name') }}",
        "-w",
        p,
        "--json",
    ]);
    assert_eq!(code, 0, "{out}");
    let rsp = serde_json::from_str::<serde_json::Value>(&out).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let args = [
        "render",
        "example-api",
        &route,
        "--response",
        &rsp,
        "--seed",
        "5",
        "-w",
        p,
        "--json",
    ];
    let (code, out, _) = run(&args);
    assert_eq!(code, 0, "{out}");
    let mut v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let again: serde_json::Value = serde_json::from_str(&run(&args).1).unwrap();
    assert_eq!(v["body"], again["body"]);
    assert!(v["body"].as_str().unwrap().starts_with("GET "));
    v["body"] = "GET <name>".into();
    v["route"] = "<route>".into();
    v["response"] = "<response>".into();
    v["render_ms"] = 0.into();
    v["server"] = "<server>".into();
    snap_json(Some("render"), &v);
    let (code, _, _) = run(&["render", "example-api", "rte_nope", "-w", p]);
    assert_eq!(code, 2);
}

#[test]
fn env_and_var_commands_are_stable() {
    let dir = workspace();
    let p = dir.path().to_str().unwrap();
    let json = |args: &[&str]| -> serde_json::Value {
        let mut a = args.to_vec();
        a.extend(["-w", p, "--json"]);
        let (code, out, err) = run(&a);
        assert_eq!(code, 0, "{args:?}: {err}");
        serde_json::from_str(&out).unwrap()
    };
    let mut v = json(&["env", "list"]);
    snap_json(Some("env_list"), &v);
    v = json(&["env", "add", "staging"]);
    snap_json(Some("env_add"), &v);
    v = json(&["env", "use", "staging"]);
    snap_json(Some("env_use"), &v);
    v = json(&[
        "var",
        "set",
        "api_token",
        "s3cret",
        "--scope",
        "environment",
        "--env",
        "staging",
    ]);
    snap_json(Some("var_set_secret"), &v);
    v = json(&[
        "var",
        "set",
        "base",
        "http://s",
        "--scope",
        "environment",
        "--env",
        "staging",
    ]);
    snap_json(Some("var_set"), &v);
    v = json(&["var", "get", "base"]);
    snap_json(Some("var_get"), &v);
    v = json(&[
        "var",
        "unset",
        "base",
        "--scope",
        "environment",
        "--env",
        "staging",
    ]);
    snap_json(Some("var_unset"), &v);
    let (code, _, _) = run(&["var", "get", "base", "-w", p]);
    assert_eq!(code, 1);
    let (code, _, _) = run(&["var", "set", "x", "1", "-w", p]);
    assert_eq!(code, 2);
    let (code, _, _) = run(&["var", "set", "x", "1", "--scope", "runtime", "-w", p]);
    assert_eq!(code, 4, "runtime scope needs a host");
}

#[test]
fn data_commands_are_stable() {
    let dir = workspace();
    let p = dir.path().to_str().unwrap();
    std::fs::write(
        dir.path().join("data/people.json"),
        r#"[{"id":1,"name":"Ann"},{"id":2,"name":"Bob"}]"#,
    )
    .unwrap();
    let f = dir.path().join("servers/example-api.toml");
    let mut t = std::fs::read_to_string(&f).unwrap();
    t.push_str("\n[[datasets]]\nname = \"people\"\nseed = \"data/people.json\"\nid_strategy = \"increment\"\n");
    std::fs::write(&f, t).unwrap();
    let json = |args: &[&str]| -> serde_json::Value {
        let mut a = args.to_vec();
        a.extend(["-w", p, "--json"]);
        let (code, out, err) = run(&a);
        assert_eq!(code, 0, "{args:?}: {err}");
        serde_json::from_str(&out).unwrap()
    };
    snap_json(Some("data_list"), &json(&["data", "list"]));
    snap_json(
        Some("data_show"),
        &json(&["data", "show", "example-api", "people"]),
    );
    let out = dir.path().join("out.json");
    let v = json(&[
        "data",
        "export",
        "example-api",
        "people",
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(v["rows"], 2);
    let src = dir.path().join("in.json");
    std::fs::write(&src, r#"[{"id":7,"name":"Zed"}]"#).unwrap();
    snap_json(
        Some("data_import"),
        &json(&[
            "data",
            "import",
            "example-api",
            "people",
            src.to_str().unwrap(),
        ]),
    );
    assert_eq!(json(&["data", "list"])["datasets"][0]["rows"], 1);
    snap_json(Some("data_reset"), &json(&["data", "reset"]));
    let (code, _, _) = run(&["data", "show", "example-api", "nope", "-w", p]);
    assert_eq!(code, 1);
}
