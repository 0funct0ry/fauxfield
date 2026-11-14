//! End to end: `run` hosts a workspace, `status`/`logs`/`stop` attach over the control socket.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command as Std, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::Command;

/// Key order depends on serde_json features unified across the build, so sort it.
fn snap(name: &str, v: &serde_json::Value) {
    let mut settings = insta::Settings::clone_current();
    settings.set_sort_maps(true);
    settings.bind(|| insta::assert_json_snapshot!(name, v));
}

fn bin() -> std::path::PathBuf {
    assert_cmd::cargo::cargo_bin("fauxfield")
}

fn free_offset() -> (u16, u16) {
    // The example server is on 3001; pick a free port above it and offset to it.
    loop {
        let p = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        if p > 3001 {
            return (p - 3001, p);
        }
    }
}

fn http_get(port: u16, path: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nhost: x\r\nconnection: close\r\n\r\n"
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

fn wait_for(port: u16) {
    let t = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "server never came up"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

struct Host(Child);
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    Command::cargo_bin("fauxfield")
        .unwrap()
        .args(["init", dir.path().to_str().unwrap()])
        .assert()
        .success();
    dir
}

fn fauxfield(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("fauxfield")
        .unwrap()
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn status_stop_and_logs_without_a_host_exit_4() {
    let dir = workspace();
    for args in [&["status"][..], &["stop"], &["logs"]] {
        let out = fauxfield(dir.path(), args);
        assert_eq!(out.status.code(), Some(4), "{args:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("no host is running"));
    }
}

#[test]
fn run_serves_and_attach_commands_work() {
    let dir = workspace();
    let (offset, port) = free_offset();
    let child = Std::new(bin())
        .current_dir(dir.path())
        .args([
            "run",
            "--port-offset",
            &offset.to_string(),
            "--log-format",
            "json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut host = Host(child);
    wait_for(port);

    let resp = http_get(port, "/health");
    assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
    assert!(resp.contains(r#"{"status": "ok"}"#));
    assert!(http_get(port, "/nope").starts_with("HTTP/1.1 404"));

    let out = fauxfield(dir.path(), &["status", "--json"]);
    assert_eq!(out.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["servers"][0]["status"]["state"], "running");
    assert_eq!(v["servers"][0]["port"], port);
    let mut norm = v.clone();
    norm["host"]["pid"] = "<pid>".into();
    norm["host"]["version"] = "<version>".into();
    norm["host"]["workspace"] = "<workspace>".into();
    norm["host"]["started_at"] = "<ts>".into();
    norm["servers"][0]["port"] = "<port>".into();
    norm["servers"][0]["id"] = "<id>".into();
    norm["servers"][0]["status"]["addr"] = "<addr>".into();
    snap("status_json", &norm);

    let out = fauxfield(dir.path(), &["logs", "--json"]);
    let lines: Vec<serde_json::Value> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["path"], "/health");
    assert_eq!(lines[0]["status"], 200);
    assert_eq!(lines[1]["status"], 404);
    snap("logs_json_line", &{
        let mut l = lines[0].clone();
        l["ts"] = "<ts>".into();
        l["duration_ms"] = "<ms>".into();
        assert!(l["route"].as_str().unwrap().starts_with("rte_"));
        l["route"] = "<route>".into();
        l
    });
    let only404 = fauxfield(dir.path(), &["logs", "--status", "4xx", "--json"]);
    assert_eq!(String::from_utf8_lossy(&only404.stdout).lines().count(), 1);

    // A second host for the same workspace is refused.
    let second = fauxfield(
        dir.path(),
        &["run", "--port-offset", &(offset + 1).to_string()],
    );
    assert_eq!(second.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&second.stderr).contains("already hosted"));

    let out = fauxfield(dir.path(), &["stop", "--json"]);
    assert_eq!(out.status.code(), Some(0));
    let mut stopped: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    stopped["servers"][0]["port"] = "<port>".into();
    stopped["servers"][0]["id"] = "<id>".into();
    snap("stop_json", &stopped);
    // The headless host exits cleanly and frees its port.
    let t = Instant::now();
    let status = loop {
        if let Some(s) = host.0.try_wait().unwrap() {
            break s;
        }
        assert!(t.elapsed() < Duration::from_secs(10), "host did not exit");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(status.code(), Some(0));
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    // The request log reached stdout as JSON lines too.
    let mut stdout = String::new();
    host.0
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    assert_eq!(stdout.lines().count(), 2);
    // host.json is gone with the host.
    assert_eq!(fauxfield(dir.path(), &["status"]).status.code(), Some(4));
}

#[test]
fn run_reports_port_in_use_with_exit_5() {
    let dir = workspace();
    let blocker = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = blocker.local_addr().unwrap().port();
    if port <= 3001 {
        return;
    }
    let out = fauxfield(
        dir.path(),
        &["run", "--port-offset", &(port - 3001).to_string()],
    );
    assert_eq!(out.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&out.stderr).contains("already in use"));
}

#[test]
fn run_rejects_invalid_workspaces_with_exit_3() {
    let dir = workspace();
    // Two servers on one port is a validation error.
    let s = std::fs::read_to_string(dir.path().join("servers/example-api.toml")).unwrap();
    std::fs::write(
        dir.path().join("servers/dup.toml"),
        s.replace("srv_", "srv_x")
            .replace("rte_", "rte_x")
            .replace("rsp_", "rsp_x"),
    )
    .unwrap();
    let out = fauxfield(dir.path(), &["run"]);
    assert_eq!(
        out.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn explicit_start_of_a_disabled_server_fails_and_run_skips_it() {
    let dir = workspace();
    let file = dir.path().join("servers/example-api.toml");
    let t = std::fs::read_to_string(&file)
        .unwrap()
        .replace("enabled = true", "enabled = false");
    std::fs::write(&file, t).unwrap();
    let out = fauxfield(dir.path(), &["run", "--server", "Example API"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("is disabled. Enable it first."),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn logs_har_is_not_available_yet() {
    let dir = workspace();
    let out = fauxfield(dir.path(), &["logs", "--har", "out.har"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr).trim(),
        "error: --har arrives in M12"
    );
}

#[test]
fn ws_commands_need_a_host_and_drive_connections() {
    let dir = workspace();
    for args in [
        &["ws", "list", "S"][..],
        &["ws", "send", "S", "x"],
        &["ws", "log", "S"],
    ] {
        assert_eq!(
            fauxfield(dir.path(), args).status.code(),
            Some(4),
            "{args:?}"
        );
    }
    // Add a websocket Route to the sample server and run.
    let (offset, port) = free_offset();
    let servers = dir.path().join("servers");
    let file = std::fs::read_dir(&servers)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut text = std::fs::read_to_string(&file).unwrap();
    text.push_str("\n[[routes]]\nid = \"rte_ws\"\nkind = \"websocket\"\npath = \"/ws\"\n");
    std::fs::write(&file, text).unwrap();
    let child = Std::new(bin())
        .current_dir(dir.path())
        .args(["run", "--port-offset", &offset.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _host = Host(child);
    wait_for(port);
    let slug = file.file_stem().unwrap().to_string_lossy().into_owned();
    let out = fauxfield(dir.path(), &["ws", "list", &slug, "--json"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    snap("ws_list_json", &v);
    let out = fauxfield(dir.path(), &["ws", "send", &slug, "hi", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    snap("ws_send_json", &v);
    let out = fauxfield(dir.path(), &["ws", "log", &slug, "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    snap("ws_log_json", &v);
    let _ = fauxfield(dir.path(), &["stop"]);
}

#[test]
fn callback_refire_needs_a_host_and_fires_again() {
    let dir = workspace();
    assert_eq!(
        fauxfield(dir.path(), &["callback", "refire", "S", "1"])
            .status
            .code(),
        Some(4)
    );
    let (offset, port) = free_offset();
    let dead = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let file = std::fs::read_dir(dir.path().join("servers"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut text = std::fs::read_to_string(&file).unwrap();
    text.push_str(&format!(
        "\n[[routes]]\nid = \"rte_hook\"\npath = \"/hook-me\"\n[[routes.responses]]\nid = \"rsp_hook\"\nstatus = 202\n\
         [[routes.responses.callbacks]]\nid = \"cb_hook\"\nurl = \"http://127.0.0.1:{dead}/x\"\ntimeout_ms = 500\n"
    ));
    std::fs::write(&file, text).unwrap();
    let child = Std::new(bin())
        .current_dir(dir.path())
        .args(["run", "--port-offset", &offset.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _host = Host(child);
    wait_for(port);
    let slug = file.file_stem().unwrap().to_string_lossy().into_owned();
    assert!(http_get(port, "/hook-me").starts_with("HTTP/1.1 202"));
    let out = fauxfield(
        dir.path(),
        &[
            "callback",
            "refire",
            &slug,
            "1",
            "--callback",
            "cb_hook",
            "--json",
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    for c in v["callbacks"].as_array_mut().unwrap() {
        c["url"] = "http://127.0.0.1:<port>/x".into();
        c["id"] = 0.into();
        assert!(
            c["error"]
                .as_str()
                .unwrap()
                .starts_with("connection failed")
        );
        c["error"] = "connection failed: <os error>".into();
    }
    snap("callback_refire_json", &v);
    let out = fauxfield(dir.path(), &["callback", "refire", &slug, "999"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no recording 999"));
    let _ = fauxfield(dir.path(), &["stop"]);
}
