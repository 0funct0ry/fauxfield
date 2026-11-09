//! View model of the loaded Workspace, derived from `fauxfield-store` snapshots (M2). Plain data
//! for rendering; the files are authoritative (standing rule 2).

#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    pub id: String,
    pub name: String,
    /// One-line note shown under the name (SPEC §5.5).
    pub description: String,
    pub status: u16,
    pub default: bool,
    pub disabled: bool,
    /// `and` or `or`.
    pub rules_logic: String,
    pub rules: Vec<fauxfield_model::Rule>,
    pub content_type: String,
    pub delay_ms: u32,
    pub body: String,
    /// Workspace-relative file served as the body instead of `body`.
    pub body_file: Option<String>,
    pub headers: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub id: String,
    pub method: String,
    pub path: String,
    pub name: String,
    pub responses: Vec<Response>,
    /// `match` as written in the file (`None` = inferred).
    pub match_explicit: Option<String>,
    /// Effective mode: exact, param or regex.
    pub match_mode: String,
    pub priority: i32,
    /// `rules` (default), `sequential`, `random` or `disabled`.
    pub response_mode: String,
    /// Winning Route ID when this Route is fully shadowed.
    pub shadowed_by: Option<String>,
    /// Pattern error (E033/E034/E035) when the pattern is invalid.
    pub pattern_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub collapsed: bool,
    pub routes: Vec<Route>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Server {
    pub id: String,
    /// Workspace-relative file, e.g. `servers/users-api.toml`.
    pub file: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub prefix: String,
    pub enabled: bool,
    /// Closest-routes hint on 404 (default true).
    pub unmatched_hint: bool,
    pub strict_slash: bool,
    pub running: bool,
    /// `host:port` while running (engine status).
    pub listen: Option<String>,
    /// Last start error while failed (engine status).
    pub error: Option<String>,
    pub folders: Vec<Folder>,
    pub routes: Vec<Route>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarSource {
    File,
    Dotenv,
    Process,
}

impl VarSource {
    pub fn label(self) -> &'static str {
        match self {
            VarSource::File => "file",
            VarSource::Dotenv => "dotenv",
            VarSource::Process => "process env",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Variable {
    pub key: String,
    pub value: String,
    pub source: VarSource,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Environment {
    pub name: String,
    pub vars: Vec<Variable>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Workspace {
    pub name: String,
    pub root: std::path::PathBuf,
    pub servers: Vec<Server>,
    pub environments: Vec<Environment>,
    pub datasets: Vec<String>,
}

impl Server {
    pub fn all_routes(&self) -> impl Iterator<Item = &Route> {
        self.folders
            .iter()
            .flat_map(|f| f.routes.iter())
            .chain(self.routes.iter())
    }
}

impl Workspace {
    pub fn empty() -> Workspace {
        Workspace {
            name: String::new(),
            root: Default::default(),
            servers: vec![],
            environments: vec![],
            datasets: vec![],
        }
    }

    pub fn find_route(&self, id: &str) -> Option<(&Server, &Route)> {
        self.servers
            .iter()
            .find_map(|s| s.all_routes().find(|r| r.id == id).map(|r| (s, r)))
    }

    pub fn find_folder(&self, id: &str) -> Option<(&Server, &Folder)> {
        self.servers
            .iter()
            .find_map(|s| s.folders.iter().find(|f| f.id == id).map(|f| (s, f)))
    }

    pub fn find_server(&self, id: &str) -> Option<&Server> {
        self.servers.iter().find(|s| s.id == id)
    }

    pub fn find_environment(&self, name: &str) -> Option<&Environment> {
        self.environments.iter().find(|e| e.name == name)
    }

    /// Servers that Run all starts (disabled ones are skipped).
    pub fn enabled_count(&self) -> usize {
        self.servers.iter().filter(|s| s.enabled).count()
    }

    pub fn running_count(&self) -> usize {
        self.servers.iter().filter(|s| s.running).count()
    }
}

/// A validation finding shown in the Problems panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub warning: bool,
    pub code: String,
    pub message: String,
    pub file: String,
    pub path: String,
    /// Route or server to open when the problem is clicked.
    pub target: Option<String>,
}

impl Workspace {
    /// Build the view from a store snapshot. Servers never run before M3, so `running` is false.
    pub fn from_snapshot(snap: &fauxfield_store::Snapshot) -> Workspace {
        use fauxfield_model as m;
        let route = |r: &m::Route, shadows: &[m::pattern::Shadow], strict: bool| Route {
            match_explicit: r.match_mode.clone(),
            match_mode: match m::pattern::parse_mode(r.match_mode.as_deref()) {
                Ok(Some(x)) => x,
                _ => m::pattern::infer_mode(&r.path),
            }
            .label()
            .to_string(),
            priority: r.priority.unwrap_or(0),
            response_mode: r.response_mode.clone().unwrap_or_else(|| "rules".into()),
            shadowed_by: shadows
                .iter()
                .find(|x| x.route == r.id)
                .map(|x| x.by.clone()),
            pattern_error: if r.kind == "http" {
                m::pattern::Pattern::parse(&r.path, r.match_mode.as_deref(), strict).err()
            } else {
                None
            },
            id: r.id.clone(),
            method: r.method.clone(),
            path: r.path.clone(),
            name: r
                .name
                .clone()
                .unwrap_or_else(|| format!("{} {}", r.method, r.path)),
            responses: r
                .responses
                .iter()
                .map(|x| Response {
                    id: x.id.clone(),
                    name: x.name.clone().unwrap_or_else(|| x.status.to_string()),
                    description: x.description.clone().unwrap_or_default(),
                    status: x.status,
                    default: x.default,
                    disabled: x.disabled,
                    rules_logic: x.rules_logic.clone().unwrap_or_else(|| "and".into()),
                    rules: x.rules.clone(),
                    content_type: x.content_type.clone().unwrap_or_default(),
                    delay_ms: x.delay_ms.unwrap_or(0) as u32,
                    body: x.body.clone().unwrap_or_default(),
                    body_file: x.body_file.clone(),
                    headers: x
                        .headers
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                })
                .collect(),
        };
        let mut datasets = vec![];
        let servers = snap
            .servers
            .iter()
            .map(|(file, s)| {
                let shadows = m::pattern::shadowed_routes(s);
                let strict = s.strict_slash.unwrap_or(false);
                let route = |r: &m::Route| route(r, &shadows, strict);
                if let Some(toml::Value::Array(a)) = s.extra.get("datasets") {
                    for d in a {
                        if let Some(n) = d.get("name").and_then(|n| n.as_str()) {
                            datasets.push(n.to_string());
                        }
                    }
                }
                Server {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    file: file.clone(),
                    host: s.host.clone(),
                    port: s.port,
                    prefix: s.prefix.clone().unwrap_or_default(),
                    enabled: s.enabled,
                    unmatched_hint: s.unmatched_hint.unwrap_or(true),
                    strict_slash: s.strict_slash.unwrap_or(false),
                    running: false,
                    listen: None,
                    error: None,
                    folders: s
                        .folders
                        .iter()
                        .map(|f| Folder {
                            id: f.id.clone(),
                            name: f.name.clone(),
                            enabled: f.enabled,
                            collapsed: f.collapsed,
                            routes: s
                                .routes
                                .iter()
                                .filter(|r| r.folder.as_deref() == Some(&f.id))
                                .map(route)
                                .collect(),
                        })
                        .collect(),
                    routes: s
                        .routes
                        .iter()
                        .filter(|r| {
                            r.folder
                                .as_ref()
                                .is_none_or(|id| !s.folders.iter().any(|f| &f.id == id))
                        })
                        .map(route)
                        .collect(),
                }
            })
            .collect();
        let environments = snap
            .environments
            .iter()
            .map(|(_, e)| Environment {
                name: e.name.clone(),
                vars: e
                    .variables
                    .iter()
                    .map(|(k, v)| Variable {
                        key: k.clone(),
                        value: v
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| v.to_string()),
                        source: VarSource::File,
                    })
                    .collect(),
            })
            .collect();
        Workspace {
            name: if snap.manifest.name.is_empty() {
                snap.root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            } else {
                snap.manifest.name.clone()
            },
            root: snap.root.clone(),
            servers,
            environments,
            datasets,
        }
    }

    /// Diagnostics as Problems, each linked to the route or server it concerns.
    pub fn problems(snap: &fauxfield_store::Snapshot) -> Vec<Problem> {
        snap.diagnostics
            .iter()
            .map(|d| {
                let target = d
                    .path
                    .strip_prefix("routes[")
                    .and_then(|p| p.split(']').next())
                    .map(str::to_string)
                    .or_else(|| {
                        snap.servers
                            .iter()
                            .find(|(f, _)| *f == d.file)
                            .map(|(_, s)| s.id.clone())
                    });
                Problem {
                    warning: d.severity == fauxfield_model::Severity::Warning,
                    code: d.code.clone(),
                    message: format!("{}: {}", d.file, d.message),
                    file: d.file.clone(),
                    path: d.path.clone(),
                    target,
                }
            })
            .collect()
    }
}

/// Test support: the committed example workspace copied to a temp dir (autosave writes files).
#[cfg(test)]
pub fn example_store() -> (tempfile::TempDir, fauxfield_store::Store) {
    fn copy(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            let (p, t) = (e.path(), to.join(e.file_name()));
            if p.is_dir() {
                copy(&p, &t)
            } else {
                std::fs::copy(&p, &t).unwrap();
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    copy(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/example-workspace"),
        dir.path(),
    );
    let store = fauxfield_store::Store::open(dir.path()).unwrap();
    (dir, store)
}

/// Shell options for tests: a throwaway copy of the example workspace, no persistence.
#[cfg(test)]
pub fn test_options() -> crate::shell::ShellOptions {
    let (dir, _store) = example_store();
    // Tests start real servers: bind each test's servers at a distinct offset so parallel tests
    // never collide (the files and the UI keep the example ports).
    static NEXT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    crate::shell::ShellOptions {
        workspace: Some(dir.keep()),
        port_offset: 20_000 + (std::process::id() % 200) as u16 * 150 + (n % 40) * 20,
        ..Default::default()
    }
}

#[cfg(test)]
pub fn workspace() -> Workspace {
    let (_d, s) = example_store();
    Workspace::from_snapshot(&s.snapshot())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_lookups() {
        let w = workspace();
        let (s, r) = w.find_route("rt_users_get").unwrap();
        assert_eq!((s.port, r.method.as_str()), (4010, "GET"));
        assert!(w.find_route("nope").is_none());
        assert_eq!(w.find_environment("staging").unwrap().vars.len(), 2);
        assert_eq!(w.datasets, vec!["users".to_string(), "charges".to_string()]);
    }

    #[test]
    fn ids_are_unique() {
        let w = workspace();
        let mut ids: Vec<&str> = w
            .servers
            .iter()
            .flat_map(|s| s.all_routes().map(|r| r.id.as_str()))
            .collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }
}
