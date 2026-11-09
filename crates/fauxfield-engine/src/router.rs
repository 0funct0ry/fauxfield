use std::collections::HashMap;

use fauxfield_model::pattern::{self, Pattern, Seg, effective_routes, split_path};
use fauxfield_model::{Response, Route, Server};
use regex::{Regex, RegexSet};

/// Compiled, immutable route table for one server; swapped atomically on hot reload.
/// Exact routes use a hash map, param routes a segment trie, regex routes a `RegexSet`
/// (SPEC §5.2, §20.17).
#[derive(Debug, Clone, Default)]
pub struct RouteTable {
    entries: Vec<Entry>,
    exact: HashMap<String, Vec<usize>>,
    trie: Node,
    regexes: Vec<(usize, Regex)>,
    regex_set: Option<RegexSet>,
    prefix: String,
    strict: bool,
    pub unmatched_hint: bool,
}

#[derive(Debug, Clone)]
struct Entry {
    method: String,
    /// Path as written (without the server prefix).
    path: String,
    /// Param/wildcard names in pattern order.
    names: Vec<String>,
    class: u8,
    priority: i32,
    route: Route,
}

#[derive(Debug, Clone, Default)]
struct Node {
    lits: HashMap<String, Node>,
    param: Option<Box<Node>>,
    wild: Vec<usize>,
    leaves: Vec<usize>,
}

/// One matching route with its captured params.
#[derive(Debug, Clone)]
pub struct Match<'a> {
    pub route: &'a Route,
    pub params: Vec<(String, String)>,
    pub class: u8,
    pub priority: i32,
    /// `METHOD path` as written, e.g. `GET /users/:id`.
    pub pattern: String,
}

/// Drops the query, collapses a trailing slash (SPEC §5.2), keeps a leading one.
pub fn normalize(path: &str) -> String {
    let path = path.split('?').next().unwrap_or("");
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

/// Percent-decode; invalid sequences are left as-is, UTF-8 lossy.
pub fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|d| d as u8)
}

/// Params named like secrets are masked in recordings (SPEC §20.17).
pub fn mask_param(name: &str, value: &str) -> String {
    let n = name.to_ascii_lowercase();
    if ["authorization", "token", "password", "secret"].contains(&n.as_str()) {
        "***".into()
    } else {
        value.into()
    }
}

impl RouteTable {
    pub fn compile(server: &Server) -> RouteTable {
        let prefix = server.prefix.clone().unwrap_or_default();
        let strict = server.strict_slash.unwrap_or(false);
        let mut t = RouteTable {
            prefix: prefix.trim_end_matches('/').to_string(),
            strict,
            unmatched_hint: server.unmatched_hint.unwrap_or(true),
            ..Default::default()
        };
        let mut regex_src = Vec::new();
        for r in effective_routes(server) {
            let Ok(pat) = Pattern::parse(&r.path, r.match_mode.as_deref(), strict) else {
                continue;
            };
            let idx = t.entries.len();
            let mut names = Vec::new();
            match &pat {
                Pattern::Regex(src) => {
                    let Ok(re) = pattern::compile_regex(src) else {
                        continue;
                    };
                    names = re.capture_names().flatten().map(String::from).collect();
                    regex_src.push(src.clone());
                    t.regexes.push((idx, re));
                }
                Pattern::Segs { segs, .. } => {
                    names.extend(segs.iter().filter_map(|s| match s {
                        Seg::Param(n) | Seg::Wild(n) => Some(n.clone()),
                        Seg::Lit(_) => None,
                    }));
                    if pat.class() == 0 {
                        let key = segs
                            .iter()
                            .map(|s| if let Seg::Lit(l) = s { l.as_str() } else { "" })
                            .collect::<Vec<_>>()
                            .join("/");
                        t.exact.entry(key).or_default().push(idx);
                    } else {
                        let mut n = &mut t.trie;
                        let mut wild = false;
                        for s in segs {
                            match s {
                                Seg::Lit(l) => n = n.lits.entry(l.clone()).or_default(),
                                Seg::Param(_) => n = n.param.get_or_insert_with(Default::default),
                                Seg::Wild(_) => wild = true,
                            }
                        }
                        if wild {
                            n.wild.push(idx);
                        } else {
                            n.leaves.push(idx);
                        }
                    }
                }
            }
            t.entries.push(Entry {
                method: r.method.to_ascii_uppercase(),
                path: r.path.clone(),
                names,
                class: pat.class(),
                priority: r.priority.unwrap_or(0),
                route: r.clone(),
            });
        }
        t.regex_set = RegexSet::new(&regex_src).ok();
        t
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Best matching route, if any.
    pub fn find(&self, method: &str, path: &str) -> Option<Match<'_>> {
        self.candidates(method, path).into_iter().next()
    }

