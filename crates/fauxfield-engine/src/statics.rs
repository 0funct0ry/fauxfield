//! `static` routes (SPEC §5.9): directory serving with MIME detection, `ETag`/`Last-Modified`,
//! single-range requests, `index`, traversal protection and Tera-rendered template files.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use fauxfield_model::Route;
use serde::Serialize;

use crate::render::TplEnv;

pub(crate) struct StaticOut {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
    /// Workspace-relative file served (for the Traffic detail).
    pub file: Option<String>,
    pub error: Option<String>,
}

/// One file under a static route's directory (file browser preview, MCP `static.list_files`).
#[derive(Debug, Clone, Serialize)]
pub struct StaticFile {
    /// Path relative to the route's `dir`, `/`-separated.
    pub path: String,
    pub size: u64,
    /// MIME type the engine serves it with (inner extension for templates).
    pub mime: String,
    pub templated: bool,
}

fn problem(status: u16, title: &str, detail: &str, path: &str) -> StaticOut {
    let body = serde_json::json!({
        "type": "about:blank", "title": title, "status": status,
        "detail": detail, "instance": path,
    });
    StaticOut {
        status,
        headers: vec![("content-type".into(), "application/problem+json".into())],
        body: Bytes::from(body.to_string()),
        file: None,
        error: None,
    }
}

/// The template extension a file name ends with, if any (case-insensitive).
fn template_ext<'a>(name: &str, exts: &'a [String]) -> Option<&'a str> {
    let lower = name.to_ascii_lowercase();
    exts.iter()
        .map(String::as_str)
        .find(|e| lower.len() > e.len() && lower.ends_with(&e.to_ascii_lowercase()))
}

/// MIME type of `name`, looking through a template extension (`page.html.tera` → `text/html`).
pub fn mime_of(name: &str, exts: &[String]) -> String {
    let inner = match template_ext(name, exts) {
        Some(e) => &name[..name.len() - e.len()],
        None => name,
    };
    mime_guess::from_path(inner)
        .first_raw()
        .unwrap_or("application/octet-stream")
        .to_string()
}

/// Resolve `rel` inside `dir`; `None` when it escapes (`..`, absolute, NUL, symlink).
fn confine(dir: &Path, rel: &str) -> Option<PathBuf> {
    if rel.contains('\0') || rel.contains('\\') || rel.starts_with('/') {
        return None;
    }
    if rel.split('/').any(|c| c == "..") {
        return None;
    }
    let base = dir.canonicalize().ok()?;
    let full = base.join(rel).canonicalize().ok()?;
    full.starts_with(&base).then_some(full)
}

fn mtime(meta: &std::fs::Metadata) -> Option<SystemTime> {
    meta.modified().ok()
}

fn etag(len: u64, m: Option<SystemTime>) -> String {
    let ns = m
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    format!("W/\"{len:x}-{ns:x}\"")
}

fn hdr<'a>(h: &'a [(String, String)], k: &str) -> Option<&'a str> {
    h.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

/// Parse a single `bytes=` range against `len`. `Err(())` = unsatisfiable, `Ok(None)` = ignore.
fn parse_range(v: &str, len: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(spec) = v.trim().strip_prefix("bytes=") else {
        return Ok(None);
    };
    if spec.contains(',') {
        return Ok(None);
    }
    let Some((a, b)) = spec.split_once('-') else {
        return Ok(None);
    };
    let (a, b) = (a.trim(), b.trim());
    let (start, end) = if a.is_empty() {
        let n: u64 = b.parse().map_err(|_| ())?;
        if n == 0 {
            return Err(());
        }
        (len.saturating_sub(n), len.saturating_sub(1))
    } else {
        let s: u64 = a.parse().map_err(|_| ())?;
        let e: u64 = if b.is_empty() {
            len.saturating_sub(1)
        } else {
            b.parse::<u64>().map_err(|_| ())?.min(len.saturating_sub(1))
        };
        (s, e)
    };
    if len == 0 || start >= len || start > end {
        return Err(());
    }
    Ok(Some((start, end)))
}

