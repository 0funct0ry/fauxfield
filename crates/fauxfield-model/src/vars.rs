//! Variable resolution (SPEC §4.3, §8.1).
//!
//! Precedence, most specific first: runtime > route > server > environment > global.
//! Values written `${env:NAME}` expand from the process environment; a dotenv file
//! supplies extra environment keys. Expanded values are never written back to disk.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Global,
    Environment,
    Server,
    Route,
    Runtime,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Global => "global",
            Scope::Environment => "environment",
            Scope::Server => "server",
            Scope::Route => "route",
            Scope::Runtime => "runtime",
        }
    }
    pub fn parse(s: &str) -> Option<Scope> {
        Some(match s {
            "global" => Scope::Global,
            "environment" | "env" => Scope::Environment,
            "server" => Scope::Server,
            "route" => Scope::Route,
            "runtime" => Scope::Runtime,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    File,
    Dotenv,
    Process,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedVar {
    pub key: String,
    /// Effective (expanded) value as text.
    pub value: String,
    pub scope: Scope,
    pub source: Source,
    /// Lower-precedence definitions this value hides.
    pub shadows: Vec<Scope>,
}

/// True for keys whose values are masked and redacted: `*token*`, `*secret*`, `*password*`, `*key*`.
pub fn is_secret_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    ["token", "secret", "password", "key"]
        .iter()
        .any(|w| k.contains(w))
}

pub fn value_text(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Parse dotenv text: `KEY=value`, optional `export `, `#` comments, quotes stripped.
pub fn parse_dotenv(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim();
        let v = if v.len() >= 2
            && ((v.starts_with('"') && v.ends_with('"'))
                || (v.starts_with('\'') && v.ends_with('\'')))
        {
            &v[1..v.len() - 1]
        } else {
            v
        };
        out.insert(k.trim().to_string(), v.to_string());
    }
    out
}

/// Expand `${env:NAME}` using dotenv first, then `lookup` (the process environment).
/// Returns the text and the source of the last expansion (File if none).
pub fn expand_env(
    raw: &str,
    dotenv: &BTreeMap<String, String>,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> (String, Source) {
    let mut out = String::new();
    let mut src = Source::File;
    let mut rest = raw;
    while let Some(i) = rest.find("${env:") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 6..];
        let Some(j) = after.find('}') else {
            out.push_str(&rest[i..]);
            return (out, src);
        };
        let name = &after[..j];
        if let Some(v) = dotenv.get(name) {
            out.push_str(v);
            src = Source::Dotenv;
        } else if let Some(v) = lookup(name) {
            out.push_str(&v);
            src = Source::Process;
        }
        rest = &after[j + 1..];
    }
    out.push_str(rest);
    (out, src)
}

/// The layers to merge. Each layer is raw values; `${env:}` is expanded for all of them.
#[derive(Default)]
pub struct Layers<'a> {
    pub global: Option<&'a BTreeMap<String, toml::Value>>,
    pub environment: Option<&'a BTreeMap<String, toml::Value>>,
    pub server: Option<&'a BTreeMap<String, toml::Value>>,
    pub route: Option<&'a BTreeMap<String, toml::Value>>,
    pub runtime: Option<&'a BTreeMap<String, String>>,
    /// Parsed dotenv of the active environment.
    pub dotenv: BTreeMap<String, String>,
}

/// Merge layers into one resolved set, sorted by key.
pub fn resolve(layers: &Layers<'_>, lookup: &dyn Fn(&str) -> Option<String>) -> Vec<ResolvedVar> {
    let mut map: BTreeMap<String, ResolvedVar> = BTreeMap::new();
    let mut put = |key: &str, value: String, scope: Scope, source: Source| {
        let shadows = match map.get(key) {
            Some(old) => {
                let mut s = old.shadows.clone();
                s.push(old.scope);
                s
            }
            None => Vec::new(),
        };
        map.insert(
            key.to_string(),
            ResolvedVar {
                key: key.to_string(),
                value,
                scope,
                source,
                shadows,
            },
        );
    };
    let ordered = [
        (Scope::Global, layers.global),
        (Scope::Environment, layers.environment),
        (Scope::Server, layers.server),
        (Scope::Route, layers.route),
    ];
    for (scope, vars) in ordered {
        let Some(vars) = vars else { continue };
        for (k, v) in vars {
            let (val, src) = expand_env(&value_text(v), &layers.dotenv, lookup);
            put(k, val, scope, src);
        }
    }
    if let Some(rt) = layers.runtime {
        for (k, v) in rt {
            put(k, v.clone(), Scope::Runtime, Source::File);
        }
    }
    map.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tv(pairs: &[(&str, &str)]) -> BTreeMap<String, toml::Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), toml::Value::String(v.to_string())))
            .collect()
    }
    fn none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn precedence_route_server_env_global() {
        let g = tv(&[("a", "g"), ("b", "g"), ("c", "g"), ("d", "g")]);
        let e = tv(&[("b", "e"), ("c", "e"), ("d", "e")]);
        let s = tv(&[("c", "s"), ("d", "s")]);
        let r = tv(&[("d", "r")]);
        let out = resolve(
            &Layers {
                global: Some(&g),
                environment: Some(&e),
                server: Some(&s),
                route: Some(&r),
                ..Default::default()
            },
            &none,
        );
        let get = |k: &str| out.iter().find(|v| v.key == k).unwrap().clone();
        assert_eq!(
            (get("a").value.as_str(), get("a").scope),
            ("g", Scope::Global)
        );
        assert_eq!(get("b").scope, Scope::Environment);
        assert_eq!(get("c").scope, Scope::Server);
        assert_eq!(get("d").value, "r");
        assert_eq!(
            get("d").shadows,
            vec![Scope::Global, Scope::Environment, Scope::Server]
        );
    }

    #[test]
    fn runtime_wins() {
        let s = tv(&[("x", "s")]);
        let rt: BTreeMap<_, _> = [("x".to_string(), "rt".to_string())].into();
        let out = resolve(
            &Layers {
                server: Some(&s),
                runtime: Some(&rt),
                ..Default::default()
            },
            &none,
        );
        assert_eq!(out[0].value, "rt");
        assert_eq!(out[0].scope, Scope::Runtime);
    }

    #[test]
    fn env_expansion_and_sources() {
        let e = tv(&[
            ("p", "${env:HOME_X}/z"),
            ("d", "${env:FROM_DOT}"),
            ("m", "${env:NOPE}"),
        ]);
        let dot = parse_dotenv("# c\nexport FROM_DOT=\"hello\"\nOTHER=1\n");
        let look = |n: &str| (n == "HOME_X").then(|| "/h".to_string());
        let out = resolve(
            &Layers {
                environment: Some(&e),
                dotenv: dot,
                ..Default::default()
            },
            &look,
        );
        let get = |k: &str| out.iter().find(|v| v.key == k).unwrap().clone();
        assert_eq!(
            (get("p").value.as_str(), get("p").source),
            ("/h/z", Source::Process)
        );
        assert_eq!(
            (get("d").value.as_str(), get("d").source),
            ("hello", Source::Dotenv)
        );
        assert_eq!(get("m").value, "");
    }

    #[test]
    fn secret_keys() {
        for k in ["auth_token", "API_KEY", "db_password", "client_Secret"] {
            assert!(is_secret_key(k), "{k}");
        }
        assert!(!is_secret_key("base_url"));
    }
}
