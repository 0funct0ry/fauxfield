//! Fuzzy ranking for Quick Search and the Command Palette (SPEC §9.6, §9.7). GPUI-free.
//!
//! Prefixes narrow the scope: `>` commands, `@` servers, `/` routes by path, `#` folders,
//! `$` variables.

use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::mock::Workspace;

pub const MAX_HITS: usize = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    All,
    Commands,
    Servers,
    Routes,
    Folders,
    Variables,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Route(String),
    Server(String),
    Folder(String),
    Environment(String),
    Dataset(String),
    Variable { environment: String, key: String },
    Action(String),
    Theme(String),
    SwitchEnvironment(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Route,
    Server,
    Folder,
    Environment,
    Dataset,
    Variable,
    Command,
    Theme,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub label: String,
    pub detail: String,
    pub kind: Kind,
    /// HTTP method for route hits, so the row can draw a badge.
    pub method: Option<String>,
    pub binding: Option<String>,
    pub target: Target,
    pub score: u32,
}

/// An action as the palette sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandEntry {
    pub id: String,
    pub label: String,
    pub binding: Option<String>,
}

/// Splits a leading scope prefix off the query.
pub fn parse_query(q: &str) -> (Scope, &str) {
    let scope = match q.chars().next() {
        Some('>') => Scope::Commands,
        Some('@') => Scope::Servers,
        Some('/') => Scope::Routes,
        Some('#') => Scope::Folders,
        Some('$') => Scope::Variables,
        _ => return (Scope::All, q),
    };
    // `/` is part of a route path, so it stays in the needle.
    let rest = if scope == Scope::Routes { q } else { &q[1..] };
    (scope, rest.trim_start())
}

struct Scorer {
    matcher: Matcher,
    pattern: Option<Pattern>,
    buf: Vec<char>,
}

impl Scorer {
    fn new(needle: &str) -> Scorer {
        let needle = needle.trim();
        Scorer {
            matcher: Matcher::new(Config::DEFAULT),
            pattern: (!needle.is_empty())
                .then(|| Pattern::parse(needle, CaseMatching::Ignore, Normalization::Smart)),
            buf: Vec::new(),
        }
    }

    /// `Some(score)` when the haystack matches (an empty needle matches everything with 0).
    fn score(&mut self, hay: &str) -> Option<u32> {
        match &self.pattern {
            None => Some(0),
            Some(p) => p.score(Utf32Str::new(hay, &mut self.buf), &mut self.matcher),
        }
    }
}

fn finish(mut hits: Vec<Hit>) -> Vec<Hit> {
    // Stable: ties keep the natural (tree) order.
    hits.sort_by(|a, b| b.score.cmp(&a.score));
    hits.truncate(MAX_HITS);
    hits
}

/// Quick Search over the workspace tree.
pub fn quick_search(query: &str, ws: &Workspace, commands: &[CommandEntry]) -> Vec<Hit> {
    let (scope, needle) = parse_query(query);
    if scope == Scope::Commands {
        return command_hits(needle, commands);
    }
    let mut sc = Scorer::new(needle);
    let mut hits = Vec::new();
    let all = scope == Scope::All;
    for s in &ws.servers {
        if (all || scope == Scope::Servers)
            && let Some(score) = sc.score(&s.name)
        {
            hits.push(Hit {
                label: s.name.clone(),
                detail: format!(":{}", s.port),
                kind: Kind::Server,
                method: None,
                binding: None,
                target: Target::Server(s.id.clone()),
                score,
            });
        }
        if all || scope == Scope::Folders {
            for f in &s.folders {
                if let Some(score) = sc.score(&f.name) {
                    hits.push(Hit {
                        label: f.name.clone(),
                        detail: s.name.clone(),
                        kind: Kind::Folder,
                        method: None,
                        binding: None,
                        target: Target::Folder(f.id.clone()),
                        score,
                    });
                }
            }
        }
        if all || scope == Scope::Routes {
            for r in s.all_routes() {
                let hay = format!("{} {} {}", r.method, r.path, r.name);
                let hay = if scope == Scope::Routes {
                    r.path.clone()
                } else {
                    hay
                };
                if let Some(score) = sc.score(&hay) {
                    hits.push(Hit {
                        label: format!("{}  {}", r.method, r.path),
                        detail: format!("{} · {}", r.name, s.name),
                        kind: Kind::Route,
                        method: Some(r.method.clone()),
                        binding: None,
                        target: Target::Route(r.id.clone()),
                        score,
                    });
                }
            }
        }
    }
    if all {
        for e in &ws.environments {
            if let Some(score) = sc.score(&e.name) {
                hits.push(Hit {
                    label: e.name.clone(),
                    detail: "Environment".into(),
                    kind: Kind::Environment,
                    method: None,
                    binding: None,
                    target: Target::Environment(e.name.clone()),
                    score,
                });
            }
        }
        for d in &ws.datasets {
            if let Some(score) = sc.score(d) {
                hits.push(Hit {
                    label: d.clone(),
                    detail: "Dataset".into(),
                    kind: Kind::Dataset,
                    method: None,
                    binding: None,
                    target: Target::Dataset(d.clone()),
                    score,
                });
            }
        }
    }
    if scope == Scope::Variables {
        for e in &ws.environments {
            for v in &e.vars {
                if let Some(score) = sc.score(&v.key) {
                    hits.push(Hit {
                        label: v.key.clone(),
                        detail: format!("{} · {}", e.name, v.source.label()),
                        kind: Kind::Variable,
                        method: None,
                        binding: None,
                        target: Target::Variable {
                            environment: e.name.clone(),
                            key: v.key.clone(),
                        },
                        score,
                    });
                }
            }
        }
    }
    finish(hits)
}

