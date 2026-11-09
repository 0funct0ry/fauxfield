//! Variable context held by the engine (SPEC §8.1): globals, Environments, the active
//! Environment name, and runtime variables.

use std::collections::BTreeMap;

use fauxfield_model::vars::{Layers, ResolvedVar, resolve};
use fauxfield_model::{Environment, Manifest, Server};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Default)]
pub struct EnvData {
    pub variables: BTreeMap<String, toml::Value>,
    pub dotenv: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct VarContext {
    pub active_env: String,
    pub globals: BTreeMap<String, toml::Value>,
    pub envs: BTreeMap<String, EnvData>,
    pub runtime: BTreeMap<String, String>,
}

impl VarContext {
    /// Build from a loaded workspace. `dotenv` supplies the parsed dotenv per Environment name.
    pub fn from_workspace(
        manifest: &Manifest,
        envs: &[Environment],
        dotenv: impl Fn(&str) -> BTreeMap<String, String>,
        active: Option<&str>,
        runtime: BTreeMap<String, String>,
    ) -> VarContext {
        let active_env = active
            .map(str::to_string)
            .or_else(|| manifest.default_environment.clone())
            .unwrap_or_default();
        VarContext {
            active_env,
            globals: manifest.variables.clone(),
            envs: envs
                .iter()
                .map(|e| {
                    (
                        e.name.clone(),
                        EnvData {
                            variables: e.variables.clone(),
                            dotenv: dotenv(&e.name),
                        },
                    )
                })
                .collect(),
            runtime,
        }
    }

    /// Resolved variables for a Server (and optionally one Route's variables).
    pub fn resolve(&self, server: &Server, route_id: Option<&str>) -> Vec<ResolvedVar> {
        let env = self.envs.get(&self.active_env);
        let route = route_id.and_then(|id| server.routes.iter().find(|r| r.id == id));
        let layers = Layers {
            global: Some(&self.globals),
            environment: env.map(|e| &e.variables),
            server: Some(&server.variables),
            route: route.map(|r| &r.variables),
            runtime: Some(&self.runtime),
            dotenv: env.map(|e| e.dotenv.clone()).unwrap_or_default(),
        };
        resolve(&layers, &|n| std::env::var(n).ok())
    }

    /// `vars` object for the template context.
    pub fn vars_json(&self, server: &Server, route_id: Option<&str>) -> Value {
        let m: Map<String, Value> = self
            .resolve(server, route_id)
            .into_iter()
            .map(|v| (v.key, Value::String(v.value)))
            .collect();
        Value::Object(m)
    }

    pub fn state_json(&self) -> Map<String, Value> {
        self.runtime
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect()
    }
}
