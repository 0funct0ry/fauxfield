//! Tera templating (SPEC §8): context, functions, filters, seeded RNG, limits.
//!
//! One long-lived `Tera` holds every compiled template keyed by a content hash (compile cache,
//! §8.4). Per-render state (RNG, workspace root, deadline, sequences) lives in a thread-local,
//! because a render is synchronous on one thread.

mod fake;
mod funcs;
mod sugar;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use std::time::{Duration, Instant};

use rand::SeedableRng;
use rand::rngs::ChaCha8Rng;
use sha2::{Digest, Sha256};

pub use fake::{FAKE_KINDS, resolve_kind};
pub use funcs::{FILTERS, FUNCTIONS};

pub const CPU_BUDGET: Duration = Duration::from_millis(50);
pub const OUTPUT_CAP: usize = 10 * 1024 * 1024;
const CACHE_CAP: usize = 4096;

/// Shared `seq(name=)` counters (one map per Server runtime).
pub type Sequences = Arc<Mutex<HashMap<String, u64>>>;

pub struct Input<'a> {
    /// `request`, `vars`, `server`, `env`, `state`, ... (SPEC §8.1).
    pub context: serde_json::Value,
    pub root: &'a Path,
    pub seed: Option<u64>,
    pub route_id: &'a str,
    pub counter: u64,
    pub seq: Option<Sequences>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct RenderError {
    pub message: String,
    pub line: Option<usize>,
    pub col: Option<usize>,
}

pub(crate) struct State {
    pub rng: ChaCha8Rng,
    pub seeded: bool,
    pub root: PathBuf,
    pub deadline: Instant,
    pub seq: Option<Sequences>,
}

thread_local! {
    pub(crate) static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

pub(crate) fn with_state<T>(f: impl FnOnce(&mut State) -> T) -> tera::Result<T> {
    STATE.with(|s| match s.borrow_mut().as_mut() {
        Some(st) => {
            if Instant::now() > st.deadline {
                return Err(tera::Error::msg("template exceeded the 50 ms CPU budget"));
            }
            Ok(f(st))
        }
        None => Err(tera::Error::msg("no render in progress")),
    })
}

static TERA: LazyLock<RwLock<tera::Tera>> = LazyLock::new(|| RwLock::new(new_tera()));

fn new_tera() -> tera::Tera {
    let mut t = tera::Tera::default();
    t.autoescape_on(vec![]);
    funcs::register(&mut t);
    t
}

fn key(src: &str) -> String {
    let h = Sha256::digest(src.as_bytes());
    h.iter().take(12).map(|b| format!("{b:02x}")).collect()
}

/// Derive the per-render RNG seed from (seed, route id, request counter) (§8.3).
pub fn derive_seed(seed: u64, route_id: &str, counter: u64) -> u64 {
    let mut h = Sha256::new();
    h.update(seed.to_le_bytes());
    h.update(route_id.as_bytes());
    h.update(counter.to_le_bytes());
    let d = h.finalize();
    u64::from_le_bytes(d[..8].try_into().unwrap())
}

fn has_syntax(src: &str) -> bool {
    src.contains("{{") || src.contains("{%") || src.contains("{#")
}

fn chain(e: &tera::Error) -> String {
    let mut out = e.to_string();
    let mut cur: Option<&dyn std::error::Error> = std::error::Error::source(e);
    while let Some(c) = cur {
        out.push_str(": ");
        out.push_str(&c.to_string());
        cur = c.source();
    }
    out
}

fn locate(msg: &str) -> (Option<usize>, Option<usize>) {
    // pest errors contain " --> LINE:COL".
    if let Some(i) = msg.find("--> ") {
        let rest = &msg[i + 4..];
        let end = rest
            .find(|c: char| !c.is_ascii_digit() && c != ':')
            .unwrap_or(rest.len());
        let mut it = rest[..end].split(':');
        if let (Some(l), Some(c)) = (it.next(), it.next()) {
            return (l.parse().ok(), c.parse().ok());
        }
    }
    (None, None)
}

fn err(e: &tera::Error) -> RenderError {
    let message = chain(e);
    let (line, col) = locate(&message);
    RenderError { message, line, col }
}

/// Syntax-only check (no functions needed); used for inline diagnostics.
pub fn check_syntax(src: &str) -> Result<(), RenderError> {
    if !has_syntax(src) {
        return Ok(());
    }
    tera::Tera::default()
        .add_raw_template("t", &sugar::desugar(src))
        .map_err(|e| err(&e))
}

/// Render `src`. Plain text without Tera syntax is returned untouched.
pub fn render(src: &str, input: &Input<'_>) -> Result<String, RenderError> {
    if !has_syntax(src) {
        return Ok(src.to_string());
    }
    let name = key(src);
    {
        let have = TERA.read().unwrap().get_template_names().any(|n| n == name);
        if !have {
            let mut w = TERA.write().unwrap();
            if w.get_template_names().count() >= CACHE_CAP {
                *w = new_tera();
            }
            w.add_raw_template(&name, &sugar::desugar(src))
                .map_err(|e| err(&e))?;
        }
    }
    let ctx = tera::Context::from_value(input.context.clone()).map_err(|e| err(&e))?;
    let (rng, seeded) = match input.seed {
        Some(s) => (
            ChaCha8Rng::seed_from_u64(derive_seed(s, input.route_id, input.counter)),
            true,
        ),
        None => {
            let mut b = [0u8; 8];
            let _ = getrandom::fill(&mut b);
            (ChaCha8Rng::seed_from_u64(u64::from_le_bytes(b)), false)
        }
    };
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            rng,
            seeded,
            root: input.root.to_path_buf(),
            deadline: Instant::now() + CPU_BUDGET,
            seq: input.seq.clone(),
        })
    });
    let started = Instant::now();
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        TERA.read().unwrap().render(&name, &ctx)
    }));
    STATE.with(|s| *s.borrow_mut() = None);
    let out = match out {
        Ok(r) => r.map_err(|e| err(&e))?,
        Err(_) => {
            return Err(RenderError {
                message: "template panicked".into(),
                line: None,
                col: None,
            });
        }
    };
    if started.elapsed() > CPU_BUDGET * 4 {
        return Err(RenderError {
            message: "template exceeded the 50 ms CPU budget".into(),
            line: None,
            col: None,
        });
    }
    if out.len() > OUTPUT_CAP {
        return Err(RenderError {
            message: "template output exceeds 10 MiB".into(),
            line: None,
            col: None,
        });
    }
    Ok(out)
}

/// True when the template calls something random or time-dependent (dice icon, §8.3).
pub fn is_nondeterministic(src: &str) -> bool {
    let src = &*sugar::desugar(src);
    has_syntax(src)
        && [
            "fake(",
            "uuid(",
            "ulid(",
            "random_int",
            "random_float",
            "pick(",
            "weighted(",
            "lorem(",
            "now(",
        ]
        .iter()
        .any(|f| src.contains(f))
}

#[cfg(test)]
mod tests;
