use std::collections::{BTreeMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::Instant;

use fauxfield_model::{
    Diagnostic, Environment, Manifest, Rule, Server, new_id, parse_environment, parse_manifest,
    parse_server, validate_environment_names, validate_servers,
};
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, value};

use crate::init::slugify;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
}

/// What an edit addresses (by stable ID, SPEC §4.5).
#[derive(Debug, Clone)]
pub enum Target {
    Server(String),
    Folder(String),
    Route(String),
    Response(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Contents changed (own edit, undo, or silent reload of an external edit).
    Changed,
    /// An external edit collided with unsaved GUI edits to `file`.
    MergeConflict { file: String },
}

/// Typed, validated view of the whole workspace.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub root: PathBuf,
    pub manifest: Manifest,
    pub environments: Vec<(String, Environment)>,
    pub servers: Vec<(String, Server)>,
    pub diagnostics: Vec<Diagnostic>,
}

pub(crate) struct Doc {
    pub doc: Option<DocumentMut>,
    /// Text when the file does not parse as TOML.
    pub raw: String,
    pub dirty: bool,
    pub dirty_since: Option<Instant>,
    pub written_hash: u64,
    pub undo: Vec<String>,
    pub redo: Vec<String>,
    /// Disk text waiting for a merge decision.
    pub conflict: Option<String>,
}

