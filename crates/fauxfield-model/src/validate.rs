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
                if let Some(bad) = r
                    .crud_ops
                    .iter()
                    .find(|o| !crate::CRUD_OPS.contains(&o.as_str()))
                {
                    out.push(Diagnostic::error(
                        file,
                        format!("{rp}.crud_ops"),
                        "E101",
                        format!(
                            "crud_ops entries must be one of {}, got `{bad}`",
                            crate::CRUD_OPS.join(", ")
                        ),
                    ));
                }
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
                        "top-level responses are ignored on a crud Route; use operation Responses",
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
            if r.kind == "websocket" {
                if !r.path.starts_with('/') {
                    out.push(Diagnostic::error(
                        file,
                        format!("{rp}.path"),
                        "E031",
                        "path must start with `/`",
                    ));
                }
                if let Some(m) = r.mode.as_deref()
                    && !crate::WS_MODES.contains(&m)
                {
                    out.push(Diagnostic::error(
                        file,
                        format!("{rp}.mode"),
                        "E102",
                        format!("mode must be echo, script or broadcast, got `{m}`"),
                    ));
                }
                for (k, m) in r.messages.iter().enumerate() {
                    let mp = format!("{rp}.messages[{k}]");
                    if !crate::WS_TRIGGERS.contains(&m.trigger.as_str()) {
                        out.push(Diagnostic::error(
                            file,
                            format!("{mp}.trigger"),
                            "E103",
                            format!(
                                "trigger must be connect, interval or message, got `{}`",
                                m.trigger
                            ),
                        ));
                    }
                    if m.trigger == "interval" {
                        match m.every.as_deref() {
                            None => out.push(Diagnostic::error(
                                file,
                                format!("{mp}.every"),
                                "E104",
                                "an interval message needs `every`, e.g. \"5s\"",
                            )),
                            Some(e) if crate::parse_duration_ms(e).is_none() => {
                                out.push(Diagnostic::error(
                                    file,
                                    format!("{mp}.every"),
                                    "E104",
                                    format!("`every` must look like 500ms, 5s or 1m (min 10ms), got `{e}`"),
                                ))
                            }
                            _ => {}
                        }
                    }
                    if let Some(w) = &m.when {
                        if m.trigger != "message" {
                            out.push(Diagnostic::warning(
                                file,
                                format!("{mp}.when"),
                                "W093",
                                "`when` only applies to message triggers",
                            ));
                        }
                        if let Err((code, msg)) = crate::rules::check_rule(w) {
                            out.push(Diagnostic::error(file, format!("{mp}.when"), code, msg));
                        }
                    }
                    if m.binary
                        && !m.body.bytes().all(|b| {
                            b.is_ascii_alphanumeric()
                                || matches!(b, b'+' | b'/' | b'=' | b'\n' | b'\r' | b' ')
                        })
                    {
                        out.push(Diagnostic::error(
                            file,
                            format!("{mp}.body"),
                            "E105",
                            "binary message body must be base64",
                        ));
                    }
                    if let Some(f) = m.format.as_deref()
                        && !crate::WS_FORMATS.contains(&f)
                    {
                        out.push(Diagnostic::error(
                            file,
                            format!("{mp}.format"),
                            "E107",
                            format!("format must be compact, pretty or raw, got `{f}`"),
                        ));
                    }
                    if let Some(c) = m.close_code
                        && !(1000..=4999).contains(&c)
                    {
                        out.push(Diagnostic::error(
                            file,
                            format!("{mp}.close_code"),
                            "E106",
                            "close_code must be between 1000 and 4999",
                        ));
                    }
                }
                if !r.responses.is_empty() {
                    out.push(Diagnostic::warning(
                        file,
                        format!("{rp}.responses"),
                        "W070",
                        "responses are ignored on a websocket Route",
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
            if r.kind != "crud" && !r.operations.is_empty() {
                out.push(Diagnostic::warning(
                    file,
                    format!("{rp}.operations"),
                    "W070",
                    "operations are ignored on a Route that is not crud",
                ));
            }
            if r.kind == "crud" {
                for (op, o) in &r.operations {
                    let op_path = format!("{rp}.operations.{op}");
                    if !crate::CRUD_OPS.contains(&op.as_str()) {
                        out.push(Diagnostic::error(
                            file,
                            op_path.clone(),
                            "E116",
                            format!(
                                "unknown operation `{op}`; use one of {}",
                                crate::CRUD_OPS.join(", ")
                            ),
                        ));
                    } else if !r.crud_ops.is_empty()
                        && !r.crud_ops.iter().any(|c| c == op)
                        && !o.responses.is_empty()
                    {
                        out.push(Diagnostic::error(
                            file,
                            op_path.clone(),
                            "E117",
                            format!(
                                "operation `{op}` is not in `crud_ops`, so its Responses never run"
                            ),
                        ));
                    }
                    for (j, resp) in o.responses.iter().enumerate() {
                        let sp = format!("{op_path}.responses[{j}]");
                        if resp.default {
                            out.push(Diagnostic::warning(
                                file,
                                format!("{sp}.default"),
                                "W098",
                                "`default` has no effect inside an operation",
                            ));
                        }
                        if resp.rules.is_empty() {
                            out.push(Diagnostic::warning(
                                file,
                                format!("{sp}.rules"),
                                "W073",
                                format!("Response `{}` has no Rules, so it never matches", resp.id),
                            ));
                        }
                    }
                }
            }
            let mut all_responses: Vec<(String, &crate::Response)> = Vec::new();
            for (j, resp) in r.responses.iter().enumerate() {
                all_responses.push((format!("{rp}.responses[{j}]"), resp));
            }
            for (op, o) in &r.operations {
                for (j, resp) in o.responses.iter().enumerate() {
                    all_responses.push((format!("{rp}.operations.{op}.responses[{j}]"), resp));
                }
            }
            for (sp, resp) in all_responses {
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
                let mut cb_ids = std::collections::HashSet::new();
                for (k, cb) in resp.callbacks.iter().enumerate() {
                    let cp = format!("{sp}.callbacks[{k}]");
                    if cb.id.trim().is_empty() || !cb_ids.insert(cb.id.clone()) {
                        out.push(Diagnostic::error(
                            file,
                            format!("{cp}.id"),
                            "E108",
                            "Callback id is empty or repeated within the Response",
                        ));
                    }
                    if cb.url.trim().is_empty() {
                        out.push(Diagnostic::error(
                            file,
                            format!("{cp}.url"),
                            "E109",
                            "Callback needs a url",
                        ));
                    } else if !(cb.url.contains("{{")
                        || cb.url.contains("{%")
                        || cb.url.starts_with("http://")
                        || cb.url.starts_with("https://"))
                    {
                        out.push(Diagnostic::error(
                            file,
                            format!("{cp}.url"),
                            "E110",
                            "Callback url must be http:// or https://",
                        ));
                    }
                    if let Some(m) = cb.method.as_deref()
                        && !crate::types::CALLBACK_METHODS
                            .contains(&m.to_ascii_uppercase().as_str())
                    {
                        out.push(Diagnostic::error(
                            file,
                            format!("{cp}.method"),
                            "E111",
                            format!("unknown Callback method `{m}`"),
                        ));
                    }
                    if cb.retries.unwrap_or(0) > crate::types::CALLBACK_MAX_RETRIES {
                        out.push(Diagnostic::error(
                            file,
                            format!("{cp}.retries"),
                            "E112",
                            format!("retries is at most {}", crate::types::CALLBACK_MAX_RETRIES),
                        ));
                    }
                    if resp.fault.as_deref().is_some_and(|f| f != "none") && !cb.disabled {
                        out.push(Diagnostic::warning(
                            file,
                            cp,
                            "W094",
                            "Callbacks do not fire when the Response injects a fault that drops the connection",
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
                if !crate::columns::default_fits(c) {
                    out.push(Diagnostic::error(
                        file,
                        format!("{dp}.columns[{ci}].default"),
                        "E099",
                        format!(
                            "default of column `{}` does not fit type `{}`",
                            c.name, c.ty
                        ),
                    ));
                }
            }
            let pks: Vec<&crate::DatasetColumn> =
                d.columns.iter().filter(|c| c.primary_key).collect();
            if pks.len() > 1 {
                out.push(Diagnostic::error(
                    file,
                    format!("{dp}.columns"),
                    "E098",
                    "at most one column may be `primary_key`",
                ));
            } else if let Some(pk) = pks.first()
                && d.key
                    .as_deref()
                    .is_some_and(|k| !k.is_empty() && k != pk.name)
            {
                out.push(Diagnostic::error(
                    file,
                    format!("{dp}.columns"),
                    "E100",
                    format!(
                        "primary key column `{}` does not match the Dataset `key` `{}`",
                        pk.name,
                        d.key_name()
                    ),
                ));
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
        docs_checks(file, s, &mut out);
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
/// True for a docs/openapi path that is absolute, has no `:`, `*` or `{`, and no trailing slash.
fn docs_path_ok(p: &str) -> bool {
    p.starts_with('/') && p != "/" && !p.ends_with('/') && !p.contains([':', '*', '{', '}'])
}

fn confined_path(p: &str) -> bool {
    !(p.starts_with('/') || p.split(['/', '\\']).any(|c| c == ".."))
}

/// M17.06 model-level checks (SPEC §12.4): E113, E114, E042 on schema files, W095.
fn docs_checks(file: &str, s: &Server, out: &mut Vec<Diagnostic>) {
    let mut op_ids: HashMap<String, &str> = HashMap::new();
    for r in &s.routes {
        let rp = format!("routes[{}]", r.id);
        let ids = [(r.openapi.as_ref().and_then(|o| o.operation_id.clone()))
            .unwrap_or_else(|| r.id.clone())];
        for id in ids {
            if let Some(prev) = op_ids.insert(id.clone(), &r.id) {
                out.push(Diagnostic::error(
                    file,
                    format!("{rp}.openapi.operation_id"),
                    "E114",
                    format!("operationId `{id}` is also used by Route `{prev}`"),
                ));
            }
        }
        if let Some(o) = &r.openapi {
            if let Some(id) = o.operation_id.as_deref()
                && (id.is_empty() || id.contains(char::is_whitespace))
            {
                out.push(Diagnostic::error(
                    file,
                    format!("{rp}.openapi.operation_id"),
                    "E114",
                    "operation_id must be non-empty and contain no whitespace",
                ));
            }
            if let Some(p) = o.path.as_deref()
                && (!p.starts_with('/') || p.is_empty())
            {
                out.push(Diagnostic::error(
                    file,
                    format!("{rp}.openapi.path"),
                    "E114",
                    "openapi.path must start with `/`",
                ));
            }
            if let Some(f) = o.request_schema.as_deref()
                && !confined_path(f)
            {
                out.push(Diagnostic::error(
                    file,
                    format!("{rp}.openapi.request_schema"),
                    "E042",
                    "request_schema must stay inside the workspace",
                ));
            }
        }
        for (j, resp) in r.responses.iter().enumerate() {
            if let Some(f) = resp.schema.as_deref()
                && !confined_path(f)
            {
                out.push(Diagnostic::error(
                    file,
                    format!("{rp}.responses[{j}].schema"),
                    "E042",
                    "schema must stay inside the workspace",
                ));
            }
        }
    }
    let Some(d) = &s.docs_site else { return };
    if let Some(p) = d.path.as_deref()
        && !docs_path_ok(p)
    {
        out.push(Diagnostic::error(
            file,
            "docs_site.path",
            "E113",
            format!("docs_site.path `{p}` is invalid"),
        ));
    }
    if d.enabled && !matches!(s.host.as_str(), "127.0.0.1" | "localhost" | "::1") {
        out.push(Diagnostic::warning(
            file,
            "docs_site.enabled",
            "W095",
            format!(
                "API documentation is published on non-loopback host `{}`",
                s.host
            ),
        ));
    }
}

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
    fn websocket_routes_are_checked() {
        let t = "id=\"srv_a\"\nname=\"n\"\nport=1\n[[routes]]\nid=\"r1\"\nkind=\"websocket\"\npath=\"/ws\"\nmode=\"bogus\"\n\
            [[routes.messages]]\ntrigger=\"interval\"\nevery=\"soon\"\n\
            [[routes.messages]]\ntrigger=\"nope\"\n\
            [[routes.messages]]\ntrigger=\"connect\"\nclose_code=5\nformat=\"fancy\"\n";
        let d = validate_servers(&[("servers/a.toml".into(), parse_server(t).unwrap())]);
        for c in ["E102", "E103", "E104", "E106", "E107"] {
            assert!(d.iter().any(|d| d.code == c), "{c} in {d:?}");
        }
        let ok = "id=\"srv_a\"\nname=\"n\"\nport=1\n[[routes]]\nid=\"r1\"\nkind=\"websocket\"\npath=\"/ws\"\nmode=\"script\"\n\
            [[routes.messages]]\ntrigger=\"interval\"\nevery=\"500ms\"\nbody=\"x\"\n";
        let d = validate_servers(&[("servers/a.toml".into(), parse_server(ok).unwrap())]);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(crate::parse_duration_ms("1m"), Some(60_000));
        assert_eq!(crate::parse_duration_ms("5ms"), None);
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
    fn callbacks_are_checked() {
        let t = "id=\"srv_a\"\nname=\"n\"\nport=1\n[[routes]]\nid=\"rte_1\"\npath=\"/x\"\n[[routes.responses]]\nid=\"rsp_1\"\n\
            [[routes.responses.callbacks]]\nid=\"a\"\nurl=\"\"\n\
            [[routes.responses.callbacks]]\nid=\"a\"\nurl=\"ftp://x\"\nmethod=\"NOPE\"\nretries=99\n";
        let d = validate_servers(&[("servers/a.toml".into(), parse_server(t).unwrap())]);
        let codes: Vec<_> = d.iter().map(|d| d.code.as_str()).collect();
        for c in ["E108", "E109", "E110", "E111", "E112"] {
            assert!(codes.contains(&c), "{c} in {codes:?}");
        }
    }

    #[test]
    fn docs_checks_fire() {
        let t = "id=\"srv_a\"\nname=\"n\"\nport=1\nhost=\"0.0.0.0\"\n[docs_site]\nenabled=true\npath=\"/a/\"\n\
            [[routes]]\nid=\"rte_1\"\npath=\"/x\"\n[routes.openapi]\noperation_id=\"dup\"\nrequest_schema=\"../x.json\"\n\
            [[routes]]\nid=\"dup\"\npath=\"/y\"\n";
        let d = validate_servers(&[("servers/a.toml".into(), parse_server(t).unwrap())]);
        let codes: Vec<_> = d.iter().map(|d| d.code.as_str()).collect();
        for c in ["E113", "E114", "E042", "W095"] {
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
    fn column_constraint_checks() {
        let t = "id=\"srv_a\"\nname=\"n\"\nport=1\n\
            [[datasets]]\nname=\"u\"\nkey=\"id\"\n\
            [[datasets.columns]]\nname=\"a\"\ntype=\"int\"\nprimary_key=true\ndefault=\"x\"\n\
            [[datasets.columns]]\nname=\"b\"\nprimary_key=true\n";
        let d = validate_servers(&[("servers/a.toml".into(), parse_server(t).unwrap())]);
        let codes: Vec<_> = d.iter().map(|d| d.code.as_str()).collect();
        for c in ["E098", "E099"] {
            assert!(codes.contains(&c), "{c} in {codes:?}");
        }
        let t = "id=\"srv_a\"\nname=\"n\"\nport=1\n\
            [[datasets]]\nname=\"u\"\nkey=\"id\"\n\
            [[datasets.columns]]\nname=\"sku\"\nprimary_key=true\n";
        let d = validate_servers(&[("servers/a.toml".into(), parse_server(t).unwrap())]);
        assert!(d.iter().any(|d| d.code == "E100"));
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
    fn operation_responses_are_checked() {
        let t = "id=\"srv_a\"\nname=\"n\"\nport=1\n[[datasets]]\nname=\"u\"\n\
            [[routes]]\nid=\"r1\"\nkind=\"crud\"\npath=\"/u\"\ndataset=\"u\"\ncrud_ops=[\"list\",\"create\"]\n\
            [[routes.responses]]\nid=\"top\"\n\
            [[routes.operations.create.responses]]\nid=\"a\"\ndefault=true\n\
            [[routes.operations.create.responses]]\nid=\"a\"\nstatus=409\n\
            [[routes.operations.create.responses.rules]]\ntarget=\"nope\"\nop=\"eq\"\n\
            [[routes.operations.bogus.responses]]\nid=\"b\"\n\
            [[routes.operations.delete.responses]]\nid=\"c\"\n";
        let d = validate_servers(&[("servers/a.toml".into(), parse_server(t).unwrap())]);
        let codes: Vec<_> = d.iter().map(|d| d.code.as_str()).collect();
        for c in ["W070", "W073", "W098", "E013", "E116", "E117", "E044"] {
            assert!(codes.contains(&c), "{c} in {codes:?}");
        }
        let at = |c: &str| d.iter().find(|x| x.code == c).unwrap().path.clone();
        assert_eq!(at("E116"), "routes[r1].operations.bogus");
        assert_eq!(at("E117"), "routes[r1].operations.delete");
        assert_eq!(
            at("W098"),
            "routes[r1].operations.create.responses[0].default"
        );
        assert!(d.iter().all(|x| x.help.is_some()), "{d:?}");
    }

    #[test]
    fn operations_round_trip_and_stay_out_of_files_without_them() {
        let plain = "id=\"srv_a\"\nname=\"n\"\nport=1\n[[routes]]\nid=\"r\"\npath=\"/x\"\n";
        let s = parse_server(plain).unwrap();
        assert!(!toml::to_string(&s).unwrap().contains("operations"));
        let t = format!("{plain}[[routes.operations.create.responses]]\nid=\"o\"\nstatus=409\n");
        let s = parse_server(&t).unwrap();
        assert_eq!(s.routes[0].operations["create"].responses[0].status, 409);
        let again = parse_server(&toml::to_string(&s).unwrap()).unwrap();
        assert_eq!(again.routes[0].operations["create"].responses[0].id, "o");
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
