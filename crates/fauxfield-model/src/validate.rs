use std::collections::HashMap;

use crate::pattern::{self, MatchMode};
use crate::{Diagnostic, SCHEMA_VERSION, Server};

const METHODS: &[&str] = &[
    "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "ANY",
];

/// Validate parsed servers. Each entry is `(workspace-relative file, server)`.
/// Cross-file checks (duplicate IDs and ports) are included.
pub fn validate_servers(servers: &[(String, Server)]) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut srv_ids: HashMap<&str, &str> = HashMap::new();
    let mut ids: HashMap<&str, &str> = HashMap::new(); // folder/route/response ids
    let mut ports: HashMap<(String, u16), &str> = HashMap::new();

    for (file, s) in servers {
        let file = file.as_str();
        if s.schema != SCHEMA_VERSION {
            out.push(Diagnostic::error(
                file,
                "schema",
                "E002",
                format!(
                    "unsupported schema {} (expected {SCHEMA_VERSION})",
                    s.schema
                ),
            ));
        }
        if let Some(prev) = srv_ids.insert(&s.id, file) {
            out.push(Diagnostic::error(
                file,
                "id",
                "E010",
                format!("duplicate server id `{}` (also in {prev})", s.id),
            ));
        }
        if !s.id.starts_with("srv_") {
            out.push(Diagnostic::warning(
                file,
                "id",
                "W050",
                format!("server id `{}` should start with `srv_`", s.id),
            ));
        }
        if s.port == 0 {
            out.push(Diagnostic::error(
                file,
                "port",
                "E021",
                "port must be 1-65535",
            ));
        } else if s.enabled
            && let Some(prev) = ports.insert((s.host.clone(), s.port), file)
        {
            out.push(Diagnostic::error(
                file,
                "port",
                "E020",
                format!("port {} on {} is already used by {prev}", s.port, s.host),
            ));
        }
        for (i, f) in s.folders.iter().enumerate() {
            if let Some(prev) = ids.insert(&f.id, file) {
                out.push(Diagnostic::error(
                    file,
                    format!("folders[{i}].id"),
                    "E012",
                    format!("duplicate id `{}` (also in {prev})", f.id),
                ));
            }
        }
        for (i, r) in s.routes.iter().enumerate() {
            let rp = format!("routes[{}]", r.id);
            if let Some(prev) = ids.insert(&r.id, file) {
                out.push(Diagnostic::error(
                    file,
                    format!("routes[{i}].id"),
                    "E011",
                    format!("duplicate id `{}` (also in {prev})", r.id),
                ));
            }
            if let Some(fl) = &r.folder
                && !s.folders.iter().any(|f| &f.id == fl)
            {
                out.push(Diagnostic::error(
                    file,
                    format!("{rp}.folder"),
                    "E030",
                    format!("unknown folder `{fl}`"),
                ));
            }
            if r.kind == "http" {
                let strict = s.strict_slash.unwrap_or(false);
                match pattern::parse_mode(r.match_mode.as_deref()) {
                    Err(m) => out.push(Diagnostic::error(file, format!("{rp}.match"), "E034", m)),
                    Ok(mode) => {
                        let eff = mode.unwrap_or_else(|| pattern::infer_mode(&r.path));
                        match eff {
                            MatchMode::Regex => {
                                if let Err(m) = pattern::compile_regex(&r.path) {
                                    out.push(Diagnostic::error(
                                        file,
                                        format!("{rp}.path"),
                                        "E035",
                                        m,
                                    ));
                                }
                            }
                            MatchMode::Param => {
                                if let Err(e) = pattern::parse_param(&r.path, strict) {
                                    out.push(Diagnostic::error(
                                        file,
                                        format!("{rp}.path"),
                                        "E033",
                                        e.to_string(),
                                    ));
                                }
                            }
                            MatchMode::Exact => {
                                if mode.is_some() && pattern::looks_like_param(&r.path) {
                                    out.push(Diagnostic::warning(
                                        file,
                                        format!("{rp}.path"),
                                        "W052",
                                        "looks like a param pattern but match is exact",
                                    ));
                                }
                            }
                        }
                    }
                }
                let is_regex = r.match_mode.as_deref() == Some("regex");
                if !is_regex && !r.path.starts_with('/') {
                    out.push(Diagnostic::error(
                        file,
                        format!("{rp}.path"),
                        "E031",
                        "path must start with `/`",
                    ));
                }
                if !METHODS.contains(&r.method.to_ascii_uppercase().as_str()) {
                    out.push(Diagnostic::error(
                        file,
                        format!("{rp}.method"),
                        "E032",
                        format!("unknown method `{}`", r.method),
                    ));
                }
            }
            if let Some(m) = r.response_mode.as_deref()
                && !crate::rules::RESPONSE_MODES.contains(&m)
            {
                out.push(Diagnostic::error(
                    file,
                    format!("{rp}.response_mode"),
                    "E043",
                    format!("unknown response_mode `{m}`"),
                ));
            }
            for (j, resp) in r.responses.iter().enumerate() {
                let sp = format!("{rp}.responses[{j}]");
                if let Some(prev) = ids.insert(&resp.id, file) {
                    out.push(Diagnostic::error(
                        file,
                        format!("{sp}.id"),
                        "E013",
                        format!("duplicate id `{}` (also in {prev})", resp.id),
                    ));
                }
                if !(100..=599).contains(&resp.status) {
                    out.push(Diagnostic::error(
                        file,
                        format!("{sp}.status"),
                        "E040",
                        format!("status {} is not 100-599", resp.status),
                    ));
                }
                if let Some(l) = resp.rules_logic.as_deref()
                    && l != "and"
                    && l != "or"
                {
                    out.push(Diagnostic::error(
                        file,
                        format!("{sp}.rules_logic"),
                        "E049",
                        format!("rules_logic must be `and` or `or`, got `{l}`"),
                    ));
                }
                for (k, rule) in resp.rules.iter().enumerate() {
                    if let Err((code, m)) = crate::rules::check_rule(rule) {
                        out.push(Diagnostic::error(file, format!("{sp}.rules[{k}]"), code, m));
                    }
                }
                if resp.body.is_some() && resp.body_file.is_some() {
                    out.push(Diagnostic::error(
                        file,
                        format!("{sp}.body"),
                        "E041",
                        "set either `body` or `body_file`, not both",
                    ));
                }
                if let Some(bf) = &resp.body_file
                    && (bf.starts_with('/') || bf.split(['/', '\\']).any(|c| c == ".."))
                {
                    out.push(Diagnostic::error(
                        file,
                        format!("{sp}.body_file"),
                        "E042",
                        "body_file must stay inside the workspace",
                    ));
                }
            }
            if let Some(d) = r.responses.iter().find(|x| x.default && x.disabled) {
                out.push(Diagnostic::warning(
                    file,
                    format!("{rp}.responses"),
                    "W044",
                    format!("default response `{}` is disabled", d.id),
                ));
            }
            let defaults = r.responses.iter().filter(|x| x.default).count();
            if defaults > 1 {
                out.push(Diagnostic::warning(
                    file,
                    format!("{rp}.responses"),
                    "W043",
                    "more than one default response",
                ));
            }
        }
        for sh in pattern::shadowed_routes(s) {
            let by = s.routes.iter().find(|r| r.id == sh.by);
            let me = s.routes.iter().find(|r| r.id == sh.route);
            if let (Some(by), Some(me)) = (by, me) {
                out.push(Diagnostic::warning(
                    file,
                    format!("routes[{}]", sh.route),
                    "W051",
                    format!(
                        "Route `{} {}` is never reached: shadowed by `{} {}` ({})",
                        me.method, me.path, by.method, by.path, by.id
                    ),
                ));
            }
        }
    }
    out
}

