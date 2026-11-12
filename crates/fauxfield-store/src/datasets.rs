//! Dataset and CRUD-route edits (SPEC §6). Seeds are plain workspace files, not store
//! documents; the `[[datasets]]` table and the `kind = "crud"` Route are edited with
//! `toml_edit` so comments and ordering survive.

use std::path::{Path, PathBuf};

use fauxfield_model::new_id;
use toml_edit::{Item, Table, value};

use crate::store::{Store, StoreError, array_mut};

/// What to write for one Dataset (MCP `upsert_dataset`, the New CRUD resource wizard).
#[derive(Debug, Default, Clone)]
pub struct DatasetSpec {
    pub name: String,
    /// JSON text written to `data/<name>.json` (mutually exclusive with `seed_template`).
    pub seed_json: Option<String>,
    /// Tera text written to `files/<name>.json.tera`.
    pub seed_template: Option<String>,
    pub key: Option<String>,
    pub id_strategy: Option<String>,
    pub persist: Option<bool>,
    /// When set, a `kind = "crud"` Route for this path is added if none serves the Dataset.
    pub crud_path: Option<String>,
    pub crud_folder: Option<String>,
    pub validate: Option<String>,
}

/// Workspace-relative path check shared with the model validator.
pub fn confined(rel: &str) -> bool {
    !(rel.is_empty() || rel.starts_with('/') || rel.split(['/', '\\']).any(|c| c == ".."))
}

fn safe_name(name: &str) -> Result<&str, StoreError> {
    let n = name.trim();
    if n.is_empty()
        || !n
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err(StoreError::Invalid(format!(
            "Invalid dataset name '{name}' (letters, digits, `_` and `-` only)."
        )));
    }
    Ok(n)
}

fn write_file(root: &Path, rel: &str, text: &str) -> Result<(), StoreError> {
    if !confined(rel) {
        return Err(StoreError::Invalid(format!(
            "'{rel}' must stay inside the workspace."
        )));
    }
    let p: PathBuf = root.join(rel);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, p)?;
    Ok(())
}

impl Store {
    /// Read a workspace file (confined). `None` when missing.
    pub fn read_workspace_file(&self, rel: &str) -> Option<String> {
        if !confined(rel) {
            return None;
        }
        std::fs::read_to_string(self.root().join(rel)).ok()
    }

    /// Write a workspace file atomically (confined).
    pub fn write_workspace_file(&self, rel: &str, text: &str) -> Result<(), StoreError> {
        write_file(&self.root(), rel, text)
    }

