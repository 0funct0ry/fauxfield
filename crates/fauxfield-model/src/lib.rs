//! Workspace file model (SPEC §4): serde types, prefixed IDs, validation.
//! Must not depend on GPUI.

mod diagnostic;
mod id;
pub mod pattern;
pub mod rules;
mod types;
mod validate;

pub use diagnostic::{Diagnostic, Severity};
pub use id::new_id;
pub use types::{
    Environment, Folder, Manifest, Response, Route, Rule, SCHEMA_VERSION, Server,
    parse_environment, parse_manifest, parse_server,
};
pub use validate::{validate_environment_names, validate_servers};

/// JSON Schema (draft 2020-12) for a server file, as published for Taplo.
pub fn server_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Server)).expect("schema serialises")
}

/// Validate a workspace name: trimmed, 1 to 80 characters, no control characters.
pub fn validate_workspace_name(name: &str) -> Result<String, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("Name cannot be empty.".into());
    }
    if n.chars().count() > 80 {
        return Err("Name must be 80 characters or fewer.".into());
    }
    if n.chars().any(char::is_control) {
        return Err("Name cannot contain control characters or newlines.".into());
    }
    Ok(n.to_string())
}

/// Validate a server host: IPv4, IPv6 or hostname without spaces; empty is rejected.
pub fn validate_host(host: &str) -> Result<String, String> {
    let h = host.trim();
    if h.is_empty() {
        return Err("Host cannot be empty.".into());
    }
    if h.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("Host cannot contain spaces.".into());
    }
    Ok(h.to_string())
}

/// True when a host is reachable from outside this machine.
pub fn host_is_network_reachable(host: &str) -> bool {
    let h = host.trim().trim_matches(['[', ']']);
    !(h.eq_ignore_ascii_case("localhost")
        || h == "::1"
        || h.starts_with("127.")
        || h.ends_with(".localhost"))
}

/// Validate a prefix: empty means none; otherwise starts with `/` and does not end with `/`.
pub fn validate_prefix(prefix: &str) -> Result<Option<String>, String> {
    let p = prefix.trim();
    if p.is_empty() {
        return Ok(None);
    }
    if !p.starts_with('/') {
        return Err("Prefix must start with \"/\".".into());
    }
    if p.ends_with('/') {
        return Err("Prefix must not end with \"/\".".into());
    }
    Ok(Some(p.to_string()))
}

#[cfg(test)]
mod validator_tests {
    use super::*;

    #[test]
    fn validators() {
        assert!(validate_workspace_name("  ").is_err());
        assert!(validate_workspace_name("a\nb").is_err());
        assert!(validate_workspace_name(&"x".repeat(81)).is_err());
        assert_eq!(validate_workspace_name(" A ").unwrap(), "A");
        assert!(validate_host("").is_err());
        assert!(validate_host("a b").is_err());
        assert!(host_is_network_reachable("0.0.0.0"));
        assert!(!host_is_network_reachable("127.0.0.1"));
        assert_eq!(validate_prefix("").unwrap(), None);
        assert!(validate_prefix("api").is_err());
        assert!(validate_prefix("/api/").is_err());
        assert_eq!(
            validate_prefix("/api/{{ v }}").unwrap().unwrap(),
            "/api/{{ v }}"
        );
    }
}
