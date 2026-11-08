use fauxfield_model::{Response, Route, Server};

/// Compiled, immutable route table for one server; swapped atomically on hot reload.
#[derive(Debug, Clone, Default)]
pub struct RouteTable {
    entries: Vec<Entry>,
    pub unmatched_hint: bool,
}

#[derive(Debug, Clone)]
struct Entry {
    method: String,
    path: String,
    route: Route,
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

impl RouteTable {
    /// Exact routes only (M4 adds params and regex). The server prefix is prepended.
    pub fn compile(server: &Server) -> RouteTable {
        let prefix = server.prefix.clone().unwrap_or_default();
        let entries = server
            .routes
            .iter()
            .filter(|r| r.kind == "http")
            .filter(|r| matches!(r.match_mode.as_deref(), None | Some("exact")))
            .map(|r| Entry {
                method: r.method.to_ascii_uppercase(),
                path: normalize(&format!("{prefix}/{}", r.path.trim_start_matches('/'))),
                route: r.clone(),
            })
            .collect();
        RouteTable {
            entries,
            unmatched_hint: server.unmatched_hint.unwrap_or(true),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// First route (file order) whose method and path match.
    pub fn find(&self, method: &str, path: &str) -> Option<&Route> {
        let path = normalize(path);
        self.entries
            .iter()
            .find(|e| {
                (e.method == "ANY" || e.method.eq_ignore_ascii_case(method)) && e.path == path
            })
            .map(|e| &e.route)
    }

    /// Up to three routes closest to `METHOD path` by edit distance.
    pub fn closest(&self, method: &str, path: &str) -> Vec<(String, String, usize)> {
        let target = format!("{} {}", method.to_ascii_uppercase(), normalize(path));
        let mut scored: Vec<(String, String, usize)> = self
            .entries
            .iter()
            .map(|e| {
                let cand = format!("{} {}", e.method, e.path);
                let d = levenshtein(&target, &cand);
                (e.method.clone(), e.path.clone(), d)
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
        assert_eq!(t.find("GET", "/api/users").unwrap().id, "rte_1");
        assert_eq!(t.find("get", "/api/users/?x=1").unwrap().id, "rte_1");
        assert!(t.find("POST", "/api/users").is_none());
        assert!(t.find("GET", "/users").is_none());
        assert_eq!(t.find("DELETE", "/api/ping").unwrap().id, "rte_2");
    }

    #[test]
    fn hints_are_close_and_capped() {
        let t = RouteTable::compile(&server());
        let c = t.closest("GET", "/api/user");
        assert_eq!(c[0].1, "/api/users");
        assert!(t.closest("GET", "/zzzzzzzzzzzzzzzzzzzzzzzz").is_empty());
    }
}
