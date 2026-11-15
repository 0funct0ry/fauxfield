//! `init`, `validate`, `server list`, `route list` (M2). Every command has a stable `--json`.

use std::path::{Path, PathBuf};

use clap::Subcommand;
use fauxfield_model::Severity;
use fauxfield_store::{Store, discover, init_workspace};
use serde_json::json;

use crate::cli::{EXIT_OK, EXIT_RUNTIME, EXIT_USAGE, EXIT_VALIDATION};

#[derive(clap::Args, Debug)]
pub struct WsArgs {
    /// Workspace folder (default: the nearest ancestor containing fauxfield.toml, else the current directory).
    #[arg(short = 'w', long = "workspace", value_name = "PATH")]
    pub workspace: Option<PathBuf>,
    /// Machine-readable output with a stable schema.
    #[arg(long)]
    pub json: bool,
}

#[derive(Subcommand, Debug)]
pub enum WorkspaceAction {
    /// Show the workspace name, folder, environments and servers.
    Info(WsArgs),
    /// Rename the workspace (the `name` in fauxfield.toml; the folder on disk is not renamed).
    Rename {
        /// New name: 1 to 80 characters, no control characters.
        name: String,
        #[command(flatten)]
        ws: WsArgs,
    },
}

#[derive(Subcommand, Debug)]
pub enum ServerAction {
    /// List Servers.
    List(WsArgs),
    /// Create a Server file.
    Add {
        /// Server name (the file slug derives from it).
        name: String,
        /// Port to listen on.
        #[arg(long)]
        port: u16,
        /// Bind address (default 127.0.0.1).
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Delete a Server file (refused while a host is running unless --force).
    Rm {
        /// Server (ID, name, or file slug).
        server: String,
        /// Delete even though a host is running.
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Show one Server with its Routes.
    Show {
        /// Server (ID, name, or file slug).
        server: String,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Rename a Server; the file follows the slug, the ID stays.
    Rename {
        /// Server (ID, name, or file slug).
        server: String,
        /// New name.
        name: String,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Enable a Server.
    Enable {
        /// Server (ID, name, or file slug).
        server: String,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Disable a Server.
    Disable {
        /// Server (ID, name, or file slug).
        server: String,
        #[command(flatten)]
        ws: WsArgs,
    },
}

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum RouteAction {
    /// List Routes.
    List {
        /// Only Routes of this Server (ID, name, or file slug).
        #[arg(long)]
        server: Option<String>,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Delete a Route.
    Rm {
        /// Route ID.
        route: String,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Show one Route with its Responses and Rules.
    Show {
        /// Route ID.
        route: String,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Add a Route to a Server file (a running host hot-reloads).
    Add {
        /// Server (ID, name, or file slug).
        #[arg(required_unless_present = "from_json")]
        server: Option<String>,
        /// HTTP method (case-insensitive) or ANY.
        #[arg(required_unless_present = "from_json")]
        method: Option<String>,
        /// Path or pattern (`/users/:id`, `/files/*rest`, or a regex with --match regex).
        #[arg(required_unless_present = "from_json")]
        path: Option<String>,
        /// Status of the default Response (100-599).
        #[arg(long)]
        status: Option<u16>,
        /// Body of the default Response.
        #[arg(long, conflicts_with = "body_file")]
        body: Option<String>,
        /// Read the default Response body from a file.
        #[arg(long, value_name = "FILE")]
        body_file: Option<PathBuf>,
        /// Full Route definition as JSON (same schema as the MCP `upsert_route` tool); `-` reads stdin.
        #[arg(long, value_name = "FILE|-", conflicts_with_all = ["method", "path", "status", "body", "body_file", "match_mode", "folder", "name", "priority"])]
        from_json: Option<String>,
        /// Match mode; omitted means inferred and nothing is written.
        #[arg(long = "match", value_name = "MODE", value_parser = ["exact", "param", "regex"])]
        match_mode: Option<String>,
        /// Folder (ID or unique name).
        #[arg(long)]
        folder: Option<String>,
        /// Route name.
        #[arg(long)]
        name: Option<String>,
        /// Priority (higher wins; default 0).
        #[arg(long, allow_hyphen_values = true)]
        priority: Option<i32>,
        #[command(flatten)]
        ws: WsArgs,
    },
}

pub(crate) fn open_store(a: &WsArgs) -> Result<Store, i32> {
    let root = match &a.workspace {
        Some(p) => p.clone(),
        None => {
            let cwd = std::env::current_dir().unwrap_or_default();
            discover(&cwd).unwrap_or(cwd)
        }
    };
    Store::open(&root).map_err(|e| {
        crate::err!("{e}");
        EXIT_RUNTIME
    })
}

pub(crate) use crate::output::print_json;

pub fn init(path: Option<PathBuf>, agents: bool, json: bool) -> i32 {
    let root = path.unwrap_or_else(|| PathBuf::from("."));
    match init_workspace(&root) {
        Ok(files) => {
            let agent_files = if agents {
                match crate::agents_cmd::apply_defaults(&root) {
                    Ok(f) => f,
                    Err(e) => {
                        crate::err!("{e}");
                        return EXIT_RUNTIME;
                    }
                }
            } else {
                vec![]
            };
            let rel: Vec<String> = files
                .iter()
                .map(|f| {
                    f.strip_prefix(&root)
                        .unwrap_or(f)
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect();
            if json {
                print_json(
                    &json!({ "path": root.to_string_lossy(), "created": rel, "agents": agent_files }),
                );
            } else {
                println!("Created workspace in {}", root.display());
                for f in &rel {
                    crate::output::info(&format!("  {f}"));
                }
                for f in &agent_files {
                    crate::output::info(&format!("  {f}  (agents)"));
                }
                crate::output::info(&format!(
                    "\nNext steps:\n  fauxfield {}      open the app\n  fauxfield validate -w {}",
                    root.display(),
                    root.display()
                ));
            }
            EXIT_OK
        }
        Err(e) => {
            crate::err!("{e}");
            EXIT_RUNTIME
        }
    }
}

pub fn validate(a: &WsArgs) -> i32 {
    let store = match open_store(a) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let snap = store.snapshot();
    let errors = snap
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    let warnings = snap.diagnostics.len() - errors;
    if a.json {
        print_json(
            &json!({ "ok": errors == 0, "errors": errors, "warnings": warnings, "diagnostics": snap.diagnostics }),
        );
    } else {
        for d in &snap.diagnostics {
            let sev = if d.severity == Severity::Error {
                "error"
            } else {
                "warning"
            };
            println!("{sev}[{}] {} {}: {}", d.code, d.file, d.path, d.message);
        }
        println!("{errors} error(s), {warnings} warning(s)");
    }
    if errors > 0 { EXIT_VALIDATION } else { EXIT_OK }
}

fn find_srv(store: &Store, q: &str) -> Option<(String, fauxfield_model::Server)> {
    let snap = store.snapshot();
    snap.servers.into_iter().find(|(f, s)| {
        s.id == q
            || s.name == q
            || Path::new(f)
                .file_stem()
                .is_some_and(|x| x.to_string_lossy() == q)
    })
}

fn server_mutation(action: ServerAction) -> i32 {
    let usage = |m: String| {
        crate::err!("{m}");
        EXIT_USAGE
    };
    let rt_fail = |e: fauxfield_store::StoreError| {
        crate::err!("{e}");
        EXIT_RUNTIME
    };
    match action {
        ServerAction::Add {
            name,
            port,
            host,
            ws,
        } => {
            let store = match open_store(&ws) {
                Ok(s) => s,
                Err(c) => return c,
            };
            if port == 0 {
                return usage("port must be 1-65535".into());
            }
            let id = match store.create_server_at(&name, &host, port) {
                Ok(id) => id,
                Err(e) => return rt_fail(e),
            };
            if let Err(e) = store.flush() {
                return rt_fail(e);
            }
            if ws.json {
                print_json(&json!({ "id": id, "name": name, "host": host, "port": port }));
            } else {
                println!("Added server {id}  {name}  {host}:{port}");
            }
            EXIT_OK
        }
        ServerAction::Rm { server, force, ws } => {
            let store = match open_store(&ws) {
                Ok(s) => s,
                Err(c) => return c,
            };
            let Some((_, srv)) = find_srv(&store, &server) else {
                return usage(format!("unknown server `{server}`"));
            };
            if !force && fauxfield_control::read_host(&store.snapshot().root).is_some() {
                crate::err!(
                    "a host is running for this workspace; stop it or pass --force to delete `{}`",
                    srv.name
                );
                return EXIT_RUNTIME;
            }
            if let Err(e) = store.delete_server(&srv.id) {
                return rt_fail(e);
            }
            if ws.json {
                print_json(&json!({ "deleted": srv.id }));
            } else {
                println!("Deleted server {}  {}", srv.id, srv.name);
            }
            EXIT_OK
        }
        ServerAction::Show { server, ws } => {
            let store = match open_store(&ws) {
                Ok(s) => s,
                Err(c) => return c,
            };
            let Some((file, srv)) = find_srv(&store, &server) else {
                return usage(format!("unknown server `{server}`"));
            };
            let v = json!({
                "id": srv.id, "name": srv.name, "file": file, "host": srv.host,
                "port": srv.port, "enabled": srv.enabled, "prefix": srv.prefix,
                "folders": srv.folders.len(),
                "routes": srv.routes.iter().map(|r| json!({
                    "id": r.id, "method": r.method, "path": r.path, "name": r.name,
                    "responses": r.responses.len(),
                })).collect::<Vec<_>>(),
            });
            if ws.json {
                print_json(&v);
            } else {
                println!(
                    "{}  {}  {}:{}{}",
                    srv.id,
                    srv.name,
                    srv.host,
                    srv.port,
                    if srv.enabled { "" } else { "  (disabled)" }
                );
                for r in &srv.routes {
                    println!("  {}  {:<6} {}", r.id, r.method, r.path);
                }
            }
            EXIT_OK
        }
        ServerAction::Rename { server, name, ws } => {
            let store = match open_store(&ws) {
                Ok(s) => s,
                Err(c) => return c,
            };
            let Some((_, srv)) = find_srv(&store, &server) else {
                return usage(format!("unknown server `{server}`"));
            };
            if let Err(e) = store.rename_server(&srv.id, &name) {
                return rt_fail(e);
            }
            if let Err(e) = store.flush() {
                return rt_fail(e);
            }
            if ws.json {
                print_json(&json!({ "id": srv.id, "name": name }));
            } else {
                println!("Renamed {} to {name}", srv.id);
            }
            EXIT_OK
        }
        ServerAction::Enable { server, ws } => set_enabled(&server, true, &ws),
        ServerAction::Disable { server, ws } => set_enabled(&server, false, &ws),
        ServerAction::List(_) => EXIT_USAGE,
    }
}

fn set_enabled(server: &str, enabled: bool, ws: &WsArgs) -> i32 {
    let store = match open_store(ws) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let Some((file, srv)) = find_srv(&store, server) else {
        crate::err!("unknown server `{server}`");
        return EXIT_USAGE;
    };
    let r = store
        .edit(&file, |d| {
            d["enabled"] = toml_edit::value(enabled);
            Ok(())
        })
        .and_then(|()| store.flush().map(|_| ()));
    if let Err(e) = r {
        crate::err!("{e}");
        return EXIT_RUNTIME;
    }
    if ws.json {
        print_json(&json!({ "id": srv.id, "enabled": enabled }));
    } else {
        println!(
            "{} is now {}",
            srv.name,
            if enabled { "enabled" } else { "disabled" }
        );
    }
    EXIT_OK
}

fn route_find(
    store: &Store,
    id: &str,
) -> Option<(fauxfield_model::Server, fauxfield_model::Route)> {
    store.snapshot().servers.into_iter().find_map(|(_, s)| {
        let r = s.routes.iter().find(|r| r.id == id).cloned()?;
        Some((s, r))
    })
}

fn route_show(id: &str, ws: &WsArgs) -> i32 {
    let store = match open_store(ws) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let Some((srv, r)) = route_find(&store, id) else {
        crate::err!("unknown route `{id}`");
        return EXIT_USAGE;
    };
    let mut v = serde_json::to_value(&r).unwrap_or_default();
    if let Some(o) = v.as_object_mut() {
        o.insert("server".into(), json!(srv.id));
    }
    if ws.json {
        print_json(&json!({ "route": v }));
    } else {
        println!("{}  {}  {}", r.id, r.method, r.path);
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    }
    EXIT_OK
}

fn route_rm(id: &str, ws: &WsArgs) -> i32 {
    use fauxfield_store::Target;
    let store = match open_store(ws) {
        Ok(s) => s,
        Err(c) => return c,
    };
    if route_find(&store, id).is_none() {
        crate::err!("unknown route `{id}`");
        return EXIT_USAGE;
    }
    if let Err(e) = store
        .delete(&Target::Route(id.to_owned()))
        .and_then(|_| store.flush().map(|_| ()))
    {
        crate::err!("{e}");
        return EXIT_RUNTIME;
    }
    if ws.json {
        print_json(&json!({ "deleted": id }));
    } else {
        println!("Deleted route {id}");
    }
    EXIT_OK
}

/// `route add --from-json`: the same code path as the MCP `upsert_route` tool.
fn route_from_json(store: &Store, src: &str, server: Option<String>, ws: &WsArgs) -> i32 {
    use std::io::Read;
    let mut text = String::new();
    let read = if src == "-" {
        std::io::stdin().read_to_string(&mut text).map(|_| ())
    } else {
        std::fs::read_to_string(src).map(|t| text = t)
    };
    if let Err(e) = read {
        crate::err!("cannot read {src}: {e}");
        return EXIT_USAGE;
    }
    let mut v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            crate::err!("invalid JSON: {e}");
            return EXIT_USAGE;
        }
    };
    let Some(obj) = v.as_object_mut() else {
        crate::err!("the route definition must be a JSON object");
        return EXIT_USAGE;
    };
    if let Some(sv) = server {
        obj.insert("server".into(), json!(sv));
    }
    let args = obj.clone();
    let mut reg = fauxfield_mcp::Registry::new();
    fauxfield_mcp::workspace_tools::register_workspace_tools(
        &mut reg,
        fauxfield_mcp::workspace_tools::fixed_store(store.clone()),
    );
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            crate::err!("cannot start runtime: {e}");
            return EXIT_RUNTIME;
        }
    };
    match rt.block_on(reg.call(fauxfield_mcp::AllowedClasses::all(), "upsert_route", args)) {
        Ok(out) => {
            if ws.json {
                print_json(&out);
            } else {
                println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
            }
            EXIT_OK
        }
        Err(fauxfield_mcp::CallError::Tool(m)) => {
            crate::err!("{m}");
            EXIT_VALIDATION
        }
        Err(e) => {
            crate::err!("{e:?}");
            EXIT_RUNTIME
        }
    }
}

pub fn server(action: ServerAction) -> i32 {
    let ServerAction::List(a) = action else {
        return server_mutation(action);
    };
    let store = match open_store(&a) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let snap = store.snapshot();
    let rows: Vec<_> = snap
        .servers
        .iter()
        .map(|(file, s)| json!({ "id": s.id, "name": s.name, "file": file, "host": s.host, "port": s.port, "enabled": s.enabled, "routes": s.routes.len() }))
        .collect();
    if a.json {
        print_json(&json!({ "servers": rows }));
    } else if rows.is_empty() {
        println!("No servers. Create one in the app or add servers/<name>.toml.");
    } else {
        for r in &rows {
            println!(
                "{}  {}  {}:{}  {} route(s){}",
                r["id"].as_str().unwrap(),
                r["name"].as_str().unwrap(),
                r["host"].as_str().unwrap(),
                r["port"],
                r["routes"],
                if r["enabled"] == true {
                    ""
                } else {
                    "  (disabled)"
                }
            );
        }
    }
    EXIT_OK
}

#[derive(Subcommand, Debug)]
pub enum ResponseAction {
    /// Add a Response to a Route (a running host hot-reloads).
    Add {
        /// Route ID.
        route: String,
        /// Response name.
        #[arg(long)]
        name: Option<String>,
        /// Status code (100-599, default 200).
        #[arg(long)]
        status: Option<u16>,
        /// Response body.
        #[arg(long)]
        body: Option<String>,
        /// Content type.
        #[arg(long)]
        content_type: Option<String>,
        /// Mark as the Route's default Response.
        #[arg(long)]
        default: bool,
        /// Add the RFC 9457 problem+json preset for this status instead (400, 401, ... 504).
        #[arg(long, value_name = "STATUS", conflicts_with_all = ["name", "status", "body", "content_type"])]
        preset: Option<u16>,
        /// Add a fault preset instead: 502-after-10s, 504-after-30s, timeout, reset, close, empty,
        /// malformed, slow-body.
        #[arg(long, value_name = "ID", conflicts_with = "preset")]
        fault_preset: Option<String>,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Make a Response the default of its Route.
    SetDefault {
        /// Response ID.
        response: String,
        #[command(flatten)]
        ws: WsArgs,
    },
}

pub fn response(action: ResponseAction) -> i32 {
    use fauxfield_store::{StoreError, Target};
    let fail = |e: StoreError| match e {
        StoreError::NotFound(m) => {
            crate::err!("not found: {m}");
            EXIT_USAGE
        }
        StoreError::Invalid(m) => {
            crate::err!("{m}");
            EXIT_USAGE
        }
        e => {
            crate::err!("{e}");
            EXIT_RUNTIME
        }
    };
    match action {
        ResponseAction::Add {
            route,
            name,
            status,
            body,
            content_type,
            default,
            preset,
            fault_preset,
            ws,
        } => {
            let store = match open_store(&ws) {
                Ok(s) => s,
                Err(c) => return c,
            };
            if let Some(s) = status
                && !(100..=599).contains(&s)
            {
                crate::err!("status {s} is not 100-599");
                return EXIT_USAGE;
            }
            let created = match (preset, fault_preset) {
                (Some(p), _) => store.create_error_preset(&route, p),
                (None, Some(f)) => store.create_fault_preset(&route, &f),
                (None, None) => store.create_response(&route).and_then(|id| {
                    let t = Target::Response(id.clone());
                    store.set_str(&t, "name", Some(name.as_deref().unwrap_or("New response")))?;
                    if let Some(s) = status {
                        store.set_int(&t, "status", Some(i64::from(s)))?;
                    }
                    if let Some(b) = &body {
                        store.set_str(&t, "body", Some(b))?;
                    }
                    if let Some(c) = &content_type {
                        store.set_str(&t, "content_type", Some(c))?;
                    }
                    Ok(id)
                }),
            };
            let id = match created {
                Ok(id) => id,
                Err(e) => return fail(e),
            };
            if default && let Err(e) = store.set_default_response(&id) {
                return fail(e);
            }
            if let Err(e) = store.flush() {
                return fail(e);
            }
            let snap = store.snapshot();
            let resp = snap
                .servers
                .iter()
                .flat_map(|(_, s)| s.routes.iter())
                .flat_map(|r| r.responses.iter())
                .find(|r| r.id == id);
            let (n, st) = resp.map_or((String::new(), 0), |r| {
                (r.name.clone().unwrap_or_default(), r.status)
            });
            if ws.json {
                print_json(&json!({
                    "id": id, "route": route, "name": n, "status": st, "default": default,
                }));
            } else {
                println!("Added {id}  {st} {n}");
            }
            EXIT_OK
        }
        ResponseAction::SetDefault { response, ws } => {
            let store = match open_store(&ws) {
                Ok(s) => s,
                Err(c) => return c,
            };
            if store.file_of(&Target::Response(response.clone())).is_none() {
                crate::err!("not found: {response}");
                return EXIT_USAGE;
            }
            if let Err(e) = store
                .set_default_response(&response)
                .and_then(|()| store.flush().map(|_| ()))
            {
                return fail(e);
            }
            if ws.json {
                print_json(&json!({ "id": response, "default": true }));
            } else {
                println!("{response} is now the default Response");
            }
            EXIT_OK
        }
    }
}

pub fn route(action: RouteAction) -> i32 {
    let (server, ws) = match action {
        RouteAction::List { server, ws } => (server, ws),
        RouteAction::Add { .. } => return route_add(action),
        RouteAction::Rm { route, ws } => return route_rm(&route, &ws),
        RouteAction::Show { route, ws } => return route_show(&route, &ws),
    };
    let store = match open_store(&ws) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let snap = store.snapshot();
    let slug = |f: &str| {
        Path::new(f)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let mut rows = vec![];
    for (file, s) in &snap.servers {
        if let Some(q) = &server
            && *q != s.id
            && *q != s.name
            && *q != slug(file)
        {
            continue;
        }
        let shadows = fauxfield_model::pattern::shadowed_routes(s);
        for r in &s.routes {
            let shadowed_by = shadows
                .iter()
                .find(|x| x.route == r.id)
                .map(|x| x.by.clone());
            let folder = r
                .folder
                .as_ref()
                .and_then(|id| s.folders.iter().find(|f| &f.id == id))
                .map(|f| f.name.clone());
            rows.push(json!({ "id": r.id, "server": s.id, "folder": folder, "kind": r.kind, "method": r.method, "path": r.path, "name": r.name, "responses": r.responses.len(), "match": r.match_mode, "priority": r.priority.unwrap_or(0), "shadowed_by": shadowed_by }));
        }
    }
    if ws.json {
        print_json(&json!({ "routes": rows }));
    } else if rows.is_empty() {
        println!("No routes.");
    } else {
        for r in &rows {
            println!(
                "{}  {:<6} {}  ({} response(s))",
                r["id"].as_str().unwrap(),
                r["method"].as_str().unwrap(),
                r["path"].as_str().unwrap(),
                r["responses"]
            );
        }
    }
    EXIT_OK
}

fn route_add(action: RouteAction) -> i32 {
    let RouteAction::Add {
        server,
        method,
        path,
        status,
        body,
        body_file,
        from_json,
        match_mode,
        folder,
        name,
        priority,
        ws,
    } = action
    else {
        return EXIT_USAGE;
    };
    let store = match open_store(&ws) {
        Ok(s) => s,
        Err(c) => return c,
    };
    if let Some(src) = from_json {
        return route_from_json(&store, &src, server, &ws);
    }
    let (Some(server), Some(method), Some(path)) = (server, method, path) else {
        crate::err!("server, method and path are required");
        return EXIT_USAGE;
    };
    if let Some(s) = status
        && !(100..=599).contains(&s)
    {
        crate::err!("status {s} is not 100-599");
        return EXIT_USAGE;
    }
    let body = match (&body, &body_file) {
        (Some(b), _) => Some(b.clone()),
        (None, Some(f)) => match std::fs::read_to_string(f) {
            Ok(t) => Some(t),
            Err(e) => {
                crate::err!("cannot read {}: {e}", f.display());
                return EXIT_USAGE;
            }
        },
        _ => None,
    };
    let usage = |m: String| {
        crate::err!("{m}");
        EXIT_USAGE
    };
    let snap = store.snapshot();
    let slug = |f: &str| {
        Path::new(f)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let Some((file, srv)) = snap
        .servers
        .iter()
        .find(|(f, s)| s.id == server || s.name == server || slug(f) == server)
    else {
        return usage(format!("unknown server `{server}`"));
    };
    let method = method.to_ascii_uppercase();
    if ![
        "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "ANY",
    ]
    .contains(&method.as_str())
    {
        return usage(format!("unknown method `{method}`"));
    }
    let folder_id = match &folder {
        None => None,
        Some(q) => {
            let by_id: Vec<_> = srv.folders.iter().filter(|f| &f.id == q).collect();
            let hits = if by_id.is_empty() {
                srv.folders.iter().filter(|f| &f.name == q).collect()
            } else {
                by_id
            };
            match hits.as_slice() {
                [f] => Some(f.id.clone()),
                [] => return usage(format!("unknown folder `{q}`")),
                _ => return usage(format!("folder name `{q}` is ambiguous; use its ID")),
            }
        }
    };
    let _ = file;
    let opts = fauxfield_store::RouteOpts {
        match_mode: match_mode.clone(),
        name,
        priority,
    };
    let id = match store.create_route_with(&srv.id, folder_id.as_deref(), &method, &path, &opts) {
        Ok(id) => id,
        Err(fauxfield_store::StoreError::Invalid(m)) => {
            crate::err!("E033: {m}");
            return EXIT_VALIDATION;
        }
        Err(e) => {
            crate::err!("{e}");
            return EXIT_RUNTIME;
        }
    };
    if status.is_some() || body.is_some() {
        use fauxfield_store::Target;
        let snap = store.snapshot();
        let resp = snap
            .servers
            .iter()
            .flat_map(|(_, s)| s.routes.iter())
            .find(|r| r.id == id)
            .and_then(|r| r.responses.first().map(|x| x.id.clone()));
        let Some(resp) = resp else {
            crate::err!("route {id} has no default Response to set --status/--body on");
            return EXIT_RUNTIME;
        };
        let t = Target::Response(resp);
        let r = (|| {
            if let Some(s) = status {
                store.set_int(&t, "status", Some(i64::from(s)))?;
            }
            if let Some(b) = &body {
                store.set_str(&t, "body", Some(b))?;
            }
            Ok::<(), fauxfield_store::StoreError>(())
        })();
        if let Err(e) = r {
            crate::err!("{e}");
            return EXIT_RUNTIME;
        }
    }
    if let Err(e) = store.flush() {
        crate::err!("{e}");
        return EXIT_RUNTIME;
    }
    let after = store.snapshot();
    let warnings: Vec<_> = after
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.path == format!("routes[{id}]"))
        .map(|d| json!({ "code": d.code, "message": d.message }))
        .collect();
    if ws.json {
        print_json(&json!({
            "id": id, "server": srv.id, "method": method, "path": path,
            "match": match_mode, "folder": folder_id,
            "priority": priority.unwrap_or(0), "warnings": warnings,
        }));
    } else {
        println!("Added {id}  {method} {path}");
        for w in &warnings {
            eprintln!(
                "warning[{}]: {}",
                w["code"].as_str().unwrap(),
                w["message"].as_str().unwrap()
            );
        }
    }
    EXIT_OK
}

pub fn workspace(action: WorkspaceAction) -> i32 {
    match action {
        WorkspaceAction::Info(a) => {
            let store = match open_store(&a) {
                Ok(s) => s,
                Err(c) => return c,
            };
            let snap = store.snapshot();
            let v = json!({
                "name": snap.manifest.name,
                "root": snap.root.to_string_lossy(),
                "default_environment": snap.manifest.default_environment,
                "environments": snap.environments.iter().map(|(_, e)| e.name.clone()).collect::<Vec<_>>(),
                "servers": snap.servers.len(),
            });
            if a.json {
                print_json(&v);
            } else {
                println!("{}  ({})", snap.manifest.name, snap.root.display());
                println!(
                    "{} server(s), {} environment(s)",
                    snap.servers.len(),
                    snap.environments.len()
                );
            }
            EXIT_OK
        }
        WorkspaceAction::Rename { name, ws } => {
            let name = match fauxfield_model::validate_workspace_name(&name) {
                Ok(n) => n,
                Err(m) => {
                    crate::err!("{m}");
                    return EXIT_VALIDATION;
                }
            };
            let store = match open_store(&ws) {
                Ok(s) => s,
                Err(c) => return c,
            };
            if let Err(e) = store
                .set_workspace_name(&name)
                .and_then(|_| store.flush().map(|_| ()))
            {
                crate::err!("{e}");
                return EXIT_RUNTIME;
            }
            let root = store.snapshot().root;
            if ws.json {
                print_json(&json!({ "name": name, "root": root.to_string_lossy() }));
            } else {
                println!("Renamed workspace to {name} (folder not renamed)");
            }
            EXIT_OK
        }
    }
}

#[derive(clap::Args, Debug)]
pub struct RenderArgs {
    /// Server ID, name or file slug.
    pub server: String,
    /// Route ID.
    pub route: String,
    /// Response ID (default: the Route's default or first Response).
    #[arg(long)]
    pub response: Option<String>,
    /// Sample request method (default: the Route's method).
    #[arg(long)]
    pub method: Option<String>,
    /// Sample request path and query, relative to the Server prefix (default: the Route path).
    #[arg(long)]
    pub url: Option<String>,
    /// Sample request header `Name: value` (repeatable).
    #[arg(short = 'H', long = "header", value_name = "NAME:VALUE")]
    pub headers: Vec<String>,
    /// Sample request body.
    #[arg(long, default_value = "")]
    pub body: String,
    /// Override the Server's fake_seed for this render.
    #[arg(long)]
    pub seed: Option<u64>,
    #[command(flatten)]
    pub ws: WsArgs,
}

pub fn render(a: RenderArgs) -> i32 {
    let store = match open_store(&a.ws) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let snap = store.snapshot();
    let slug = |f: &str| {
        Path::new(f)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let Some((_, server)) = snap
        .servers
        .iter()
        .find(|(f, s)| s.id == a.server || s.name == a.server || slug(f) == a.server)
    else {
        crate::err!("not found: server `{}`", a.server);
        return EXIT_USAGE;
    };
    let Some(route) = server.routes.iter().find(|r| r.id == a.route) else {
        crate::err!("not found: route `{}`", a.route);
        return EXIT_USAGE;
    };
    let resp = match &a.response {
        Some(id) => route.responses.iter().find(|r| r.id == *id),
        None => route
            .responses
            .iter()
            .find(|r| r.default)
            .or(route.responses.first()),
    };
    let Some(resp) = resp else {
        crate::err!("not found: response in route `{}`", route.id);
        return EXIT_USAGE;
    };
    let mut server = server.clone();
    if a.seed.is_some() {
        server.fake_seed = a.seed;
    }
    let headers = a
        .headers
        .iter()
        .filter_map(|h| h.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    let url = a.url.clone().unwrap_or_else(|| {
        format!(
            "{}{}",
            server.prefix.clone().unwrap_or_default(),
            route.path.replace(['{', '}'], "")
        )
    });
    let sample = fauxfield_engine::SampleRequest {
        method: a.method.clone().unwrap_or_else(|| route.method.clone()),
        url,
        headers,
        body: a.body.clone(),
        datasets: None,
    };
    let p = fauxfield_engine::preview(&store.root(), &server, route, resp, &sample, None);
    if a.ws.json {
        print_json(&serde_json::json!({
            "server": server.id,
            "route": route.id,
            "response": resp.id,
            "status": p.status,
            "headers": p.headers.iter().map(|(k, v)| serde_json::json!([k, v])).collect::<Vec<_>>(),
            "body": p.body,
            "render_ms": p.render_ms,
            "error": p.error,
            "json_error": p.json_error,
            "deterministic": !p.nondeterministic,
        }));
    } else {
        for (k, v) in &p.headers {
            eprintln!("{k}: {v}");
        }
        println!("{}", p.body);
        if let Some(e) = &p.error {
            crate::err!("{e}");
        }
    }
    if p.error.is_some() { EXIT_USAGE } else { 0 }
}