    /// All matching routes, best first (priority desc, class asc, effective order asc).
    pub fn candidates(&self, method: &str, path: &str) -> Vec<Match<'_>> {
        let path = path.split('?').next().unwrap_or("");
        let rest = if self.prefix.is_empty() {
            path
        } else {
            match path.strip_prefix(self.prefix.as_str()) {
                Some("") => "/",
                Some(r) if r.starts_with('/') => r,
                _ => return Vec::new(),
            }
        };
        let segs: Vec<String> = split_path(rest, self.strict)
            .into_iter()
            .map(percent_decode)
            .collect();
        let mut hits: Vec<(usize, Vec<String>)> = Vec::new();
        if let Some(v) = self.exact.get(&segs.join("/")) {
            hits.extend(v.iter().map(|&i| (i, Vec::new())));
        }
        let mut caps = Vec::new();
        Self::walk(&self.trie, &segs, &mut caps, &mut hits);
        if let Some(set) = &self.regex_set {
            for k in set.matches(rest).iter() {
                let (i, re) = &self.regexes[k];
                if re.is_match(rest) {
                    hits.push((*i, Vec::new()));
                }
            }
        }
        let mut out: Vec<(usize, Match<'_>)> = hits
            .into_iter()
            .filter_map(|(i, vals)| {
                let e = &self.entries[i];
                if !(e.method == "ANY" || e.method.eq_ignore_ascii_case(method)) {
                    return None;
                }
                let params = if e.class == 2 {
                    // Named groups that did not participate are omitted.
                    let re = &self.regexes.iter().find(|(x, _)| *x == i)?.1;
                    let c = re.captures(rest)?;
                    re.capture_names()
                        .enumerate()
                        .filter_map(|(n, name)| {
                            Some((name?.to_string(), c.get(n)?.as_str().to_string()))
                        })
                        .collect()
                } else {
                    e.names.iter().cloned().zip(vals).collect()
                };
                Some((
                    i,
                    Match {
                        route: &e.route,
                        params,
                        class: e.class,
                        priority: e.priority,
                        pattern: format!("{} {}", e.method, e.path),
                    },
                ))
            })
            .collect();
        out.sort_by_key(|(i, m)| (std::cmp::Reverse(m.priority), m.class, *i));
        out.dedup_by_key(|(i, _)| *i);
        out.into_iter().map(|(_, m)| m).collect()
    }

    fn walk(
        n: &Node,
        segs: &[String],
        caps: &mut Vec<String>,
        hits: &mut Vec<(usize, Vec<String>)>,
    ) {
        let Some((first, rest)) = segs.split_first() else {
            hits.extend(n.leaves.iter().map(|&i| (i, caps.clone())));
            return;
        };
        if !n.wild.is_empty() {
            let mut c = caps.clone();
            c.push(segs.join("/"));
            hits.extend(n.wild.iter().map(|&i| (i, c.clone())));
        }
        if let Some(child) = n.lits.get(first) {
            Self::walk(child, rest, caps, hits);
        }
        if let Some(child) = &n.param
            && !first.is_empty()
        {
            caps.push(first.clone());
            Self::walk(child, rest, caps, hits);
            caps.pop();
        }
    }

    /// Up to three exact/param routes closest to `METHOD path` by edit distance.
    pub fn closest(&self, method: &str, path: &str) -> Vec<(String, String, usize)> {
        let target = format!("{} {}", method.to_ascii_uppercase(), normalize(path));
        let mut scored: Vec<(String, String, usize)> = self
            .entries
            .iter()
            .filter(|e| e.class != 2)
            .map(|e| {
                let p = normalize(&format!(
                    "{}/{}",
                    self.prefix,
                    e.path.trim_start_matches('/')
                ));
                let d = levenshtein(&target, &format!("{} {p}", e.method));
                (e.method.clone(), p, d)
            })
            .filter(|(m, p, d)| *d <= 3.max(format!("{m} {p}").len() / 3))
            .collect();
        scored.sort_by_key(|s| s.2);
        scored.truncate(3);
        scored
    }
}

