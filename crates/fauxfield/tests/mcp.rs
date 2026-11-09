//! End-to-end: the real `fauxfield mcp` process driven by an rmcp client over stdio and the
//! unix socket (M1.1 acceptance).

#[cfg(unix)]
use std::path::Path;
use std::{process::Stdio, time::Duration};

use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::json;
use tokio::process::{Child, Command};

const BIN: &str = env!("CARGO_BIN_EXE_fauxfield");

fn spawn(args: &[&str]) -> Child {
    Command::new(BIN)
        .arg("mcp")
        .args(args)
        .current_dir(std::env::temp_dir())
        .env("FAUXFIELD_SETTINGS", "/nonexistent/settings.toml")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap()
}

#[cfg(unix)]
async fn wait_for(path: &Path) {
    for _ in 0..200 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("{} never appeared", path.display());
}

/// Connect, retrying while the server is still replacing a stale file.
#[cfg(unix)]
async fn connect_retry(path: &Path) -> tokio::net::UnixStream {
    for _ in 0..200 {
        if let Ok(s) = tokio::net::UnixStream::connect(path).await {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("could not connect to {}", path.display());
}

async fn ping<T>(c: &rmcp::service::RunningService<rmcp::RoleClient, T>)
where
    T: rmcp::service::Service<rmcp::RoleClient>,
{
    let r = c
        .call_tool(CallToolRequestParams::new("ping"))
        .await
        .unwrap();
    assert_eq!(r.structured_content, Some(json!({"ok": true})));
}

#[tokio::test]
async fn stdio_lists_tools_and_pings() {
    let mut child = spawn(&[]);
    let io = (child.stdout.take().unwrap(), child.stdin.take().unwrap());
    let c = ().serve(io).await.unwrap();
    assert_eq!(c.list_all_tools().await.unwrap().len(), 2);
    ping(&c).await;
    c.cancel().await.unwrap();
    // stdin closed: a stdio-only server exits.
    assert!(
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .is_ok()
    );
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn term(child: &Child) {
        let pid = child.id().unwrap().to_string();
        assert!(
            std::process::Command::new("kill")
                .args(["-TERM", &pid])
                .status()
                .unwrap()
                .success()
        );
    }

    #[tokio::test]
    async fn socket_serves_is_0600_and_removed_on_exit() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("m.sock");
        let mut child = spawn(&["--transport", "unix", "--socket", sock.to_str().unwrap()]);
        wait_for(&sock).await;
        assert_eq!(
            std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let c = ().serve(tokio::net::UnixStream::connect(&sock).await.unwrap()).await.unwrap();
        ping(&c).await;
        drop(c);
        term(&child);
        assert!(
            tokio::time::timeout(Duration::from_secs(5), child.wait())
                .await
                .is_ok()
        );
        assert!(!sock.exists(), "socket must be removed on clean exit");
    }

    fn run_scenario(file: &Path, sock: &Path) -> std::process::Output {
        std::process::Command::new(BIN)
            .args([
                "mcp",
                "scenario",
                file.to_str().unwrap(),
                "--no-capture",
                "--json",
            ])
            .args(["--socket", sock.to_str().unwrap()])
            .output()
            .unwrap()
    }

    /// `fauxfield mcp scenario`: exit 2 without an app, exit 1 when a step fails (the headless
    /// server has no `ui.*` tools, so the first step fails).
    #[tokio::test]
    async fn scenario_command_exit_codes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("s.yaml");
        std::fs::write(&file, "name: s\nsteps:\n  - key: a\n").unwrap();
        let sock = dir.path().join("s.sock");
        assert_eq!(run_scenario(&file, &sock).status.code(), Some(2));
        let mut child = spawn(&["--transport", "unix", "--socket", sock.to_str().unwrap()]);
        wait_for(&sock).await;
        let (file2, sock2) = (file.clone(), sock.clone());
        let out = tokio::task::spawn_blocking(move || run_scenario(&file2, &sock2))
            .await
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["passed"], false);
        assert_eq!(v["failure"]["index"], 1);
        term(&child);
        let _ = child.wait().await;
    }

    #[tokio::test]
    async fn stale_socket_replaced_live_one_refused() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("s.sock");
        // A stale file: bind then drop the listener without unlinking.
        drop(std::os::unix::net::UnixListener::bind(&sock).unwrap());
        assert!(sock.exists());
        let mut live = spawn(&["--transport", "unix", "--socket", sock.to_str().unwrap()]);
        let stream = connect_retry(&sock).await;
        let c = ().serve(stream).await.unwrap();
        ping(&c).await;

        let second = std::process::Command::new(BIN)
            .args([
                "mcp",
                "--transport",
                "unix",
                "--socket",
                sock.to_str().unwrap(),
            ])
            .env("FAUXFIELD_SETTINGS", "/nonexistent/settings.toml")
            .output()
            .unwrap();
        assert_eq!(second.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&second.stderr).contains("in use"));
        assert!(
            sock.exists(),
            "a refused start must not remove the live socket"
        );
        drop(c);
        term(&live);
        let _ = live.wait().await;
    }

    #[tokio::test]
    async fn both_keeps_serving_the_socket_after_stdin_closes() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("b.sock");
        let mut child = spawn(&["--transport", "both", "--socket", sock.to_str().unwrap()]);
        wait_for(&sock).await;
        let io = (child.stdout.take().unwrap(), child.stdin.take().unwrap());
        let stdio = ().serve(io).await.unwrap();
        ping(&stdio).await;
        stdio.cancel().await.unwrap(); // closes the child's stdin
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            child.try_wait().unwrap().is_none(),
            "process must outlive stdin"
        );
        let c = ().serve(tokio::net::UnixStream::connect(&sock).await.unwrap()).await.unwrap();
        ping(&c).await;
        drop(c);
        term(&child);
        assert!(
            tokio::time::timeout(Duration::from_secs(5), child.wait())
                .await
                .is_ok()
        );
        assert!(!sock.exists());
    }

    #[tokio::test]
    async fn read_only_flag_reflected_in_server_info() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("r.sock");
        let child = spawn(&[
            "--read-only",
            "--transport",
            "unix",
            "--socket",
            sock.to_str().unwrap(),
        ]);
        wait_for(&sock).await;
        let c = ().serve(tokio::net::UnixStream::connect(&sock).await.unwrap()).await.unwrap();
        let v = c
            .call_tool(CallToolRequestParams::new("server_info"))
            .await
            .unwrap()
            .structured_content
            .unwrap();
        assert_eq!(v["allowed_classes"], json!(["read"]));
        drop(c);
        term(&child);
    }
}