pub(crate) fn hash(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

impl Doc {
    pub(crate) fn parse(text: String) -> Doc {
        let doc = text.parse::<DocumentMut>().ok();
        Doc {
            written_hash: hash(&text),
            raw: text,
            doc,
            dirty: false,
            dirty_since: None,
            undo: vec![],
            redo: vec![],
            conflict: None,
        }
    }
    pub fn text(&self) -> String {
        self.doc
            .as_ref()
            .map(|d| d.to_string())
            .unwrap_or_else(|| self.raw.clone())
    }
}

pub(crate) struct State {
    pub root: PathBuf,
    /// Keyed by workspace-relative path.
    pub docs: BTreeMap<String, Doc>,
    pub subscribers: Vec<mpsc::Sender<Event>>,
}

/// Shared handle to a workspace; cheap to clone.
#[derive(Clone)]
pub struct Store {
    pub(crate) inner: Arc<Mutex<State>>,
}

pub(crate) const MANIFEST: &str = "fauxfield.toml";

impl Store {
    pub fn open(root: &Path) -> Result<Store, StoreError> {
        let root = root.canonicalize()?;
        if !root.join(MANIFEST).is_file() {
            return Err(StoreError::Invalid(format!(
                "{} is not a workspace (no fauxfield.toml)",
                root.display()
            )));
        }
        let mut docs = BTreeMap::new();
        for rel in list_files(&root) {
            let text = std::fs::read_to_string(root.join(&rel))?;
            docs.insert(rel, Doc::parse(text));
        }
        Ok(Store {
            inner: Arc::new(Mutex::new(State {
                root,
                docs,
                subscribers: vec![],
            })),
        })
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn root(&self) -> PathBuf {
        self.lock().root.clone()
    }

    pub fn subscribe(&self) -> mpsc::Receiver<Event> {
        let (tx, rx) = mpsc::channel();
        self.lock().subscribers.push(tx);
        rx
    }

    /// Parse every file and validate. Never fails: problems become diagnostics.
    pub fn snapshot(&self) -> Snapshot {
        let st = self.lock();
        let mut diags = Vec::new();
        let mut manifest = Manifest::default();
        let mut environments = Vec::new();
        let mut servers = Vec::new();
        for (rel, d) in &st.docs {
            let text = d.text();
            if d.doc.is_none() {
                let msg = text
                    .parse::<DocumentMut>()
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_default();
                diags.push(Diagnostic::error(
                    rel,
                    "",
                    "E001",
                    format!("TOML syntax error: {msg}"),
                ));
                continue;
            }
            if rel == MANIFEST {
                match parse_manifest(&text) {
                    Ok(m) => manifest = m,
                    Err(e) => diags.push(Diagnostic::error(rel, "", "E001", e)),
                }
            } else if rel.starts_with("environments/") {
                match parse_environment(&text) {
                    Ok(e) => environments.push((rel.clone(), e)),
                    Err(e) => diags.push(Diagnostic::error(rel, "", "E001", e)),
                }
            } else if rel.starts_with("servers/") {
                match parse_server(&text) {
                    Ok(s) => servers.push((rel.clone(), s)),
                    Err(e) => diags.push(Diagnostic::error(rel, "", "E001", e)),
                }
            }
        }
        diags.extend(validate_servers(&servers));
        let names: Vec<_> = environments
            .iter()
            .map(|(f, e)| (f.clone(), e.name.clone()))
            .collect();
        diags.extend(validate_environment_names(&names));
        for (file, s) in &servers {
            for (i, r) in s.routes.iter().enumerate() {
                for (j, resp) in r.responses.iter().enumerate() {
                    if let Some(bf) = &resp.body_file
                        && !bf.contains("..")
                        && !bf.starts_with('/')
                        && !st.root.join(bf).exists()
                    {
                        diags.push(Diagnostic::warning(
                            file,
                            format!("routes[{}].responses[{j}].body_file", r.id),
                            "W042",
                            format!("body_file `{bf}` does not exist (route {i})"),
                        ));
                    }
                }
            }
            for (i, d) in s.datasets.iter().enumerate() {
                for (k, f) in [("seed", &d.seed), ("seed_template", &d.seed_template)] {
                    if let Some(f) = f
                        && crate::datasets::confined(f)
                        && !st.root.join(f).exists()
                    {
                        diags.push(Diagnostic::warning(
                            file,
                            format!("datasets[{i}].{k}"),
                            "W071",
                            format!("{k} `{f}` does not exist (dataset {})", d.name),
                        ));
                    }
                }
            }
        }
        Snapshot {
            root: st.root.clone(),
            manifest,
            environments,
            servers,
            diagnostics: diags,
        }
    }

    /// Current text of a file (including unsaved edits).
    pub fn text(&self, file: &str) -> Option<String> {
        self.lock().docs.get(file).map(Doc::text)
    }

    pub fn is_dirty(&self, file: &str) -> bool {
        self.lock().docs.get(file).is_some_and(|d| d.dirty)
    }

    pub fn conflicts(&self) -> Vec<String> {
        self.lock()
            .docs
            .iter()
            .filter(|(_, d)| d.conflict.is_some())
            .map(|(f, _)| f.clone())
            .collect()
    }

    /// File holding the server with this ID.
    pub fn server_file(&self, id: &str) -> Option<String> {
        let st = self.lock();
        st.docs
            .iter()
            .filter(|(f, _)| f.starts_with("servers/"))
            .find_map(|(f, d)| {
                let doc = d.doc.as_ref()?;
                (doc.get("id").and_then(Item::as_str) == Some(id)).then(|| f.clone())
            })
    }

    /// File containing the folder/route/response with this ID.
    pub fn file_of(&self, target: &Target) -> Option<String> {
        let st = self.lock();
        let find = |d: &Doc| -> bool {
            let Some(doc) = d.doc.as_ref() else {
                return false;
            };
            match target {
                Target::Server(id) => doc.get("id").and_then(Item::as_str) == Some(id),
                Target::Folder(id) => has_id(doc.get("folders"), id),
                Target::Route(id) => has_id(doc.get("routes"), id),
                Target::Response(id) => doc
                    .get("routes")
                    .and_then(Item::as_array_of_tables)
                    .is_some_and(|rs| rs.iter().any(|r| has_id(r.get("responses"), id))),
            }
        };
        st.docs
            .iter()
            .filter(|(f, _)| f.starts_with("servers/"))
            .find(|(_, d)| find(d))
            .map(|(f, _)| f.clone())
    }

    // ---- editing ----

    /// Apply a mutation to one file's document with undo bookkeeping.
    pub fn edit<R>(
        &self,
        file: &str,
        f: impl FnOnce(&mut DocumentMut) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        let mut st = self.lock();
        let r = {
            let d = st
                .docs
                .get_mut(file)
                .ok_or_else(|| StoreError::NotFound(file.into()))?;
            let doc = d.doc.as_mut().ok_or_else(|| {
                StoreError::Invalid(format!(
                    "{file} has a syntax error; fix it in the Raw file tab"
                ))
            })?;
            let before = doc.to_string();
            let r = f(doc)?;
            if doc.to_string() != before {
                d.undo.push(before);
                if d.undo.len() > 200 {
                    d.undo.remove(0);
                }
                d.redo.clear();
                d.dirty = true;
                d.dirty_since.get_or_insert_with(Instant::now);
            }
            r
        };
        notify_all(&mut st, Event::Changed);
        Ok(r)
    }

    /// Whether `file` has an edit to undo (drives the Edit menu).
    pub fn can_undo(&self, file: &str) -> bool {
        self.lock()
            .docs
            .get(file)
            .is_some_and(|d| !d.undo.is_empty())
    }
    /// Whether `file` has an undone edit to redo.
    pub fn can_redo(&self, file: &str) -> bool {
        self.lock()
            .docs
            .get(file)
            .is_some_and(|d| !d.redo.is_empty())
    }

    pub fn undo(&self, file: &str) -> bool {
        self.swap_history(file, true)
    }
    pub fn redo(&self, file: &str) -> bool {
        self.swap_history(file, false)
    }
    fn swap_history(&self, file: &str, undo: bool) -> bool {
        let mut st = self.lock();
        let Some(d) = st.docs.get_mut(file) else {
            return false;
        };
        let Some(prev) = (if undo { d.undo.pop() } else { d.redo.pop() }) else {
            return false;
        };
        let Ok(parsed) = prev.parse::<DocumentMut>() else {
            return false;
        };
        let cur = d.text();
        if undo {
            d.redo.push(cur)
        } else {
            d.undo.push(cur)
        }
        d.doc = Some(parsed);
        d.dirty = true;
        d.dirty_since.get_or_insert_with(Instant::now);
        notify_all(&mut st, Event::Changed);
        true
    }

    /// Replace a file's whole text (Raw file tab). Invalid TOML is kept as raw text.
    pub fn set_raw(&self, file: &str, text: &str) -> Result<(), StoreError> {
        let mut st = self.lock();
        let d = st
            .docs
            .get_mut(file)
            .ok_or_else(|| StoreError::NotFound(file.into()))?;
        let before = d.text();
        if before == text {
            return Ok(());
        }
        d.undo.push(before);
        d.redo.clear();
        d.doc = text.parse::<DocumentMut>().ok();
        d.raw = text.to_string();
        d.dirty = true;
        d.dirty_since.get_or_insert_with(Instant::now);
        notify_all(&mut st, Event::Changed);
        Ok(())
    }

    pub fn create_server(&self, name: &str, port: u16) -> Result<String, StoreError> {
        let id = new_id("srv");
        let mut st = self.lock();
        let slug = slugify(name);
        let mut rel = format!("servers/{slug}.toml");
        let mut n = 2;
        while st.docs.contains_key(&rel) || st.root.join(&rel).exists() {
            rel = format!("servers/{slug}-{n}.toml");
            n += 1;
        }
        let text = format!(
            "schema = 1\nid = \"{id}\"\nname = {}\nenabled = true\nhost = \"127.0.0.1\"\nport = {port}\n",
            value(name).to_string().trim()
        );
        let mut d = Doc::parse(text);
        d.dirty = true;
        d.dirty_since = Some(Instant::now());
        d.written_hash = 0;
        st.docs.insert(rel, d);
        notify_all(&mut st, Event::Changed);
        Ok(id)
    }

    /// Delete a server: removes its file immediately.
    pub fn delete_server(&self, id: &str) -> Result<(), StoreError> {
        let file = self
            .server_file(id)
            .ok_or_else(|| StoreError::NotFound(id.into()))?;
        let mut st = self.lock();
        let path = st.root.join(&file);
        st.docs.remove(&file);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        notify_all(&mut st, Event::Changed);
        Ok(())
    }

    /// Rename a server; its file follows the slug, the ID stays (SPEC §4.5).
    pub fn rename_server(&self, id: &str, name: &str) -> Result<(), StoreError> {
        let file = self
            .server_file(id)
            .ok_or_else(|| StoreError::NotFound(id.into()))?;
        self.edit(&file, |d| {
            d["name"] = value(name);
            Ok(())
        })?;
        self.flush()?;
        let mut st = self.lock();
        let slug = slugify(name);
        let mut rel = format!("servers/{slug}.toml");
        if rel == file {
            return Ok(());
        }
        let mut n = 2;
        while st.docs.contains_key(&rel) {
            rel = format!("servers/{slug}-{n}.toml");
            n += 1;
        }
        std::fs::rename(st.root.join(&file), st.root.join(&rel))?;
        if let Some(d) = st.docs.remove(&file) {
            st.docs.insert(rel, d);
        }
        notify_all(&mut st, Event::Changed);
        Ok(())
    }

    /// Set `name` in fauxfield.toml (comments and key order survive).
    pub fn set_workspace_name(&self, name: &str) -> Result<(), StoreError> {
        let name = fauxfield_model::validate_workspace_name(name).map_err(StoreError::Invalid)?;
        self.edit(MANIFEST, |d| {
            d["name"] = value(name.as_str());
            Ok(())
        })
    }

    pub fn duplicate_server(&self, id: &str) -> Result<String, StoreError> {
        let file = self
            .server_file(id)
            .ok_or_else(|| StoreError::NotFound(id.into()))?;
        let text = self.text(&file).unwrap_or_default();
        let mut doc: DocumentMut = text
            .parse()
            .map_err(|e| StoreError::Invalid(format!("{e}")))?;
        let new = new_id("srv");
        let name = doc
            .get("name")
            .and_then(Item::as_str)
            .unwrap_or("Server")
            .to_string();
        let mut st = self.lock();
        // New unique port (max + 1) and unique name ("<name> copy", "<name> copy 2", ...).
        let mut max_port = 0i64;
        let mut names = Vec::new();
        for (f, d) in st.docs.iter() {
            if !f.starts_with("servers/") {
                continue;
            }
            if let Ok(o) = d.text().parse::<DocumentMut>() {
                max_port = max_port.max(o.get("port").and_then(Item::as_integer).unwrap_or(0));
                if let Some(n) = o.get("name").and_then(Item::as_str) {
                    names.push(n.to_string());
                }
            }
        }
        let port = (max_port + 1).min(65535);
        let mut new_name = format!("{name} copy");
        let mut k = 2;
        while names.contains(&new_name) {
            new_name = format!("{name} copy {k}");
            k += 1;
        }
        doc["id"] = value(new.clone());
        doc["name"] = value(new_name.clone());
        doc["port"] = value(port);
        regen_ids(&mut doc);
        let slug = slugify(&new_name);
        let mut rel = format!("servers/{slug}.toml");
        let mut n = 2;
        while st.docs.contains_key(&rel) {
            rel = format!("servers/{slug}-{n}.toml");
            n += 1;
        }
        let mut d = Doc::parse(doc.to_string());
        d.dirty = true;
        d.dirty_since = Some(Instant::now());
        d.written_hash = 0;
        st.docs.insert(rel, d);
        notify_all(&mut st, Event::Changed);
        Ok(new)
    }

    pub fn create_folder(&self, server_id: &str, name: &str) -> Result<String, StoreError> {
        let file = self
            .server_file(server_id)
            .ok_or_else(|| StoreError::NotFound(server_id.into()))?;
        let id = new_id("fld");
        self.edit(&file, |d| {
            let mut t = Table::new();
            t["id"] = value(id.clone());
            t["name"] = value(name);
            array_mut(d, "folders").push(t);
            Ok(())
        })?;
        Ok(id)
    }

    pub fn create_route(
        &self,
        server_id: &str,
        folder: Option<&str>,
        method: &str,
        path: &str,
    ) -> Result<String, StoreError> {
        self.create_route_with(server_id, folder, method, path, &RouteOpts::default())
    }

    /// Create a Route with optional match mode, name and priority. Defaults are not written.
    pub fn create_route_with(
        &self,
        server_id: &str,
        folder: Option<&str>,
        method: &str,
        path: &str,
        opts: &RouteOpts,
    ) -> Result<String, StoreError> {
        validate_pattern(path, opts.match_mode.as_deref())?;
        let file = self
            .server_file(server_id)
            .ok_or_else(|| StoreError::NotFound(server_id.into()))?;
        let id = new_id("rte");
        self.edit(&file, |d| {
            let mut t = Table::new();
            t["id"] = value(id.clone());
            if let Some(f) = folder {
                t["folder"] = value(f);
            }
            t["method"] = value(method.to_ascii_uppercase());
            t["path"] = value(path);
            if let Some(n) = &opts.name {
                t["name"] = value(n.as_str());
            }
            if let Some(m) = &opts.match_mode {
                t["match"] = value(m.as_str());
            }
            if let Some(p) = opts.priority.filter(|p| *p != 0) {
                t["priority"] = value(i64::from(p));
            }
            let mut resp = Table::new();
            resp["id"] = value(new_id("rsp"));
            resp["name"] = value("OK");
            resp["default"] = value(true);
            resp["status"] = value(200);
            let mut rs = ArrayOfTables::new();
            rs.push(resp);
            t["responses"] = Item::ArrayOfTables(rs);
            array_mut(d, "routes").push(t);
            Ok(())
        })?;
        Ok(id)
    }

    /// Set or clear (`None` = inferred) a Route's match mode; validated like the loader.
    pub fn set_route_match(&self, route_id: &str, mode: Option<&str>) -> Result<(), StoreError> {
        let t = Target::Route(route_id.into());
        let file = self
            .file_of(&t)
            .ok_or_else(|| StoreError::NotFound(route_id.into()))?;
        self.edit(&file, |d| {
            let r = find_table(d, "routes", route_id)
                .ok_or_else(|| StoreError::NotFound(route_id.into()))?;
            let path = r
                .get("path")
                .and_then(Item::as_str)
                .unwrap_or("")
                .to_string();
            validate_pattern(&path, mode)?;
            match mode {
                Some(m) => r["match"] = value(m),
                None => {
                    r.remove("match");
                }
            }
            Ok(())
        })
    }

    /// Set a Route's priority; `0` removes the key.
    pub fn set_route_priority(&self, route_id: &str, priority: i32) -> Result<(), StoreError> {
        self.set_int(
            &Target::Route(route_id.into()),
            "priority",
            (priority != 0).then_some(i64::from(priority)),
        )
    }

    pub fn rename_folder(&self, folder_id: &str, name: &str) -> Result<(), StoreError> {
        self.set_str(&Target::Folder(folder_id.into()), "name", Some(name))
    }

    /// `enabled = true` is the default and is removed rather than written.
    pub fn set_folder_enabled(&self, folder_id: &str, enabled: bool) -> Result<(), StoreError> {
        self.set_item(
            &Target::Folder(folder_id.into()),
            "enabled",
            (!enabled).then(|| value(false)),
        )
    }

    /// `collapsed = false` is the default and is removed rather than written.
    pub fn set_folder_collapsed(&self, folder_id: &str, collapsed: bool) -> Result<(), StoreError> {
        self.set_item(
            &Target::Folder(folder_id.into()),
            "collapsed",
            collapsed.then(|| value(true)),
        )
    }

    /// `strict_slash = false` is the default and is removed rather than written.
    pub fn set_strict_slash(&self, server_id: &str, strict: bool) -> Result<(), StoreError> {
        self.set_item(
            &Target::Server(server_id.into()),
            "strict_slash",
            strict.then(|| value(true)),
        )
    }

    /// Delete a Folder; `with_routes` removes its Routes, otherwise they move to the
    /// end of the server root in their current relative order.
    pub fn delete_folder(&self, folder_id: &str, with_routes: bool) -> Result<(), StoreError> {
        if !with_routes {
            return self.delete(&Target::Folder(folder_id.into()));
        }
        let t = Target::Folder(folder_id.into());
        let file = self
            .file_of(&t)
            .ok_or_else(|| StoreError::NotFound(folder_id.into()))?;
        self.edit(&file, |d| {
            if let Some(rs) = d.get_mut("routes").and_then(Item::as_array_of_tables_mut) {
                let mut keep = Vec::new();
                while !rs.is_empty() {
                    let r = rs.get(0).cloned().expect("non-empty");
                    rs.remove(0);
                    if r.get("folder").and_then(Item::as_str) != Some(folder_id) {
                        keep.push(r);
                    }
                }
                for r in keep {
                    rs.push(r);
                }
            }
            remove_by_id(d.get_mut("folders"), folder_id);
            Ok(())
        })
    }

    /// Move a Route into `target` (`None` = server root), immediately before sibling
    /// `before`, or at the end of the target block. Physical order then equals effective
    /// order for that block (SPEC §20.17). A no-op move writes nothing.
    pub fn move_route(
        &self,
        route_id: &str,
        target: Option<&str>,
        before: Option<&str>,
    ) -> Result<(), StoreError> {
        let t = Target::Route(route_id.into());
        let file = self
            .file_of(&t)
            .ok_or_else(|| StoreError::NotFound(route_id.into()))?;
        self.edit(&file, |d| {
            if let Some(f) = target
                && !has_id(d.get("folders"), f)
            {
                return Err(StoreError::NotFound(f.into()));
            }
            if before == Some(route_id) {
                return Ok(());
            }
            let rs = d
                .get_mut("routes")
                .and_then(Item::as_array_of_tables_mut)
                .ok_or_else(|| StoreError::NotFound(route_id.into()))?;
            let (mut v, pos) = take_all(rs);
            let from = v
                .iter()
                .position(|t| id_of(t) == Some(route_id))
                .ok_or_else(|| StoreError::NotFound(route_id.into()))?;
            let mut r = v.remove(from);
            let folder_of = |t: &Table| t.get("folder").and_then(Item::as_str).map(String::from);
            let before_idx = match before {
                Some(b) => Some(
                    v.iter()
                        .position(|t| id_of(t) == Some(b))
                        .ok_or_else(|| StoreError::NotFound(b.into()))?,
                ),
                None => None,
            };
            let at = match before_idx {
                Some(i) => i,
                None => v
                    .iter()
                    .rposition(|t| folder_of(t).as_deref() == target)
                    .map_or(v.len(), |i| i + 1),
            };
            match target {
                Some(f) => r["folder"] = value(f),
                None => {
                    r.remove("folder");
                }
            }
            v.insert(at, r);
            put_all(rs, v, pos);
            Ok(())
        })
    }

    /// Reorder a Folder before sibling `before` (or to the end). Routes are untouched.
    pub fn move_folder(&self, folder_id: &str, before: Option<&str>) -> Result<(), StoreError> {
        let t = Target::Folder(folder_id.into());
        let file = self
            .file_of(&t)
            .ok_or_else(|| StoreError::NotFound(folder_id.into()))?;
        self.edit(&file, |d| {
            if before == Some(folder_id) {
                return Ok(());
            }
            let fs = d
                .get_mut("folders")
                .and_then(Item::as_array_of_tables_mut)
                .ok_or_else(|| StoreError::NotFound(folder_id.into()))?;
            let (mut v, pos) = take_all(fs);
            let from = v
                .iter()
                .position(|t| id_of(t) == Some(folder_id))
                .ok_or_else(|| StoreError::NotFound(folder_id.into()))?;
            let f = v.remove(from);
            let at = match before {
                Some(b) => v
                    .iter()
                    .position(|t| id_of(t) == Some(b))
                    .ok_or_else(|| StoreError::NotFound(b.into()))?,
                None => v.len(),
            };
            v.insert(at, f);
            put_all(fs, v, pos);
            Ok(())
        })
    }

    pub fn create_response(&self, route_id: &str) -> Result<String, StoreError> {
        let target = Target::Route(route_id.into());
        let file = self
            .file_of(&target)
            .ok_or_else(|| StoreError::NotFound(route_id.into()))?;
        let id = new_id("rsp");
        self.edit(&file, |d| {
            let r = find_table(d, "routes", route_id)
                .ok_or_else(|| StoreError::NotFound(route_id.into()))?;
            let mut t = Table::new();
            t["id"] = value(id.clone());
            t["name"] = value("New response");
            t["status"] = value(200);
            if r.get("responses").is_none() {
                r["responses"] = Item::ArrayOfTables(ArrayOfTables::new());
            }
            r["responses"]
                .as_array_of_tables_mut()
                .ok_or_else(|| StoreError::Invalid("responses is not a table array".into()))?
                .push(t);
            Ok(())
        })?;
        Ok(id)
    }

    // ---- response ordering, rules, presets (M5) ----

    /// Reorder a Response before sibling `before` (or to the end) within its Route.
    pub fn move_response(&self, response_id: &str, before: Option<&str>) -> Result<(), StoreError> {
        let t = Target::Response(response_id.into());
        let file = self
            .file_of(&t)
            .ok_or_else(|| StoreError::NotFound(response_id.into()))?;
        self.edit(&file, |d| {
            if before == Some(response_id) {
                return Ok(());
            }
            let rs = responses_of_mut(d, response_id)
                .ok_or_else(|| StoreError::NotFound(response_id.into()))?;
            let (mut v, pos) = take_all(rs);
            let from = v
                .iter()
                .position(|t| id_of(t) == Some(response_id))
                .expect("found above");
            let r = v.remove(from);
            let at = match before {
                Some(b) => v
                    .iter()
                    .position(|t| id_of(t) == Some(b))
                    .ok_or_else(|| StoreError::NotFound(b.into()))?,
                None => v.len(),
            };
            v.insert(at, r);
            put_all(rs, v, pos);
            renumber(d);
            Ok(())
        })
    }

    /// Append (or with `index`, replace) a rule on a Response. Returns the rule's index.
    pub fn set_rule(
        &self,
        response_id: &str,
        index: Option<usize>,
        rule: &Rule,
    ) -> Result<usize, StoreError> {
        let t = Target::Response(response_id.into());
        let file = self
            .file_of(&t)
            .ok_or_else(|| StoreError::NotFound(response_id.into()))?;
        let mut out = 0;
        self.edit(&file, |d| {
            let resp = resolve(d, &t).ok_or_else(|| StoreError::NotFound(response_id.into()))?;
            let tbl = rule_table(rule)?;
            let list = rules_mut(resp)?;
            match index {
                Some(i) if i < list.len() => {
                    list.remove(i);
                    // Re-insert at the same place.
                    let (mut v, pos) = take_all(list);
                    v.insert(i, tbl);
                    put_all(list, v, pos);
                    out = i;
                }
                Some(i) => return Err(StoreError::NotFound(format!("rule {i}"))),
                None => {
                    out = list.len();
                    list.push(tbl);
                }
            }
            renumber(d);
            Ok(())
        })?;
        Ok(out)
    }

    pub fn delete_rule(&self, response_id: &str, index: usize) -> Result<(), StoreError> {
        let t = Target::Response(response_id.into());
        let file = self
            .file_of(&t)
            .ok_or_else(|| StoreError::NotFound(response_id.into()))?;
        self.edit(&file, |d| {
            let resp = resolve(d, &t).ok_or_else(|| StoreError::NotFound(response_id.into()))?;
            let list = rules_mut(resp)?;
            if index >= list.len() {
                return Err(StoreError::NotFound(format!("rule {index}")));
            }
            list.remove(index);
            if list.is_empty() {
                resp.remove("rules");
            }
            Ok(())
        })
    }

    /// Move a rule from index `from` to index `to` (after removal) within its Response.
    pub fn move_rule(&self, response_id: &str, from: usize, to: usize) -> Result<(), StoreError> {
        let t = Target::Response(response_id.into());
        let file = self
            .file_of(&t)
            .ok_or_else(|| StoreError::NotFound(response_id.into()))?;
        self.edit(&file, |d| {
            let resp = resolve(d, &t).ok_or_else(|| StoreError::NotFound(response_id.into()))?;
            let list = rules_mut(resp)?;
            if from >= list.len() || to >= list.len() {
                return Err(StoreError::NotFound(format!("rule {from}/{to}")));
            }
            let (mut v, pos) = take_all(list);
            let r = v.remove(from);
            v.insert(to, r);
            put_all(list, v, pos);
            renumber(d);
            Ok(())
        })
    }

    /// Set `disabled` (the key is removed when false).
    pub fn set_response_disabled(
        &self,
        response_id: &str,
        disabled: bool,
    ) -> Result<(), StoreError> {
        let t = Target::Response(response_id.into());
        self.set_item(&t, "disabled", disabled.then(|| value(true)))
    }

    /// Set the Route's response mode (`rules`, the default, removes the key).
    pub fn set_response_mode(&self, route_id: &str, mode: &str) -> Result<(), StoreError> {
        if !fauxfield_model::rules::RESPONSE_MODES.contains(&mode) {
            return Err(StoreError::Invalid(format!(
                "unknown response_mode `{mode}`"
            )));
        }
        let t = Target::Route(route_id.into());
        self.set_item(&t, "response_mode", (mode != "rules").then(|| value(mode)))
    }

    /// `and` (the default) removes the key.
    pub fn set_rules_logic(&self, response_id: &str, logic: &str) -> Result<(), StoreError> {
        if logic != "and" && logic != "or" {
            return Err(StoreError::Invalid("rules_logic must be and or or".into()));
        }
        let t = Target::Response(response_id.into());
        self.set_item(&t, "rules_logic", (logic != "and").then(|| value(logic)))
    }

    /// Add an RFC 9457 problem+json Response for `status` (SPEC §5.6).
    pub fn create_error_preset(&self, route_id: &str, status: u16) -> Result<String, StoreError> {
        let (title, detail) = error_preset(status)
            .ok_or_else(|| StoreError::Invalid(format!("no error preset for {status}")))?;
        let id = self.create_response(route_id)?;
        let t = Target::Response(id.clone());
        self.set_str(&t, "name", Some(&format!("{status} {title}")))?;
        self.set_int(&t, "status", Some(i64::from(status)))?;
        self.set_str(&t, "content_type", Some("application/problem+json"))?;
        let body = format!(
            "{{\n  \"type\": \"about:blank\",\n  \"title\": \"{title}\",\n  \"status\": {status},\n  \"detail\": \"{detail}\"\n}}"
        );
        self.set_str(&t, "body", Some(&body))?;
        Ok(id)
    }

    /// Rename (`name`) any item. Servers use `rename_server`.
    pub fn set_str(&self, target: &Target, key: &str, val: Option<&str>) -> Result<(), StoreError> {
        self.set_item(target, key, val.map(value))
    }
    pub fn set_int(&self, target: &Target, key: &str, val: Option<i64>) -> Result<(), StoreError> {
        self.set_item(target, key, val.map(value))
    }
    pub fn set_bool(&self, target: &Target, key: &str, val: bool) -> Result<(), StoreError> {
        self.set_item(target, key, Some(value(val)))
    }

    fn set_item(&self, target: &Target, key: &str, val: Option<Item>) -> Result<(), StoreError> {
        let file = self
            .file_of(target)
            .ok_or_else(|| StoreError::NotFound(format!("{target:?}")))?;
        self.edit(&file, |d| {
            let t =
                resolve(d, target).ok_or_else(|| StoreError::NotFound(format!("{target:?}")))?;
            match val {
                Some(v) => {
                    t[key] = v;
                }
                None => {
                    t.remove(key);
                }
            }
            Ok(())
        })
    }

    /// Set (or with `None`, remove) one response header.
    pub fn set_header(
        &self,
        response_id: &str,
        name: &str,
        val: Option<&str>,
    ) -> Result<(), StoreError> {
        let target = Target::Response(response_id.into());
        let file = self
            .file_of(&target)
            .ok_or_else(|| StoreError::NotFound(response_id.into()))?;
        self.edit(&file, |d| {
            let t = resolve(d, &target).ok_or_else(|| StoreError::NotFound(response_id.into()))?;
            match val {
                Some(v) => {
                    if t.get("headers").is_none() {
                        t["headers"] = Item::Table(Table::new());
                    }
                    t["headers"][name] = value(v);
                }
                None => {
                    if let Some(h) = t.get_mut("headers").and_then(Item::as_table_mut) {
                        h.remove(name);
                        if h.is_empty() {
                            t.remove("headers");
                        }
                    }
                }
            }
            Ok(())
        })
    }

    /// Set (or with `None`, remove) one Route header (`[routes.headers]`, SPEC §5.5).
    pub fn set_route_header(
        &self,
        route_id: &str,
        name: &str,
        val: Option<&str>,
    ) -> Result<(), StoreError> {
        let target = Target::Route(route_id.into());
        let file = self
            .file_of(&target)
            .ok_or_else(|| StoreError::NotFound(route_id.into()))?;
        self.edit(&file, |d| {
            let t = resolve(d, &target).ok_or_else(|| StoreError::NotFound(route_id.into()))?;
            match val {
                Some(v) => {
                    if t.get("headers").is_none() {
                        t["headers"] = Item::Table(Table::new());
                    }
                    t["headers"][name] = value(v);
                }
                None => {
                    if let Some(h) = t.get_mut("headers").and_then(Item::as_table_mut) {
                        h.remove(name);
                        if h.is_empty() {
                            t.remove("headers");
                        }
                    }
                }
            }
            Ok(())
        })
    }

    /// Set (or with `None`, remove) the Server's `fake_seed` (SPEC §8.3).
    pub fn set_fake_seed(&self, server_id: &str, seed: Option<u64>) -> Result<(), StoreError> {
        self.set_int(
            &Target::Server(server_id.into()),
            "fake_seed",
            seed.map(|s| s as i64),
        )
    }

    /// Set whether a Response is rendered as a template (`template = false` sends it verbatim).
    pub fn set_response_template(&self, response_id: &str, on: bool) -> Result<(), StoreError> {
        let t = Target::Response(response_id.into());
        if on {
            self.set_item(&t, "template", None)
        } else {
            self.set_bool(&t, "template", false)
        }
    }

    /// Make one response the default of its route.
    /// Rename a header keeping its position, value and comments. Fails if `new` exists
    /// (case-insensitively) or `old` is missing.
    pub fn rename_header(&self, response_id: &str, old: &str, new: &str) -> Result<(), StoreError> {
        let target = Target::Response(response_id.into());
        let file = self
            .file_of(&target)
            .ok_or_else(|| StoreError::NotFound(response_id.into()))?;
        self.edit(&file, |d| {
            let t = resolve(d, &target).ok_or_else(|| StoreError::NotFound(response_id.into()))?;
            let h = t
                .get_mut("headers")
                .and_then(Item::as_table_mut)
                .ok_or_else(|| StoreError::NotFound(format!("header {old}")))?;
            if old == new {
                return Ok(());
            }
            if h.iter().any(|(k, _)| k.eq_ignore_ascii_case(new)) {
                return Err(StoreError::Invalid(format!(
                    "header `{new}` already exists"
                )));
            }
            let mut entries: Vec<(toml_edit::Key, Item)> = h
                .iter()
                .filter_map(|(k, _)| h.get_key_value(k).map(|(k, v)| (k.clone(), v.clone())))
                .collect();
            let at = entries
                .iter()
                .position(|(k, _)| k.get() == old)
                .ok_or_else(|| StoreError::NotFound(format!("header {old}")))?;
            let mut key = toml_edit::Key::new(new);
            *key.leaf_decor_mut() = entries[at].0.leaf_decor().clone();
            entries[at].0 = key;
            h.clear();
            for (k, v) in entries {
                h.insert_formatted(&k, v);
            }
            Ok(())
        })
    }

    pub fn set_default_response(&self, response_id: &str) -> Result<(), StoreError> {
        let target = Target::Response(response_id.into());
        let file = self
            .file_of(&target)
            .ok_or_else(|| StoreError::NotFound(response_id.into()))?;
        self.edit(&file, |d| {
            let routes = d
                .get_mut("routes")
                .and_then(Item::as_array_of_tables_mut)
                .ok_or_else(|| StoreError::NotFound(response_id.into()))?;
            for r in routes.iter_mut() {
                if let Some(rs) = r
                    .get_mut("responses")
                    .and_then(Item::as_array_of_tables_mut)
                    && rs
                        .iter()
                        .any(|x| x.get("id").and_then(Item::as_str) == Some(response_id))
                {
                    for x in rs.iter_mut() {
                        let is = x.get("id").and_then(Item::as_str) == Some(response_id);
                        if is {
                            x["default"] = value(true);
                        } else {
                            x.remove("default");
                        }
                    }
                }
            }
            Ok(())
        })
    }

    pub fn delete(&self, target: &Target) -> Result<(), StoreError> {
        if let Target::Server(id) = target {
            return self.delete_server(id);
        }
        let file = self
            .file_of(target)
            .ok_or_else(|| StoreError::NotFound(format!("{target:?}")))?;
        self.edit(&file, |d| {
            match target {
                Target::Folder(id) => {
                    // Routes in the folder move to the root.
                    if let Some(rs) = d.get_mut("routes").and_then(Item::as_array_of_tables_mut) {
                        for r in rs.iter_mut() {
                            if r.get("folder").and_then(Item::as_str) == Some(id) {
                                r.remove("folder");
                            }
                        }
                    }
                    remove_by_id(d.get_mut("folders"), id);
                }
                Target::Route(id) => remove_by_id(d.get_mut("routes"), id),
                Target::Response(id) => {
                    if let Some(rs) = d.get_mut("routes").and_then(Item::as_array_of_tables_mut) {
                        for r in rs.iter_mut() {
                            remove_by_id(r.get_mut("responses"), id);
                        }
                    }
                }
                Target::Server(_) => {}
            }
            Ok(())
        })
    }

    /// Duplicate a folder (with its routes), route, or response; returns the new ID.
    pub fn duplicate(&self, target: &Target) -> Result<String, StoreError> {
        if let Target::Server(id) = target {
            return self.duplicate_server(id);
        }
        let file = self
            .file_of(target)
            .ok_or_else(|| StoreError::NotFound(format!("{target:?}")))?;
        let mut new_root = String::new();
        self.edit(&file, |d| {
            match target {
                Target::Route(id) => {
                    let mut t = find_table(d, "routes", id)
                        .ok_or_else(|| StoreError::NotFound(id.clone()))?
                        .clone();
                    new_root = new_id("rte");
                    t["id"] = value(new_root.clone());
                    regen_response_ids(&mut t);
                    let name = t
                        .get("name")
                        .and_then(Item::as_str)
                        .map(|n| format!("{n} copy"));
                    if let Some(n) = name {
                        t["name"] = value(n);
                    }
                    array_mut(d, "routes").push(t);
                }
                Target::Response(id) => {
                    new_root = new_id("rsp");
                    let rs = d
                        .get_mut("routes")
                        .and_then(Item::as_array_of_tables_mut)
                        .ok_or_else(|| StoreError::NotFound(id.clone()))?;
                    for r in rs.iter_mut() {
                        if let Some(list) = r
                            .get_mut("responses")
                            .and_then(Item::as_array_of_tables_mut)
                        {
                            let found = list
                                .iter()
                                .find(|x| x.get("id").and_then(Item::as_str) == Some(id))
                                .cloned();
                            if let Some(src) = found {
                                let mut c = src;
                                c["id"] = value(new_root.clone());
                                c.remove("default");
                                if let Some(n) = c
                                    .get("name")
                                    .and_then(Item::as_str)
                                    .map(|n| format!("{n} copy"))
                                {
                                    c["name"] = value(n);
                                }
                                list.push(c);
                                break;
                            }
                        }
                    }
                }
                Target::Folder(id) => {
                    let mut f = find_table(d, "folders", id)
                        .ok_or_else(|| StoreError::NotFound(id.clone()))?
                        .clone();
                    new_root = new_id("fld");
                    f["id"] = value(new_root.clone());
                    if let Some(n) = f
                        .get("name")
                        .and_then(Item::as_str)
                        .map(|n| format!("{n} copy"))
                    {
                        f["name"] = value(n);
                    }
                    let copies: Vec<Table> = d
                        .get("routes")
                        .and_then(Item::as_array_of_tables)
                        .map(|rs| {
                            rs.iter()
                                .filter(|r| r.get("folder").and_then(Item::as_str) == Some(id))
                                .cloned()
                                .collect()
                        })
                        .unwrap_or_default();
                    array_mut(d, "folders").push(f);
                    for mut r in copies {
                        r["id"] = value(new_id("rte"));
                        r["folder"] = value(new_root.clone());
                        regen_response_ids(&mut r);
                        array_mut(d, "routes").push(r);
                    }
                }
                Target::Server(_) => {}
            }
            Ok(())
        })?;
        Ok(new_root)
    }

    // ---- persistence ----

    /// Write every dirty document atomically. Returns the files written.
    pub fn flush(&self) -> Result<Vec<String>, StoreError> {
        self.flush_where(|_| true)
    }

    pub(crate) fn flush_where(
        &self,
        pick: impl Fn(&Doc) -> bool,
    ) -> Result<Vec<String>, StoreError> {
        let mut st = self.lock();
        let root = st.root.clone();
        let mut written = vec![];
        for (rel, d) in st.docs.iter_mut() {
            if !d.dirty || d.conflict.is_some() || !pick(d) {
                continue;
            }
            let text = d.text();
            write_atomic(&root.join(rel), &text)?;
            d.written_hash = hash(&text);
            d.dirty = false;
            d.dirty_since = None;
            written.push(rel.clone());
        }
        Ok(written)
    }

    /// Resolve a merge conflict: keep the GUI's version or load the disk's.
    pub fn resolve_conflict(&self, file: &str, keep_mine: bool) -> Result<(), StoreError> {
        let mut st = self.lock();
        let d = st
            .docs
            .get_mut(file)
            .ok_or_else(|| StoreError::NotFound(file.into()))?;
        let Some(disk) = d.conflict.take() else {
            return Ok(());
        };
        if keep_mine {
            d.dirty = true;
            d.dirty_since = Some(Instant::now());
        } else {
            d.doc = disk.parse::<DocumentMut>().ok();
            d.raw = disk.clone();
            d.written_hash = hash(&disk);
            d.dirty = false;
            d.dirty_since = None;
        }
        notify_all(&mut st, Event::Changed);
        Ok(())
    }
}

pub(crate) fn notify_all(st: &mut State, ev: Event) {
    st.subscribers.retain(|tx| tx.send(ev.clone()).is_ok());
}

pub(crate) fn list_files(root: &Path) -> Vec<String> {
    let mut out = vec![MANIFEST.to_string()];
    for dir in ["servers", "environments"] {
        if let Ok(rd) = std::fs::read_dir(root.join(dir)) {
            let mut names: Vec<_> = rd
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".toml"))
                .collect();
            names.sort();
            out.extend(names.into_iter().map(|n| format!("{dir}/{n}")));
        }
    }
    out
}

