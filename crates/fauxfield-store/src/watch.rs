use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};
use toml_edit::DocumentMut;

use crate::store::{Doc, Event, Store, StoreError, hash, list_files, notify_all};

/// Autosave + file watcher. Dropping it stops both threads.
pub struct Background {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    _watcher: notify::RecommendedWatcher,
}

impl Drop for Background {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Store {
    /// Start debounced autosave (flush after `delay` of idle) and the external-edit watcher.
    pub fn start_background(&self, delay: Duration) -> Result<Background, StoreError> {
        let root = self.root();
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if res.is_ok() {
                let _ = tx.send(());
            }
        })
        .map_err(|e| StoreError::Invalid(e.to_string()))?;
        watcher
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|e| StoreError::Invalid(e.to_string()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let (store, flag) = (self.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            let mut pending = false;
            let mut last = Instant::now();
            while !flag.load(Ordering::SeqCst) {
                if rx.recv_timeout(Duration::from_millis(25)).is_ok() {
                    while rx.try_recv().is_ok() {}
                    pending = true;
                    last = Instant::now();
                }
                // Let bursts of fs events settle before scanning.
                if pending && last.elapsed() >= Duration::from_millis(40) {
                    pending = false;
                    store.reconcile();
                }
                let _ = store
                    .flush_where(|d: &Doc| d.dirty_since.is_some_and(|t| t.elapsed() >= delay));
            }
        });
        Ok(Background {
            stop,
            thread: Some(thread),
            _watcher: watcher,
        })
    }

    /// Compare disk against memory; reload clean docs, flag conflicts on dirty ones.
    pub fn reconcile(&self) {
        let mut st = self.lock();
        let root = st.root.clone();
        let mut changed = false;
        let mut conflicts = vec![];
        let on_disk = list_files(&root);
        for rel in &on_disk {
            let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
                continue;
            };
            match st.docs.get_mut(rel) {
                None => {
                    let doc = text.parse::<DocumentMut>().ok();
                    let d = Doc {
                        written_hash: hash(&text),
                        raw: text,
                        doc,
                        dirty: false,
                        dirty_since: None,
                        undo: vec![],
                        redo: vec![],
                        conflict: None,
                    };
                    st.docs.insert(rel.clone(), d);
                    changed = true;
                }
                Some(d) => {
                    let h = hash(&text);
                    if h == d.written_hash || text == d.text() {
                        continue; // our own write, or no real change
                    }
                    if d.dirty {
                        if d.conflict.as_deref() != Some(text.as_str()) {
                            d.conflict = Some(text);
                            conflicts.push(rel.clone());
                        }
                    } else {
                        d.doc = text.parse::<DocumentMut>().ok();
                        d.raw = text;
                        d.written_hash = h;
                        changed = true;
                    }
                }
            }
        }
        let gone: Vec<String> = st
            .docs
            .iter()
            .filter(|(rel, d)| !on_disk.contains(rel) && !d.dirty && Path::new(&root).exists())
            .map(|(r, _)| r.clone())
            .collect();
        for g in gone {
            st.docs.remove(&g);
            changed = true;
        }
        if changed {
            notify_all(&mut st, Event::Changed);
        }
        for file in conflicts {
            notify_all(&mut st, Event::MergeConflict { file });
        }
    }
}