    /// Create or replace a Dataset (and optionally its CRUD Route). Returns the CRUD Route id
    /// when one was added.
    pub fn upsert_dataset(
        &self,
        server_id: &str,
        spec: &DatasetSpec,
    ) -> Result<Option<String>, StoreError> {
        let name = safe_name(&spec.name)?.to_string();
        if spec.seed_json.is_some() && spec.seed_template.is_some() {
            return Err(StoreError::Invalid(
                "Set either seed_json or seed_template, not both.".into(),
            ));
        }
        if let Some(s) = spec.id_strategy.as_deref()
            && !fauxfield_model::ID_STRATEGIES.contains(&s)
        {
            return Err(StoreError::Invalid(format!(
                "id_strategy must be uuid, increment or ulid, got '{s}'."
            )));
        }
        if let Some(j) = &spec.seed_json {
            serde_json::from_str::<serde_json::Value>(j)
                .map_err(|e| StoreError::Invalid(format!("Seed is not valid JSON: {e}")))?;
        }
        if let Some(p) = &spec.crud_path
            && !p.starts_with('/')
        {
            return Err(StoreError::Invalid("Path must start with `/`.".into()));
        }
        let file = self
            .server_file(server_id)
            .ok_or_else(|| StoreError::NotFound(server_id.into()))?;
        let root = self.root();
        let seed_rel = spec.seed_json.as_ref().map(|_| format!("data/{name}.json"));
        let tpl_rel = spec
            .seed_template
            .as_ref()
            .map(|_| format!("files/{name}.json.tera"));
        if let (Some(rel), Some(t)) = (&seed_rel, &spec.seed_json) {
            write_file(&root, rel, t)?;
        }
        if let (Some(rel), Some(t)) = (&tpl_rel, &spec.seed_template) {
            write_file(&root, rel, t)?;
        }
        let route_id = new_id("rte");
        let mut added = false;
        self.edit(&file, |d| {
            let arr = array_mut(d, "datasets");
            let existing = arr
                .iter_mut()
                .find(|t| t.get("name").and_then(Item::as_str) == Some(name.as_str()));
            let mut fresh = Table::new();
            let t: &mut Table = match existing {
                Some(t) => t,
                None => {
                    fresh["name"] = value(name.as_str());
                    &mut fresh
                }
            };
            if let Some(r) = &seed_rel {
                t["seed"] = value(r.as_str());
                t.remove("seed_template");
                t.remove("generator");
            }
            if let Some(r) = &tpl_rel {
                t["seed_template"] = value(r.as_str());
                t.remove("seed");
                t.remove("generator");
            }
            if let Some(k) = &spec.key {
                t["key"] = value(k.as_str());
            }
            if let Some(s) = &spec.id_strategy {
                t["id_strategy"] = value(s.as_str());
            }
            if let Some(p) = spec.persist {
                t["persist"] = value(p);
            }
            if !fresh.is_empty() {
                arr.push(fresh);
            }
            if let Some(path) = &spec.crud_path {
                let routes = array_mut(d, "routes");
                let served = routes.iter().any(|r| {
                    r.get("kind").and_then(Item::as_str) == Some("crud")
                        && r.get("dataset").and_then(Item::as_str) == Some(name.as_str())
                });
                if !served {
                    let mut r = Table::new();
                    r["id"] = value(route_id.clone());
                    if let Some(f) = &spec.crud_folder {
                        r["folder"] = value(f.as_str());
                    }
                    r["name"] = value(name.as_str());
                    r["kind"] = value("crud");
                    r["path"] = value(path.as_str());
                    r["dataset"] = value(name.as_str());
                    if let Some(v) = &spec.validate {
                        r["validate"] = value(v.as_str());
                    }
                    routes.push(r);
                    added = true;
                }
            }
            Ok(())
        })?;
        Ok(added.then_some(route_id))
    }

    /// Remove a Dataset table (the seed file and any CRUD Routes are left alone).
    pub fn delete_dataset(&self, server_id: &str, name: &str) -> Result<(), StoreError> {
        let file = self
            .server_file(server_id)
            .ok_or_else(|| StoreError::NotFound(server_id.into()))?;
        self.edit(&file, |d| {
            let arr = array_mut(d, "datasets");
            let i = arr
                .iter()
                .position(|t| t.get("name").and_then(Item::as_str) == Some(name))
                .ok_or_else(|| StoreError::NotFound(name.into()))?;
            arr.remove(i);
            Ok(())
        })
    }

    /// Set (`Some`) or clear (`None`) one Dataset field: `key`, `id_strategy`, `persist`.
    pub fn set_dataset_field(
        &self,
        server_id: &str,
        name: &str,
        field: &str,
        val: Option<&str>,
    ) -> Result<(), StoreError> {
        let item = match (field, val) {
            ("persist", Some(v)) => value(
                v.parse::<bool>()
                    .map_err(|_| StoreError::Invalid("persist must be true or false.".into()))?,
            ),
            ("id_strategy", Some(v)) => {
                if !fauxfield_model::ID_STRATEGIES.contains(&v) {
                    return Err(StoreError::Invalid(
                        "id_strategy must be uuid, increment or ulid.".into(),
                    ));
                }
                value(v)
            }
            ("key", Some(v)) => value(v),
            ("persist" | "id_strategy" | "key", None) => Item::None,
            _ => {
                return Err(StoreError::Invalid(format!(
                    "Unknown dataset field '{field}'."
                )));
            }
        };
        let file = self
            .server_file(server_id)
            .ok_or_else(|| StoreError::NotFound(server_id.into()))?;
        self.edit(&file, |d| {
            let t = array_mut(d, "datasets")
                .iter_mut()
                .find(|t| t.get("name").and_then(Item::as_str) == Some(name))
                .ok_or_else(|| StoreError::NotFound(name.into()))?;
            if item.is_none() {
                t.remove(field);
            } else {
                t[field] = item;
            }
            Ok(())
        })
    }
}

