//! `init`, `validate`, `server list`, `route list` (M2). Every command has a stable `--json`.

use std::path::{Path, PathBuf};

use clap::Subcommand;
use fauxfield_model::Severity;
use fauxfield_store::{Store, discover, init_workspace};
use serde_json::json;

use crate::cli::{EXIT_OK, EXIT_RUNTIME, EXIT_VALIDATION};

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
}

#[derive(Subcommand, Debug)]
pub enum RouteAction {
    /// List Routes.
    List {
        /// Only Routes of this Server (ID, name, or file slug).
        #[arg(long)]
        server: Option<String>,
        #[command(flatten)]
        ws: WsArgs,
    },
}

fn open_store(a: &WsArgs) -> Result<Store, i32> {
    let root = match &a.workspace {
        Some(p) => p.clone(),
        None => {
            let cwd = std::env::current_dir().unwrap_or_default();
            discover(&cwd).unwrap_or(cwd)
        }
    };
    Store::open(&root).map_err(|e| {
        eprintln!("error: {e}");
        EXIT_RUNTIME
    })
}

fn print_json(v: &serde_json::Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap());
}

pub fn init(path: Option<PathBuf>, json: bool) -> i32 {
    let root = path.unwrap_or_else(|| PathBuf::from("."));
    match init_workspace(&root) {
        Ok(files) => {
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
                print_json(&json!({ "path": root.to_string_lossy(), "created": rel }));
            } else {
                println!("Created workspace in {}", root.display());
                for f in &rel {
                    println!("  {f}");
                }
                println!(
                    "\nNext steps:\n  fauxfield {}      open the app\n  fauxfield validate -w {}",
                    root.display(),
                    root.display()
                );
            }
            EXIT_OK
        }
        Err(e) => {
            eprintln!("error: {e}");
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

pub fn server(action: ServerAction) -> i32 {
    let ServerAction::List(a) = action;
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

pub fn route(action: RouteAction) -> i32 {
    let RouteAction::List { server, ws } = action;
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
        for r in &s.routes {
            let folder = r
                .folder
                .as_ref()
                .and_then(|id| s.folders.iter().find(|f| &f.id == id))
                .map(|f| f.name.clone());
            rows.push(json!({ "id": r.id, "server": s.id, "folder": folder, "kind": r.kind, "method": r.method, "path": r.path, "name": r.name, "responses": r.responses.len() }));
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
                    eprintln!("error: {m}");
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
                eprintln!("error: {e}");
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
