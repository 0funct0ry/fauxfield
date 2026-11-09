//! Route path patterns (SPEC §5.2, §20.17): parsing, classes, shadowing.
//! Shared by validation, the engine and the GUI so they never disagree.

use crate::{Route, Server};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    Exact,
    Param,
    Regex,
}

impl MatchMode {
    pub fn label(self) -> &'static str {
        match self {
            MatchMode::Exact => "exact",
            MatchMode::Param => "param",
            MatchMode::Regex => "regex",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seg {
    Lit(String),
    Param(String),
    /// Last segment only; captures one or more segments.
    Wild(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternError {
    pub offset: usize,
    pub message: String,
}

impl std::fmt::Display for PatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (at byte {})", self.message, self.offset)
    }
}

/// Parse the `match` key. `None`/empty means inferred.
pub fn parse_mode(m: Option<&str>) -> Result<Option<MatchMode>, String> {
    match m {
        None => Ok(None),
        Some("exact") => Ok(Some(MatchMode::Exact)),
        Some("param") => Ok(Some(MatchMode::Param)),
        Some("regex") => Ok(Some(MatchMode::Regex)),
        Some(o) => Err(format!(
            "unknown match mode `{o}` (expected exact, param or regex)"
        )),
    }
}

fn is_name(n: &str) -> bool {
    let mut c = n.chars();
    matches!(c.next(), Some(f) if f.is_ascii_alphabetic() || f == '_')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

/// Split a path into segments after the leading `/`. A trailing empty segment is
/// dropped unless `strict`. `/` is always zero segments.
pub fn split_path(path: &str, strict: bool) -> Vec<&str> {
    let p = path.strip_prefix('/').unwrap_or(path);
    if p.is_empty() {
        return Vec::new();
    }
    let mut v: Vec<&str> = p.split('/').collect();
    if !strict && v.last() == Some(&"") {
        v.pop();
    }
    v
}

fn seg_kind(s: &str, offset: usize) -> Result<Seg, PatternError> {
    let err = |o: usize, m: &str| PatternError {
        offset: o,
        message: m.to_string(),
    };
    if let Some(n) = s.strip_prefix(':') {
        if !is_name(n) {
            return Err(err(offset + 1, "invalid parameter name"));
        }
        return Ok(Seg::Param(n.into()));
    }
    if s.starts_with('{') || s.ends_with('}') {
        let inner = s.strip_prefix('{').and_then(|x| x.strip_suffix('}'));
        return match inner {
            Some(n) if is_name(n) => Ok(Seg::Param(n.into())),
            Some(_) => Err(err(offset + 1, "invalid parameter name")),
            None => Err(err(offset, "`{` and `}` must wrap a whole segment")),
        };
    }
    if let Some(n) = s.strip_prefix('*') {
        if n.is_empty() {
            return Ok(Seg::Wild("*".into()));
        }
        if !is_name(n) {
            return Err(err(offset + 1, "invalid wildcard name"));
        }
        return Ok(Seg::Wild(n.into()));
    }
    Ok(Seg::Lit(s.into()))
}

/// Parse a `param`-mode path. Errors are E033.
pub fn parse_param(path: &str, strict: bool) -> Result<Vec<Seg>, PatternError> {
    let parts = split_path(path, strict);
    let mut out = Vec::with_capacity(parts.len());
    let mut off = 1;
    for (i, p) in parts.iter().enumerate() {
        let seg = seg_kind(p, off)?;
        if let Seg::Wild(_) = seg
            && i + 1 != parts.len()
        {
            return Err(PatternError {
                offset: off,
                message: "`*` must be the last segment".into(),
            });
        }
        if let Seg::Param(n) | Seg::Wild(n) = &seg
            && out
                .iter()
                .any(|s| matches!(s, Seg::Param(m) | Seg::Wild(m) if m == n))
        {
            return Err(PatternError {
                offset: off,
                message: format!("duplicate parameter `{n}`"),
            });
        }
        out.push(seg);
        off += p.len() + 1;
    }
    Ok(out)
}

/// Literal segments for `exact` mode.
pub fn parse_exact(path: &str, strict: bool) -> Vec<Seg> {
    split_path(path, strict)
        .into_iter()
        .map(|s| Seg::Lit(s.into()))
        .collect()
}

/// Inferred mode: param when any segment is a param or wildcard, else exact.
pub fn infer_mode(path: &str) -> MatchMode {
    let has = split_path(path, false).iter().any(|s| {
        s.starts_with(':')
            || s.starts_with('*')
            || (s.starts_with('{') && s.ends_with('}') && s.len() > 2)
    });
    if has {
        MatchMode::Param
    } else {
        MatchMode::Exact
    }
}

/// True when `path` looks like a param pattern (for W052).
pub fn looks_like_param(path: &str) -> bool {
    infer_mode(path) == MatchMode::Param
}

pub const REGEX_SIZE_LIMIT: usize = 1 << 20;

/// Compile a regex route; checks the size limit and group-name charset (E035).
pub fn compile_regex(src: &str) -> Result<regex::Regex, String> {
    let re = regex::RegexBuilder::new(src)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
        .map_err(|e| e.to_string())?;
    for n in re.capture_names().flatten() {
        if !is_name(n) {
            return Err(format!("invalid group name `{n}`"));
        }
    }
    Ok(re)
}

/// A fully parsed route pattern.
#[derive(Debug, Clone)]
pub enum Pattern {
    Segs { mode: MatchMode, segs: Vec<Seg> },
    Regex(String),
}

impl Pattern {
    /// Effective mode: explicit `match`, else inferred.
    pub fn parse(path: &str, mode: Option<&str>, strict: bool) -> Result<Pattern, String> {
        let eff = parse_mode(mode)?.unwrap_or_else(|| infer_mode(path));
        match eff {
            MatchMode::Regex => {
                compile_regex(path)?;
                Ok(Pattern::Regex(path.into()))
            }
            MatchMode::Exact => Ok(Pattern::Segs {
                mode: eff,
                segs: parse_exact(path, strict),
            }),
            MatchMode::Param => parse_param(path, strict)
                .map(|segs| Pattern::Segs { mode: eff, segs })
                .map_err(|e| e.to_string()),
        }
    }

    /// 0 exact, 1 param, 2 regex, 3 wildcard.
    pub fn class(&self) -> u8 {
        match self {
            Pattern::Regex(_) => 2,
            Pattern::Segs { segs, .. } => {
                if segs.iter().any(|s| matches!(s, Seg::Wild(_))) {
                    3
                } else if segs.iter().any(|s| matches!(s, Seg::Param(_))) {
                    1
                } else {
                    0
                }
            }
        }
    }
}

/// Routes in effective order (SPEC §20.17): Folders in order, each with its Routes,
/// then Routes without a Folder. Disabled Folders and non-http Routes are skipped.
pub fn effective_routes(s: &Server) -> Vec<&Route> {
    effective_routes_of(s, &["http"])
}

/// [`effective_routes`] for any set of Route kinds (the engine also compiles `crud`).
pub fn effective_routes_of<'a>(s: &'a Server, kinds: &[&str]) -> Vec<&'a Route> {
    let mut v = Vec::new();
    for f in s.folders.iter().filter(|f| f.enabled) {
        v.extend(s.routes.iter().filter(|r| {
            kinds.contains(&r.kind.as_str()) && r.folder.as_deref() == Some(f.id.as_str())
        }));
    }
    v.extend(s.routes.iter().filter(|r| {
        kinds.contains(&r.kind.as_str())
            && !r
                .folder
                .as_deref()
                .is_some_and(|id| s.folders.iter().any(|f| f.id == id))
    }));
    v
}

fn covers(a: &Pattern, b: &Pattern) -> bool {
    match (a, b) {
        (Pattern::Regex(x), Pattern::Regex(y)) => x == y,
        (Pattern::Regex(x), Pattern::Segs { segs, .. }) => {
            if segs.iter().any(|s| !matches!(s, Seg::Lit(_))) {
                return false;
            }
            let path = format!(
                "/{}",
                segs.iter()
                    .map(|s| if let Seg::Lit(l) = s { l.as_str() } else { "" })
                    .collect::<Vec<_>>()
                    .join("/")
            );
            compile_regex(x).is_ok_and(|r| r.is_match(&path))
        }
        (Pattern::Segs { .. }, Pattern::Regex(_)) => false,
        (Pattern::Segs { segs: sa, .. }, Pattern::Segs { segs: sb, .. }) => {
            let wild = matches!(sa.last(), Some(Seg::Wild(_)));
            let k = if wild { sa.len() - 1 } else { sa.len() };
            if wild {
                if sb.len() < k + 1 {
                    return false;
                }
            } else if sb.len() != k || matches!(sb.last(), Some(Seg::Wild(_))) {
                return false;
            }
            sa[..k].iter().zip(sb).all(|(x, y)| match (x, y) {
                (Seg::Lit(p), Seg::Lit(q)) => p == q,
                (Seg::Param(_), Seg::Lit(_) | Seg::Param(_)) => true,
                _ => false,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shadow {
    /// Shadowed route ID.
    pub route: String,
    /// Winning route ID.
    pub by: String,
}

/// Conservative shadowing analysis (SPEC §5.2): reports B only when provably unreachable.
pub fn shadowed_routes(s: &Server) -> Vec<Shadow> {
    let strict = s.strict_slash.unwrap_or(false);
    let eff = effective_routes(s);
    let parsed: Vec<Option<Pattern>> = eff
        .iter()
        .map(|r| Pattern::parse(&r.path, r.match_mode.as_deref(), strict).ok())
        .collect();
    let mut out = Vec::new();
    for (bi, b) in eff.iter().enumerate() {
        let Some(pb) = &parsed[bi] else { continue };
        let bm = b.method.to_ascii_uppercase();
        for (ai, a) in eff.iter().enumerate() {
            if ai == bi {
                continue;
            }
            let Some(pa) = &parsed[ai] else { continue };
            let am = a.method.to_ascii_uppercase();
            if !(am == "ANY" || am == bm) {
                continue;
            }
            let (ap, bp) = (a.priority.unwrap_or(0), b.priority.unwrap_or(0));
            let wins = ap > bp || (ap == bp && (pa.class(), ai) < (pb.class(), bi));
            if wins && covers(pa, pb) {
                out.push(Shadow {
                    route: b.id.clone(),
                    by: a.id.clone(),
                });
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammar() {
        assert!(parse_param("/users/:id", false).is_ok());
        assert!(parse_param("/users/{id}/x", false).is_ok());
        assert!(parse_param("/files/*rest", false).is_ok());
        assert!(parse_param("/a:b", false).is_ok());
        for bad in ["/:", "/{{ x }}", "/*a/b", "/:a/:a", "/{a", "/:1"] {
            assert!(parse_param(bad, false).is_err(), "{bad}");
        }
    }

    #[test]
    fn inference_and_class() {
        assert_eq!(infer_mode("/a/b"), MatchMode::Exact);
        assert_eq!(infer_mode("/a/:b"), MatchMode::Param);
        assert_eq!(Pattern::parse("/f/*", None, false).unwrap().class(), 3);
        assert_eq!(
            Pattern::parse("^/x", Some("regex"), false).unwrap().class(),
            2
        );
        assert!(Pattern::parse("/x", Some("bogus"), false).is_err());
        assert!(Pattern::parse("(", Some("regex"), false).is_err());
    }

    #[test]
    fn shadowing() {
        let s = crate::parse_server(
            r#"
id="srv_a"
name="A"
port=1
[[routes]]
id="rte_1"
path="/users/:id"
[[routes]]
id="rte_2"
path="/users/42"
[[routes]]
id="rte_3"
path="/users/:id"
priority=5
"#,
        )
        .unwrap();
        let sh = shadowed_routes(&s);
        // rte_2 exact beats rte_1 only by class; rte_3 (priority) shadows rte_1.
        assert!(sh.contains(&Shadow {
            route: "rte_1".into(),
            by: "rte_3".into()
        }));
        assert!(!sh.iter().any(|x| x.route == "rte_3"));
    }

    #[test]
    fn fuzz_smoke() {
        let mut x: u64 = 0x9E3779B97F4A7C15;
        let alpha = b"/:{}*a_1-.%(^$\\";
        for _ in 0..10_000 {
            let mut s = String::new();
            for _ in 0..(x % 12) {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                s.push(alpha[(x % alpha.len() as u64) as usize] as char);
            }
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            for m in [None, Some("exact"), Some("param"), Some("regex")] {
                if let Ok(p) = Pattern::parse(&s, m, false) {
                    assert!(covers(&p, &p), "{s}");
                }
            }
        }
    }
}
