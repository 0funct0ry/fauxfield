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
    /// Dataset actions (M10.06).
    pub actions: Vec<fauxfield_model::DatasetAction>,
    /// Status when an action fails (empty = default 409).
    pub action_error_status: String,
    pub content_type: String,
    /// Delay as written: milliseconds or a range such as `200..800`; empty = none.
    pub delay: String,
    /// Fault kind (SPEC §5.5); empty = none.
    pub fault: String,
    /// Fault probability as text (`1`, `0.3`); empty = always.
    pub fault_rate: String,
    pub body: String,
    /// Workspace-relative file served as the body instead of `body`.
    pub body_file: Option<String>,
    pub headers: Vec<(String, String)>,
    /// Rendered with Tera (default); false sends body, file and headers verbatim.
    pub template: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub id: String,
    pub method: String,
    pub path: String,
    pub name: String,
    pub responses: Vec<Response>,
    /// Route headers (values templated), in file order.
    pub headers: Vec<(String, String)>,
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
    /// `http`, `crud`, ... (`method` is `CRUD` for a crud Route).
    pub kind: String,
    /// crud Routes: the Dataset served and the optional JSON Schema file.
    pub dataset: Option<String>,
    pub validate: Option<String>,
    /// static Routes: directory, template extensions and index file (M10).
    pub dir: Option<String>,
    pub template_extensions: Vec<String>,
    pub index: Option<String>,
}

impl Route {
    pub fn is_crud(&self) -> bool {
        self.kind == "crud"
    }
    pub fn is_static(&self) -> bool {
        self.kind == "static"
    }
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
    /// Seed for fake data (SPEC §8.3); `None` means random output.
    pub fake_seed: Option<u64>,
    /// `[network]` (latency, jitter, bandwidth).
    pub network: fauxfield_model::Network,
    /// `[rate_limit]` as written (defaults filled in), `None` when absent.
    pub rate_limit: Option<fauxfield_model::RateLimit>,
    /// Chaos fault rate while the chaos switch is on (runtime only, SPEC §5.6).
    pub chaos: Option<f64>,
    /// `[tls]` as written (M10); `https()` says whether it is enabled.
    pub tls: Option<fauxfield_model::Tls>,
    /// `[cors]` as written (M10).
    pub cors: Option<fauxfield_model::Cors>,
    /// Certificate details while an HTTPS Server runs (engine status).
    pub cert: Option<fauxfield_engine::tls::CertStatus>,
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
    /// Effective value (`${env:NAME}` expanded); what the Environment tab shows.
    pub value: String,
    /// Text as written in the file; what editing starts from and what is saved.
    pub raw: String,
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
    pub datasets: Vec<Dataset>,
}

/// A Dataset as the sidebar and Dataset tab list it (live row count when a host runs).
#[derive(Clone, Debug, PartialEq)]
pub struct Dataset {
    pub name: String,
    pub server_id: String,
    pub server: String,
    pub rows: usize,
    pub key: String,
    pub id_strategy: String,
    pub persist: bool,
    pub seed: Option<String>,
    pub seed_template: Option<String>,
    pub error: Option<String>,
    /// Declared columns `(name, type)` (M10.04).
    pub columns: Vec<(String, String)>,
    /// Active seed source: `static`, `template`, `generator` or `none` (M10.04).
    pub source: String,
    /// Generator seed summary lines `name: kind` (M10.02), empty when the seed is not a generator.
    pub generator_fields: Vec<String>,
    /// First sample rows of the generator as JSON lines, or the generation error.
    pub generator_preview: Vec<String>,
    /// crud Routes serving this Dataset: `(route id, "GET /users" label)` (M10.01).
    pub used_by: Vec<(String, String)>,
}

/// Tooltip of the dice icon (SPEC §8.3).
pub const RANDOM_OUTPUT_TIP: &str = "Output is random: set a fake_seed for deterministic data";

