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
    let (code, _, err) = run(&["tail"]);
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
    // Under --json a failure is one error document on stdout (SPEC §10).
    let (code, out, _) = add(&["get", "/x", "--folder", "nope"]);
    assert_eq!((code, out.contains("unknown folder")), (2, true));
    let e: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        (e["schema"].as_str(), e["ok"].as_bool()),
        (Some("fauxfield.error/v1"), Some(false))
    );
    let (code, out, _) = add(&["get", "/a/*x/b"]);
    assert_eq!((code, out.contains("last segment")), (3, true));
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
    // M10.05: CSV append (ragged row warns), then export to every format and re-import.
    let csv = dir.path().join("more.csv");
    std::fs::write(&csv, "id,name\n8,Amy\n9\n").unwrap();
    let v = json(&[
        "data",
        "import",
        "example-api",
        "people",
        csv.to_str().unwrap(),
        "--append",
    ]);
    assert_eq!(v["rows"], 3);
    assert_eq!(v["format"], "csv");
    assert!(v["warnings"][0].as_str().unwrap().contains("ragged"));
    for ext in ["csv", "parquet", "json"] {
        let f = dir.path().join(format!("rt.{ext}"));
        let v = json(&[
            "data",
            "export",
            "example-api",
            "people",
            "--out",
            f.to_str().unwrap(),
        ]);
        assert_eq!(v["format"], ext);
        assert_eq!(v["rows"], 3);
        let v = json(&[
            "data",
            "import",
            "example-api",
            "people",
            f.to_str().unwrap(),
        ]);
        assert_eq!(v["rows"], 3, "{ext}");
    }
    let (code, _, _) = run(&[
        "data",
        "export",
        "example-api",
        "people",
        "--format",
        "parquet",
        "-w",
        p,
    ]);
    assert_eq!(code, 1);
    let (code, _, _) = run(&[
        "data",
        "import",
        "example-api",
        "people",
        csv.to_str().unwrap(),
        "--hard-limit",
        "1",
        "-w",
        p,
    ]);
    assert_eq!(code, 1);
    json(&[
        "data",
        "import",
        "example-api",
        "people",
        src.to_str().unwrap(),
    ]);
    snap_json(Some("data_reset"), &json(&["data", "reset"]));
    let (code, _, _) = run(&["data", "show", "example-api", "nope", "-w", p]);
    assert_eq!(code, 1);
    // M10.01 lifecycle
    snap_json(
        Some("data_create"),
        &json(&["data", "create", "example-api", "orders"]),
    );
    snap_json(
        Some("data_refs"),
        &json(&["data", "refs", "example-api", "orders"]),
    );
    snap_json(
        Some("data_rename"),
        &json(&["data", "rename", "example-api", "orders", "purchases"]),
    );
    snap_json(
        Some("data_delete"),
        &json(&["data", "delete", "example-api", "purchases"]),
    );
    let (code, _, _) = run(&["data", "delete", "example-api", "purchases", "-w", p]);
    assert_eq!(code, 1, "already gone");
}

#[test]
fn run_rejects_bad_network_and_chaos_values_with_exit_2() {
    let (code, _, err) = run(&["run", "--network", "dialup"]);
    assert_eq!(code, 2);
    assert!(
        err.contains("fast-3g") && err.contains("flaky-wifi"),
        "{err}"
    );
    let (code, _, err) = run(&["run", "--chaos", "1.5"]);
    assert_eq!(code, 2);
    assert!(err.contains("between 0 and 1"), "{err}");
}