pub(crate) fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Optional Route fields for [`Store::create_route_with`].
#[derive(Debug, Clone, Default)]
pub struct RouteOpts {
    pub match_mode: Option<String>,
    pub name: Option<String>,
    pub priority: Option<i32>,
}

fn validate_pattern(path: &str, mode: Option<&str>) -> Result<(), StoreError> {
    fauxfield_model::pattern::Pattern::parse(path, mode, false)
        .map(|_| ())
        .map_err(StoreError::Invalid)
}

pub fn error_preset(status: u16) -> Option<(&'static str, &'static str)> {
    Some(match status {
        400 => (
            "Bad Request",
            "The request was malformed or failed validation.",
        ),
        401 => ("Unauthorized", "Authentication is required."),
        403 => ("Forbidden", "You do not have access to this resource."),
        404 => ("Not Found", "The resource does not exist."),
        409 => ("Conflict", "The request conflicts with the current state."),
        422 => (
            "Unprocessable Entity",
            "The request body could not be processed.",
        ),
        429 => ("Too Many Requests", "Rate limit exceeded. Retry later."),
        500 => (
            "Internal Server Error",
            "The server hit an unexpected error.",
        ),
        502 => (
            "Bad Gateway",
            "The upstream server returned an invalid response.",
        ),
        503 => (
            "Service Unavailable",
            "The service is temporarily unavailable.",
        ),
        504 => (
            "Gateway Timeout",
            "The upstream server did not answer in time.",
        ),
        _ => return None,
    })
}

