//! `data list|show|reset|export|import` (M8). Every command has a stable `--json`.
//! With a running host they act on live data; otherwise an in-process engine reads the
//! workspace (seeds, `seed_template`, persisted state) without serving anything.

use std::path::PathBuf;

use clap::Subcommand;
use fauxfield_control::{ControlApi, RemoteControl, read_host};
use fauxfield_engine::{DatasetInfo, Engine, EngineConfig};
use serde_json::{Value, json};

use crate::cli::{EXIT_OK, EXIT_RUNTIME};
use crate::host_cmd::{control_failed, root_of, rt};
use crate::workspace_cmd::{WsArgs, open_store, print_json};

#[derive(Subcommand, Debug)]
pub enum DataAction {
    /// List Datasets with their row counts.
    List {
        /// Only this Server (ID, name or slug).
        #[arg(long)]
        server: Option<String>,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Print a Dataset's rows (live when a host runs, else the seed or persisted state).
    Show {
        server: String,
        dataset: String,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Reset Datasets to their seed and clear counters and runtime variables.
    Reset {
        /// Only this Server.
        #[arg(long)]
        server: Option<String>,
        /// Only this Dataset (needs --server).
        #[arg(long, requires = "server")]
        dataset: Option<String>,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Write a Dataset's rows as JSON to stdout or a file.
    Export {
        server: String,
        dataset: String,
        /// Write to this file instead of stdout.
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Replace a Dataset's rows from a JSON file: live on a running host, else the seed file.
    Import {
        server: String,
        dataset: String,
        file: PathBuf,
        #[command(flatten)]
        ws: WsArgs,
    },
}

fn fail(msg: &str) -> i32 {
    eprintln!("error: {msg}");
    EXIT_RUNTIME
}

fn host_api(ws: &WsArgs) -> Option<RemoteControl> {
    read_host(&root_of(ws)).map(|i| RemoteControl::new(&i.socket))
}

/// An engine over the workspace files that never serves (offline reads and resets).
fn offline(ws: &WsArgs) -> Result<Engine, i32> {
    let store = open_store(ws)?;
    let snap = store.snapshot();
    let rt: &'static tokio::runtime::Runtime = Box::leak(Box::new(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime"),
    ));
    let e = Engine::new(
        rt.handle().clone(),
        EngineConfig {
            root: store.root(),
            ..Default::default()
        },
    );
    e.apply(snap.servers.into_iter().map(|(_, s)| s).collect());
    Ok(e)
}

fn rows_label(v: &Value) -> usize {
    v.as_array().map_or(1, Vec::len)
}

pub fn data(action: DataAction) -> i32 {
    match action {
        DataAction::List { server, ws } => {
            let (live, list) = match host_api(&ws) {
                Some(c) => match rt().block_on(c.data_list(server.clone())) {
                    Ok(l) => (true, l),
                    Err(e) => return control_failed(e),
                },
                None => {
                    let e = match offline(&ws) {
                        Ok(e) => e,
                        Err(c) => return c,
                    };
                    match e.datasets(server.as_deref()) {
                        Ok(l) => (false, l),
                        Err(e) => return fail(&e.to_string()),
                    }
                }
            };
            if ws.json {
                let rows: Vec<Value> = list.iter().map(info_json).collect();
                print_json(&json!({"live": live, "datasets": rows}));
            } else if list.is_empty() {
                println!("No datasets.");
            } else {
                for d in &list {
                    println!("{} · {} · {} rows", d.server, d.name, d.rows);
                }
            }
            EXIT_OK
        }
        DataAction::Show {
            server,
            dataset,
            ws,
        } => show_or_export(server, dataset, ws, None),
        DataAction::Export {
            server,
            dataset,
            out,
            ws,
        } => show_or_export(
            server,
            dataset,
            ws,
            Some(out.unwrap_or_else(|| PathBuf::from("-"))),
        ),
        DataAction::Reset {
            server,
            dataset,
            ws,
        } => {
            let live = match host_api(&ws) {
                Some(c) => {
                    if let Err(e) = rt().block_on(c.reset_state(server.clone(), dataset.clone())) {
                        return control_failed(e);
                    }
                    true
                }
                None => {
                    let e = match offline(&ws) {
                        Ok(e) => e,
                        Err(c) => return c,
                    };
                    let r = match (&server, &dataset) {
                        (Some(s), Some(d)) => e.dataset_reset(s, d),
                        _ => e.reset_state(server.as_deref()),
                    };
                    if let Err(e) = r {
                        return fail(&e.to_string());
                    }
                    false
                }
            };
            if ws.json {
                print_json(
                    &json!({"reset": true, "live": live, "server": server, "dataset": dataset}),
                );
            } else {
                println!(
                    "Reset {}{}",
                    dataset.as_deref().unwrap_or("all datasets"),
                    if live {
                        ""
                    } else {
                        " (persisted state cleared; no host running)"
                    }
                );
            }
            EXIT_OK
        }
        DataAction::Import {
            server,
            dataset,
            file,
            ws,
        } => {
            let text = match std::fs::read_to_string(&file) {
                Ok(t) => t,
                Err(e) => return fail(&format!("{}: {e}", file.display())),
            };
            let rows: Value = match serde_json::from_str(&text) {
                Ok(v @ (Value::Array(_) | Value::Object(_))) => v,
                Ok(_) => return fail("the file must hold a JSON array or object"),
                Err(e) => return fail(&format!("{}: not valid JSON: {e}", file.display())),
            };
            let n = rows_label(&rows);
            let live = match host_api(&ws) {
                Some(c) => {
                    if let Err(e) = rt().block_on(c.data_set(server.clone(), dataset.clone(), rows))
                    {
                        return control_failed(e);
                    }
                    true
                }
                None => {
                    let store = match open_store(&ws) {
                        Ok(s) => s,
                        Err(c) => return c,
                    };
                    let snap = store.snapshot();
                    let Some((_, s)) = snap.servers.iter().find(|(_, s)| {
                        s.id == server
                            || s.name == server
                            || fauxfield_store::slugify(&s.name) == server
                    }) else {
                        return fail(&format!("no server `{server}`"));
                    };
                    if !s.datasets.iter().any(|d| d.name == dataset) {
                        return fail(&format!("no dataset `{dataset}`"));
                    }
                    let spec = fauxfield_store::DatasetSpec {
                        name: dataset.clone(),
                        seed_json: Some(serde_json::to_string_pretty(&rows).unwrap_or_default()),
                        ..Default::default()
                    };
                    if let Err(e) = store
                        .upsert_dataset(&s.id, &spec)
                        .and_then(|_| store.flush().map(|_| ()))
                    {
                        return fail(&e.to_string());
                    }
                    false
                }
            };
            if ws.json {
                print_json(&json!({"dataset": dataset, "rows": n, "live": live}));
            } else {
                println!(
                    "Imported {n} row(s) into {dataset}{}",
                    if live {
                        ""
                    } else {
                        " (seed file; no host running)"
                    }
                );
            }
            EXIT_OK
        }
    }
}

fn show_or_export(server: String, dataset: String, ws: WsArgs, out: Option<PathBuf>) -> i32 {
    let rows = match host_api(&ws) {
        Some(c) => match rt().block_on(c.data_show(server, dataset)) {
            Ok(r) => r,
            Err(e) => return control_failed(e),
        },
        None => {
            let e = match offline(&ws) {
                Ok(e) => e,
                Err(c) => return c,
            };
            match e.dataset_rows(&server, &dataset) {
                Ok(r) => r,
                Err(e) => return fail(&e.to_string()),
            }
        }
    };
    let text = serde_json::to_string_pretty(&rows).unwrap_or_default();
    match out.filter(|p| p.as_os_str() != "-") {
        Some(p) => {
            if let Err(e) = std::fs::write(&p, &text) {
                return fail(&format!("{}: {e}", p.display()));
            }
            if ws.json {
                print_json(&json!({"file": p.display().to_string(), "rows": rows_label(&rows)}));
            } else {
                println!("Wrote {} row(s) to {}", rows_label(&rows), p.display());
            }
        }
        None => println!("{text}"),
    }
    EXIT_OK
}

fn info_json(d: &DatasetInfo) -> Value {
    json!({
        "server": d.server, "name": d.name, "rows": d.rows, "key": d.key,
        "id_strategy": d.id_strategy, "persist": d.persist, "seed": d.seed,
        "seed_template": d.seed_template, "error": d.error,
    })
}
