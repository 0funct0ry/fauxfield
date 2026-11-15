//! `env list|use|add` and `var get|set|unset` (M7). Every command has a stable `--json`.
//! Offline they edit workspace files through the store; with a running host they also act
//! on it live (the host hot-reloads file edits, runtime variables live only in the host).

use clap::Subcommand;
use fauxfield_control::{ControlApi, read_host};
use fauxfield_model::vars::{Scope, is_secret_key};
use serde_json::json;

use crate::cli::{EXIT_OK, EXIT_RUNTIME, EXIT_USAGE};
use crate::host_cmd::{attach, control_failed, root_of, rt};
use crate::workspace_cmd::{WsArgs, open_store, print_json};

#[derive(Subcommand, Debug)]
pub enum EnvAction {
    /// List Environments and the active one.
    List(WsArgs),
    /// Make an Environment the default and, on a running host, switch it live.
    Use {
        name: String,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Create an empty Environment (environments/<name>.toml).
    Add {
        name: String,
        #[command(flatten)]
        ws: WsArgs,
    },
}

#[derive(clap::Args, Debug)]
pub struct VarTarget {
    /// Scope: global, environment, server, route or runtime. `get` without it shows the
    /// effective value.
    #[arg(long, value_name = "SCOPE")]
    pub scope: Option<String>,
    /// Environment name (scope environment; for `get`, the Environment to resolve in).
    #[arg(long = "env", value_name = "NAME")]
    pub env: Option<String>,
    /// Server ID, name or slug (scope server; for `get`, the Server to resolve in).
    #[arg(long, value_name = "SERVER")]
    pub server: Option<String>,
    /// Route ID (scope route).
    #[arg(long, value_name = "ROUTE")]
    pub route: Option<String>,
    #[command(flatten)]
    pub ws: WsArgs,
}

#[derive(Subcommand, Debug)]
pub enum VarAction {
    /// Print a variable.
    Get {
        key: String,
        #[command(flatten)]
        t: VarTarget,
    },
    /// Set a variable.
    Set {
        key: String,
        value: String,
        #[command(flatten)]
        t: VarTarget,
    },
    /// Remove a variable.
    Unset {
        key: String,
        #[command(flatten)]
        t: VarTarget,
    },
}

fn fail(msg: &str) -> i32 {
    crate::err!("{msg}");
    EXIT_RUNTIME
}

fn host_api(ws: &WsArgs) -> Option<fauxfield_control::RemoteControl> {
    read_host(&root_of(ws)).map(|i| fauxfield_control::RemoteControl::new(&i.socket))
}

pub fn env(action: EnvAction) -> i32 {
    match action {
        EnvAction::List(ws) => {
            let store = match open_store(&ws) {
                Ok(s) => s,
                Err(c) => return c,
            };
            let snap = store.snapshot();
            let default = snap.manifest.default_environment.clone();
            let active = host_api(&ws)
                .and_then(|c| rt().block_on(c.env_active()).ok())
                .filter(|a| !a.is_empty())
                .or_else(|| default.clone());
            let rows: Vec<_> = snap
                .environments
                .iter()
                .map(|(f, e)| json!({"name": e.name, "file": f, "variables": e.variables.len(), "dotenv": e.dotenv, "active": Some(&e.name) == active.as_ref()}))
                .collect();
            if ws.json {
                print_json(&json!({"default": default, "active": active, "environments": rows}));
            } else if rows.is_empty() {
                println!("No environments. Create one with `fauxfield env add <name>`.");
            } else {
                for r in &rows {
                    println!(
                        "{} {}  {} variable(s)",
                        if r["active"] == true { "*" } else { " " },
                        r["name"].as_str().unwrap(),
                        r["variables"]
                    );
                }
            }
            EXIT_OK
        }
        EnvAction::Add { name, ws } => {
            let store = match open_store(&ws) {
                Ok(s) => s,
                Err(c) => return c,
            };
            let file = match store.create_environment(&name) {
                Ok(f) => f,
                Err(e) => return fail(&e.to_string()),
            };
            if let Err(e) = store.flush() {
                return fail(&e.to_string());
            }
            if ws.json {
                print_json(&json!({"name": name, "file": file}));
            } else {
                println!("Created {file}");
            }
            EXIT_OK
        }
        EnvAction::Use { name, ws } => {
            let store = match open_store(&ws) {
                Ok(s) => s,
                Err(c) => return c,
            };
            if store.snapshot().environment(&name).is_none() {
                return fail(&format!("no environment named `{name}`"));
            }
            if let Err(e) = store
                .set_default_environment(Some(&name))
                .and_then(|_| store.flush().map(|_| ()))
            {
                return fail(&e.to_string());
            }
            let live = match host_api(&ws) {
                Some(c) => match rt().block_on(c.env_use(name.clone())) {
                    Ok(_) => true,
                    Err(e) => return control_failed(e),
                },
                None => false,
            };
            if ws.json {
                print_json(&json!({"active": name, "live": live}));
            } else {
                println!(
                    "Active environment: {name}{}",
                    if live {
                        " (switched on the running host)"
                    } else {
                        ""
                    }
                );
            }
            EXIT_OK
        }
    }
}

fn scope_of(t: &VarTarget, need: bool) -> Result<Option<Scope>, i32> {
    match t.scope.as_deref() {
        None if need => {
            crate::err!("--scope is required (global, environment, server, route, runtime)");
            Err(EXIT_USAGE)
        }
        None => Ok(None),
        Some(s) => Scope::parse(s).map(Some).ok_or_else(|| {
            crate::err!("unknown scope `{s}` (global, environment, server, route, runtime)");
            EXIT_USAGE
        }),
    }
}

fn server_id(snap: &fauxfield_store::Snapshot, key: &str) -> Option<String> {
    snap.servers
        .iter()
        .map(|(_, s)| s)
        .find(|s| s.id == key || s.name == key || fauxfield_store::slugify(&s.name) == key)
        .map(|s| s.id.clone())
}

pub fn var(action: VarAction) -> i32 {
    match action {
        VarAction::Get { key, t } => {
            let scope = match scope_of(&t, false) {
                Ok(s) => s,
                Err(c) => return c,
            };
            let store = match open_store(&t.ws) {
                Ok(s) => s,
                Err(c) => return c,
            };
            let snap = store.snapshot();
            let host = host_api(&t.ws);
            let env = t.env.clone().or_else(|| {
                host.as_ref()
                    .and_then(|c| rt().block_on(c.env_active()).ok())
                    .filter(|a| !a.is_empty())
                    .or_else(|| snap.manifest.default_environment.clone())
            });
            let sid = t.server.as_deref().and_then(|k| server_id(&snap, k));
            if t.server.is_some() && sid.is_none() {
                return fail("unknown server");
            }
            // With a host the engine's answer includes runtime variables.
            let live = match (
                host.as_ref(),
                sid.clone()
                    .or_else(|| snap.servers.first().map(|(_, s)| s.id.clone())),
            ) {
                (Some(c), Some(id)) => rt().block_on(c.vars(id, t.route.clone())).ok(),
                _ => None,
            };
            let all = live.unwrap_or_else(|| {
                snap.vars_in_scope(env.as_deref(), sid.as_deref(), t.route.as_deref(), None)
            });
            let Some(v) = all
                .into_iter()
                .find(|v| v.key == key && scope.is_none_or(|s| s == v.scope))
            else {
                return fail(&format!("variable `{key}` is not defined"));
            };
            if t.ws.json {
                print_json(
                    &json!({"key": v.key, "value": v.value, "scope": v.scope, "source": v.source, "secret": is_secret_key(&v.key)}),
                );
            } else {
                println!("{}", v.value);
            }
            EXIT_OK
        }
        VarAction::Set { key, value, t } => write_var(&key, Some(&value), &t),
        VarAction::Unset { key, t } => write_var(&key, None, &t),
    }
}

fn write_var(key: &str, val: Option<&str>, t: &VarTarget) -> i32 {
    let scope = match scope_of(t, true) {
        Ok(Some(s)) => s,
        Ok(None) => return EXIT_USAGE,
        Err(c) => return c,
    };
    if scope == Scope::Runtime {
        let (_, c) = match attach(&root_of(&t.ws)) {
            Ok(x) => x,
            Err(c) => return c,
        };
        if let Err(e) = rt().block_on(c.var_runtime(key.to_string(), val.map(str::to_string))) {
            return control_failed(e);
        }
    } else {
        let store = match open_store(&t.ws) {
            Ok(s) => s,
            Err(c) => return c,
        };
        let snap = store.snapshot();
        let owner = match scope {
            Scope::Global | Scope::Runtime => None,
            Scope::Environment => t.env.clone().or(snap.manifest.default_environment.clone()),
            Scope::Server => t.server.as_deref().and_then(|k| server_id(&snap, k)),
            Scope::Route => t.route.clone(),
        };
        if owner.is_none() && scope != Scope::Global {
            let need = match scope {
                Scope::Environment => "--env <name>",
                Scope::Server => "a valid --server",
                _ => "--route <id>",
            };
            return fail(&format!("--scope {} needs {need}", scope.as_str()));
        }
        if let Err(e) = store
            .set_variable(scope, owner.as_deref(), key, val)
            .and_then(|_| store.flush().map(|_| ()))
        {
            return fail(&e.to_string());
        }
    }
    if t.ws.json {
        print_json(
            &json!({"key": key, "scope": scope, "value": val.filter(|_| !is_secret_key(key)), "removed": val.is_none()}),
        );
    } else if val.is_some() {
        println!("Set {key} ({})", scope.as_str());
    } else {
        println!("Removed {key} ({})", scope.as_str());
    }
    EXIT_OK
}
