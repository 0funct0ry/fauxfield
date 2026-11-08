use std::collections::{BTreeMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::Instant;

use fauxfield_model::{
    Diagnostic, Environment, Manifest, Server, new_id, parse_environment, parse_manifest,
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
    fn parse(text: String) -> Doc {
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

const MANIFEST: &str = "fauxfield.toml";

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
        let port = doc.get("port").and_then(Item::as_integer).unwrap_or(3000) + 1;
        doc["id"] = value(new.clone());
        doc["name"] = value(format!("{name} copy"));
        doc["port"] = value(port);
        regen_ids(&mut doc);
        let mut st = self.lock();
        let slug = slugify(&format!("{name} copy"));
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

    /// Make one response the default of its route.
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

fn has_id(item: Option<&Item>, id: &str) -> bool {
    item.and_then(Item::as_array_of_tables).is_some_and(|a| {
        a.iter()
            .any(|t| t.get("id").and_then(Item::as_str) == Some(id))
    })
}

fn array_mut<'a>(d: &'a mut DocumentMut, key: &str) -> &'a mut ArrayOfTables {
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

fn resolve<'a>(d: &'a mut DocumentMut, target: &Target) -> Option<&'a mut Table> {
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