/// Default response, else the first one (SPEC §5.2).
pub fn pick_response(route: &Route) -> Option<&Response> {
    route
        .responses
        .iter()
        .find(|r| r.default)
        .or_else(|| route.responses.first())
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        fauxfield_model::parse_server(
            r#"
id = "srv_a"
name = "A"
port = 1
prefix = "/api"
[[routes]]
id = "rte_1"
method = "GET"
path = "/users"
[[routes.responses]]
id = "rsp_1"
[[routes]]
id = "rte_2"
method = "ANY"
path = "/ping/"
"#,
        )
        .unwrap()
    }

    #[test]
    fn matches_prefix_slash_and_any() {
        let t = RouteTable::compile(&server());
        assert_eq!(t.find("GET", "/api/users").unwrap().route.id, "rte_1");
        assert_eq!(t.find("get", "/api/users/?x=1").unwrap().route.id, "rte_1");
        assert!(t.find("POST", "/api/users").is_none());
        assert!(t.find("GET", "/users").is_none());
        assert_eq!(t.find("DELETE", "/api/ping").unwrap().route.id, "rte_2");
    }

    #[test]
    fn hints_are_close_and_capped() {
        let t = RouteTable::compile(&server());
        let c = t.closest("GET", "/api/user");
        assert_eq!(c[0].1, "/api/users");
        assert!(t.closest("GET", "/zzzzzzzzzzzzzzzzzzzzzzzz").is_empty());
    }

    fn table(extra: &str, head: &str) -> RouteTable {
        let text = format!("id=\"srv_a\"\nname=\"A\"\nport=1\n{head}\n{extra}");
        RouteTable::compile(&fauxfield_model::parse_server(&text).unwrap())
    }

    fn id<'a>(t: &'a RouteTable, m: &str, p: &str) -> Option<(&'a str, Vec<(String, String)>)> {
        t.find(m, p).map(|x| (x.route.id.as_str(), x.params))
    }

    #[test]
    fn params_wildcard_and_regex() {
        let t = table(
            r#"
[[routes]]
id="a"
path="/users/:id"
[[routes]]
id="b"
path="/files/*rest"
[[routes]]
id="c"
path='^/v[0-9]+/orders/(?P<id>\d+)$'
match="regex"
[[routes]]
id="d"
path="/users/me"
"#,
            "",
        );
        assert_eq!(
            id(&t, "GET", "/users/42").unwrap(),
            ("a", vec![("id".into(), "42".into())])
        );
        assert_eq!(id(&t, "GET", "/users/me").unwrap().0, "d");
        assert_eq!(
            id(&t, "GET", "/files/a/b.txt").unwrap().1,
            vec![("rest".to_string(), "a/b.txt".to_string())]
        );
        assert!(id(&t, "GET", "/files").is_none());
        assert_eq!(
            id(&t, "GET", "/v2/orders/17").unwrap().1,
            vec![("id".to_string(), "17".to_string())]
        );
        assert!(id(&t, "GET", "/users//").is_none());
        assert_eq!(id(&t, "GET", "/users/a%2Fb").unwrap().1[0].1, "a/b");
    }

    #[test]
    fn precedence_and_strict_slash() {
        let t = table(
            r#"
[[routes]]
id="p"
path="/x/:a"
[[routes]]
id="hi"
path="/x/:b"
priority=3
[[routes]]
id="ex"
path="/x/1"
"#,
            "",
        );
        assert_eq!(id(&t, "GET", "/x/1").unwrap().0, "hi");
        assert_eq!(t.candidates("GET", "/x/1").len(), 3);
        let t = table("[[routes]]\nid=\"u\"\npath=\"/u\"\n", "strict_slash = true");
        assert!(id(&t, "GET", "/u").is_some());
        assert!(id(&t, "GET", "/u/").is_none());
    }
}
