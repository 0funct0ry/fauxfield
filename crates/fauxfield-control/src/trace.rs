//! Startup trace (M17.04, SPEC §15): `FAUXFIELD_TRACE=startup` records named spans and
//! writes them as Chrome trace JSON (viewable in Perfetto) to `target/trace/startup.json`
//! (or `FAUXFIELD_TRACE_FILE`). Unset, `span` is one relaxed atomic load: no allocation, no lock.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use serde_json::{Value, json};

static STATE: AtomicU8 = AtomicU8::new(0); // 0 unknown, 1 off, 2 on
static T0: OnceLock<Instant> = OnceLock::new();
static SPANS: Mutex<Vec<(&'static str, u64, u64)>> = Mutex::new(Vec::new());

fn on() -> bool {
    match STATE.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            let v = std::env::var("FAUXFIELD_TRACE")
                .is_ok_and(|v| v.split(',').any(|p| p == "startup"));
            STATE.store(if v { 2 } else { 1 }, Ordering::Relaxed);
            if v {
                T0.get_or_init(Instant::now);
            }
            v
        }
    }
}

/// Process start reference; call first thing in `main` so spans are relative to it.
pub fn init() {
    on();
}

pub struct Span(Option<(&'static str, Instant)>);

/// Time the scope until the guard drops.
pub fn span(name: &'static str) -> Span {
    Span(on().then(|| (name, Instant::now())))
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some((name, start)) = self.0.take() {
            let t0 = *T0.get_or_init(Instant::now);
            let ts = start.saturating_duration_since(t0).as_micros() as u64;
            let dur = start.elapsed().as_micros() as u64;
            SPANS.lock().unwrap().push((name, ts, dur));
        }
    }
}

/// `{enabled, total_ms, spans: [{name, ms}]}` for `ui.state`.
pub fn summary() -> Value {
    if !on() {
        return json!({ "enabled": false });
    }
    let spans = SPANS.lock().unwrap();
    let end = spans.iter().map(|s| s.1 + s.2).max().unwrap_or(0);
    json!({
        "enabled": true,
        "total_ms": end as f64 / 1000.0,
        "spans": spans.iter().map(|s| json!({"name": s.0, "ms": s.2 as f64 / 1000.0})).collect::<Vec<_>>(),
    })
}

/// Write the Chrome trace file; returns its path. No-op (None) when tracing is off.
pub fn flush() -> Option<std::path::PathBuf> {
    if !on() {
        return None;
    }
    let path = std::env::var_os("FAUXFIELD_TRACE_FILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "target/trace/startup.json".into());
    let events: Vec<Value> = SPANS
        .lock()
        .unwrap()
        .iter()
        .map(|(n, ts, d)| json!({"name": n, "ph": "X", "ts": ts, "dur": d, "pid": 1, "tid": 1}))
        .collect();
    std::fs::create_dir_all(path.parent()?).ok()?;
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({ "traceEvents": events })).ok()?,
    )
    .ok()?;
    Some(path)
}