fn responses_of_mut<'a>(
    d: &'a mut DocumentMut,
    response_id: &str,
) -> Option<&'a mut ArrayOfTables> {
    d.get_mut("routes")?
        .as_array_of_tables_mut()?
        .iter_mut()
        .filter_map(|r| r.get_mut("responses")?.as_array_of_tables_mut())
        .find(|rs| rs.iter().any(|t| id_of(t) == Some(response_id)))
}

fn rules_mut(resp: &mut Table) -> Result<&mut ArrayOfTables, StoreError> {
    if resp.get("rules").is_none() {
        resp["rules"] = Item::ArrayOfTables(ArrayOfTables::new());
    }
    resp["rules"]
        .as_array_of_tables_mut()
        .ok_or_else(|| StoreError::Invalid("rules is not a table array".into()))
}

fn rule_table(r: &Rule) -> Result<Table, StoreError> {
    let mut t = Table::new();
    t["target"] = value(r.target.clone());
    if let Some(k) = r.key.as_deref().filter(|k| !k.is_empty()) {
        t["key"] = value(k);
    }
    t["op"] = value(r.op.clone());
    if let Some(v) = &r.value {
        let parsed = v
            .to_string()
            .parse::<toml_edit::Value>()
            .map_err(|e| StoreError::Invalid(format!("bad rule value: {e}")))?;
        t["value"] = Item::Value(parsed);
    }
    if r.negate {
        t["negate"] = value(true);
    }
    Ok(t)
}