#[test]
fn http_transport_is_not_available_yet() {
    let out = std::process::Command::new(BIN)
        .args(["mcp", "--transport", "http"])
        .env("FAUXFIELD_SETTINGS", "/nonexistent/settings.toml")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not available until M16"));
    assert!(out.stdout.is_empty(), "stdout is protocol-only");
}

#[test]
fn bad_flags_are_usage_errors() {
    for args in [
        vec!["mcp", "--allow", "bogus"],
        vec!["mcp", "--read-only", "--allow", "read"],
        vec!["mcp", "--transport", "tcp"],
        // The wrong native socket for this platform names the right one.
        vec![
            "mcp",
            "--transport",
            if cfg!(windows) { "unix" } else { "pipe" },
        ],
    ] {
        let out = std::process::Command::new(BIN)
            .args(&args)
            .env("FAUXFIELD_SETTINGS", "/nonexistent/settings.toml")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
}

/// M2: workspace tools over stdio edit the files through the store.
#[tokio::test]
async fn workspace_tools_edit_files_and_respect_read_only() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new(BIN)
            .args(["init", dir.path().to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
    let p = dir.path().to_str().unwrap();
    let mut child = spawn(&[p]);
    let c = ().serve((child.stdout.take().unwrap(), child.stdin.take().unwrap())).await.unwrap();
    let call = |name: &'static str, args: serde_json::Value| {
        let c = &c;
        async move {
            c.call_tool(
                CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone()),
            )
            .await
        }
    };
    let info = call("workspace_info", json!({})).await.unwrap();
    assert_eq!(
        info.structured_content.unwrap()["servers"][0]["name"],
        "Example API"
    );
    let r = call(
        "upsert_route",
        json!({"server": "example-api", "method": "post", "path": "/users", "name": "Create",
               "responses": [{"name": "Created", "status": 201, "body": "{}", "headers": {"x-a": "1"}}]}),
    )
    .await
    .unwrap();
    let route = r.structured_content.unwrap();
    assert_eq!(route["method"], "POST");
    assert_eq!(route["responses"][0]["status"], 201);
    assert_eq!(route["responses"].as_array().unwrap().len(), 1);
    let text = std::fs::read_to_string(dir.path().join("servers/example-api.toml")).unwrap();
    assert!(text.contains("/users") && text.contains("x-a"));
    let v = call("validate", json!({}))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(v["ok"], true);
    call("delete_route", json!({"route": route["id"]}))
        .await
        .unwrap();
    c.cancel().await.unwrap();

    let mut child = spawn(&[p, "--read-only"]);
    let c = ().serve((child.stdout.take().unwrap(), child.stdin.take().unwrap())).await.unwrap();
    let names: Vec<_> = c
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(
        names.contains(&"list_routes".to_string()) && !names.contains(&"upsert_route".to_string())
    );
    c.cancel().await.unwrap();
}

