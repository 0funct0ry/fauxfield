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
            }
            if let Some(r) = &tpl_rel {
                t["seed_template"] = value(r.as_str());
                t.remove("seed");
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

/// `.fauxfield/state/<server>/<dataset>.json` (persisted Dataset rows).
pub fn state_path(root: &Path, server_id: &str, dataset: &str) -> PathBuf {
    root.join(".fauxfield/state")
        .join(server_id)
        .join(format!("{dataset}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_workspace;

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
}
