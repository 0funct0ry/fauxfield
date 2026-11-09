use serde::{Deserialize, Serialize};

/// Bodies are truncated at 1 MiB (SPEC §7.2).
pub const BODY_LIMIT: usize = 1024 * 1024;
/// Headers masked before storage (SPEC §7.2 default).
pub const REDACT: &[&str] = &["authorization", "cookie", "set-cookie", "x-api-key"];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Timing {
    /// Route matching, microseconds.
    pub match_us: u64,
    /// Configured response delay, milliseconds.
    pub delay_ms: u64,
    /// Whole request, milliseconds.
    pub total_ms: u64,
}

/// One handled request (SPEC §7.2, mocked traffic only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recording {
    pub id: u64,
    pub ts: String,
    pub ts_ms: u64,
    pub server_id: String,
    pub server: String,
    /// Route ID, or `unmatched`.
    pub route: String,
    /// The Response that answered (empty when unmatched).
    #[serde(default)]
    pub response_id: String,
    #[serde(default)]
    pub response_name: String,
    /// Why that Response was chosen (SPEC §5.3).
    #[serde(default)]
    pub trace: Option<crate::rules::Trace>,
    /// Captured path params in pattern order (secret-named values masked).
    #[serde(default)]
    pub params: Vec<(String, String)>,
    /// `GET /users/:id`; empty when unmatched.
    #[serde(default)]
    pub route_pattern: String,
    pub method: String,
    pub url: String,
    pub request_headers: Vec<(String, String)>,
    pub request_body: String,
    pub status: u16,
    pub response_headers: Vec<(String, String)>,
    pub response_body: String,
    pub timing: Timing,
    pub truncated: bool,
    /// `mock` for real clients, `test` for Send test.
    pub source: String,
}

pub(crate) fn redact(headers: Vec<(String, String)>) -> Vec<(String, String)> {
    headers
        .into_iter()
        .map(|(k, v)| {
            if REDACT.contains(&k.to_ascii_lowercase().as_str()) {
                (k, "***".to_string())
            } else {
                (k, v)
            }
        })
        .collect()
}

/// Body bytes as text (lossy), cut at [`BODY_LIMIT`]. Returns whether it was cut.
pub(crate) fn body_text(bytes: &[u8]) -> (String, bool) {
    if bytes.len() > BODY_LIMIT {
        (
            String::from_utf8_lossy(&bytes[..BODY_LIMIT]).into_owned(),
            true,
        )
    } else {
        (String::from_utf8_lossy(bytes).into_owned(), false)
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// `curl` command reproducing the recorded request against `base` (e.g. `http://127.0.0.1:4010`).
pub fn to_curl(rec: &Recording, base: &str) -> String {
    let mut out = format!(
        "curl -X {} {}",
        rec.method,
        shell_quote(&format!("{base}{}", rec.url))
    );
    for (k, v) in &rec.request_headers {
        let lk = k.to_ascii_lowercase();
        if matches!(lk.as_str(), "host" | "content-length" | "connection") {
            continue;
        }
        out.push_str(&format!(" -H {}", shell_quote(&format!("{k}: {v}"))));
    }
    if !rec.request_body.is_empty() {
        out.push_str(&format!(" --data-raw {}", shell_quote(&rec.request_body)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secrets_and_quotes_curl() {
        let h = redact(vec![
            ("Authorization".into(), "Bearer x".into()),
            ("accept".into(), "*/*".into()),
        ]);
        assert_eq!(h[0].1, "***");
        assert_eq!(h[1].1, "*/*");
        let rec = Recording {
            id: 1,
            ts: String::new(),
            ts_ms: 0,
            server_id: "s".into(),
            server: "s".into(),
            route: "r".into(),
            response_id: String::new(),
            response_name: String::new(),
            trace: None,
            params: vec![],
            route_pattern: String::new(),
            method: "POST".into(),
            url: "/a?b=1".into(),
            request_headers: vec![("host".into(), "x".into()), ("x-a".into(), "it's".into())],
            request_body: "{\"a\":1}".into(),
            status: 200,
            response_headers: vec![],
            response_body: String::new(),
            timing: Timing::default(),
            truncated: false,
            source: "mock".into(),
        };
        let c = to_curl(&rec, "http://127.0.0.1:1");
        assert!(c.starts_with("curl -X POST 'http://127.0.0.1:1/a?b=1'"));
        assert!(c.contains("-H 'x-a: it'\\''s'"));
        assert!(c.contains("--data-raw '{\"a\":1}'"));
        assert!(!c.contains("host"));
    }

    #[test]
    fn truncates_large_bodies() {
        let (t, cut) = body_text(&vec![b'a'; BODY_LIMIT + 5]);
        assert!(cut);
        assert_eq!(t.len(), BODY_LIMIT);
    }
}
