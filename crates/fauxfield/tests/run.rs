//! End to end: `run` hosts a workspace, `status`/`logs`/`stop` attach over the control socket.

use assert_cmd::Command;

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
fn logs_har_needs_a_host_and_rejects_follow() {
    let dir = workspace();
    // No host: exit 4, as for every attach command.
    let out = fauxfield(dir.path(), &["logs", "--har", "out.har"]);
    assert_eq!(out.status.code(), Some(4));
    // --har and --follow together are a usage error.
    let out = fauxfield(dir.path(), &["logs", "--har", "out.har", "--follow"]);
    assert_eq!(out.status.code(), Some(2));
}