/// M3: start a server, hit it, read its traffic, and stop it, all through MCP tools.
#[cfg(unix)]
#[tokio::test]
async fn control_tools_start_send_traffic_stop() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new(BIN)
            .args(["init", dir.path().to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let p = dir.path().to_str().unwrap();
    let mut child = spawn(&[p]);
    let c = ().serve((child.stdout.take().unwrap(), child.stdin.take().unwrap())).await.unwrap();
    let call = |name: &'static str, args: serde_json::Value| {
        let c = &c;
        async move {
            c.call_tool(
                CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone()),
            )
            .await
        }
    };
    let names: Vec<_> = c
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    for t in [
        "start_server",
        "stop_server",
        "restart_server",
        "get_traffic",
        "send_test_request",
    ] {
        assert!(names.contains(&t.to_string()), "{t} missing");
    }
    call(
        "update_server",
        json!({"server": "example-api", "port": port}),
    )
    .await
    .unwrap();
    let r = call("start_server", json!({"server": "example-api"}))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(r["servers"][0]["status"]["state"], "running");
    let r = call(
        "send_test_request",
        json!({"server": "example-api", "path": "/health"}),
    )
    .await
    .unwrap()
    .structured_content
    .unwrap();
    assert_eq!(r["status"], 200);
    assert_eq!(r["body"], r#"{"status": "ok"}"#);
    // A plain curl-style request shows up in traffic too.
    let t = call(
        "get_traffic",
        json!({"server": "example-api", "status": "2xx"}),
    )
    .await
    .unwrap()
    .structured_content
    .unwrap();
    assert_eq!(t["count"], 1);
    assert!(
        t["traffic"][0]["route"]
            .as_str()
            .unwrap()
            .starts_with("rte_")
    );
    let miss = call("get_traffic", json!({"status": "5xx"}))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(miss["count"], 0);
    // An attached CLI sees the same host.
    let status = std::process::Command::new(BIN)
        .args(["status", "--json", "-w", p])
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(0));
    let r = call("restart_server", json!({"server": "example-api"}))
        .await
        .unwrap();
    assert_eq!(
        r.structured_content.unwrap()["servers"][0]["status"]["state"],
        "running"
    );
    let r = call("stop_server", json!({})).await.unwrap();
    assert_eq!(
        r.structured_content.unwrap()["servers"][0]["status"]["state"],
        "stopped"
    );
    c.cancel().await.unwrap();

    // Read-only hides the execute tools.
    let mut child = spawn(&[p, "--read-only"]);
    let c = ().serve((child.stdout.take().unwrap(), child.stdin.take().unwrap())).await.unwrap();
    let names: Vec<_> = c
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(names.contains(&"get_traffic".to_string()));
    assert!(!names.contains(&"start_server".to_string()));
    assert!(!names.contains(&"send_test_request".to_string()));
    c.cancel().await.unwrap();
}

