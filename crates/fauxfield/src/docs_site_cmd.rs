//! `docs spec|url|open` (M17.06): the published API documentation of a Server (SPEC §12.4).
//! `spec` works from the files alone; `url` and `open` need a running host (exit 4 otherwise).

use clap::Subcommand;
use fauxfield_control::ControlApi;
use serde_json::json;
use std::path::Path;

use crate::cli::{EXIT_OK, EXIT_RUNTIME, EXIT_USAGE};
use crate::host_cmd::{attach, control_failed, root_of, rt};
use crate::workspace_cmd::{WsArgs, open_store, print_json};

#[derive(Subcommand, Debug)]
pub enum DocsAction {
    /// Write the generated OpenAPI 3.1 document of a Server (no host needed).
    Spec {
        /// Server ID, name or slug.
        server: String,
        /// Write to this file instead of stdout.
        #[arg(short = 'o', long = "output", value_name = "FILE")]
        output: Option<std::path::PathBuf>,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Print the documentation URL of every running Server that publishes it.
    Url {
        /// Only this Server.
        #[arg(long)]
        server: Option<String>,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Open the documentation of a running Server in the browser.
    Open {
        /// Server ID, name or slug.
        server: String,
        /// Print the URL without opening a browser.
        #[arg(long)]
        no_launch: bool,
        #[command(flatten)]
        ws: WsArgs,
    },
}

fn launch(url: &str) -> bool {
    let (prog, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("open", vec![url])
    } else if cfg!(target_os = "windows") {
        ("cmd", vec!["/c", "start", "", url])
    } else {
        ("xdg-open", vec![url])
    };
    std::process::Command::new(prog)
        .args(args)
        .status()
        .is_ok_and(|s| s.success())
}

pub fn docs(action: DocsAction) -> i32 {
    match action {
        DocsAction::Spec { server, output, ws } => spec(&server, output.as_deref(), &ws),
        DocsAction::Url { server, ws } => {
            let Ok((_, c)) = attach(&root_of(&ws)) else {
                return crate::cli::EXIT_NO_HOST;
            };
            let names: Vec<String> = match server {
                Some(s) => vec![s],
                None => match rt().block_on(c.status()) {
                    Ok(l) => l.into_iter().map(|i| i.id).collect(),
                    Err(e) => return control_failed(e),
                },
            };
            let mut out = Vec::new();
            for n in names {
                match rt().block_on(c.openapi_get(n, false)) {
                    Ok(i) => {
                        if let Some(url) = i.url {
                            out.push(json!({"server": i.server_id, "name": i.server, "url": url}));
                        }
                    }
                    Err(e) => return control_failed(e),
                }
            }
            if ws.json {
                print_json(&json!({ "docs": out }));
            } else if out.is_empty() {
                println!("No running Server publishes API documentation.");
            } else {
                for o in &out {
                    println!(
                        "{}  {}",
                        o["server"].as_str().unwrap_or(""),
                        o["url"].as_str().unwrap_or("")
                    );
                }
            }
            EXIT_OK
        }
        DocsAction::Open {
            server,
            no_launch,
            ws,
        } => {
            let Ok((_, c)) = attach(&root_of(&ws)) else {
                return crate::cli::EXIT_NO_HOST;
            };
            let info = match rt().block_on(c.openapi_get(server.clone(), false)) {
                Ok(i) => i,
                Err(e) => return control_failed(e),
            };
            let Some(url) = info.url else {
                crate::err!(
                    "`{}` is not running with API documentation enabled. Start it and set docs_site.enabled = true.",
                    server
                );
                return EXIT_RUNTIME;
            };
            let opened = !no_launch && launch(&url);
            if ws.json {
                print_json(&json!({ "server": info.server_id, "url": url, "opened": opened }));
            } else {
                println!("{url}");
            }
            EXIT_OK
        }
    }
}

fn spec(server: &str, output: Option<&Path>, ws: &WsArgs) -> i32 {
    let store = match open_store(ws) {
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
    let Some((_, s)) = snap
        .servers
        .iter()
        .find(|(f, s)| s.id == server || s.name == server || slug(f) == server)
    else {
        crate::err!("not found: server `{server}`");
        return EXIT_USAGE;
    };
    let doc = fauxfield_engine::openapi_document(&store.root(), &s.id, s);
    for w in &doc.warnings {
        crate::err!("warning: {w}");
    }
    let text = doc.to_json();
    if let Some(p) = output
        && let Err(e) = std::fs::write(p, format!("{text}\n"))
    {
        crate::err!("could not write {}: {e}", p.display());
        return EXIT_RUNTIME;
    }
    if ws.json {
        print_json(&json!({
            "server": s.id,
            "stats": {"documented": doc.stats.documented, "skipped": doc.stats.skipped},
            "output": output.map(|p| p.display().to_string()),
            "document": doc.value,
        }));
    } else if output.is_none() {
        println!("{text}");
    }
    EXIT_OK
}
