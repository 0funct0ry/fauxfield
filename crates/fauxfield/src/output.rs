//! The `--json` contract (SPEC §10, M16): exactly one JSON document on stdout, wrapped in an
//! envelope `{schema: "fauxfield.<cmd>/v<N>", ok, ...}`; failures are
//! `{schema: "fauxfield.error/v1", ok: false, code, message, hint, diagnostics}`.
//! Logs and prompts go to stderr.

use std::sync::OnceLock;

use clap::CommandFactory;
use serde_json::{Map, Value, json};

/// Version of a command's payload schema. Bump when a field is removed or changes meaning;
/// additive fields do not bump. A snapshot test pins every id.
pub const SCHEMA_VERSION: u32 = 1;

struct Ctx {
    command: String,
    json: bool,
    quiet: bool,
}

static CTX: OnceLock<Ctx> = OnceLock::new();

/// Walk the clap tree along `args` to name the command (`route.list`, `status`).
fn command_name(args: &[String]) -> String {
    let mut cmd = crate::cli::Cli::command();
    let mut parts = Vec::new();
    for a in args.iter().skip(1) {
        if a.starts_with('-') {
            continue;
        }
        match cmd.find_subcommand(a) {
            Some(sub) => {
                parts.push(sub.get_name().to_owned());
                cmd = sub.clone();
            }
            None => {
                if cmd.has_subcommands() {
                    break;
                }
            }
        }
    }
    if parts.is_empty() {
        "open".into()
    } else {
        parts.join(".")
    }
}

/// Record the command and the global flags once, right after parsing.
pub fn init(args: &[String]) {
    let json = args.iter().any(|a| a == "--json");
    let quiet = args.iter().any(|a| a == "-q" || a == "--quiet");
    let _ = CTX.set(Ctx {
        command: command_name(args),
        json,
        quiet,
    });
}

pub fn json_mode() -> bool {
    CTX.get().is_some_and(|c| c.json)
}

pub fn quiet() -> bool {
    CTX.get().is_some_and(|c| c.quiet)
}

pub fn schema_id(command: &str) -> String {
    format!("fauxfield.{command}/v{SCHEMA_VERSION}")
}

/// Wrap a payload in the success envelope. Non-object payloads go under `data`.
pub fn envelope(command: &str, payload: &Value) -> Value {
    let mut m = Map::new();
    m.insert("schema".into(), Value::String(schema_id(command)));
    match payload {
        Value::Object(o) => {
            if !o.contains_key("ok") {
                m.insert("ok".into(), Value::Bool(true));
            }
            for (k, v) in o {
                m.insert(k.clone(), v.clone());
            }
        }
        other => {
            m.insert("ok".into(), Value::Bool(true));
            m.insert("data".into(), other.clone());
        }
    }
    Value::Object(m)
}

pub fn error_envelope(code: &str, message: &str, hint: &str, diagnostics: Value) -> Value {
    json!({
        "schema": "fauxfield.error/v1",
        "ok": false,
        "code": code,
        "message": message,
        "hint": hint,
        "diagnostics": diagnostics,
    })
}

/// Print a success document (the one on stdout).
pub fn print_json(v: &Value) {
    let cmd = CTX.get().map(|c| c.command.as_str()).unwrap_or("unknown");
    println!(
        "{}",
        serde_json::to_string_pretty(&envelope(cmd, v)).unwrap_or_default()
    );
}

fn classify(message: &str) -> (&'static str, &'static str) {
    if message.starts_with("no host") {
        (
            "host_not_running",
            "Start one with `fauxfield run` or the app.",
        )
    } else if message.contains("not available until") {
        ("not_available", "")
    } else if message.contains("in use") {
        (
            "port_in_use",
            "Pick another port or stop the process using it.",
        )
    } else {
        ("runtime", "")
    }
}

#[macro_export]
macro_rules! err {
    ($($t:tt)*) => { $crate::output::err_line(format!($($t)*)) };
}

/// Report a failure: JSON error document on stdout under `--json`, else `error: ...` on stderr.
pub fn err_line(message: String) {
    if json_mode() {
        let (code, hint) = classify(&message);
        println!(
            "{}",
            serde_json::to_string_pretty(&error_envelope(code, &message, hint, json!([])))
                .unwrap_or_default()
        );
    } else {
        eprintln!("error: {message}");
    }
}

/// Informational output that `-q` suppresses.
pub fn info(line: &str) {
    if !quiet() {
        println!("{line}");
    }
}

/// Exit-1 stub for commands whose milestone has not landed; never a silent no-op.
pub fn not_available(what: &str, milestone: &str) -> i32 {
    err_line(format!("`{what}` is not available until {milestone}"));
    crate::cli::EXIT_RUNTIME
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_wraps_objects_and_scalars() {
        let v = envelope("route.list", &json!({"routes": []}));
        assert_eq!(v["schema"], "fauxfield.route.list/v1");
        assert_eq!(v["ok"], true);
        assert_eq!(v["routes"], json!([]));
        let v = envelope("x", &json!({"ok": false, "errors": 1}));
        assert_eq!(v["ok"], false);
        assert_eq!(envelope("x", &json!([1]))["data"], json!([1]));
    }

    #[test]
    fn names_nested_commands() {
        let a = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            command_name(&a(&["fauxfield", "route", "list", "--json"])),
            "route.list"
        );
        assert_eq!(command_name(&a(&["fauxfield", "status"])), "status");
        assert_eq!(command_name(&a(&["fauxfield"])), "open");
    }

    #[test]
    fn classifies_errors() {
        assert_eq!(classify("no host is running").0, "host_not_running");
        assert_eq!(
            classify("`record` is not available until M12").0,
            "not_available"
        );
    }
}