/// `run` with the dev CA directory pointed at a temp dir (never the real platform one).
fn run_with_data(data: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::cargo_bin("fauxfield")
        .unwrap()
        .env("FAUXFIELD_DATA_DIR", data)
        .env("FAUXFIELD_SETTINGS", data.join("settings.toml"))
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
fn certs_and_doctor_are_stable() {
    let data = tempfile::tempdir().unwrap();
    let ws = workspace();
    let w = ws.path().to_str().unwrap();
    let redact = |mut v: serde_json::Value| {
        for k in [
            "path",
            "trust",
            "trust_command",
            "untrust_command",
            "fingerprint",
            "not_after",
            "days_left",
            "workspace",
            "command",
        ] {
            if v.get(k).is_some_and(|x| !x.is_null()) {
                v[k] = format!("<{k}>").into();
            }
        }
        v
    };
    let (code, out, _) = run_with_data(data.path(), &["certs", "status", "--json"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["exists"], false);
    snap_json(Some("certs_status_none"), &redact(v));
    // `trust --json` creates the CA, prints the command and never runs it.
    let (code, out, _) = run_with_data(data.path(), &["certs", "trust", "--json"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["ran"], false);
    assert!(v["command"].as_str().unwrap().contains("ca.pem"));
    snap_json(Some("certs_trust"), &redact(v));
    let (_, out, _) = run_with_data(data.path(), &["certs", "status", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["exists"], true);
    assert!(v["fingerprint"].is_string() && v["days_left"].as_i64().unwrap() > 3000);
    snap_json(Some("certs_status_created"), &redact(v));
    // Without --yes and without a terminal the command is printed, not run.
    let (code, out, err) = run_with_data(data.path(), &["certs", "untrust"]);
    assert_eq!(code, 0);
    assert!(out.contains("ca") && err.contains("Not run"));
    let (code, out, _) = run_with_data(data.path(), &["doctor", "-w", w, "--json"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let mut v = redact(v);
    v["version"] = "<version>".into();
    v["gui"] = "<bool>".into();
    v["ports"] = v["ports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| serde_json::json!({"server": p["server"], "available": "<bool>"}))
        .collect::<Vec<_>>()
        .into();
    v["ca"] = redact(v["ca"].clone());
    snap_json(Some("doctor"), &v);
}

#[test]
fn data_generate_is_deterministic_and_bakes() {
    let dir = workspace();
    let p = dir.path().to_str().unwrap();
    let f = dir.path().join("servers/example-api.toml");
    let mut t = std::fs::read_to_string(&f).unwrap();
    t.push_str(
        r#"
[[datasets]]
name = "users"
[datasets.generator]
rows = 4
[[datasets.generator.fields]]
name = "id"
kind = "sequence"
[[datasets.generator.fields]]
name = "email"
kind = "fake"
fake = "internet.email"
[[datasets.generator.fields]]
name = "role"
kind = "one_of"
values = ["admin", "user"]
"#,
    );
    std::fs::write(&f, t).unwrap();
    let gen_rows = |extra: &[&str]| {
        let mut a = vec![
            "data",
            "generate",
            "example-api",
            "users",
            "-w",
            p,
            "--json",
        ];
        a.extend(extra);
        let (code, out, err) = run(&a);
        assert_eq!(code, 0, "{err}");
        out
    };
    let a = gen_rows(&["--seed", "42"]);
    assert_eq!(a, gen_rows(&["--seed", "42"]), "same seed, byte-identical");
    assert_ne!(a, gen_rows(&["--seed", "43"]));
    let v: serde_json::Value = serde_json::from_str(&a).unwrap();
    assert_eq!(v["rows"], 4);
    assert_eq!(
        gen_rows(&["--seed", "42", "--rows", "2"])
            .matches("\"email\"")
            .count(),
        2
    );
    // Bake: static seed replaces the generator.
    let b: serde_json::Value =
        serde_json::from_str(&gen_rows(&["--seed", "42", "--bake"])).unwrap();
    assert_eq!(b["baked"], "data/users.json");
    let baked: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("data/users.json")).unwrap())
            .unwrap();
    assert_eq!(baked, v["data"]);
    let server = std::fs::read_to_string(&f).unwrap();
    assert!(!server.contains("generator"), "{server}");
    let (code, _, _) = run(&["data", "generate", "example-api", "users", "-w", p]);
    assert_eq!(code, 1, "no generator any more");
}

#[test]
fn agents_init_dry_run_yes_rerun_and_usage() {
    let dir = workspace();
    let p = dir.path().to_str().unwrap();
    let files = |d: &std::path::Path| {
        let mut v: Vec<String> = std::fs::read_dir(d)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    };
    let before = files(dir.path());
    // No client flag and no TTY: usage error 2.
    let (code, _, err) = run(&["agents", "init", "-w", p]);
    assert_eq!(code, 2);
    assert!(err.contains("no client chosen"), "{err}");
    // Bad class: usage error 2.
    let (code, _, _) = run(&["agents", "init", "--claude", "--allow", "root", "-w", p]);
    assert_eq!(code, 2);
    // --dry-run prints the diff and writes nothing.
    let (code, out, _) = run(&["agents", "init", "--claude", "--dry-run", "-w", p]);
    assert_eq!(code, 0);
    assert!(out.contains("+++ b/.mcp.json"), "{out}");
    assert_eq!(files(dir.path()), before);
    // --json without --yes and no TTY: refuses rather than hanging.
    let (code, _, _) = run(&["agents", "init", "--claude", "--json", "-w", p]);
    assert_eq!(code, 2);
    assert_eq!(files(dir.path()), before);
    // --yes writes; the JSON payload lists the plan.
    let (code, out, _) = run(&["agents", "init", "--claude", "--yes", "--json", "-w", p]);
    assert_eq!(code, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["schema"], "fauxfield.agents.init/v1");
    assert_eq!(v["written"], true);
    let acts: Vec<_> = v["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["path"].as_str().unwrap(), f["action"].as_str().unwrap()))
        .collect();
    assert_eq!(
        acts,
        [
            (".mcp.json", "create"),
            ("AGENTS.md", "create"),
            (".gitignore", "unchanged")
        ]
    );
    assert!(dir.path().join(".mcp.json").is_file());
    // Re-running changes nothing.
    let (code, out, _) = run(&["agents", "init", "--claude", "--yes", "-w", p]);
    assert_eq!(code, 0);
    assert!(out.contains("Already configured"), "{out}");
}

#[test]
fn json_failures_are_one_error_document_with_the_right_exit_code() {
    let dir = workspace();
    let p = dir.path().to_str().unwrap();
    // No host: exit 4 and a host_not_running document on stdout, nothing on stdout otherwise.
    let (code, out, _) = run(&["status", "--json", "-w", p]);
    assert_eq!(code, 4);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["schema"], "fauxfield.error/v1");
    assert_eq!(
        (v["ok"].as_bool(), v["code"].as_str()),
        (Some(false), Some("host_not_running"))
    );
    // A stub command for a later milestone: exit 1, never a silent no-op.
    for (cmd, ms) in [
        ("record", "M12"),
        ("import", "M15"),
        ("export", "M15"),
        ("template", "M15"),
    ] {
        let (code, out, _) = run(&[cmd, "x", "--json"]);
        assert_eq!(code, 1, "{cmd}");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["code"], "not_available");
        assert!(v["message"].as_str().unwrap().contains(ms), "{cmd}");
    }
    // Unknown route and server: usage error 2.
    let (code, _, _) = run(&["route", "show", "nope", "-w", p]);
    assert_eq!(code, 2);
    let (code, _, _) = run(&["server", "show", "nope", "-w", p]);
    assert_eq!(code, 2);
}

#[test]
fn server_and_route_management_round_trip() {
    let dir = workspace();
    let p = dir.path().to_str().unwrap();
    let json = |args: &[&str]| {
        let mut a = args.to_vec();
        a.extend_from_slice(&["-w", p, "--json"]);
        let (code, out, err) = run(&a);
        assert_eq!(code, 0, "{args:?}: {out}{err}");
        serde_json::from_str::<serde_json::Value>(&out).unwrap()
    };
    let added = json(&["server", "add", "Shop", "--port", "4100"]);
    assert_eq!(added["schema"], "fauxfield.server.add/v1");
    let r = json(&[
        "route", "add", "shop", "GET", "/orders", "--status", "201", "--body", "[1]",
    ]);
    let rid = r["id"].as_str().unwrap().to_owned();
    let shown = json(&["route", "show", &rid]);
    assert_eq!(shown["route"]["responses"][0]["status"], 201);
    assert_eq!(shown["route"]["responses"][0]["body"], "[1]");
    assert_eq!(json(&["server", "disable", "shop"])["enabled"], false);
    assert_eq!(json(&["server", "enable", "shop"])["enabled"], true);
    assert_eq!(
        json(&["server", "rename", "shop", "Store"])["name"],
        "Store"
    );
    json(&["route", "rm", &rid]);
    let (code, _, _) = run(&["route", "show", &rid, "-w", p]);
    assert_eq!(code, 2);
    json(&["server", "rm", "store"]);
    let list = json(&["server", "list"]);
    assert!(
        list["servers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["name"] != "Store")
    );
}

#[test]
fn help_output_is_stable() {
    for (name, args) in [
        ("help_top", vec!["--help"]),
        ("help_server", vec!["server", "--help"]),
        ("help_route_add", vec!["route", "add", "--help"]),
        ("help_agents_init", vec!["agents", "init", "--help"]),
        ("help_theme", vec!["theme", "--help"]),
        ("help_logs", vec!["logs", "--help"]),
    ] {
        let (code, out, _) = run(&args);
        assert_eq!(code, 0, "{args:?}");
        insta::assert_snapshot!(name, out);
    }
}

#[test]
fn completions_exist_for_all_five_shells_and_parse() {
    for sh in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let (code, out, _) = run(&["completions", sh]);
        assert_eq!(code, 0, "{sh}");
        for word in ["fauxfield", "agents", "theme", "route"] {
            assert!(out.contains(word), "{sh} completions lack `{word}`");
        }
        // Syntax-check where the shell is installed.
        let checker: Option<(&str, &[&str])> = match sh {
            "bash" => Some(("bash", &["-n"])),
            "zsh" => Some(("zsh", &["-n"])),
            "fish" => Some(("fish", &["--no-execute"])),
            _ => None,
        };
        if let Some((bin, flags)) = checker
            && std::process::Command::new(bin)
                .arg("--version")
                .output()
                .is_ok()
        {
            let dir = tempfile::tempdir().unwrap();
            let f = dir.path().join(format!("fauxfield.{sh}"));
            std::fs::write(&f, &out).unwrap();
            let st = std::process::Command::new(bin)
                .args(flags)
                .arg(&f)
                .output()
                .unwrap();
            assert!(
                st.status.success(),
                "{sh}: {}",
                String::from_utf8_lossy(&st.stderr)
            );
        }
    }
    let (code, _, _) = run(&["completions", "nushell"]);
    assert_eq!(code, 2);
}

/// Pins every `--json` schema id. A payload that changes incompatibly needs a new version in
/// `output::SCHEMA_VERSION` (or a per-command bump) and an accepted snapshot; additive fields do not.
#[test]
fn json_schema_ids_are_pinned() {
    let (code, out, _) = run(&["docs-cli"]);
    assert_eq!(code, 0);
    let ids = out
        .lines()
        .find(|l| l.starts_with("Schema ids: "))
        .unwrap()
        .trim_start_matches("Schema ids: ")
        .replace("`, `", "\n")
        .replace('`', "");
    insta::assert_snapshot!("json_schema_ids", ids);
}

#[test]
fn theme_list_and_set_write_settings_without_clobbering() {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.toml");
    std::fs::write(&settings, "# mine\nui_font_size = 13\n").unwrap();
    let theme = |args: &[&str]| {
        let out = Command::cargo_bin("fauxfield")
            .unwrap()
            .args(args)
            .env("FAUXFIELD_SETTINGS", &settings)
            .output()
            .unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8(out.stdout).unwrap(),
        )
    };
    let (code, out) = theme(&["theme", "list", "--json"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["schema"], "fauxfield.theme.list/v1");
    let names: Vec<_> = v["themes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(&names[..2], ["One Dark", "One Light"]);
    assert!(names.contains(&"Nord Dark") && names.contains(&"Tokyo Night"));
    let (code, _) = theme(&["theme", "set", "one light"]);
    assert_eq!(code, 0);
    let text = std::fs::read_to_string(&settings).unwrap();
    assert!(text.contains("# mine") && text.contains("ui_font_size = 13"));
    assert!(text.contains("light = \"One Light\"") && text.contains("mode = \"light\""));
    let (code, _) = theme(&["theme", "set", "nope"]);
    assert_eq!(code, 2);
}
