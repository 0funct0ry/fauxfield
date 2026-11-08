//! End-to-end: the real `fauxfield mcp` process driven by an rmcp client over stdio and the
//! unix socket (M1.1 acceptance).

use std::{path::Path, process::Stdio, time::Duration};

use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::json;
use tokio::process::{Child, Command};

const BIN: &str = env!("CARGO_BIN_EXE_fauxfield");

fn spawn(args: &[&str]) -> Child {
    Command::new(BIN)
        .arg("mcp")
        .args(args)
        .env("FAUXFIELD_SETTINGS", "/nonexistent/settings.toml")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap()
}

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
    ] {
        let out = std::process::Command::new(BIN)
            .args(&args)
            .env("FAUXFIELD_SETTINGS", "/nonexistent/settings.toml")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
}