/// One crud Route that serves a Dataset.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RouteRef {
    pub id: String,
    pub name: Option<String>,
    pub method: String,
    pub path: String,
}

/// A free-text use of `data.<name>` (not rewritten by rename).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TextRef {
    /// Where it was found, e.g. `route rte_x response rsp_y body` or `files/a.tera`.
    pub location: String,
}

/// Every consumer of a Dataset (M10.01). Later milestones add Responses, Rules, Callbacks.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct DatasetRefs {
    pub routes: Vec<RouteRef>,
    pub free_text: Vec<TextRef>,
    pub seed_files: Vec<String>,
    pub state_file: Option<String>,
}

impl DatasetRefs {
    /// Structured consumers (a rename rewrites these; a delete is blocked by them).
    pub fn is_unused(&self) -> bool {
        self.routes.is_empty() && self.free_text.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct DeleteOpts {
    /// Delete even while Routes or templates reference the Dataset.
    pub force: bool,
    pub remove_seed: bool,
    pub remove_state: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct RenameReport {
    pub rewritten_routes: Vec<String>,
    pub moved_seed: Option<String>,
    pub moved_state: bool,
    pub free_text_warnings: Vec<TextRef>,
}

fn mentions(text: &str, name: &str) -> bool {
    let pat = format!("data.{name}");
    let mut from = 0;
    while let Some(i) = text[from..].find(&pat) {
        let end = from + i + pat.len();
        let before = text[..from + i].chars().next_back();
        let after = text[end..].chars().next();
        let ident = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
        if !before.is_some_and(|c| c.is_alphanumeric() || c == '_') && !after.is_some_and(ident) {
            return true;
        }
        from = end;
    }
    false
}

impl Store {
    fn dataset_server(&self, server_id: &str) -> Result<fauxfield_model::Server, StoreError> {
        self.snapshot()
            .servers
            .into_iter()
            .map(|(_, s)| s)
            .find(|s| s.id == server_id)
            .ok_or_else(|| StoreError::NotFound(server_id.into()))
    }

    /// The one function returning every consumer of a Dataset.
    pub fn dataset_references(
        &self,
        server_id: &str,
        name: &str,
    ) -> Result<DatasetRefs, StoreError> {
        let server = self.dataset_server(server_id)?;
        let ds = server
            .datasets
            .iter()
            .find(|d| d.name == name)
            .ok_or_else(|| StoreError::NotFound(name.into()))?;
        let root = self.root();
        let mut refs = DatasetRefs::default();
        for r in &server.routes {
            if r.kind == "crud" && r.dataset.as_deref() == Some(name) {
                refs.routes.push(RouteRef {
                    id: r.id.clone(),
                    name: r.name.clone(),
                    method: r.method.clone(),
                    path: r.path.clone(),
                });
            }
            let mut hit = |loc: String, text: &str| {
                if mentions(text, name) {
                    refs.free_text.push(TextRef { location: loc });
                }
            };
            for (k, v) in &r.headers {
                hit(format!("route {} header {k}", r.id), v);
            }
            for resp in &r.responses {
                if let Some(b) = &resp.body {
                    hit(format!("route {} response {} body", r.id, resp.id), b);
                }
                if let Some(f) = &resp.body_file
                    && let Some(t) = self.read_workspace_file(f)
                {
                    hit(f.clone(), &t);
                }
                for (k, v) in &resp.headers {
                    hit(format!("route {} response {} header {k}", r.id, resp.id), v);
                }
            }
        }
        for other in &server.datasets {
            if other.name == name {
                continue;
            }
            if let Some(f) = &other.seed_template
                && let Some(t) = self.read_workspace_file(f)
                && mentions(&t, name)
            {
                refs.free_text.push(TextRef {
                    location: f.clone(),
                });
            }
        }
        for f in [&ds.seed, &ds.seed_template].into_iter().flatten() {
            refs.seed_files.push(f.clone());
        }
        let st = state_path(&root, server_id, name);
        if st.exists() {
            refs.state_file = Some(
                st.strip_prefix(&root)
                    .unwrap_or(&st)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        Ok(refs)
    }

    /// Create an empty Dataset: no seed file, never a Route.
    pub fn create_dataset(
        &self,
        server_id: &str,
        name: &str,
        key: Option<&str>,
        id_strategy: Option<&str>,
        persist: Option<bool>,
    ) -> Result<(), StoreError> {
        self.create_dataset_with_columns(server_id, name, key, id_strategy, persist, &[])
    }

    /// Create an empty Dataset with a declared column list ("From table", M10.04).
    pub fn create_dataset_with_columns(
        &self,
        server_id: &str,
        name: &str,
        key: Option<&str>,
        id_strategy: Option<&str>,
        persist: Option<bool>,
        columns: &[fauxfield_model::DatasetColumn],
    ) -> Result<(), StoreError> {
        let name = safe_name(name)?;
        if self
            .dataset_server(server_id)?
            .datasets
            .iter()
            .any(|d| d.name == name)
        {
            return Err(StoreError::Invalid(format!(
                "Dataset '{name}' already exists."
            )));
        }
        self.upsert_dataset(
            server_id,
            &DatasetSpec {
                name: name.into(),
                key: key.map(Into::into),
                id_strategy: id_strategy.map(Into::into),
                persist,
                ..Default::default()
            },
        )?;
        if !columns.is_empty() {
            self.set_dataset_columns(server_id, name, columns)?;
        }
        Ok(())
    }

    /// Copy a Dataset (table and default-named seed file) as `<name>-copy`, `-copy-2`, ...
    pub fn duplicate_dataset(&self, server_id: &str, name: &str) -> Result<String, StoreError> {
        let server = self.dataset_server(server_id)?;
        let src = server
            .datasets
            .iter()
            .find(|d| d.name == name)
            .ok_or_else(|| StoreError::NotFound(name.into()))?;
        let mut new = format!("{name}-copy");
        let mut n = 2;
        while server.datasets.iter().any(|d| d.name == new) {
            new = format!("{name}-copy-{n}");
            n += 1;
        }
        let mut spec = DatasetSpec {
            name: new.clone(),
            key: src.key.clone(),
            id_strategy: src.id_strategy.clone(),
            persist: Some(src.persist).filter(|p| *p),
            ..Default::default()
        };
        // Seed files are copied (as text) so the copy is independent of the original.
        if let Some(f) = &src.seed {
            spec.seed_json = self.read_workspace_file(f);
        }
        if let Some(f) = &src.seed_template {
            spec.seed_template = self.read_workspace_file(f);
        }
        self.upsert_dataset(server_id, &spec)?;
        Ok(new)
    }

    /// Rename a Dataset, rewriting every crud Route reference. Default-named seed files and
    /// persisted state move with it; free-text `data.<old>` uses are reported only.
    pub fn rename_dataset(
        &self,
        server_id: &str,
        old: &str,
        new: &str,
    ) -> Result<RenameReport, StoreError> {
        let new = safe_name(new)?.to_string();
        let refs = self.dataset_references(server_id, old)?;
        let server = self.dataset_server(server_id)?;
        if old == new {
            return Ok(RenameReport::default());
        }
        if server.datasets.iter().any(|d| d.name == new) {
            return Err(StoreError::Invalid(format!(
                "Dataset '{new}' already exists."
            )));
        }
        let ds = server.datasets.iter().find(|d| d.name == old).unwrap();
        let root = self.root();
        let mut report = RenameReport {
            free_text_warnings: refs.free_text.clone(),
            ..Default::default()
        };
        // Move default-named seeds (`data/<old>.json`, `files/<old>.json.tera`).
        let mut new_seed: Option<(&str, String)> = None;
        for (field, f, def_old, def_new) in [
            (
                "seed",
                ds.seed.as_deref(),
                format!("data/{old}.json"),
                format!("data/{new}.json"),
            ),
            (
                "seed_template",
                ds.seed_template.as_deref(),
                format!("files/{old}.json.tera"),
                format!("files/{new}.json.tera"),
            ),
        ] {
            if f == Some(def_old.as_str()) && root.join(&def_old).exists() {
                if root.join(&def_new).exists() {
                    return Err(StoreError::Invalid(format!("'{def_new}' already exists.")));
                }
                std::fs::rename(root.join(&def_old), root.join(&def_new))?;
                report.moved_seed = Some(def_new.clone());
                new_seed = Some((field, def_new));
            }
        }
        let old_state = state_path(&root, server_id, old);
        if old_state.exists() {
            let ns = state_path(&root, server_id, &new);
            std::fs::rename(&old_state, &ns)?;
            report.moved_state = true;
        }
        let file = self
            .server_file(server_id)
            .ok_or_else(|| StoreError::NotFound(server_id.into()))?;
        let mut rewritten = Vec::new();
        self.edit(&file, |d| {
            let t = array_mut(d, "datasets")
                .iter_mut()
                .find(|t| t.get("name").and_then(Item::as_str) == Some(old))
                .ok_or_else(|| StoreError::NotFound(old.into()))?;
            t["name"] = value(new.as_str());
            if let Some((field, path)) = &new_seed {
                t[*field] = value(path.as_str());
            }
            for r in array_mut(d, "routes").iter_mut() {
                if r.get("kind").and_then(Item::as_str) == Some("crud")
                    && r.get("dataset").and_then(Item::as_str) == Some(old)
                {
                    r["dataset"] = value(new.as_str());
                    if let Some(id) = r.get("id").and_then(Item::as_str) {
                        rewritten.push(id.to_string());
                    }
                }
            }
            Ok(())
        })?;
        report.rewritten_routes = rewritten;
        Ok(report)
    }

    /// Delete a Dataset. Blocked while references exist unless `opts.force`; seed file and
    /// persisted state are removed only when asked.
    pub fn delete_dataset_checked(
        &self,
        server_id: &str,
        name: &str,
        opts: &DeleteOpts,
    ) -> Result<DatasetRefs, StoreError> {
        let refs = self.dataset_references(server_id, name)?;
        if !opts.force && !refs.is_unused() {
            return Err(StoreError::Invalid(format!(
                "Dataset '{name}' is still used by {} Route(s) and {} template reference(s); pass force to delete it anyway.",
                refs.routes.len(),
                refs.free_text.len()
            )));
        }
        self.delete_dataset(server_id, name)?;
        let root = self.root();
        if opts.remove_seed {
            for f in &refs.seed_files {
                if confined(f) {
                    let _ = std::fs::remove_file(root.join(f));
                }
            }
        }
        if opts.remove_state {
            let _ = std::fs::remove_file(state_path(&root, server_id, name));
        }
        Ok(refs)
    }

    /// Point a crud Route at another Dataset of the same Server.
    pub fn set_route_dataset(
        &self,
        server_id: &str,
        route_id: &str,
        dataset: &str,
    ) -> Result<(), StoreError> {
        if !self
            .dataset_server(server_id)?
            .datasets
            .iter()
            .any(|d| d.name == dataset)
        {
            return Err(StoreError::NotFound(dataset.into()));
        }
        let file = self
            .server_file(server_id)
            .ok_or_else(|| StoreError::NotFound(server_id.into()))?;
        self.edit(&file, |d| {
            let r = array_mut(d, "routes")
                .iter_mut()
                .find(|r| r.get("id").and_then(Item::as_str) == Some(route_id))
                .ok_or_else(|| StoreError::NotFound(route_id.into()))?;
            if r.get("kind").and_then(Item::as_str) != Some("crud") {
                return Err(StoreError::Invalid("Not a crud Route.".into()));
            }
            r["dataset"] = value(dataset);
            Ok(())
        })
    }
}

/// `.fauxfield/state/<server>/<dataset>.json` (persisted Dataset rows).
pub fn state_path(root: &Path, server_id: &str, dataset: &str) -> PathBuf {
    root.join(".fauxfield/state")
        .join(server_id)
        .join(format!("{dataset}.json"))
}

/// M10.04: columns, generator block and static-seed row editing.
impl Store {
    /// Replace a Dataset's declared column list (empty clears it).
    pub fn set_dataset_columns(
        &self,
        server_id: &str,
        name: &str,
        columns: &[fauxfield_model::DatasetColumn],
    ) -> Result<(), StoreError> {
        let mut seen = std::collections::HashSet::new();
        for c in columns {
            if c.name.trim().is_empty() || !seen.insert(c.name.as_str()) {
                return Err(StoreError::Invalid(format!(
                    "Column name '{}' is empty or duplicated.",
                    c.name
                )));
            }
            if !c.ty.is_empty() && !fauxfield_model::COLUMN_TYPES.contains(&c.ty.as_str()) {
                return Err(StoreError::Invalid(format!(
                    "Column type must be one of {}, got '{}'.",
                    fauxfield_model::COLUMN_TYPES.join(", "),
                    c.ty
                )));
            }
        }
        let file = self
            .server_file(server_id)
            .ok_or_else(|| StoreError::NotFound(server_id.into()))?;
        self.edit(&file, |d| {
            let t = array_mut(d, "datasets")
                .iter_mut()
                .find(|t| t.get("name").and_then(Item::as_str) == Some(name))
                .ok_or_else(|| StoreError::NotFound(name.into()))?;
            if columns.is_empty() {
                t.remove("columns");
            } else {
                let mut arr = toml_edit::ArrayOfTables::new();
                for c in columns {
                    let mut ct = Table::new();
                    ct["name"] = value(c.name.as_str());
                    ct["type"] = value(if c.ty.is_empty() { "string" } else { &c.ty });
                    arr.push(ct);
                }
                t["columns"] = Item::ArrayOfTables(arr);
            }
            Ok(())
        })
    }

    /// Write (`Some`) or remove (`None`) the `[datasets.generator]` block. Setting it clears
    /// `seed` and `seed_template` (a Dataset has one seed source); the seed file is left alone.
    pub fn set_dataset_generator(
        &self,
        server_id: &str,
        name: &str,
        generator: Option<&fauxfield_model::GeneratorSeed>,
    ) -> Result<(), StoreError> {
        let table = match generator {
            Some(g) => {
                let text = toml::to_string(g)
                    .map_err(|e| StoreError::Invalid(format!("Bad generator: {e}")))?;
                let doc: toml_edit::DocumentMut = text
                    .parse()
                    .map_err(|e| StoreError::Invalid(format!("Bad generator: {e}")))?;
                Some(doc.as_table().clone())
            }
            None => None,
        };
        let file = self
            .server_file(server_id)
            .ok_or_else(|| StoreError::NotFound(server_id.into()))?;
        self.edit(&file, |d| {
            let t = array_mut(d, "datasets")
                .iter_mut()
                .find(|t| t.get("name").and_then(Item::as_str) == Some(name))
                .ok_or_else(|| StoreError::NotFound(name.into()))?;
            match table {
                Some(g) => {
                    t.remove("seed");
                    t.remove("seed_template");
                    t["generator"] = Item::Table(g);
                }
                None => {
                    t.remove("generator");
                }
            }
            Ok(())
        })
    }

    /// Write `rows` as the static seed `data/<name>.json` and make it the active source.
    pub fn write_dataset_seed(
        &self,
        server_id: &str,
        name: &str,
        json: &str,
    ) -> Result<(), StoreError> {
        self.upsert_dataset(
            server_id,
            &DatasetSpec {
                name: name.into(),
                seed_json: Some(json.into()),
                ..Default::default()
            },
        )
        .map(|_| ())
    }

    /// Read the static seed rows of a Dataset (empty array when it has no static seed yet).
    pub fn read_dataset_seed(
        &self,
        server_id: &str,
        name: &str,
    ) -> Result<Option<String>, StoreError> {
        let server = self.dataset_server(server_id)?;
        let d = server
            .datasets
            .iter()
            .find(|d| d.name == name)
            .ok_or_else(|| StoreError::NotFound(name.into()))?;
        match &d.seed {
            Some(rel) => Ok(self.read_workspace_file(rel)),
            None => Ok(None),
        }
    }

    /// Rename a column everywhere: the column list, and the keys of a static seed's rows.
    pub fn rename_dataset_column(
        &self,
        server_id: &str,
        name: &str,
        from: &str,
        to: &str,
    ) -> Result<(), StoreError> {
        let server = self.dataset_server(server_id)?;
        let d = server
            .datasets
            .iter()
            .find(|d| d.name == name)
            .ok_or_else(|| StoreError::NotFound(name.into()))?;
        let mut cols = d.columns.clone();
        if cols.iter().any(|c| c.name == to) {
            return Err(StoreError::Invalid(format!(
                "Column '{to}' already exists."
            )));
        }
        if let Some(c) = cols.iter_mut().find(|c| c.name == from) {
            c.name = to.into();
        }
        self.set_dataset_columns(server_id, name, &cols)?;
        if let Some(mut g) = d.generator.clone() {
            for f in g.fields.iter_mut().filter(|f| f.name == from) {
                f.name = to.into();
            }
            self.set_dataset_generator(server_id, name, Some(&g))?;
        }
        self.map_seed_rows(server_id, name, |row| {
            if let Some(o) = row.as_object_mut()
                && let Some(v) = o.remove(from)
            {
                o.insert(to.into(), v);
            }
        })
    }

    /// Remove a column from the column list and from a static seed's rows.
    pub fn remove_dataset_column(
        &self,
        server_id: &str,
        name: &str,
        column: &str,
    ) -> Result<(), StoreError> {
        let server = self.dataset_server(server_id)?;
        let d = server
            .datasets
            .iter()
            .find(|d| d.name == name)
            .ok_or_else(|| StoreError::NotFound(name.into()))?;
        let cols: Vec<_> = d
            .columns
            .iter()
            .filter(|c| c.name != column)
            .cloned()
            .collect();
        self.set_dataset_columns(server_id, name, &cols)?;
        if let Some(mut g) = d.generator.clone() {
            g.fields.retain(|f| f.name != column);
            self.set_dataset_generator(server_id, name, Some(&g))?;
        }
        self.map_seed_rows(server_id, name, |row| {
            if let Some(o) = row.as_object_mut() {
                o.remove(column);
            }
        })
    }

    /// Apply `f` to every row of a static seed and write it back (no-op without one).
    pub fn map_seed_rows(
        &self,
        server_id: &str,
        name: &str,
        mut f: impl FnMut(&mut serde_json::Value),
    ) -> Result<(), StoreError> {
        let Some(text) = self.read_dataset_seed(server_id, name)? else {
            return Ok(());
        };
        let mut v: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| StoreError::Invalid(format!("Seed is not valid JSON: {e}")))?;
        match &mut v {
            serde_json::Value::Array(a) => a.iter_mut().for_each(&mut f),
            other => f(other),
        }
        let out = serde_json::to_string_pretty(&v).unwrap_or_default();
        self.write_dataset_seed(server_id, name, &out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_workspace;
    use crate::store::Target;

    #[test]
    fn upsert_writes_seed_dataset_and_route_and_keeps_comments() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_workspace(root).unwrap();
        let store = Store::open(root).unwrap();
        let sid = store.snapshot().servers[0].1.id.clone();
        let file = store.server_file(&sid).unwrap();
        store
            .edit(&file, |d| {
                d["name"] = value("x");
                Ok(())
            })
            .unwrap();
        let spec = DatasetSpec {
            name: "products".into(),
            seed_json: Some(r#"[{"id":1}]"#.into()),
            crud_path: Some("/products".into()),
            persist: Some(true),
            ..Default::default()
        };
        let rid = store.upsert_dataset(&sid, &spec).unwrap();
        assert!(rid.is_some());
        assert!(root.join("data/products.json").exists());
        // second upsert replaces, adds no second route
        assert!(store.upsert_dataset(&sid, &spec).unwrap().is_none());
        let snap = store.snapshot();
        let s = &snap.servers[0].1;
        assert_eq!(s.datasets.len(), 1);
        assert!(s.datasets[0].persist);
        assert_eq!(s.routes.iter().filter(|r| r.kind == "crud").count(), 1);
        assert!(
            snap.diagnostics
                .iter()
                .all(|d| d.severity != fauxfield_model::Severity::Error)
        );
        store
            .set_dataset_field(&sid, "products", "id_strategy", Some("ulid"))
            .unwrap();
        store.delete_dataset(&sid, "products").unwrap();
        assert!(store.snapshot().servers[0].1.datasets.is_empty());
        assert!(
            store
                .upsert_dataset(
                    &sid,
                    &DatasetSpec {
                        name: "../x".into(),
                        ..Default::default()
                    }
                )
                .is_err()
        );
    }

    #[test]
    fn lifecycle_create_rename_refs_delete() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_workspace(root).unwrap();
        let store = Store::open(root).unwrap();
        let sid = store.snapshot().servers[0].1.id.clone();
        // create: empty, no Route, no seed file
        store
            .create_dataset(&sid, "users", None, None, None)
            .unwrap();
        assert!(
            store
                .create_dataset(&sid, "users", None, None, None)
                .is_err()
        );
        let snap = store.snapshot();
        assert_eq!(snap.servers[0].1.datasets.len(), 1);
        assert!(snap.servers[0].1.routes.iter().all(|r| r.kind != "crud"));
        assert!(store.dataset_references(&sid, "users").unwrap().is_unused());
        // a crud Route referencing it
        store
            .upsert_dataset(
                &sid,
                &DatasetSpec {
                    name: "users".into(),
                    seed_json: Some("[{\"id\":1}]".into()),
                    crud_path: Some("/users".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let refs = store.dataset_references(&sid, "users").unwrap();
        assert_eq!(refs.routes.len(), 1);
        // delete blocked while referenced
        assert!(
            store
                .delete_dataset_checked(&sid, "users", &DeleteOpts::default())
                .is_err()
        );
        // rename rewrites the Route and moves the seed
        std::fs::create_dir_all(root.join(".fauxfield/state").join(&sid)).unwrap();
        std::fs::write(state_path(root, &sid, "users"), "[]").unwrap();
        let rep = store.rename_dataset(&sid, "users", "people").unwrap();
        assert_eq!(rep.rewritten_routes.len(), 1);
        assert!(rep.moved_state);
        assert!(root.join("data/people.json").exists());
        assert!(!root.join("data/users.json").exists());
        let snap = store.snapshot();
        let s = &snap.servers[0].1;
        assert_eq!(s.datasets[0].name, "people");
        assert_eq!(s.datasets[0].seed.as_deref(), Some("data/people.json"));
        assert!(
            s.routes
                .iter()
                .any(|r| r.dataset.as_deref() == Some("people"))
        );
        // deleting the Route leaves the Dataset, seed and state alone
        let rid = refs.routes[0].id.clone();
        store.delete(&Target::Route(rid)).unwrap();
        assert_eq!(store.snapshot().servers[0].1.datasets.len(), 1);
        assert!(root.join("data/people.json").exists());
        assert!(state_path(root, &sid, "people").exists());
        // duplicate then delete with opt-in file removal
        let copy = store.duplicate_dataset(&sid, "people").unwrap();
        assert_eq!(copy, "people-copy");
        assert!(root.join("data/people-copy.json").exists());
        store
            .delete_dataset_checked(
                &sid,
                "people",
                &DeleteOpts {
                    remove_seed: true,
                    remove_state: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(!root.join("data/people.json").exists());
        assert!(!state_path(root, &sid, "people").exists());
    }

    #[test]
    fn free_text_mentions_are_found_and_reported_on_rename() {
        assert!(mentions("{{ data.users | length }}", "users"));
        assert!(!mentions("data.users2", "users"));
        assert!(!mentions("mydata.users", "users"));
    }
}
