//! Response header names and their common values, for the Headers table completion popup.
//! Pure data and lookups (no GPUI). Names are in canonical casing and match case-insensitively.

/// (name, one-line description) of the headers offered while typing a Key.
pub const HEADERS: &[(&str, &str)] = &[
    ("Accept-Ranges", "Whether partial requests are supported"),
    (
        "Access-Control-Allow-Credentials",
        "CORS: allow cookies and auth",
    ),
    (
        "Access-Control-Allow-Headers",
        "CORS: request headers allowed",
    ),
    ("Access-Control-Allow-Methods", "CORS: methods allowed"),
    ("Access-Control-Allow-Origin", "CORS: origins allowed"),
    (
        "Access-Control-Expose-Headers",
        "CORS: headers readable by scripts",
    ),
    ("Access-Control-Max-Age", "CORS: preflight cache time"),
    ("Age", "Seconds the response sat in a cache"),
    ("Allow", "Methods the resource supports"),
    ("Cache-Control", "Caching directives"),
    ("Clear-Site-Data", "Clear browsing data for the site"),
    ("Connection", "Connection options"),
    ("Content-Disposition", "How the body is presented or saved"),
    ("Content-Encoding", "Encoding applied to the body"),
    ("Content-Language", "Language of the body"),
    ("Content-Length", "Size of the body in bytes"),
    ("Content-Location", "Alternate location of the body"),
    ("Content-MD5", "Base64 MD5 digest of the body"),
    ("Content-Range", "Where a partial body belongs"),
    ("Content-Security-Policy", "Allowed content sources"),
    ("Content-Type", "Media type of the body"),
    ("Cross-Origin-Opener-Policy", "Isolate the browsing context"),
    ("Cross-Origin-Resource-Policy", "Who may load this resource"),
    ("Date", "When the response was sent"),
    ("ETag", "Version identifier of the resource"),
    ("Expires", "When the response becomes stale"),
    ("Keep-Alive", "Parameters for a persistent connection"),
    ("Last-Modified", "When the resource last changed"),
    ("Link", "Related resources"),
    ("Location", "Redirect target or created resource"),
    ("Permissions-Policy", "Browser features the page may use"),
    ("Pragma", "Legacy caching directive"),
    (
        "Proxy-Authenticate",
        "Authentication scheme a proxy requires",
    ),
    ("Referrer-Policy", "How much referrer information to send"),
    ("Retry-After", "How long to wait before retrying"),
    ("Server", "Server software"),
    ("Server-Timing", "Timing metrics of the request"),
    ("Set-Cookie", "Cookie to store"),
    ("Strict-Transport-Security", "Force HTTPS"),
    ("Timing-Allow-Origin", "Origins that may read timing data"),
    ("Trailer", "Fields sent after the body"),
    ("Transfer-Encoding", "Encoding used to transfer the body"),
    ("Upgrade", "Protocols the server can switch to"),
    ("Vary", "Request headers that change the response"),
    ("Via", "Proxies the message passed through"),
    ("WWW-Authenticate", "Authentication scheme required"),
    ("X-Content-Type-Options", "Disable MIME sniffing"),
    ("X-Correlation-Id", "Id that ties related requests together"),
    ("X-Frame-Options", "Whether the page may be framed"),
    ("X-RateLimit-Limit", "Requests allowed in the window"),
    ("X-RateLimit-Remaining", "Requests left in the window"),
    ("X-RateLimit-Reset", "When the window resets"),
    ("X-Request-Id", "Id of this request"),
    ("X-XSS-Protection", "Legacy XSS filter switch"),
];

const MIME: &[&str] = &[
    "application/json",
    "application/problem+json",
    "application/xml",
    "application/x-www-form-urlencoded",
    "application/x-ndjson",
    "application/vnd.api+json",
    "application/json-patch+json",
    "application/merge-patch+json",
    "application/graphql",
    "application/javascript",
    "application/octet-stream",
    "application/pdf",
    "application/gzip",
    "application/zip",
    "multipart/form-data",
    "text/plain",
    "text/html",
    "text/csv",
    "text/css",
    "text/markdown",
    "text/event-stream",
    "text/xml",
    "image/gif",
    "image/jpeg",
    "image/png",
    "image/svg+xml",
    "image/webp",
];

const METHODS: &[&str] = &[
    "GET",
    "POST",
    "PUT",
    "PATCH",
    "DELETE",
    "OPTIONS",
    "GET, POST, PUT, PATCH, DELETE, OPTIONS",
];