/// Reassign table positions in logical (depth-first) order so reordered or inserted tables
/// print where their parents expect them, children right after their parent.
fn renumber(d: &mut DocumentMut) {
    fn walk(t: &mut Table, n: &mut usize) {
        for (_, item) in t.iter_mut() {
            match item {
                Item::Table(c) => {
                    c.set_position(*n);
                    *n += 1;
                    walk(c, n);
                }
                Item::ArrayOfTables(a) => {
                    for c in a.iter_mut() {
                        c.set_position(*n);
                        *n += 1;
                        walk(c, n);
                    }
                }
                _ => {}
            }
        }
    }
    let mut n = 0;
    walk(d.as_table_mut(), &mut n);
}

fn id_of(t: &Table) -> Option<&str> {
    t.get("id").and_then(Item::as_str)
}

fn take_all(a: &mut ArrayOfTables) -> (Vec<Table>, Vec<usize>) {
    let mut v = Vec::with_capacity(a.len());
    while !a.is_empty() {
        v.push(a.get(0).cloned().expect("non-empty"));
        a.remove(0);
    }
    let mut pos: Vec<usize> = v.iter().filter_map(Table::position).collect();
    pos.sort_unstable();
    (v, pos)
}

/// Rebuild the array in `v`'s order. toml_edit prints tables by their document position,
/// so the existing positions are reassigned in ascending order to the new sequence.
fn put_all(a: &mut ArrayOfTables, mut v: Vec<Table>, positions: Vec<usize>) {
    for (t, p) in v.iter_mut().zip(positions) {
        t.set_position(p);
    }
    for t in v {
        a.push(t);
    }
}

