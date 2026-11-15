//! HAR 1.2 export of Recordings (`fauxfield logs --har`, M16).

use serde_json::{Value, json};

use crate::recording::Recording;
use crate::time::rfc3339;

fn headers(h: &[(String, String)]) -> Vec<Value> {
    h.iter()
        .map(|(n, v)| json!({ "name": n, "value": v }))
        .collect()
}

fn header<'a>(h: &'a [(String, String)], name: &str) -> Option<&'a str> {
    h.iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn query(url: &str) -> Vec<Value> {
    url.split_once('?')
        .map(|(_, q)| {
            q.split('&')
                .filter(|p| !p.is_empty())
                .map(|p| {
                    let (k, v) = p.split_once('=').unwrap_or((p, ""));
                    json!({ "name": k, "value": v })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

/// One HAR entry. A recorded URL is a path and query; the `Host` header makes it absolute.
fn entry(r: &Recording) -> Value {
    let host = header(&r.request_headers, "host").unwrap_or("localhost");
    let url = if r.url.starts_with("http://") || r.url.starts_with("https://") {
        r.url.clone()
    } else {
        format!("http://{host}{}", r.url)
    };
    let mime = header(&r.response_headers, "content-type").unwrap_or("");
    let total = r.timing.total_ms as f64;
    let mut request = json!({
        "method": r.method,
        "url": url,
        "httpVersion": "HTTP/1.1",
        "cookies": [],
        "headers": headers(&r.request_headers),
        "queryString": query(&r.url),
        "headersSize": -1,
        "bodySize": r.request_body.len(),
    });
    if !r.request_body.is_empty() {
        request["postData"] = json!({
            "mimeType": header(&r.request_headers, "content-type").unwrap_or(""),
            "text": r.request_body,
        });
    }
    json!({
        "startedDateTime": if r.ts.is_empty() { rfc3339(r.ts_ms) } else { r.ts.clone() },
        "time": total,
        "request": request,
        "response": {
            "status": r.status,
            "statusText": status_text(r.status),
            "httpVersion": "HTTP/1.1",
            "cookies": [],
            "headers": headers(&r.response_headers),
            "content": { "size": r.response_body.len(), "mimeType": mime, "text": r.response_body },
            "redirectURL": header(&r.response_headers, "location").unwrap_or(""),
            "headersSize": -1,
            "bodySize": r.response_body.len(),
        },
        "cache": {},
        "timings": { "send": 0, "wait": total, "receive": 0 },
        "_fauxfield": { "id": r.id, "server": r.server, "route": r.route },
    })
}

/// A complete HAR 1.2 document for `recordings`.
pub fn to_har(recordings: &[Recording]) -> Value {
    json!({
        "log": {
            "version": "1.2",
            "creator": { "name": "Fauxfield", "version": env!("CARGO_PKG_VERSION") },
            "entries": recordings.iter().map(entry).collect::<Vec<_>>(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_absolute_urls_query_and_bodies() {
        let r = Recording {
            id: 1,
            dataset_changes: vec![],
            parent_id: None,
            callback_id: None,
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
            url: "/orders?id=7&x=".into(),
            request_headers: vec![
                ("Host".into(), "127.0.0.1:4100".into()),
                ("Content-Type".into(), "application/json".into()),
            ],
            request_body: "{\"a\":1}".into(),
            status: 201,
            response_headers: vec![("content-type".into(), "text/plain".into())],
            response_body: "ok".into(),
            timing: crate::recording::Timing::default(),
            truncated: false,
            error: None,
            fault: None,
            source: "mock".into(),
            upstream: None,
        };
        let har = to_har(&[r]);
        let e = &har["log"]["entries"][0];
        assert_eq!(har["log"]["version"], "1.2");
        assert_eq!(e["request"]["url"], "http://127.0.0.1:4100/orders?id=7&x=");
        assert_eq!(e["request"]["queryString"][0]["name"], "id");
        assert_eq!(e["request"]["postData"]["text"], "{\"a\":1}");
        assert_eq!(e["response"]["statusText"], "Created");
        assert_eq!(e["response"]["content"]["mimeType"], "text/plain");
        assert_eq!(e["response"]["bodySize"], 2);
    }
}
