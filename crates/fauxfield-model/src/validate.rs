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
        if let Some(rl) = &s.rate_limit {
            if rl.requests == 0 {
                out.push(Diagnostic::error(
                    file,
                    "rate_limit.requests",
                    "E080",
                    "requests must be at least 1",
                ));
            }
            if crate::parse_window(&rl.window).is_none() {
                out.push(Diagnostic::error(
                    file,
                    "rate_limit.window",
                    "E081",
                    format!("window `{}` is not like 30s, 1m or 2h", rl.window),
                ));
            }
            if !crate::valid_rate_key(&rl.key) {
                out.push(Diagnostic::error(
                    file,
                    "rate_limit.key",
                    "E082",
                    format!("key `{}` must be ip, header:<name> or query:<name>", rl.key),
                ));
            }
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
            if !crate::ROUTE_KINDS.contains(&r.kind.as_str()) {
                out.push(Diagnostic::error(
                    file,
                    format!("{rp}.kind"),
                    "E070",
                    format!("unknown kind `{}`", r.kind),
                ));
            }
            if (r.proxy || r.pinned) && r.kind != "http" {
                out.push(Diagnostic::warning(
                    file,
                    format!("{rp}.proxy"),
                    "W092",
                    "proxy and pinned only apply to http Routes",
                ));
            }
            if r.kind == "crud" {
                if !r.path.starts_with('/') {
                    out.push(Diagnostic::error(
                        file,
                        format!("{rp}.path"),
                        "E031",
                        "path must start with `/`",
                    ));
                }
                match r.dataset.as_deref() {
                    None | Some("") => out.push(Diagnostic::error(
                        file,
                        format!("{rp}.dataset"),
                        "E071",
                        "a crud Route needs `dataset`",
                    )),
                    Some(d) if !s.datasets.iter().any(|x| x.name == d) => {
                        out.push(Diagnostic::error(
                            file,
                            format!("{rp}.dataset"),
                            "E072",
                            format!("unknown dataset `{d}`"),
                        ))
                    }
                    _ => {}
                }
                if let Some(v) = &r.validate
                    && (v.starts_with('/') || v.split(['/', '\\']).any(|c| c == ".."))
                {
                    out.push(Diagnostic::error(
                        file,
                        format!("{rp}.validate"),
                        "E042",
                        "validate must stay inside the workspace",
                    ));
                }
                if !r.responses.is_empty() {
                    out.push(Diagnostic::warning(
                        file,
                        format!("{rp}.responses"),
                        "W070",
                        "responses are ignored on a crud Route",
                    ));
                }
            }
            if r.kind == "static" {
                match r.dir.as_deref() {
                    None | Some("") => out.push(Diagnostic::error(
                        file,
                        format!("{rp}.dir"),
                        "E090",
                        "a static Route needs `dir`",
                    )),
                    Some(d) if d.starts_with('/') || d.split(['/', '\\']).any(|c| c == "..") => out
                        .push(Diagnostic::error(
                            file,
                            format!("{rp}.dir"),
                            "E042",
                            "dir must stay inside the workspace",
                        )),
                    _ => {}
                }
                if !r.path.starts_with('/') {
                    out.push(Diagnostic::error(
                        file,
                        format!("{rp}.path"),
                        "E031",
                        "path must start with `/`",
                    ));
                }
                if r.template_extensions
                    .iter()
                    .any(|e| !e.starts_with('.') || e.len() < 2)
                {
                    out.push(Diagnostic::error(
                        file,
                        format!("{rp}.template_extensions"),
                        "E091",
                        "template extensions must start with `.`, e.g. `.tera`",
                    ));
                }
                if let Some(i) = &r.index
                    && (i.starts_with('/') || i.split(['/', '\\']).any(|c| c == ".."))
                {
                    out.push(Diagnostic::error(
                        file,
                        format!("{rp}.index"),
                        "E042",
                        "index must stay inside the directory",
                    ));
                }
                if !r.responses.is_empty() {
                    out.push(Diagnostic::warning(
                        file,
                        format!("{rp}.responses"),
                        "W070",
                        "responses are ignored on a static Route",
                    ));
                }
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
                if let Some(d) = &resp.delay_ms
                    && d.bounds().is_none()
                {
                    out.push(Diagnostic::error(
                        file,
                        format!("{sp}.delay_ms"),
                        "E083",
                        format!(
                            "delay `{}` is not milliseconds or a range like 200..800",
                            d.text()
                        ),
                    ));
                }
                if let Some(f) = resp.fault.as_deref()
                    && !crate::FAULTS.contains(&f)
                {
                    out.push(Diagnostic::error(
                        file,
                        format!("{sp}.fault"),
                        "E084",
                        format!("unknown fault `{f}`"),
                    ));
                }
                if let Some(r) = resp.fault_rate
                    && !(0.0..=1.0).contains(&r)
                {
                    out.push(Diagnostic::error(
                        file,
                        format!("{sp}.fault_rate"),
                        "E085",
                        format!("fault_rate {r} is not between 0 and 1"),
                    ));
                }
                if resp.fault.as_deref() == Some("slow_body")
                    && s.network.as_ref().is_none_or(|n| n.bandwidth_kbps == 0)
                {
                    out.push(Diagnostic::warning(file, format!("{sp}.fault"), "W060", "slow_body needs `network.bandwidth_kbps` (or a network profile) to throttle"));
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
                for (k, rule) in resp.rules.iter().enumerate() {
                    if matches!(rule.target.as_str(), "dataset" | "dataset_count")
                        && let Some(key) = rule.key.as_deref()
                    {
                        let n = crate::rules::dataset_rule_name(key);
                        if !n.is_empty() && !s.datasets.iter().any(|d| d.name == n) {
                            out.push(Diagnostic::error(
                                file,
                                format!("{sp}.rules[{k}].key"),
                                "E088",
                                format!("rule reads unknown Dataset `{n}`"),
                            ));
                        }
                    }
                }
                for (k, a) in resp.actions.iter().enumerate() {
                    let ap = format!("{sp}.actions[{k}]");
                    let Some(ds) = s.datasets.iter().find(|d| d.name == a.dataset) else {
                        out.push(Diagnostic::error(
                            file,
                            format!("{ap}.dataset"),
                            "E086",
                            format!("action on unknown Dataset `{}`", a.dataset),
                        ));
                        continue;
                    };
                    if let Some(m) = validate_action(a) {
                        out.push(Diagnostic::error(file, ap.clone(), "E087", m));
                    }
                    if ds.persist && !a.disabled {
                        out.push(Diagnostic::warning(
                            file,
                            format!("{ap}.dataset"),
                            "W072",
                            format!(
                                "action writes to Dataset `{}`, which is persisted to disk",
                                ds.name
                            ),
                        ));
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
        let mut names: HashMap<&str, ()> = HashMap::new();
        for (i, d) in s.datasets.iter().enumerate() {
            let dp = format!("datasets[{i}]");
            if d.name.trim().is_empty() {
                out.push(Diagnostic::error(
                    file,
                    format!("{dp}.name"),
                    "E073",
                    "dataset name is empty",
                ));
            } else if names.insert(&d.name, ()).is_some() {
                out.push(Diagnostic::error(
                    file,
                    format!("{dp}.name"),
                    "E073",
                    format!("duplicate dataset `{}`", d.name),
                ));
            }
            if let Some(st) = d.id_strategy.as_deref()
                && !crate::ID_STRATEGIES.contains(&st)
            {
                out.push(Diagnostic::error(
                    file,
                    format!("{dp}.id_strategy"),
                    "E074",
                    format!("id_strategy must be uuid, increment or ulid, got `{st}`"),
                ));
            }
            let sources = [
                d.seed.is_some(),
                d.seed_template.is_some(),
                d.generator.is_some(),
            ];
            if sources.iter().filter(|b| **b).count() > 1 {
                out.push(Diagnostic::error(
                    file,
                    format!("{dp}.seed"),
                    "E075",
                    "set at most one of `seed`, `seed_template` and `generator`",
                ));
            }
            let mut col_names = std::collections::HashSet::new();
            for (ci, c) in d.columns.iter().enumerate() {
                if c.name.trim().is_empty() || !col_names.insert(c.name.as_str()) {
                    out.push(Diagnostic::error(
                        file,
                        format!("{dp}.columns[{ci}].name"),
                        "E078",
                        format!("column name `{}` is empty or duplicated", c.name),
                    ));
                }
                if !c.ty.is_empty() && !crate::COLUMN_TYPES.contains(&c.ty.as_str()) {
                    out.push(Diagnostic::error(
                        file,
                        format!("{dp}.columns[{ci}].type"),
                        "E079",
                        format!(
                            "column type must be one of {}, got `{}`",
                            crate::COLUMN_TYPES.join(", "),
                            c.ty
                        ),
                    ));
                }
            }
            if let Some(g) = &d.generator {
                let names: Vec<&str> = s.datasets.iter().map(|x| x.name.as_str()).collect();
                let mut refs = Vec::new();
                let mut problems = Vec::new();
                crate::generator::check_fields(
                    &g.fields,
                    &format!("{dp}.generator.fields"),
                    &names,
                    &mut refs,
                    &mut problems,
                );
                for (p, m) in problems {
                    out.push(Diagnostic::error(file, p, "E076", m));
                }
            }
            for (k, v) in [("seed", &d.seed), ("seed_template", &d.seed_template)] {
                if let Some(v) = v
                    && (v.starts_with('/') || v.split(['/', '\\']).any(|c| c == ".."))
                {
                    out.push(Diagnostic::error(
                        file,
                        format!("{dp}.{k}"),
                        "E042",
                        format!("{k} must stay inside the workspace"),
                    ));
                }
            }
        }
        if let Some(cycle) = crate::generator::ref_cycle(&s.datasets) {
            out.push(Diagnostic::error(
                file,
                "datasets",
                "E077",
                format!("generator `ref` cycle: {}", cycle.join(" -> ")),
            ));
        }
        if let Some(t) = &s.tls {
            if !crate::TLS_MODES.contains(&t.mode_name()) {
                out.push(Diagnostic::error(
                    file,
                    "tls.mode",
                    "E092",
                    format!("tls mode must be dev-ca or files, got `{}`", t.mode_name()),
                ));
            }
            if t.enabled && t.mode_name() == "files" {
                for (k, v) in [("cert", &t.cert), ("key", &t.key)] {
                    match v.as_deref() {
                        None | Some("") => out.push(Diagnostic::error(
                            file,
                            format!("tls.{k}"),
                            "E093",
                            format!("tls mode `files` needs `{k}`"),
                        )),
                        Some(p)
                            if p.starts_with('/') || p.split(['/', '\\']).any(|c| c == "..") =>
                        {
                            out.push(Diagnostic::error(
                                file,
                                format!("tls.{k}"),
                                "E042",
                                format!("{k} must stay inside the workspace"),
                            ))
                        }
                        _ => {}
                    }
                }
            }
            if t.also_http_port == Some(s.port) {
                out.push(Diagnostic::error(
                    file,
                    "tls.also_http_port",
                    "E094",
                    "also_http_port must differ from the server port",
                ));
            }
        }
        if let Some(c) = &s.cors
            && c.enabled
            && c.origins.is_empty()
        {
            out.push(Diagnostic::warning(
                file,
                "cors.origins",
                "W090",
                "CORS is enabled but no origins are allowed",
            ));
        }
        if let Some(p) = &s.proxy {
            if let Some(m) = p.mode.as_deref()
                && !m.is_empty()
                && !crate::PROXY_MODES.contains(&m)
            {
                out.push(Diagnostic::error(
                    file,
                    "proxy.mode",
                    "E095",
                    format!("unknown proxy mode `{m}` (fallback or passthrough)"),
                ));
            }
            let up = p.upstream.as_deref().unwrap_or("").trim();
            if p.enabled && up.is_empty() {
                out.push(Diagnostic::error(
                    file,
                    "proxy.upstream",
                    "E096",
                    "the proxy is enabled but has no upstream",
                ));
            } else if !up.is_empty() && !up.contains("{{") && !up.contains("{%") {
                let ok = up.split_once("://").is_some_and(|(sch, rest)| {
                    (sch == "http" || sch == "https") && !rest.trim_start_matches('/').is_empty()
                });
                if !ok {
                    out.push(Diagnostic::error(
                        file,
                        "proxy.upstream",
                        "E097",
                        format!("upstream `{up}` must be an http:// or https:// URL"),
                    ));
                }
            }
            if p.insecure {
                out.push(Diagnostic::warning(
                    file,
                    "proxy.insecure",
                    "W091",
                    "invalid upstream certificates are accepted",
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

/// Static problems of one action step (unknown op, missing inputs, bad Tera); `None` when fine.
pub fn validate_action(a: &crate::DatasetAction) -> Option<String> {
    if !crate::ACTION_OPS.contains(&a.op.as_str()) {
        return Some(format!("unknown action op `{}`", a.op));
    }
    // update/set/increment may omit the key on a singleton Dataset; the engine checks that.
    let need_key = a.op == "delete";
    if need_key && a.key.as_deref().is_none_or(str::is_empty) {
        return Some(format!("action `{}` needs a `key` expression", a.op));
    }
    if a.op == "delete_where" && a.filter.is_empty() {
        return Some("`delete_where` needs a `filter`".into());
    }
    if matches!(a.op.as_str(), "set" | "increment") && a.field.as_deref().is_none_or(str::is_empty)
    {
        return Some(format!("action `{}` needs a `field`", a.op));
    }
    if a.op == "set" && a.value.is_none() {
        return Some("`set` needs a `value`".into());
    }
    if let Some(m) = a.mode.as_deref()
        && m != "merge"
        && m != "replace"
    {
        return Some(format!("mode must be `merge` or `replace`, got `{m}`"));
    }
    None
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
    fn dataset_actions_and_rules_are_checked() {
        let t = "id=\"srv_a\"\nname=\"n\"\nport=1\n[[datasets]]\nname=\"o\"\npersist=true\n\
            [[routes]]\nid=\"rte_1\"\npath=\"/x\"\n[[routes.responses]]\nid=\"rsp_1\"\n\
            [[routes.responses.actions]]\ndataset=\"nope\"\nop=\"insert\"\n\
            [[routes.responses.actions]]\ndataset=\"o\"\nop=\"explode\"\n\
            [[routes.responses.actions]]\ndataset=\"o\"\nop=\"insert\"\n\
            [[routes.responses.rules]]\ntarget=\"dataset_count\"\nkey=\"zzz?a=1\"\nop=\"gte\"\nvalue=1\n";
        let d = validate_servers(&[("servers/a.toml".into(), parse_server(t).unwrap())]);
        let codes: Vec<_> = d.iter().map(|d| d.code.as_str()).collect();
        for c in ["E086", "E087", "E088", "W072"] {
            assert!(codes.contains(&c), "{c} in {codes:?}");
        }
    }

    #[test]
    fn clean_server_has_no_findings() {
        assert!(validate_servers(&[srv("srv_a", 3000, "rte_1")]).is_empty());
    }

    #[test]
    fn unknown_fields_are_tolerated() {
        let s = parse_server(
            "id=\"srv_a\"\nname=\"n\"\nport=1\nfuture=1\n[future_table]\nenabled=false\n",
        )
        .unwrap();
        assert!(s.extra.contains_key("future") && s.extra.contains_key("future_table"));
    }

    #[test]
    fn dataset_and_crud_checks() {
        let t = "id=\"srv_a\"\nname=\"n\"\nport=1\n\
            [[datasets]]\nname=\"u\"\nid_strategy=\"nope\"\nseed=\"a.json\"\nseed_template=\"b\"\n\
            [[datasets]]\nname=\"u\"\n\
            [[routes]]\nid=\"r1\"\nkind=\"crud\"\npath=\"/u\"\ndataset=\"missing\"\n\
            [[routes]]\nid=\"r2\"\nkind=\"crud\"\npath=\"/v\"\n\
            [[routes]]\nid=\"r3\"\nkind=\"bogus\"\npath=\"/w\"\n";
        let d = validate_servers(&[("servers/a.toml".into(), parse_server(t).unwrap())]);
        let codes: Vec<_> = d.iter().map(|d| d.code.as_str()).collect();
        for c in ["E070", "E071", "E072", "E073", "E074", "E075"] {
            assert!(codes.contains(&c), "{c} in {codes:?}");
        }
    }

    #[test]
    fn proxy_checks() {
        let s = crate::parse_server(
        "id=\"srv_a\"\nname=\"A\"\nport=1\n[proxy]\nenabled=true\nmode=\"bogus\"\nupstream=\"ftp://x\"\ninsecure=true\n[[routes]]\nid=\"r1\"\npath=\"/a\"\nkind=\"static\"\ndir=\"p\"\nproxy=true\n",
    )
    .unwrap();
        let d = validate_servers(&[("a.toml".into(), s)]);
        let codes: Vec<_> = d.iter().map(|d| d.code.as_str()).collect();
        for c in ["E095", "E097", "W091", "W092"] {
            assert!(codes.contains(&c), "{c} in {codes:?}");
        }
    }

    #[test]
    fn static_tls_cors_checks() {
        let t = "id=\"srv_a\"\nname=\"n\"\nport=1\n\
            [tls]\nenabled=true\nmode=\"files\"\nalso_http_port=1\n\
            [cors]\nenabled=true\n\
            [[routes]]\nid=\"r1\"\nkind=\"static\"\npath=\"/a/*\"\n\
            [[routes]]\nid=\"r2\"\nkind=\"static\"\npath=\"/b/*\"\ndir=\"../x\"\ntemplate_extensions=[\"tera\"]\n";
        let d = validate_servers(&[("servers/a.toml".into(), parse_server(t).unwrap())]);
        let codes: Vec<_> = d.iter().map(|d| d.code.as_str()).collect();
        for c in ["E090", "E091", "E042", "E093", "E094", "W090"] {
            assert!(codes.contains(&c), "{c} in {codes:?}");
        }
    }
}