fn has_id(item: Option<&Item>, id: &str) -> bool {
    item.and_then(Item::as_array_of_tables).is_some_and(|a| {
        a.iter()
            .any(|t| t.get("id").and_then(Item::as_str) == Some(id))
    })
}

pub(crate) fn array_mut<'a>(d: &'a mut DocumentMut, key: &str) -> &'a mut ArrayOfTables {
    if d.get(key).and_then(Item::as_array_of_tables).is_none() {
        d[key] = Item::ArrayOfTables(ArrayOfTables::new());
    }
    d[key].as_array_of_tables_mut().expect("just created")
}

fn find_table<'a>(d: &'a mut DocumentMut, key: &str, id: &str) -> Option<&'a mut Table> {
    d.get_mut(key)?
        .as_array_of_tables_mut()?
        .iter_mut()
        .find(|t| t.get("id").and_then(Item::as_str) == Some(id))
}

pub(crate) fn resolve<'a>(d: &'a mut DocumentMut, target: &Target) -> Option<&'a mut Table> {
    match target {
        Target::Server(_) => Some(d.as_table_mut()),
        Target::Folder(id) => find_table(d, "folders", id),
        Target::Route(id) => find_table(d, "routes", id),
        Target::Response(id) => d
            .get_mut("routes")?
            .as_array_of_tables_mut()?
            .iter_mut()
            .find_map(|r| {
                r.get_mut("responses")?
                    .as_array_of_tables_mut()?
                    .iter_mut()
                    .find(|t| t.get("id").and_then(Item::as_str) == Some(id))
            }),
    }
}

