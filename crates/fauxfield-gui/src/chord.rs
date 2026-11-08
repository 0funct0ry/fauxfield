//! The Cmd+K chord state machine (SPEC §9.8). `Cmd+K` is Quick Search *and* the first key of
//! chords such as `Cmd+K Cmd+T`; a 400 ms timeout disambiguates. Pure and clock-free: the caller
//! starts a [`CHORD_TIMEOUT`] timer when `feed` returns `Pending(seq)` and calls `expire(seq)`
//! when it fires, so the same code runs under GPUI's test clock.

use std::time::Duration;

pub const CHORD_TIMEOUT: Duration = Duration::from_millis(400);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The keystroke is not part of any chord; handle it normally.
    Passthrough,
    /// First key of a chord: swallow it and start the timeout for this sequence number.
    Pending(u64),
    /// A chord completed: run this action.
    Run(String),
    /// A chord was pending and the key did not continue it: nothing runs, the key is dropped.
    Cancelled,
}

#[derive(Clone, Debug)]
struct Entry {
    first: String,
    second: String,
    action: String,
}

#[derive(Clone, Debug, Default)]
pub struct ChordState {
    entries: Vec<Entry>,
    /// Action to run when the first key is released without a second key (the exact binding).
    fallback: Vec<(String, String)>,
    pending: Option<(String, u64)>,
    seq: u64,
}

impl ChordState {
    /// `chords`: (keys "a b", action). `singles`: (key, action) for keys that are also a chord
    /// prefix and run on timeout. All keystrokes in canonical form.
    pub fn new(chords: &[(String, String)], singles: &[(String, String)]) -> ChordState {
        let entries = chords
            .iter()
            .filter_map(|(keys, action)| {
                let mut it = keys.split_whitespace();
                Some(Entry {
                    first: it.next()?.to_string(),
                    second: it.next()?.to_string(),
                    action: action.clone(),
                })
            })
            .collect();
        ChordState {
            entries,
            fallback: singles.to_vec(),
            pending: None,
            seq: 0,
        }
    }

    pub fn is_prefix(&self, stroke: &str) -> bool {
        self.entries.iter().any(|e| e.first == stroke)
    }

    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Feeds one keystroke.
    pub fn feed(&mut self, stroke: &str) -> Outcome {
        if let Some((first, _)) = self.pending.take() {
            return match self
                .entries
                .iter()
                .find(|e| e.first == first && e.second == stroke)
            {
                Some(e) => Outcome::Run(e.action.clone()),
                None => Outcome::Cancelled,
            };
        }
        if self.is_prefix(stroke) {
            self.seq += 1;
            self.pending = Some((stroke.to_string(), self.seq));
            return Outcome::Pending(self.seq);
        }
        Outcome::Passthrough
    }

    /// The timeout for `seq` fired. Returns the action bound to the lone first key, if the chord
    /// is still waiting (a stale timer from an earlier chord does nothing).
    pub fn expire(&mut self, seq: u64) -> Option<String> {
        match &self.pending {
            Some((_, s)) if *s == seq => {}
            _ => return None,
        }
        let (first, _) = self.pending.take()?;
        self.fallback
            .iter()
            .find(|(k, _)| *k == first)
            .map(|(_, a)| a.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine() -> ChordState {
        ChordState::new(
            &[("secondary-k secondary-t".into(), "select_theme".into())],
            &[("secondary-k".into(), "quick_search".into())],
        )
    }

    #[test]
    fn unrelated_keys_pass_through() {
        let mut m = machine();
        assert_eq!(m.feed("secondary-b"), Outcome::Passthrough);
        assert!(!m.is_pending());
    }

    #[test]
    fn chord_completes_before_the_timeout() {
        let mut m = machine();
        assert_eq!(m.feed("secondary-k"), Outcome::Pending(1));
        assert_eq!(m.feed("secondary-t"), Outcome::Run("select_theme".into()));
        assert!(!m.is_pending());
        // The timer of the finished chord is stale.
        assert_eq!(m.expire(1), None);
    }

    #[test]
    fn lone_first_key_runs_quick_search_when_the_timer_fires() {
        let mut m = machine();
        let Outcome::Pending(seq) = m.feed("secondary-k") else {
            panic!("expected pending");
        };
        assert_eq!(m.expire(seq), Some("quick_search".into()));
        assert!(!m.is_pending());
        assert_eq!(m.expire(seq), None);
    }

    #[test]
    fn wrong_second_key_cancels_and_stale_timers_are_ignored() {
        let mut m = machine();
        m.feed("secondary-k");
        assert_eq!(m.feed("x"), Outcome::Cancelled);
        let Outcome::Pending(seq2) = m.feed("secondary-k") else {
            panic!("expected pending");
        };
        assert_eq!(seq2, 2);
        assert_eq!(m.expire(1), None);
        assert!(m.is_pending());
        assert_eq!(m.expire(2), Some("quick_search".into()));
    }

    #[test]
    fn timeout_is_400ms() {
        assert_eq!(CHORD_TIMEOUT.as_millis(), 400);
    }
}