/// Common values for the header `name` (case-insensitive); empty for an unknown header.
pub fn values_for(name: &str) -> &'static [&'static str] {
    match name.trim().to_ascii_lowercase().as_str() {
        "content-type" => MIME,
        "content-encoding" => &["gzip", "br", "deflate", "zstd", "identity"],
        "content-language" => &["en-US", "en-GB", "en", "fr-FR", "de-DE", "es-ES"],
        "content-disposition" => &["inline", "attachment", "attachment; filename=\"file.txt\""],
        "cache-control" => &[
            "no-store",
            "no-cache",
            "no-transform",
            "public",
            "private",
            "max-age=0",
            "max-age=3600",
            "max-age=31536000, immutable",
            "must-revalidate",
            "s-maxage=3600",
        ],
        "pragma" => &["no-cache"],
        "connection" => &["keep-alive", "close"],
        "accept-ranges" => &["bytes", "none"],
        "access-control-allow-origin" => &["*", "http://localhost:3000", "null"],
        "access-control-allow-methods" => METHODS,
        "access-control-allow-headers" => &[
            "*",
            "Content-Type",
            "Authorization",
            "Content-Type, Authorization",
            "X-Requested-With",
        ],
        "access-control-expose-headers" => &["*", "Content-Length", "X-Request-Id"],
        "access-control-allow-credentials" => &["true"],
        "access-control-max-age" => &["600", "3600", "86400"],
        "allow" => METHODS,
        "vary" => &[
            "Accept",
            "Accept-Encoding",
            "Origin",
            "Cookie",
            "Authorization",
        ],
        "retry-after" => &["30", "120", "3600"],
        "transfer-encoding" => &["chunked", "gzip", "deflate", "identity"],
        "referrer-policy" => &[
            "no-referrer",
            "same-origin",
            "origin",
            "strict-origin",
            "strict-origin-when-cross-origin",
            "unsafe-url",
        ],
        "strict-transport-security" => &[
            "max-age=31536000",
            "max-age=31536000; includeSubDomains",
            "max-age=63072000; includeSubDomains; preload",
        ],
        "content-security-policy" => &[
            "default-src 'self'",
            "default-src 'none'",
            "default-src 'self'; img-src 'self' data:",
        ],
        "x-content-type-options" => &["nosniff"],
        "x-frame-options" => &["DENY", "SAMEORIGIN"],
        "x-xss-protection" => &["0", "1; mode=block"],
        "cross-origin-opener-policy" => &["same-origin", "same-origin-allow-popups", "unsafe-none"],
        "cross-origin-resource-policy" => &["same-origin", "same-site", "cross-origin"],
        "www-authenticate" | "proxy-authenticate" => {
            &["Basic realm=\"api\"", "Bearer", "Bearer realm=\"api\""]
        }
        "set-cookie" => &[
            "id=abc123; Path=/; HttpOnly",
            "session=abc123; Path=/; Secure; HttpOnly; SameSite=Lax",
            "id=; Path=/; Max-Age=0",
        ],
        "upgrade" => &["websocket", "h2c"],
        "x-ratelimit-limit" => &["100", "1000"],
        "x-ratelimit-remaining" => &["99", "0"],
        "clear-site-data" => &["\"cache\"", "\"cookies\"", "\"storage\"", "\"*\""],
        _ => &[],
    }
}

/// Canonical casing of a known header name, if `name` is one.
pub fn canonical(name: &str) -> Option<&'static str> {
    HEADERS
        .iter()
        .map(|(n, _)| *n)
        .find(|n| n.eq_ignore_ascii_case(name.trim()))
}

/// Which cell of a row is being completed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Col {
    Key,
    Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub label: &'static str,
    /// One-line description (names only).
    pub detail: &'static str,
    /// Byte range of the match inside `label`, for highlighting.
    pub hit: (usize, usize),
}

/// Most suggestions the popup lists; the rest are reachable by typing more.
pub const MAX_SUGGESTIONS: usize = 50;

/// Suggestions for a cell: standard names while typing a Key (nothing for empty text), common
/// values of header `name` while typing a Value (all of them for empty text). Matching is a
/// case-insensitive substring, prefix matches first. Nothing is offered when the text already
/// equals the only match.
pub fn suggest(col: Col, name: &str, text: &str) -> Vec<Suggestion> {
    let q = text.trim().to_ascii_lowercase();
    let items: Vec<(&'static str, &'static str)> = match col {
        Col::Key => {
            if q.is_empty() {
                return Vec::new();
            }
            HEADERS.to_vec()
        }
        Col::Value => values_for(name).iter().map(|v| (*v, "")).collect(),
    };
    let mut out: Vec<Suggestion> = items
        .into_iter()
        .filter_map(|(label, detail)| {
            let at = label.to_ascii_lowercase().find(&q)?;
            Some(Suggestion {
                label,
                detail,
                hit: (at, at + q.len()),
            })
        })
        .collect();
    out.sort_by_key(|s| (s.hit.0 != 0, s.label));
    out.truncate(MAX_SUGGESTIONS);
    if out.len() == 1 && out[0].label.eq_ignore_ascii_case(text.trim()) {
        out.clear();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_with_descriptions() {
        let mut seen = std::collections::HashSet::new();
        for (n, d) in HEADERS {
            assert!(seen.insert(n.to_ascii_lowercase()), "duplicate {n}");
            assert!(!d.is_empty());
            assert!(!n.contains(char::is_whitespace));
        }
    }

    #[test]
    fn key_suggestions_rank_prefixes_first() {
        let s = suggest(Col::Key, "", "con");
        assert_eq!(s[0].label, "Connection");
        assert!(s.iter().any(|x| x.label == "Content-Type"));
        assert!(suggest(Col::Key, "", "").is_empty());
        // Substring match, case-insensitive; highlight range is on the label.
        let s = suggest(Col::Key, "", "ORIGIN");
        assert!(s.iter().any(|x| x.label == "Access-Control-Allow-Origin"));
        let x = s.iter().find(|x| x.label == "Timing-Allow-Origin").unwrap();
        assert_eq!(&x.label[x.hit.0..x.hit.1], "Origin");
        // The only match equal to the text offers nothing.
        assert!(suggest(Col::Key, "", "Content-MD5").is_empty());
    }

    #[test]
    fn value_suggestions_follow_the_header() {
        let all = suggest(Col::Value, "content-type", "");
        assert!(all.iter().any(|s| s.label == "application/json"));
        let some = suggest(Col::Value, "Content-Type", "json");
        assert!(some.iter().all(|s| s.label.contains("json")));
        assert!(suggest(Col::Value, "X-Custom", "").is_empty());
        assert_eq!(canonical("content-type"), Some("Content-Type"));
        assert_eq!(canonical("nope"), None);
        for (n, _) in HEADERS {
            let _ = values_for(n);
        }
    }
}
