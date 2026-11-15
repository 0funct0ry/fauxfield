//! Agent sessions and the Agent activity log, tracked in the host so MCP processes that attach
//! over the control socket show up in the GUI (SPEC §11.1, M16).

use std::{
    collections::VecDeque,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

/// A session not refreshed within this window is gone (the MCP process heartbeats every second).
pub const SESSION_TTL_MS: u64 = 3000;
const LOG_CAP: usize = 500;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSession {
    pub id: String,
    /// MCP `clientInfo.name` (`Claude Code`, `codex`, `cursor`).
    pub name: String,
    pub since_ms: u64,
}

/// One tool call by an agent. `target` never holds a secret value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentEvent {
    pub ts_ms: u64,
    pub client: String,
    pub tool: String,
    pub target: String,
    /// created, updated, deleted, started, read, denied, error.
    pub outcome: String,
}

struct Live {
    session: AgentSession,
    seen_ms: u64,
}

#[derive(Default)]
struct Inner {
    live: Vec<Live>,
    log: VecDeque<AgentEvent>,
}

#[derive(Default)]
pub struct AgentTracker {
    inner: Mutex<Inner>,
}

impl AgentTracker {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Attach or heartbeat. The first call for an `id` fixes `since_ms`.
    pub fn attach(&self, id: &str, name: &str) {
        let now = now_ms();
        let mut g = self.lock();
        match g.live.iter_mut().find(|l| l.session.id == id) {
            Some(l) => l.seen_ms = now,
            None => g.live.push(Live {
                session: AgentSession {
                    id: id.into(),
                    name: name.into(),
                    since_ms: now,
                },
                seen_ms: now,
            }),
        }
    }

    pub fn detach(&self, id: &str) {
        self.lock().live.retain(|l| l.session.id != id);
    }

    pub fn sessions(&self) -> Vec<AgentSession> {
        let now = now_ms();
        let mut g = self.lock();
        g.live
            .retain(|l| now.saturating_sub(l.seen_ms) <= SESSION_TTL_MS);
        g.live.iter().map(|l| l.session.clone()).collect()
    }

    pub fn record(&self, e: AgentEvent) {
        let mut g = self.lock();
        if g.log.len() == LOG_CAP {
            g.log.pop_front();
        }
        g.log.push_back(e);
    }

    /// Newest last; `client` limits it to one client name.
    pub fn log(&self, client: Option<&str>) -> Vec<AgentEvent> {
        self.lock()
            .log
            .iter()
            .filter(|e| client.is_none_or(|c| e.client == c))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_heartbeat_detach_and_expiry() {
        let t = AgentTracker::default();
        t.attach("a", "Claude Code");
        t.attach("b", "codex");
        let since = t.sessions()[0].since_ms;
        t.attach("a", "Claude Code");
        assert_eq!(t.sessions().len(), 2);
        assert_eq!(t.sessions()[0].since_ms, since);
        t.detach("b");
        assert_eq!(t.sessions().len(), 1);
        // Expire by rewinding the heartbeat.
        t.lock().live[0].seen_ms = now_ms() - SESSION_TTL_MS - 1;
        assert!(t.sessions().is_empty());
    }

    #[test]
    fn log_is_capped_and_filterable() {
        let t = AgentTracker::default();
        for i in 0..(LOG_CAP + 5) {
            t.record(AgentEvent {
                ts_ms: i as u64,
                client: if i % 2 == 0 { "a" } else { "b" }.into(),
                tool: "x".into(),
                target: String::new(),
                outcome: "read".into(),
            });
        }
        assert_eq!(t.log(None).len(), LOG_CAP);
        assert_eq!(t.log(None)[0].ts_ms, 5);
        assert!(t.log(Some("a")).iter().all(|e| e.client == "a"));
    }
}
