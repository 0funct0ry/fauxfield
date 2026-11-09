//! CORS (SPEC §5.7): automatic preflight answers and response headers. `apply` is the one
//! entry point response paths (mock, static, later proxy) go through.

use fauxfield_model::Cors;

const DEFAULT_METHODS: &str = "GET, HEAD, POST, PUT, PATCH, DELETE";

/// `*` matches any run of characters (`http://localhost:*`).
fn glob(pat: &str, s: &str) -> bool {
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 1 {
        return pat.eq_ignore_ascii_case(s);
    }
    let (s, first, last) = (
        s.to_ascii_lowercase(),
        parts[0].to_ascii_lowercase(),
        parts[parts.len() - 1].to_ascii_lowercase(),
    );
    if !s.starts_with(&first)
        || !s[first.len()..].ends_with(&last)
        || s.len() < first.len() + last.len()
    {
        return false;
    }
    let mut pos = first.len();
    let end = s.len() - last.len();
    for mid in &parts[1..parts.len() - 1] {
        let mid = mid.to_ascii_lowercase();
        match s[pos..end].find(&mid) {
            Some(i) => pos += i + mid.len(),
            None => return false,
        }
    }
    true
}

/// True for an `OPTIONS` request carrying `Origin` and `Access-Control-Request-Method`.
pub(crate) fn is_preflight(method: &str, lc_headers: &[(String, String)]) -> bool {
    method.eq_ignore_ascii_case("OPTIONS")
        && lc_headers.iter().any(|(k, _)| k == "origin")
        && lc_headers
            .iter()
            .any(|(k, _)| k == "access-control-request-method")
}

fn get<'a>(h: &'a [(String, String)], k: &str) -> Option<&'a str> {
    h.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

/// CORS headers for a response to a request with `lc_headers`. Empty when CORS is off or the
/// origin is not allowed. `preflight` adds the `Allow-Methods/Headers/Max-Age` set.
pub(crate) fn headers(
    cfg: &Cors,
    lc_headers: &[(String, String)],
    preflight: bool,
) -> Vec<(String, String)> {
    if !cfg.enabled {
        return vec![];
    }
    let any = cfg.origins.iter().any(|o| o.trim() == "*");
    let mut out = Vec::new();
    match get(lc_headers, "origin") {
        Some(origin) => {
            let listed = cfg
                .origins
                .iter()
                .any(|o| o.trim() != "*" && glob(o.trim(), origin));
            if !(any || listed) {
                return vec![];
            }
            let value = if any && !listed && !cfg.credentials {
                "*"
            } else {
                origin
            };
            out.push(("access-control-allow-origin".into(), value.to_string()));
            if value != "*" {
                out.push(("vary".into(), "Origin".into()));
            }
        }
        None => {
            if any && !cfg.credentials {
                out.push(("access-control-allow-origin".into(), "*".into()));
            }
            return out;
        }
    }
    if cfg.credentials {
        out.push(("access-control-allow-credentials".into(), "true".into()));
    }
    if preflight {
        let methods = if cfg.methods.is_empty() {
            DEFAULT_METHODS.to_string()
        } else {
            cfg.methods.join(", ")
        };
        out.push(("access-control-allow-methods".into(), methods));
        let wildcard = cfg.headers.is_empty() || cfg.headers.iter().any(|h| h.trim() == "*");
        let allow = if wildcard {
            get(lc_headers, "access-control-request-headers").map(str::to_string)
        } else {
            Some(cfg.headers.join(", "))
        };
        if let Some(a) = allow.filter(|a| !a.is_empty()) {
            out.push(("access-control-allow-headers".into(), a));
        }
        if let Some(m) = cfg.max_age {
            out.push(("access-control-max-age".into(), m.to_string()));
        }
    }
    out
}

/// Add CORS headers to `out`, replacing same-named ones (CORS wins; SPEC §5.5).
pub(crate) fn apply(
    cfg: &Cors,
    lc_headers: &[(String, String)],
    out: &mut Vec<(String, String)>,
    preflight: bool,
) {
    for (k, v) in headers(cfg, lc_headers, preflight) {
        if k == "vary" {
            if let Some(e) = out.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case("vary")) {
                if !e.1.to_ascii_lowercase().contains("origin") {
                    e.1 = format!("{}, Origin", e.1);
                }
                continue;
            }
        } else {
            out.retain(|(n, _)| !n.eq_ignore_ascii_case(&k));
        }
        out.push((k, v));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(p: &[(&str, &str)]) -> Vec<(String, String)> {
        p.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }
    fn cfg(origins: &[&str], credentials: bool) -> Cors {
        Cors {
            enabled: true,
            origins: origins.iter().map(|s| s.to_string()).collect(),
            credentials,
            ..Default::default()
        }
    }

    #[test]
    fn globs() {
        assert!(glob("http://localhost:*", "http://localhost:5173"));
        assert!(!glob("http://localhost:*", "http://evil.com"));
        assert!(glob("https://*.acme.dev", "https://app.acme.dev"));
        assert!(glob("http://a", "http://A"));
    }

    #[test]
    fn star_with_credentials_reflects() {
        let r = headers(
            &cfg(&["*"], true),
            &h(&[("origin", "http://x.test")]),
            false,
        );
        assert!(r.contains(&("access-control-allow-origin".into(), "http://x.test".into())));
        assert!(r.contains(&("access-control-allow-credentials".into(), "true".into())));
        let r = headers(
            &cfg(&["*"], false),
            &h(&[("origin", "http://x.test")]),
            false,
        );
        assert!(r.contains(&("access-control-allow-origin".into(), "*".into())));
    }

    #[test]
    fn disallowed_origin_gets_nothing() {
        assert!(
            headers(
                &cfg(&["http://a.test"], false),
                &h(&[("origin", "http://b.test")]),
                false
            )
            .is_empty()
        );
        assert!(headers(&Cors::default(), &h(&[("origin", "http://b.test")]), false).is_empty());
    }

    #[test]
    fn preflight_reflects_request_headers() {
        let r = headers(
            &cfg(&["*"], false),
            &h(&[
                ("origin", "http://a"),
                ("access-control-request-headers", "x-a, content-type"),
            ]),
            true,
        );
        assert!(r.contains(&(
            "access-control-allow-headers".into(),
            "x-a, content-type".into()
        )));
    }
}