pub(crate) fn serve(
    root: &Path,
    route: &Route,
    rel: &str,
    method: &str,
    req_headers: &[(String, String)],
    tpl: &TplEnv,
    req_path: &str,
) -> StaticOut {
    let head = method.eq_ignore_ascii_case("HEAD");
    if !head && !method.eq_ignore_ascii_case("GET") {
        let mut o = problem(
            405,
            "Method Not Allowed",
            "Static routes serve GET and HEAD.",
            req_path,
        );
        o.headers.push(("allow".into(), "GET, HEAD".into()));
        return o;
    }
    let Some(dir_rel) = route.dir.as_deref().filter(|d| !d.is_empty()) else {
        return problem(
            500,
            "Static route has no dir",
            "Set `dir` on the Route.",
            req_path,
        );
    };
    let dir = root.join(dir_rel);
    let exts = &route.template_extensions;
    let rel = rel.trim_matches('/');
    let Some(mut full) = (if rel.is_empty() {
        dir.canonicalize().ok()
    } else {
        confine(&dir, rel).or_else(|| {
            // `/page.html` may be served from `page.html.tera`.
            exts.iter()
                .find_map(|e| confine(&dir, &format!("{rel}{e}")))
        })
    }) else {
        let escapes = rel.contains('\0') || rel.contains('\\') || rel.split('/').any(|c| c == "..");
        return if escapes {
            problem(
                403,
                "Forbidden",
                "Path escapes the static directory.",
                req_path,
            )
        } else {
            problem(
                404,
                "File not found",
                &format!("No file at {req_path}."),
                req_path,
            )
        };
    };
    if full.is_dir() {
        let idx = route.index.as_deref().filter(|i| !i.is_empty());
        let found = idx.and_then(|i| {
            confine(&full, i)
                .or_else(|| exts.iter().find_map(|e| confine(&full, &format!("{i}{e}"))))
        });
        match found {
            Some(f) if f.is_file() => full = f,
            _ => {
                return problem(
                    404,
                    "File not found",
                    &format!("No index for {req_path}."),
                    req_path,
                );
            }
        }
    }
    let Ok(meta) = std::fs::metadata(&full) else {
        return problem(
            404,
            "File not found",
            &format!("No file at {req_path}."),
            req_path,
        );
    };
    let name = full
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let shown = full
        .strip_prefix(dir.canonicalize().unwrap_or(dir.clone()))
        .map(|p| format!("{dir_rel}/{}", p.to_string_lossy().replace('\\', "/")))
        .unwrap_or_default();
    let Ok(data) = std::fs::read(&full) else {
        return problem(
            404,
            "File not found",
            &format!("No file at {req_path}."),
            req_path,
        );
    };
    let templated = template_ext(&name, exts).is_some();
    let mime = mime_of(&name, exts);
    let mut headers: Vec<(String, String)> = vec![("content-type".into(), mime)];
    let (body, tag, modified) = if templated {
        let src = String::from_utf8_lossy(&data).into_owned();
        match tpl.render(root, &src) {
            Ok(out) => {
                let mut h = DefaultHasher::new();
                out.hash(&mut h);
                (Bytes::from(out), format!("W/\"t{:x}\"", h.finish()), None)
            }
            Err(e) => {
                let msg = format!("{shown}: {e}");
                let mut o = problem(500, "Template error", &msg, req_path);
                o.error = Some(msg);
                o.file = Some(shown);
                return o;
            }
        }
    } else {
        let m = mtime(&meta);
        (Bytes::from(data), etag(meta.len(), m), m)
    };
    headers.push(("etag".into(), tag.clone()));
    if let Some(m) = modified {
        headers.push(("last-modified".into(), httpdate::fmt_http_date(m)));
    }
    if !templated {
        headers.push(("accept-ranges".into(), "bytes".into()));
    }
    let done = |status: u16, mut headers: Vec<(String, String)>, body: Bytes| {
        if head {
            headers.push(("content-length".into(), body.len().to_string()));
        }
        StaticOut {
            status,
            headers,
            body: if head { Bytes::new() } else { body },
            file: Some(shown.clone()),
            error: None,
        }
    };

    // Conditional requests.
    let inm = hdr(req_headers, "if-none-match");
    let fresh = match inm {
        Some(v) => {
            v.trim() == "*"
                || v.split(',')
                    .any(|t| t.trim().trim_start_matches("W/") == tag.trim_start_matches("W/"))
        }
        None => match (hdr(req_headers, "if-modified-since"), modified) {
            (Some(s), Some(m)) => httpdate::parse_http_date(s).is_ok_and(|since| {
                let secs = |t: SystemTime| t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
                secs(m) <= secs(since)
            }),
            _ => false,
        },
    };
    if fresh {
        let keep: Vec<_> = headers
            .into_iter()
            .filter(|(k, _)| k != "content-type")
            .collect();
        return StaticOut {
            status: 304,
            headers: keep,
            body: Bytes::new(),
            file: Some(shown),
            error: None,
        };
    }

    // Single range.
    let len = body.len() as u64;
    if !templated && let Some(r) = hdr(req_headers, "range") {
        let if_range_ok = hdr(req_headers, "if-range").is_none_or(|v| v.trim() == tag);
        if if_range_ok {
            match parse_range(r, len) {
                Err(()) => {
                    return StaticOut {
                        status: 416,
                        headers: vec![("content-range".into(), format!("bytes */{len}"))],
                        body: Bytes::new(),
                        file: Some(shown),
                        error: None,
                    };
                }
                Ok(Some((s, e))) => {
                    headers.push(("content-range".into(), format!("bytes {s}-{e}/{len}")));
                    return done(206, headers, body.slice(s as usize..=e as usize));
                }
                Ok(None) => {}
            }
        }
    }
    done(200, headers, body)
}

