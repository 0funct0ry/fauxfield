//! Rule vocabulary and per-rule validation (SPEC §5.3, §5.4).

use crate::Rule;

pub const RESPONSE_MODES: &[&str] = &["rules", "sequential", "random", "disabled"];
pub const TARGETS: &[&str] = &[
    "header", "query", "cookie", "param", "body", "method", "path", "ip", "variable", "dataset",
    "count",
];
pub const OPS: &[&str] = &[
    "eq",
    "ne",
    "contains",
    "starts_with",
    "ends_with",
    "regex",
    "exists",
    "absent",
    "gt",
    "gte",
    "lt",
    "lte",
    "in",
    "json_schema",
];

/// Targets whose key is required.
pub fn needs_key(target: &str) -> bool {
    matches!(
        target,
        "header" | "query" | "cookie" | "param" | "variable" | "dataset"
    )
}

/// Check one rule; `Err` is `(code, message)`.
pub fn check_rule(r: &Rule) -> Result<(), (&'static str, String)> {
    if !TARGETS.contains(&r.target.as_str()) {
        return Err(("E044", format!("unknown rule target `{}`", r.target)));
    }
    if !OPS.contains(&r.op.as_str()) {
        return Err(("E045", format!("unknown rule op `{}`", r.op)));
    }
    let key = r.key.as_deref().unwrap_or("");
    if needs_key(&r.target) && key.is_empty() {
        return Err(("E046", format!("rule target `{}` needs a key", r.target)));
    }
    if r.target == "body" && key.starts_with('$') && serde_json_path::JsonPath::parse(key).is_err()
    {
        return Err(("E048", format!("invalid JSONPath `{key}`")));
    }
    let needs_value = !matches!(r.op.as_str(), "exists" | "absent");
    if needs_value && r.value.is_none() {
        return Err(("E046", format!("op `{}` needs a value", r.op)));
    }
    if r.op == "regex" {
        match &r.value {
            Some(toml::Value::String(p)) => {
                crate::pattern::compile_regex(p).map_err(|m| ("E047", m))?;
            }
            _ => return Err(("E047", "regex value must be a string".into())),
        }
    }
    if r.op == "in" && !matches!(r.value, Some(toml::Value::Array(_))) {
        return Err(("E046", "op `in` needs an array value".into()));
    }
    if matches!(r.op.as_str(), "gt" | "gte" | "lt" | "lte")
        && let Some(v) = &r.value
        && value_f64(v).is_none()
    {
        return Err(("E046", format!("op `{}` needs a numeric value", r.op)));
    }
    Ok(())
}

pub fn value_f64(v: &toml::Value) -> Option<f64> {
    match v {
        toml::Value::Integer(i) => Some(*i as f64),
        toml::Value::Float(f) => Some(*f),
        toml::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}
