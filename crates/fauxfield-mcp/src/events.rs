//! The event feed behind `ui.events` (SPEC §11.4): a replay ring buffer with a monotonic `seq`
//! and a `gap` flag, so an agent waits for something to happen instead of polling `ui.state`.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::sync::Notify;

use crate::{
    class::ToolClass,
    registry::{Registry, ToolDef},
};

pub const CAPACITY: usize = 1024;

#[derive(Clone, Debug)]
pub struct Event {
    pub seq: u64,
    /// `generation`, `toast`, `server` (a server's run state changed) and `traffic` (a new request was
    /// recorded); later milestones add `run`.
    pub kind: String,
    pub data: Value,
}

#[derive(Default)]
struct Inner {
    events: VecDeque<Event>,
    /// Seq of the next event to be pushed (first event is 1).
    next: u64,
}

/// Cheap to clone; all clones share one buffer.
#[derive(Clone)]
pub struct EventBuffer {
    inner: Arc<Mutex<Inner>>,
    notify: Arc<Notify>,
    capacity: usize,
}

impl Default for EventBuffer {
    fn default() -> Self {
        Self::with_capacity(CAPACITY)
    }
}

/// What a query returned.
#[derive(Debug)]
pub struct Batch {
    pub events: Vec<Event>,
    /// Pass this as `since` next time.
    pub last_seq: u64,
    /// Events between `since` and the oldest retained one were dropped.
    pub gap: bool,
}

impl EventBuffer {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                events: VecDeque::new(),
                next: 1,
            })),
            notify: Arc::new(Notify::new()),
            capacity,
        }
    }

    pub fn push(&self, kind: impl Into<String>, data: Value) -> u64 {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let seq = g.next;
        g.next += 1;
        g.events.push_back(Event {
            seq,
            kind: kind.into(),
            data,
        });
        while g.events.len() > self.capacity {
            g.events.pop_front();
        }
        drop(g);
        self.notify.notify_waiters();
        seq
    }

    /// Events with `seq > since` (all retained events when `since` is `None`), filtered by kind.
    pub fn query(&self, since: Option<u64>, kinds: &[String]) -> Batch {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let last_seq = g.next - 1;
        let since_seq = since.unwrap_or(0);
        let oldest = g.events.front().map(|e| e.seq).unwrap_or(g.next);
        let gap = since.is_some() && since_seq + 1 < oldest;
        let events = g
            .events
            .iter()
            .filter(|e| e.seq > since_seq)
            .filter(|e| kinds.is_empty() || kinds.contains(&e.kind))
            .cloned()
            .collect();
        Batch {
            events,
            last_seq,
            gap,
        }
    }

    /// Like `query`, but when nothing matches waits up to `wait` for something that does.
    pub async fn wait(&self, since: Option<u64>, kinds: &[String], wait: Duration) -> Batch {
        let deadline = tokio::time::Instant::now() + wait;
        let mut since = since;
        loop {
            // Register interest before checking, so a push in between is not missed.
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let batch = self.query(since, kinds);
            if !batch.events.is_empty() || wait.is_zero() {
                return batch;
            }
            // Non-matching events must not make the next check re-scan from the old cursor
            // forever, but the cursor we report stays the caller's.
            since.get_or_insert(0);
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return self.query(since, kinds);
            }
        }
    }
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct EventsInput {
    /// Return events with a larger `seq`. Omit for everything still buffered; pass the previous
    /// result's `last_seq` to continue.
    #[serde(default)]
    pub since: Option<u64>,
    /// Only these kinds (`generation`, `toast`, `server`, `traffic`). Default: all.
    #[serde(default)]
    pub kinds: Vec<String>,
    /// If nothing matches yet, wait up to this many milliseconds (max 30000).
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

pub fn batch_json(b: &Batch) -> Value {
    json!({
        "events": b.events.iter().map(|e| json!({"seq": e.seq, "kind": e.kind, "data": e.data})).collect::<Vec<_>>(),
        "last_seq": b.last_seq,
        "gap": b.gap,
    })
}

pub fn register(registry: &mut Registry, buffer: EventBuffer) {
    registry.register(ToolDef::new::<EventsInput>(
        "ui.events",
        "Events from a replay buffer (1024 entries): `generation` (ui.state changed), `toast`, `server` (run state) and `traffic` (new request). \
         Each has a monotonic `seq`; `gap` is true when events before `since` were dropped. With \
         `wait_ms` it blocks until a matching event arrives, so wait here instead of polling \
         ui.state.",
        ToolClass::Ui,
        move |args: Map<String, Value>| {
            let buffer = buffer.clone();
            Box::pin(async move {
                let i: EventsInput = serde_json::from_value(Value::Object(args))
                    .map_err(|e| format!("bad arguments: {e}"))?;
                let wait = Duration::from_millis(i.wait_ms.unwrap_or(0).min(30_000));
                Ok(batch_json(&buffer.wait(i.since, &i.kinds, wait).await))
            })
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_is_monotonic_and_since_filters() {
        let b = EventBuffer::default();
        b.push("generation", json!(1));
        b.push("toast", json!("hi"));
        let all = b.query(None, &[]);
        assert_eq!(all.events.len(), 2);
        assert_eq!(all.last_seq, 2);
        let rest = b.query(Some(1), &[]);
        assert_eq!(rest.events.len(), 1);
        assert_eq!(rest.events[0].seq, 2);
        assert!(!rest.gap);
        let toasts = b.query(None, &["toast".into()]);
        assert_eq!(toasts.events.len(), 1);
    }

    #[test]
    fn ring_drops_oldest_and_flags_a_gap() {
        let b = EventBuffer::with_capacity(3);
        for i in 0..5 {
            b.push("generation", json!(i));
        }
        let all = b.query(None, &[]);
        assert_eq!(all.events.first().unwrap().seq, 3);
        assert!(b.query(Some(1), &[]).gap);
        assert!(!b.query(Some(2), &[]).gap);
        assert!(!b.query(Some(5), &[]).gap);
    }

    #[tokio::test]
    async fn wait_returns_on_a_matching_push_or_times_out() {
        let b = EventBuffer::default();
        let b2 = b.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            b2.push("generation", json!(1));
        });
        let got = b.wait(Some(0), &[], Duration::from_secs(5)).await;
        assert_eq!(got.events.len(), 1);
        let none = b
            .wait(Some(got.last_seq), &[], Duration::from_millis(40))
            .await;
        assert!(none.events.is_empty());
    }
}
