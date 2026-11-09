use serde_json::{Value, json};

/// RFC 9457 `application/problem+json` for an unmatched request (SPEC §5.10).
pub fn not_found(
    method: &str,
    path: &str,
    closest: &[(String, String, usize)],
    hint: bool,
) -> Value {
    let mut v = json!({
        "type": "about:blank",
        "title": "No route matched",
        "status": 404,
        "detail": format!("No route matches {method} {path}."),
        "instance": path,
    });
    if hint && !closest.is_empty() {
        v["closest"] = closest
            .iter()
            .map(|(m, p, d)| json!({"method": m, "path": p, "distance": d}))
            .collect();
    }
    v
}

pub fn server_error(detail: &str, path: &str) -> Value {
    json!({
        "type": "about:blank",
        "title": "Response could not be produced",
        "status": 500,
        "detail": detail,
        "instance": path,
    })
}

/// RFC 9457 body for a rate-limited request.
pub fn too_many_requests(path: &str, retry_after: u64) -> Value {
    json!({
        "type": "about:blank",
        "title": "Too Many Requests",
        "status": 429,
        "detail": format!("Rate limit exceeded. Retry after {retry_after} s."),
        "instance": path,
    })
}

/// Body of a chaos-injected 500.
pub fn chaos_error(path: &str) -> Value {
    json!({
        "type": "about:blank",
        "title": "Chaos fault",
        "status": 500,
        "detail": "Injected by the chaos switch.",
        "instance": path,
    })
}
