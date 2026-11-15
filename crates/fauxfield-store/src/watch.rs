use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};
use toml_edit::DocumentMut;

use crate::store::{Doc, Event, Store, StoreError, hash, list_files, notify_all};

/// Quiet time after the last file-system event before the workspace is rescanned.
const SETTLE: Duration = Duration::from_millis(5);

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
                // While a burst is settling wake exactly when it has (M17.04: hot reload < 50 ms),
                // otherwise poll the stop flag and autosave every 25 ms.
                let wait = if pending {
                    SETTLE
                        .saturating_sub(last.elapsed())
                        .max(Duration::from_millis(1))
                } else {
                    Duration::from_millis(25)
                };
                if rx.recv_timeout(wait).is_ok() {
                    while rx.try_recv().is_ok() {}
                    pending = true;
                    last = Instant::now();
                }
                // Let bursts of fs events settle before scanning.
                if pending && last.elapsed() >= SETTLE {
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
        let fp = seed_fingerprint(&root);
        if fp != st.seed_fp {
            st.seed_fp = fp;
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

/// Hash of (path, length, mtime) of every file under `data/` and `files/`: seed files are not
/// store documents, but editing one must reload its Dataset (M10.02).
pub(crate) fn seed_fingerprint(root: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    fn walk(dir: &Path, h: &mut std::collections::hash_map::DefaultHasher, depth: u8) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut ents: Vec<_> = rd.flatten().collect();
        ents.sort_by_key(|e| e.file_name());
        for e in ents {
            let p = e.path();
            if let Ok(m) = e.metadata() {
                if m.is_dir() {
                    if depth < 6 {
                        walk(&p, h, depth + 1);
                    }
                } else {
                    p.hash(h);
                    m.len().hash(h);
                    m.modified().ok().hash(h);
                }
            }
        }
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for d in ["data", "files"] {
        walk(&root.join(d), &mut h, 0);
    }
    h.finish()
}