/// Environment files: names must be unique. Entries are `(file, name)`.
pub fn validate_environment_names(envs: &[(String, String)]) -> Vec<Diagnostic> {
    let mut seen: HashMap<&str, &str> = HashMap::new();
    let mut out = Vec::new();
    for (file, name) in envs {
        if let Some(prev) = seen.insert(name, file) {
            out.push(Diagnostic::error(
                file,
                "name",
                "E060",
                format!("duplicate environment `{name}` (also in {prev})"),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_server;

    fn srv(id: &str, port: u16, route: &str) -> (String, Server) {
        let t = format!(
            "id=\"{id}\"\nname=\"n\"\nport={port}\n[[routes]]\nid=\"{route}\"\npath=\"/x\"\n"
        );
        (format!("servers/{id}.toml"), parse_server(&t).unwrap())
    }

    #[test]
    fn catches_duplicate_ports_and_ids() {
        let d = validate_servers(&[srv("srv_a", 3000, "rte_1"), srv("srv_b", 3000, "rte_1")]);
        let codes: Vec<_> = d.iter().map(|d| d.code.as_str()).collect();
        assert!(codes.contains(&"E020"), "{codes:?}");
        assert!(codes.contains(&"E011"), "{codes:?}");
        let d = validate_servers(&[srv("srv_a", 3000, "rte_1"), srv("srv_a", 3001, "rte_2")]);
        assert!(d.iter().any(|d| d.code == "E010"));
    }

    #[test]
    fn clean_server_has_no_findings() {
        assert!(validate_servers(&[srv("srv_a", 3000, "rte_1")]).is_empty());
    }

    #[test]
    fn unknown_fields_are_tolerated() {
        let s = parse_server("id=\"srv_a\"\nname=\"n\"\nport=1\nfuture=1\n[tls]\nenabled=false\n")
            .unwrap();
        assert!(s.extra.contains_key("future") && s.extra.contains_key("tls"));
    }
}
