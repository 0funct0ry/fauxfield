use fauxfield_engine::Engine;
use fauxfield_store::{Event, Store};

/// Apply the store's servers to the engine now and again on every change (hot reload,
/// SPEC §3.1). Runs on a detached thread that ends when the store is dropped. The caller
/// keeps `Store::start_background` alive so external edits are noticed.
pub fn attach_store(engine: &Engine, store: &Store) {
    let apply = |engine: &Engine, store: &Store| {
        let servers = store
            .snapshot()
            .servers
            .into_iter()
            .map(|(_, s)| s)
            .collect();
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
