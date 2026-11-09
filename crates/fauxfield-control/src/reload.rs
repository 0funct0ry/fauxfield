use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use fauxfield_engine::{Engine, VarContext};
use fauxfield_model::{Server, Severity};
use fauxfield_store::{Event, Store};

/// Apply the store's servers to the engine now and again on every change (hot reload,
/// SPEC §3.1). Runs on a detached thread that ends when the store is dropped. The caller
/// keeps `Store::start_background` alive so external edits are noticed.
///
/// A server whose file has errors (or no longer parses) is never applied: the last good
/// definition of that file keeps serving, and the errors show up in Problems (SPEC §20.13).
pub fn attach_store(engine: &Engine, store: &Store) {
    attach_store_with_env(engine, store, None)
}

/// [`attach_store`] with an initial active Environment (`run --env`). Later reloads keep the
/// current active Environment and the runtime variables, rebuilding everything else.
pub fn attach_store_with_env(engine: &Engine, store: &Store, initial_env: Option<String>) {
    let initial = Arc::new(Mutex::new(initial_env));
    let last: Arc<Mutex<HashMap<String, Server>>> = Arc::default();
    let apply = move |engine: &Engine, store: &Store| {
        let snap = store.snapshot();
        let bad: std::collections::HashSet<&str> = snap
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.file.as_str())
            .collect();
        let mut last = last.lock().unwrap();
        let mut servers = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (file, s) in &snap.servers {
            seen.insert(file.clone());
            if bad.contains(file.as_str()) {
                if let Some(prev) = last.get(file) {
                    servers.push(prev.clone());
                }
            } else {
                last.insert(file.clone(), s.clone());
                servers.push(s.clone());
            }
        }
        // A file that stopped parsing keeps its previous definition; a deleted file is dropped.
        let mut gone = Vec::new();
        for (file, prev) in last.iter() {
            if !seen.contains(file) {
                if bad.contains(file.as_str()) {
                    servers.push(prev.clone());
                } else {
                    gone.push(file.clone());
                }
            }
        }
        for f in gone {
            last.remove(&f);
        }
        drop(last);
        let cur = engine.var_context();
        let active = if !cur.active_env.is_empty() {
            Some(cur.active_env.clone())
        } else {
            initial.lock().unwrap().take()
        };
        let envs: Vec<_> = snap.environments.iter().map(|(_, e)| e.clone()).collect();
        let vc = VarContext::from_workspace(
            &snap.manifest,
            &envs,
            |n| snap.dotenv(n),
            active.as_deref(),
            cur.runtime,
        );
        engine.set_var_context(vc);
        engine.apply(servers);
    };
    apply(engine, store);
    let rx = store.subscribe();
    let (engine, store) = (engine.clone(), store.clone());
    std::thread::spawn(move || {
        while let Ok(ev) = rx.recv() {
            if matches!(ev, Event::Changed) {
                apply(&engine, &store);
            }
        }
    });
}