fn init_ws() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new(BIN)
            .args(["init", dir.path().to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
    dir
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn wrong_native_transport_is_a_usage_error_naming_the_right_one() {
    let wrong = if cfg!(windows) { "unix" } else { "pipe" };
    let right = if cfg!(windows) { "pipe" } else { "unix" };
    let out = std::process::Command::new(BIN)
        .args(["mcp", "--transport", wrong])
        .env("FAUXFIELD_SETTINGS", "/nonexistent/settings.toml")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains(&format!("--transport {right}")));
}

/// M3.1: rename the workspace and flip the 404 hint through MCP.
#[tokio::test]
async fn update_workspace_and_server_hint_and_prefix() {
    let dir = init_ws();
    let p = dir.path().to_str().unwrap();
    let mut child = spawn(&[p]);
    let c = ().serve((child.stdout.take().unwrap(), child.stdin.take().unwrap())).await.unwrap();
    let call = |name: &'static str, args: serde_json::Value| {
        let c = &c;
        async move {
            c.call_tool(
                CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone()),
            )
            .await
        }
    };
    let r = call("update_workspace", json!({"name": "  Renamed  "}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["name"], "Renamed");
    assert!(
        std::fs::read_to_string(dir.path().join("fauxfield.toml"))
            .unwrap()
            .contains("name = \"Renamed\"")
    );
    assert!(
        call("update_workspace", json!({"name": ""})).await.is_err() || {
            // tool failures come back as tool errors
            call("update_workspace", json!({"name": ""}))
                .await
                .unwrap()
                .is_error
                == Some(true)
        }
    );
    call(
        "update_server",
        json!({"server": "example-api", "unmatched_hint": false, "prefix": "/api"}),
    )
    .await
    .unwrap();
    let t = std::fs::read_to_string(dir.path().join("servers/example-api.toml")).unwrap();
    assert!(t.contains("unmatched_hint = false") && t.contains("prefix = \"/api\""));
    let s = call("get_server", json!({"server": "example-api"}))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(s["unmatched_hint"], false);
    // Empty prefix removes the key; a bad one is refused.
    call(
        "update_server",
        json!({"server": "example-api", "prefix": ""}),
    )
    .await
    .unwrap();
    let t = std::fs::read_to_string(dir.path().join("servers/example-api.toml")).unwrap();
    assert!(!t.contains("prefix"));
    let bad = call(
        "update_server",
        json!({"server": "example-api", "prefix": "api/"}),
    )
    .await;
    assert!(bad.is_err() || bad.unwrap().is_error == Some(true));
    c.cancel().await.unwrap();
}

/// C3: a read-only MCP process never becomes a host.
#[tokio::test]
async fn read_only_get_traffic_never_creates_a_host() {
    let dir = init_ws();
    let p = dir.path().to_str().unwrap();
    let mut child = spawn(&[p, "--read-only"]);
    let c = ().serve((child.stdout.take().unwrap(), child.stdin.take().unwrap())).await.unwrap();
    let t = c
        .call_tool(CallToolRequestParams::new("get_traffic"))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(t["count"], 0);
    assert_eq!(t["host"], "not running");
    assert!(!dir.path().join(".fauxfield/run/host.json").exists());
    c.cancel().await.unwrap();
    let out = std::process::Command::new(BIN)
        .args(["status", "-w", p])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4), "no host must be running");
}