/// Every file under a static route's `dir` (sorted), for the file browser and MCP.
pub fn list_files(root: &Path, route: &Route) -> Vec<StaticFile> {
    let Some(d) = route.dir.as_deref().filter(|d| !d.is_empty()) else {
        return vec![];
    };
    let base = root.join(d);
    let Ok(canon) = base.canonicalize() else {
        return vec![];
    };
    if !canon.starts_with(root.canonicalize().unwrap_or_else(|_| root.to_path_buf())) {
        return vec![];
    }
    let mut out = Vec::new();
    let mut stack = vec![canon.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(real) = p.canonicalize() else { continue };
            if !real.starts_with(&canon) {
                continue;
            }
            if real.is_dir() {
                stack.push(real);
            } else if let Ok(m) = e.metadata().or_else(|_| std::fs::metadata(&real)) {
                let rel = p
                    .strip_prefix(&canon)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .replace('\\', "/");
                let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
                out.push(StaticFile {
                    mime: mime_of(&name, &route.template_extensions),
                    templated: template_ext(&name, &route.template_extensions).is_some(),
                    size: m.len(),
                    path: rel,
                });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_through_template_ext() {
        let e = vec![".tera".to_string()];
        assert_eq!(mime_of("page.html.tera", &e), "text/html");
        assert_eq!(mime_of("a.js", &e), "text/javascript");
        assert_eq!(mime_of("noext", &e), "application/octet-stream");
    }

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-9", 100), Ok(Some((0, 9))));
        assert_eq!(parse_range("bytes=90-", 100), Ok(Some((90, 99))));
        assert_eq!(parse_range("bytes=-10", 100), Ok(Some((90, 99))));
        assert_eq!(parse_range("bytes=0-999", 100), Ok(Some((0, 99))));
        assert_eq!(parse_range("bytes=100-", 100), Err(()));
        assert_eq!(parse_range("bytes=0-1,5-6", 100), Ok(None));
    }

    #[test]
    fn confinement() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "x").unwrap();
        assert!(confine(d.path(), "a.txt").is_some());
        assert!(confine(d.path(), "../a.txt").is_none());
        assert!(confine(d.path(), "/etc/passwd").is_none());
        assert!(confine(d.path(), "a\0.txt").is_none());
    }
}