impl Server {
    /// `[tls] enabled` (M10): the Server speaks HTTPS.
    pub fn https(&self) -> bool {
        self.tls.as_ref().is_some_and(|t| t.enabled)
    }
    /// True when the Server has no `fake_seed` and a templated body, Response header or Route
    /// header uses a random function (shows the dice icon).
    pub fn random_output(&self) -> bool {
        use fauxfield_engine::template::is_nondeterministic as nd;
        self.fake_seed.is_none()
            && self.all_routes().any(|r| {
                r.headers.iter().any(|(_, v)| nd(v))
                    || r.responses.iter().any(|x| {
                        x.template && (nd(&x.body) || x.headers.iter().any(|(_, v)| nd(v)))
                    })
            })
    }

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

    pub fn find_dataset(&self, key: &str) -> Option<&Dataset> {
        self.datasets.iter().find(|d| d.key() == key)
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

impl Dataset {
    /// No Route uses it (structured references only; sidebar badge).
    pub fn unused(&self) -> bool {
        self.used_by.is_empty()
    }

    /// Stable key across servers: `<server id>:<name>` (tab id; the row id adds `ds:`).
    pub fn key(&self) -> String {
        format!("{}:{}", self.server_id, self.name)
    }

    /// Sidebar label: `users · 50 rows`.
    pub fn label(&self) -> String {
        format!(
            "{} · {} {}",
            self.name,
            self.rows,
            if self.rows == 1 { "row" } else { "rows" }
        )
    }
}

/// Row count of a Dataset's seed file (0 for a template or a missing file); the shell replaces
/// it with the live count when a host runs.
fn seed_rows(root: &std::path::Path, d: &fauxfield_model::Dataset) -> usize {
    let Some(f) = d.seed.as_deref().filter(|f| fauxfield_store::confined(f)) else {
        return 0;
    };
    std::fs::read_to_string(root.join(f))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .map_or(0, |v| v.as_array().map_or(1, Vec::len))
}

/// First rows of a generator seed (empty for other seed sources). `ref` fields read the
/// preview rows of the Datasets they point at, generated in load order.
fn generator_preview(
    root: &std::path::Path,
    s: &fauxfield_model::Server,
    d: &fauxfield_model::Dataset,
) -> Vec<String> {
    use fauxfield_engine::generate::{Refs, generate};
    if d.generator.is_none() {
        return vec![];
    }
    let seed = Some(s.fake_seed.unwrap_or(0));
    let mut refs = Refs::new();
    let Ok(order) = fauxfield_model::generator::load_order(&s.datasets) else {
        return vec!["error: generator `ref` cycle".into()];
    };
    for i in order {
        let x = &s.datasets[i];
        let Some(xg) = &x.generator else { continue };
        match generate(&x.name, xg, seed, Some(xg.rows.min(20)), &refs, root) {
            Ok(v) => {
                if x.name == d.name {
                    return v
                        .as_array()
                        .map(|a| a.iter().take(3).map(|r| r.to_string()).collect())
                        .unwrap_or_default();
                }
                refs.insert(x.name.clone(), v.as_array().cloned().unwrap_or_default());
            }
            Err(e) if x.name == d.name => return vec![format!("error: {e}")],
            Err(_) => {}
        }
    }
    vec![]
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
            kind: r.kind.clone(),
            dataset: r.dataset.clone(),
            validate: r.validate.clone(),
            dir: r.dir.clone(),
            template_extensions: r.template_extensions.clone(),
            index: r.index.clone(),
            method: if r.kind == "static" {
                "FILES".to_string()
            } else if r.kind == "crud" {
                "CRUD".to_string()
            } else {
                r.method.clone()
            },
            path: r.path.clone(),
            name: r.name.clone().unwrap_or_else(|| {
                if r.kind == "crud" {
                    format!("CRUD {}", r.path)
                } else {
                    format!("{} {}", r.method, r.path)
                }
            }),
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
                    actions: x.actions.clone(),
                    action_error_status: x
                        .action_error_status
                        .map(|s| s.to_string())
                        .unwrap_or_default(),
                    content_type: x.content_type.clone().unwrap_or_default(),
                    delay: x.delay_ms.as_ref().map(|d| d.text()).unwrap_or_default(),
                    fault: x.fault.clone().filter(|f| f != "none").unwrap_or_default(),
                    fault_rate: x.fault_rate.map(|r| r.to_string()).unwrap_or_default(),
                    body: x.body.clone().unwrap_or_default(),
                    body_file: x.body_file.clone(),
                    template: x.template.unwrap_or(true),
                    headers: x
                        .headers
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                })
                .collect(),
            headers: r
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
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
                for d in &s.datasets {
                    datasets.push(Dataset {
                        name: d.name.clone(),
                        server_id: s.id.clone(),
                        server: s.name.clone(),
                        rows: seed_rows(&snap.root, d),
                        key: d.key_name().to_string(),
                        id_strategy: d.id_strategy.clone().unwrap_or_else(|| "uuid".into()),
                        persist: d.persist,
                        seed: d.seed.clone(),
                        seed_template: d.seed_template.clone(),
                        error: None,
                        columns: d
                            .columns
                            .iter()
                            .map(|c| {
                                (
                                    c.name.clone(),
                                    if c.ty.is_empty() {
                                        "string".into()
                                    } else {
                                        c.ty.clone()
                                    },
                                )
                            })
                            .collect(),
                        source: fauxfield_engine::source_kind(d).into(),
                        generator_fields: d.generator.as_ref().map_or(vec![], |g| {
                            g.fields
                                .iter()
                                .map(|f| format!("{}: {}", f.name, f.spec.kind))
                                .collect()
                        }),
                        generator_preview: generator_preview(&snap.root, s, d),
                        used_by: dataset_consumers(s, &d.name),
                    });
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
                    fake_seed: s.fake_seed,
                    network: s.network.clone().unwrap_or_default(),
                    rate_limit: s.rate_limit.clone(),
                    chaos: None,
                    tls: s.tls.clone(),
                    cors: s.cors.clone(),
                    cert: None,
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
            .map(|(_, e)| {
                use fauxfield_model::vars::{Source, expand_env, value_text};
                let dot = snap.dotenv(&e.name);
                let mut vars: Vec<Variable> = e
                    .variables
                    .iter()
                    .map(|(k, v)| {
                        let raw = value_text(v);
                        let (value, src) = expand_env(&raw, &dot, &|n| std::env::var(n).ok());
                        Variable {
                            key: k.clone(),
                            value,
                            raw,
                            source: match src {
                                Source::File => VarSource::File,
                                Source::Dotenv => VarSource::Dotenv,
                                Source::Process => VarSource::Process,
                            },
                        }
                    })
                    .collect();
                // Dotenv keys the file does not define are listed read-only.
                for (k, v) in &dot {
                    if !e.variables.contains_key(k) {
                        vars.push(Variable {
                            key: k.clone(),
                            value: v.clone(),
                            raw: v.clone(),
                            source: VarSource::Dotenv,
                        });
                    }
                }
                Environment {
                    name: e.name.clone(),
                    vars,
                }
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

/// Every Route that uses a Dataset: crud Routes, Response actions, Rules and body reads
/// (`data.<name>` or `dataset_rows(...)`), as `(route id, label)`.
fn dataset_consumers(s: &fauxfield_model::Server, name: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    let reads = |text: &str| text.contains(&format!("data.{name}"));
    for r in &s.routes {
        let at = format!("{} {}", r.method, r.path);
        if r.kind == "crud" && r.dataset.as_deref() == Some(name) {
            out.push((r.id.clone(), at.clone()));
        }
        let (mut act, mut rule, mut read) = (false, false, false);
        for x in &r.responses {
            act |= x.actions.iter().any(|a| a.dataset == name);
            rule |= x.rules.iter().any(|u| {
                matches!(u.target.as_str(), "dataset" | "dataset_count")
                    && fauxfield_model::rules::dataset_rule_name(u.key.as_deref().unwrap_or(""))
                        == name
            });
            read |= x.body.as_deref().is_some_and(reads);
        }
        for (hit, what) in [(act, "action"), (rule, "rule"), (read, "read")] {
            if hit {
                out.push((r.id.clone(), format!("{at} ({what})")));
            }
        }
    }
    out
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
        let names: Vec<_> = w.datasets.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["users", "charges"]);
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
