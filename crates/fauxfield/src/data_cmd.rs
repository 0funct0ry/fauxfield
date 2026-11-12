//! `data list|show|reset|export|import` (M8), `create|delete|rename|refs` (M10.01). Every command has a stable `--json`.
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
    /// Write a Dataset's rows as JSON, CSV or Parquet to stdout (JSON, CSV) or a file.
    Export {
        server: String,
        dataset: String,
        /// `json` (default), `csv` or `parquet`; inferred from --out's extension when omitted.
        #[arg(long)]
        format: Option<String>,
        /// Write to this file instead of stdout.
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Import a JSON, CSV or Parquet file into a Dataset: live on a running host, else the seed file.
    Import {
        server: String,
        dataset: String,
        file: PathBuf,
        /// `json`, `csv` or `parquet`; inferred from the extension when omitted.
        #[arg(long)]
        format: Option<String>,
        /// Add to the existing rows instead of replacing them.
        #[arg(long)]
        append: bool,
        /// Key column (default `id` when present).
        #[arg(long)]
        key: Option<String>,
        /// CSV delimiter (single character; detected when omitted).
        #[arg(long)]
        delimiter: Option<char>,
        /// CSV column type override `name:type` (repeatable; int, float, bool, string).
        #[arg(long = "column-type", value_name = "NAME:TYPE")]
        column_types: Vec<String>,
        /// Parquet decimals as `number` (default) or `string`.
        #[arg(long, default_value = "number")]
        decimal_as: String,
        /// Warn above this many rows.
        #[arg(long, default_value_t = fauxfield_engine::import::DEFAULT_SOFT_LIMIT)]
        soft_limit: usize,
        /// Refuse above this many rows.
        #[arg(long, default_value_t = fauxfield_engine::import::DEFAULT_HARD_LIMIT)]
        hard_limit: usize,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Create an empty Dataset (no seed, no Route). Edits the server file.
    Create {
        server: String,
        dataset: String,
        /// Row key field (default `id`).
        #[arg(long)]
        key: Option<String>,
        /// `uuid`, `increment` or `ulid`.
        #[arg(long)]
        id_strategy: Option<String>,
        /// Persist rows to `.fauxfield/state`.
        #[arg(long)]
        persist: bool,
        /// Declare a column `name:type` (repeatable; types string, int, float, bool, datetime, json).
        #[arg(long = "column", value_name = "NAME:TYPE")]
        columns: Vec<String>,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Delete a Dataset; blocked while Routes or templates use it unless --force.
    Delete {
        server: String,
        dataset: String,
        /// Delete even though the Dataset is still referenced.
        #[arg(long)]
        force: bool,
        /// Also delete the seed file.
        #[arg(long)]
        remove_seed: bool,
        /// Also delete the persisted state.
        #[arg(long)]
        remove_state: bool,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Rename a Dataset, rewriting every crud Route that references it.
    Rename {
        server: String,
        dataset: String,
        new_name: String,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Generate a Dataset's rows from its `generator` seed; print them, or `--bake` them to
    /// `data/<dataset>.json` (the generator is replaced by that static seed).
    Generate {
        server: String,
        dataset: String,
        /// Row count (default: the generator's `rows`).
        #[arg(long)]
        rows: Option<u32>,
        /// Seed (default: the Server `fake_seed`; unset means random).
        #[arg(long)]
        seed: Option<u64>,
        /// Write the rows to `data/<dataset>.json` and use them as the static seed.
        #[arg(long)]
        bake: bool,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// List everything that uses a Dataset.
    Refs {
        server: String,
        dataset: String,
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

/// Run a workspace-file edit on one Server and flush. Returns the Server id.
fn with_server<T>(
    ws: &WsArgs,
    server: &str,
    f: impl FnOnce(&fauxfield_store::Store, &str) -> Result<T, fauxfield_store::StoreError>,
) -> Result<T, i32> {
    let store = open_store(ws)?;
    let snap = store.snapshot();
    let Some((_, s)) = snap.servers.iter().find(|(_, s)| {
        s.id == server || s.name == server || fauxfield_store::slugify(&s.name) == server
    }) else {
        return Err(fail(&format!("no server `{server}`")));
    };
    let out = f(&store, &s.id).map_err(|e| fail(&e.to_string()))?;
    store.flush().map_err(|e| fail(&e.to_string()))?;
    Ok(out)
}

pub fn data(action: DataAction) -> i32 {
    match action {
        DataAction::Create {
            server,
            dataset,
            key,
            id_strategy,
            persist,
            columns,
            ws,
        } => {
            let cols: Vec<fauxfield_model::DatasetColumn> = columns
                .iter()
                .map(|c| {
                    let (n, t) = c.split_once(':').unwrap_or((c.as_str(), "string"));
                    fauxfield_model::DatasetColumn {
                        name: n.trim().into(),
                        ty: t.trim().into(),
                    }
                })
                .collect();
            if let Err(c) = with_server(&ws, &server, |st, sid| {
                st.create_dataset_with_columns(
                    sid,
                    &dataset,
                    key.as_deref(),
                    id_strategy.as_deref(),
                    persist.then_some(true),
                    &cols,
                )
            }) {
                return c;
            }
            if ws.json {
                let mut v = json!({"created": dataset, "server": server, "rows": 0, "live": false});
                if !cols.is_empty() {
                    v["columns"] = json!(cols);
                }
                print_json(&v);
            } else {
                println!("Created empty dataset {dataset}");
            }
            EXIT_OK
        }
        DataAction::Delete {
            server,
            dataset,
            force,
            remove_seed,
            remove_state,
            ws,
        } => {
            let opts = fauxfield_store::DeleteOpts {
                force,
                remove_seed,
                remove_state,
            };
            let refs = match with_server(&ws, &server, |st, sid| {
                st.delete_dataset_checked(sid, &dataset, &opts)
            }) {
                Ok(r) => r,
                Err(c) => return c,
            };
            if ws.json {
                print_json(&json!({
                    "deleted": dataset, "server": server, "live": false,
                    "removed_seed": remove_seed, "removed_state": remove_state,
                    "references": refs,
                }));
            } else {
                println!("Deleted dataset {dataset}");
            }
            EXIT_OK
        }
        DataAction::Rename {
            server,
            dataset,
            new_name,
            ws,
        } => {
            let rep = match with_server(&ws, &server, |st, sid| {
                st.rename_dataset(sid, &dataset, &new_name)
            }) {
                Ok(r) => r,
                Err(c) => return c,
            };
            if ws.json {
                print_json(&json!({
                    "renamed": {"from": dataset, "to": new_name}, "server": server,
                    "live": false, "report": rep,
                }));
            } else {
                println!(
                    "Renamed {dataset} to {new_name} ({} route(s) updated)",
                    rep.rewritten_routes.len()
                );
                for w in &rep.free_text_warnings {
                    eprintln!(
                        "warning: {} uses `data.{dataset}`; update it by hand",
                        w.location
                    );
                }
            }
            EXIT_OK
        }
        DataAction::Generate {
            server,
            dataset,
            rows,
            seed,
            bake,
            ws,
        } => generate(&server, &dataset, rows, seed, bake, &ws),
        DataAction::Refs {
            server,
            dataset,
            ws,
        } => {
            let refs =
                match with_server(&ws, &server, |st, sid| st.dataset_references(sid, &dataset)) {
                    Ok(r) => r,
                    Err(c) => return c,
                };
            if ws.json {
                print_json(&json!({"server": server, "dataset": dataset, "references": refs}));
            } else if refs.is_unused() {
                println!("{dataset} is not used.");
            } else {
                for r in &refs.routes {
                    println!("route {} {} {}", r.id, r.method, r.path);
                }
                for a in &refs.actions {
                    println!(
                        "action {} {} step {} ({})",
                        a.route_id, a.response_id, a.step, a.op
                    );
                }
                for r in &refs.rules {
                    println!(
                        "rule {} {} #{} {} {}",
                        r.route_id, r.response_id, r.rule, r.target, r.key
                    );
                }
                for t in &refs.free_text {
                    println!("template {}", t.location);
                }
            }
            EXIT_OK
        }
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
        } => show_or_export(server, dataset, ws, None, None),
        DataAction::Export {
            server,
            dataset,
            format,
            out,
            ws,
        } => show_or_export(
            server,
            dataset,
            ws,
            Some(out.unwrap_or_else(|| PathBuf::from("-"))),
            format,
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
            format,
            append,
            key,
            delimiter,
            column_types,
            decimal_as,
            soft_limit,
            hard_limit,
            ws,
        } => {
            use fauxfield_engine::import as imp;
            let mut opts = imp::ImportOptions {
                key: key.clone(),
                delimiter: delimiter.map(|c| c as u8),
                decimal_as: if decimal_as == "string" {
                    imp::DecimalAs::String
                } else {
                    imp::DecimalAs::Number
                },
                soft_limit,
                hard_limit,
                ..Default::default()
            };
            if let Some(f) = &format {
                match imp::Format::parse(f) {
                    Some(f) => opts.format = Some(f),
                    None => {
                        return fail(&format!("unknown format `{f}`; use json, csv or parquet"));
                    }
                }
            }
            for ct in &column_types {
                match ct.split_once(':') {
                    Some((n, t)) => {
                        opts.column_types.insert(n.into(), t.into());
                    }
                    None => return fail(&format!("--column-type `{ct}` must be NAME:TYPE")),
                }
            }
            let imported = match imp::read(&file, &opts) {
                Ok(r) => r,
                Err(e) => return fail(&e),
            };
            let warnings = imported.warnings.clone();
            let mut rows = imported.seed_value();
            if append && !imported.singleton {
                let existing = match host_api(&ws) {
                    Some(c) => rt()
                        .block_on(c.data_show(server.clone(), dataset.clone()))
                        .ok(),
                    None => offline(&ws)
                        .ok()
                        .and_then(|e| e.dataset_rows(&server, &dataset).ok()),
                };
                rows = imp::append_rows(&existing.unwrap_or(Value::Null), &imported.rows);
            }
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
                        key: key.clone(),
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
                print_json(
                    &json!({"dataset": dataset, "rows": n, "live": live, "format": imported.format.name(), "warnings": warnings}),
                );
            } else {
                println!(
                    "Imported {n} row(s) into {dataset}{}",
                    if live {
                        ""
                    } else {
                        " (seed file; no host running)"
                    }
                );
                for w in &warnings {
                    eprintln!("warning: {w}");
                }
            }
            EXIT_OK
        }
    }
}

fn show_or_export(
    server: String,
    dataset: String,
    ws: WsArgs,
    out: Option<PathBuf>,
    format: Option<String>,
) -> i32 {
    use fauxfield_engine::import::{Format, export};
    let fmt = match format.as_deref() {
        Some(f) => match Format::parse(f) {
            Some(f) => f,
            None => return fail(&format!("unknown format `{f}`; use json, csv or parquet")),
        },
        None => out
            .as_deref()
            .and_then(Format::detect)
            .unwrap_or(Format::Json),
    };
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
    let bytes = match export(&rows, fmt) {
        Ok(b) => b,
        Err(e) => return fail(&e),
    };
    let target = out.filter(|p| p.as_os_str() != "-");
    if fmt == Format::Parquet && target.is_none() {
        return fail("Parquet is binary; pass --out FILE");
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    match target {
        Some(p) => {
            if let Err(e) = std::fs::write(&p, &bytes) {
                return fail(&format!("{}: {e}", p.display()));
            }
            if ws.json {
                print_json(
                    &json!({"file": p.display().to_string(), "format": fmt.name(), "rows": rows_label(&rows)}),
                );
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

/// `data generate`: rows from the Dataset's generator, honouring `ref` load order.
fn generate(
    server: &str,
    dataset: &str,
    rows: Option<u32>,
    seed: Option<u64>,
    bake: bool,
    ws: &WsArgs,
) -> i32 {
    let store = match open_store(ws) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let root = store.root();
    let snap = store.snapshot();
    let Some((_, s)) = snap.servers.iter().find(|(_, s)| {
        s.id == server || s.name == server || fauxfield_store::slugify(&s.name) == server
    }) else {
        return fail(&format!("no server `{server}`"));
    };
    let Some(target) = s.datasets.iter().find(|d| d.name == dataset) else {
        return fail(&format!("no dataset `{dataset}`"));
    };
    if target.generator.is_none() {
        return fail(&format!("dataset `{dataset}` has no generator"));
    }
    let seed = seed.or(s.fake_seed);
    let result =
        match fauxfield_engine::generate::generate_dataset(&s.datasets, dataset, rows, seed, &root)
        {
            Ok(v) => v,
            Err(e) => return fail(&e),
        };
    let count = rows_label(&result);
    let mut baked = None;
    if bake {
        let text = serde_json::to_string_pretty(&result).unwrap_or_default();
        let spec = fauxfield_store::DatasetSpec {
            name: dataset.into(),
            seed_json: Some(text),
            ..Default::default()
        };
        if let Err(e) = store
            .upsert_dataset(&s.id, &spec)
            .and_then(|_| store.flush())
        {
            return fail(&e.to_string());
        }
        baked = Some(format!("data/{dataset}.json"));
    }
    if ws.json {
        print_json(&json!({
            "dataset": dataset, "server": server, "rows": count, "seed": seed,
            "baked": baked, "data": if bake { Value::Null } else { result },
        }));
    } else if let Some(f) = baked {
        println!("Baked {count} rows to {f}");
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&result).unwrap_or_default()
        );
    }
    EXIT_OK
}