fn remove_by_id(item: Option<&mut Item>, id: &str) {
    if let Some(a) = item.and_then(Item::as_array_of_tables_mut) {
        let pos = a
            .iter()
            .position(|t| t.get("id").and_then(Item::as_str) == Some(id));
        if let Some(i) = pos {
            a.remove(i);
        }
    }
}

fn regen_response_ids(route: &mut Table) {
    if let Some(rs) = route
        .get_mut("responses")
        .and_then(Item::as_array_of_tables_mut)
    {
        for r in rs.iter_mut() {
            r["id"] = value(new_id("rsp"));
        }
    }
}

fn regen_ids(d: &mut DocumentMut) {
    let mut map: Vec<(String, String)> = vec![];
    if let Some(fs) = d.get_mut("folders").and_then(Item::as_array_of_tables_mut) {
        for f in fs.iter_mut() {
            let old = f
                .get("id")
                .and_then(Item::as_str)
                .unwrap_or_default()
                .to_string();
            let new = new_id("fld");
            f["id"] = value(new.clone());
            map.push((old, new));
        }
    }
    if let Some(rs) = d.get_mut("routes").and_then(Item::as_array_of_tables_mut) {
        for r in rs.iter_mut() {
            r["id"] = value(new_id("rte"));
            if let Some(old) = r.get("folder").and_then(Item::as_str).map(str::to_string)
                && let Some((_, new)) = map.iter().find(|(o, _)| *o == old)
            {
                r["folder"] = value(new.clone());
            }
            regen_response_ids(r);
        }
    }
}

/// Per-workspace UI state (`.fauxfield/ui.json`, SPEC §13). Never fails.
pub fn load_ui_state(root: &Path) -> serde_json::Value {
    std::fs::read_to_string(root.join(".fauxfield/ui.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(serde_json::Value::Null)
}

pub fn save_ui_state(root: &Path, state: &serde_json::Value) -> std::io::Result<()> {
    let p = root.join(".fauxfield/ui.json");
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(state).unwrap_or_default(),
    )?;
    std::fs::rename(tmp, p)
}
