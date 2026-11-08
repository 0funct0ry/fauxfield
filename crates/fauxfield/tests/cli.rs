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
    insta::assert_json_snapshot!(v);
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
    let (code, _, err) = run(&["run"]);
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