/// Indices of `labels` matching `needle`, best first (ties keep the given order).
pub fn filter_labels(needle: &str, labels: &[String]) -> Vec<usize> {
    let mut sc = Scorer::new(needle);
    let mut scored: Vec<(usize, u32)> = labels
        .iter()
        .enumerate()
        .filter_map(|(i, l)| sc.score(l).map(|s| (i, s)))
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.into_iter().map(|(i, _)| i).collect()
}

/// The Command Palette: every registered action, filtered by the needle.
pub fn command_hits(needle: &str, commands: &[CommandEntry]) -> Vec<Hit> {
    let mut sc = Scorer::new(needle);
    let hits = commands
        .iter()
        .filter_map(|c| {
            sc.score(&c.label).map(|score| Hit {
                label: c.label.clone(),
                detail: String::new(),
                kind: Kind::Command,
                method: None,
                binding: c.binding.clone(),
                target: Target::Action(c.id.clone()),
                score,
            })
        })
        .collect();
    finish(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::workspace;

    fn cmds() -> Vec<CommandEntry> {
        [
            ("toggle_left_dock", "View: Toggle Left Dock"),
            ("select_theme", "Theme Selector: Toggle"),
            ("run_all", "Server: Run All"),
        ]
        .iter()
        .map(|(id, label)| CommandEntry {
            id: (*id).into(),
            label: (*label).into(),
            binding: Some("secondary-b".into()),
        })
        .collect()
    }

    #[test]
    fn prefixes_select_scope() {
        assert_eq!(parse_query("> theme"), (Scope::Commands, "theme"));
        assert_eq!(parse_query("@users"), (Scope::Servers, "users"));
        assert_eq!(parse_query("/users/:id"), (Scope::Routes, "/users/:id"));
        assert_eq!(parse_query("#Users"), (Scope::Folders, "Users"));
        assert_eq!(parse_query("$token"), (Scope::Variables, "token"));
        assert_eq!(parse_query("plain"), (Scope::All, "plain"));
    }

    #[test]
    fn empty_query_lists_the_tree_in_order() {
        let ws = workspace();
        let hits = quick_search("", &ws, &[]);
        assert_eq!(hits[0].target, Target::Server("srv_users".into()));
        assert!(hits.iter().any(|h| h.kind == Kind::Environment));
        assert!(hits.iter().any(|h| h.kind == Kind::Dataset));
        assert!(hits.len() <= MAX_HITS);
    }

    #[test]
    fn slash_prefix_matches_route_paths_only() {
        let ws = workspace();
        let hits = quick_search("/charges", &ws, &[]);
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.kind == Kind::Route));
        assert!(
            hits.iter()
                .any(|h| h.target == Target::Route("rt_charges_get".into()))
        );
        assert!(quick_search("/zzzzqq", &ws, &[]).is_empty());
    }

    #[test]
    fn server_folder_variable_scopes() {
        let ws = workspace();
        let s = quick_search("@pay", &ws, &[]);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].target, Target::Server("srv_payments".into()));
        let f = quick_search("#charges", &ws, &[]);
        assert_eq!(f[0].target, Target::Folder("fld_charges".into()));
        let v = quick_search("$secret", &ws, &[]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].detail, "staging · dotenv");
    }

    #[test]
    fn fuzzy_ranks_better_matches_first() {
        let ws = workspace();
        let hits = quick_search("delete", &ws, &[]);
        assert_eq!(hits[0].target, Target::Route("rt_users_delete".into()));
        let hits = quick_search("refunds", &ws, &[]);
        assert_eq!(hits[0].target, Target::Route("rt_refunds".into()));
    }

    #[test]
    fn filter_labels_orders_by_score_then_input() {
        let l: Vec<String> = ["One Light", "One Dark", "Nord"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(filter_labels("", &l), [0, 1, 2]);
        assert_eq!(filter_labels("dark", &l), [1]);
        assert!(filter_labels("zzz", &l).is_empty());
    }

    #[test]
    fn commands_filter_and_carry_bindings() {
        let ws = workspace();
        let hits = quick_search("> theme", &ws, &cmds());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].target, Target::Action("select_theme".into()));
        assert_eq!(hits[0].binding.as_deref(), Some("secondary-b"));
        assert_eq!(command_hits("", &cmds()).len(), 3);
    }
}