/// C3: two MCP processes that both need a host end with exactly one, and both see traffic.
#[tokio::test]
async fn two_processes_share_one_host() {
    let dir = init_ws();
    let p = dir.path().to_str().unwrap();
    let port = free_port();
    let file = dir.path().join("servers/example-api.toml");
    let t = std::fs::read_to_string(&file)
        .unwrap()
        .replace("3001", &port.to_string());
    std::fs::write(&file, t).unwrap();
    let (mut a, mut b) = (spawn(&[p]), spawn(&[p]));
    let ca = ().serve((a.stdout.take().unwrap(), a.stdin.take().unwrap())).await.unwrap();
    let cb = ().serve((b.stdout.take().unwrap(), b.stdin.take().unwrap())).await.unwrap();
    async fn start(
        c: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    ) -> Result<rmcp::model::CallToolResult, rmcp::service::ServiceError> {
        c.call_tool(
            CallToolRequestParams::new("start_server").with_arguments(
                json!({"server": "example-api"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
    }
    let (ra, rb) = tokio::join!(start(&ca), start(&cb));
    for r in [ra, rb] {
        let r = r.unwrap();
        assert_ne!(r.is_error, Some(true), "{r:?}");
        assert_eq!(
            r.structured_content.unwrap()["servers"][0]["status"]["state"],
            "running"
        );
    }
    // One request through one process is visible from the other.
    let sent = ca
        .call_tool(
            CallToolRequestParams::new("send_test_request").with_arguments(
                json!({"server": "example-api", "path": "/health"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_ne!(sent.is_error, Some(true));
    for c in [&ca, &cb] {
        let t = c
            .call_tool(CallToolRequestParams::new("get_traffic"))
            .await
            .unwrap()
            .structured_content
            .unwrap();
        assert_eq!(t["count"], 1, "{t}");
    }
    let status = std::process::Command::new(BIN)
        .args(["status", "--json", "-w", p])
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(v["servers"].as_array().unwrap().len(), 1);
    ca.cancel().await.unwrap();
    cb.cancel().await.unwrap();
}

#[cfg(windows)]
mod windows {
    use super::*;
    use tokio::net::windows::named_pipe::ClientOptions;

    fn pipe_name(tag: &str) -> String {
        format!(r"\\.\pipe\fauxfield-test-{}-{tag}", std::process::id())
    }

    async fn connect_retry(name: &str) -> tokio::net::windows::named_pipe::NamedPipeClient {
        for _ in 0..200 {
            if let Ok(c) = ClientOptions::new().open(name) {
                return c;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("could not connect to {name}");
    }

    #[tokio::test]
    async fn pipe_serves_and_refuses_a_live_name() {
        let name = pipe_name("a");
        let mut live = spawn(&["--transport", "pipe", "--socket", &name]);
        let c = ().serve(connect_retry(&name).await).await.unwrap();
        ping(&c).await;
        let second = std::process::Command::new(BIN)
            .args(["mcp", "--transport", "pipe", "--socket", &name])
            .env("FAUXFIELD_SETTINGS", "/nonexistent/settings.toml")
            .output()
            .unwrap();
        assert_eq!(second.status.code(), Some(1));
        drop(c);
        let _ = live.kill().await;
    }

    #[tokio::test]
    async fn both_keeps_serving_the_pipe_after_stdin_closes() {
        let name = pipe_name("b");
        let mut child = spawn(&["--transport", "both", "--socket", &name]);
        let io = (child.stdout.take().unwrap(), child.stdin.take().unwrap());
        let stdio = ().serve(io).await.unwrap();
        ping(&stdio).await;
        stdio.cancel().await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            child.try_wait().unwrap().is_none(),
            "process must outlive stdin"
        );
        let c = ().serve(connect_retry(&name).await).await.unwrap();
        ping(&c).await;
        let info = c
            .call_tool(CallToolRequestParams::new("server_info"))
            .await
            .unwrap()
            .structured_content
            .unwrap();
        assert_eq!(info["transports"], json!(["stdio", "pipe"]));
        drop(c);
        let _ = child.kill().await;
    }
}

/// M7: `set_variable` writes at a scope and never echoes secret values.
#[tokio::test]
async fn set_variable_writes_and_hides_secrets() {
    let dir = init_ws();
    let p = dir.path().to_str().unwrap();
    let mut child = spawn(&[p]);
    let c = ().serve((child.stdout.take().unwrap(), child.stdin.take().unwrap())).await.unwrap();
    let call = |args: serde_json::Value| {
        let c = &c;
        async move {
            c.call_tool(
                CallToolRequestParams::new("set_variable")
                    .with_arguments(args.as_object().unwrap().clone()),
            )
            .await
        }
    };
    let r = call(json!({"scope": "environment", "owner": "local", "key": "page", "value": "5"}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["value"], "5");
    let r = call(json!({"scope": "global", "key": "api_token", "value": "zzz"}))
        .await
        .unwrap();
    assert!(r.structured_content.unwrap()["value"].is_null());
    let t = std::fs::read_to_string(dir.path().join("fauxfield.toml")).unwrap();
    assert!(t.contains("api_token"));
    let bad = call(json!({"scope": "runtime", "key": "x", "value": "1"})).await;
    assert!(bad.is_err() || bad.unwrap().is_error == Some(true));
}

/// M8: `upsert_dataset` writes seed, dataset and CRUD route; `reset_state` runs on a host.
#[tokio::test]
async fn upsert_dataset_and_reset_state() {
    let dir = init_ws();
    let p = dir.path().to_str().unwrap();
    let mut child = spawn(&[p]);
    let c = ().serve((child.stdout.take().unwrap(), child.stdin.take().unwrap())).await.unwrap();
    let call = |name: &'static str, args: serde_json::Value| {
        let c = &c;
        async move {
            c.call_tool(
                CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone()),
            )
            .await
        }
    };
    let r = call(
        "upsert_dataset",
        json!({"server": "example-api", "name": "products", "seed": "[{\"id\":1}]",
               "crud_path": "/products", "persist": true}),
    )
    .await
    .unwrap();
    assert!(r.structured_content.unwrap()["route"].is_string());
    assert!(dir.path().join("data/products.json").exists());
    let t = std::fs::read_to_string(dir.path().join("servers/example-api.toml")).unwrap();
    assert!(
        t.contains("[[datasets]]") && t.contains("kind = \"crud\""),
        "{t}"
    );
    let bad = call("upsert_dataset", json!({"server": "nope", "name": "x"})).await;
    assert!(bad.is_err() || bad.unwrap().is_error == Some(true));
    let r = call("reset_state", json!({})).await.unwrap();
    assert_eq!(r.structured_content.unwrap()["reset"], true);
}

/// M9: the network profile and chaos switch are runtime state set through MCP.
#[tokio::test]
async fn network_profile_and_chaos_tools() {
    let dir = init_ws();
    let p = dir.path().to_str().unwrap();
    let mut child = spawn(&[p]);
    let c = ().serve((child.stdout.take().unwrap(), child.stdin.take().unwrap())).await.unwrap();
    let call = |name: &'static str, args: serde_json::Value| {
        let c = &c;
        async move {
            c.call_tool(
                CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone()),
            )
            .await
        }
    };
    let r = call("set_network_profile", json!({"profile": "Slow 3G"}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["profile"], "slow-3g");
    let r = call("set_network_profile", json!({})).await.unwrap();
    assert_eq!(r.structured_content.unwrap()["profile"], "slow-3g");
    let bad = call("set_network_profile", json!({"profile": "dialup"})).await;
    assert!(bad.is_err() || bad.unwrap().is_error == Some(true));
    let r = call("set_chaos", json!({"rate": 0.5})).await.unwrap();
    assert_eq!(r.structured_content.unwrap()["servers"][0]["chaos"], 0.5);
    let r = call("set_chaos", json!({"rate": 0})).await.unwrap();
    assert!(r.structured_content.unwrap()["servers"][0]["chaos"].is_null());
    let bad = call("set_chaos", json!({"rate": 3})).await;
    assert!(bad.is_err() || bad.unwrap().is_error == Some(true));
}
