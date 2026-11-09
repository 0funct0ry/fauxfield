//! Environment and variable edits plus scope resolution (SPEC §4.3, §8.1).
//! Values are written exactly as given, so `${env:NAME}` is never expanded on disk.

use std::time::Instant;

use fauxfield_model::vars::{Layers, ResolvedVar, Scope, parse_dotenv, resolve};
use toml_edit::{DocumentMut, Item, Table, value};

use crate::init::slugify;
use crate::store::{
    Doc, Event, Snapshot, State, Store, StoreError, Target, notify_all, resolve as target,
};

fn infer(v: &str) -> Item {
    if let Ok(b) = v.parse::<bool>() {
        return value(b);
    }
    if let Ok(i) = v.parse::<i64>() {
        return value(i);
    }
    value(v)
}

fn env_file_in(st: &State, name: &str) -> Option<String> {
    st.docs
        .iter()
        .filter(|(f, _)| f.starts_with("environments/"))
        .find_map(|(f, d)| {
            let doc = d.doc.as_ref()?;
            let n = doc
                .get("name")
                .and_then(Item::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| {
                    f.trim_start_matches("environments/")
                        .trim_end_matches(".toml")
                        .to_string()
                });
            (n == name).then(|| f.clone())
        })
}

fn unique_rel(st: &State, slug: &str) -> String {
    let mut rel = format!("environments/{slug}.toml");
    let mut n = 2;
    while st.docs.contains_key(&rel) || st.root.join(&rel).exists() {
        rel = format!("environments/{slug}-{n}.toml");
        n += 1;
    }
    rel
}

fn put_doc(st: &mut State, rel: String, text: String) {
    let mut d = Doc::parse(text);
    d.dirty = true;
    d.dirty_since = Some(Instant::now());
    d.written_hash = 0;
    st.docs.insert(rel, d);
}

impl Store {
    pub fn environment_file(&self, name: &str) -> Option<String> {
        env_file_in(&self.lock(), name)
    }

    pub fn create_environment(&self, name: &str) -> Result<String, StoreError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(StoreError::Invalid("Name cannot be empty.".into()));
        }
        let mut st = self.lock();
        if env_file_in(&st, name).is_some() {
            return Err(StoreError::Invalid(format!(
                "Environment '{name}' already exists."
            )));
        }
        let rel = unique_rel(&st, &slugify(name));
        let text = format!("name = {}\n\n[variables]\n", value(name).to_string().trim());
        put_doc(&mut st, rel.clone(), text);
        notify_all(&mut st, Event::Changed);
        Ok(rel)
    }

    pub fn duplicate_environment(&self, name: &str) -> Result<String, StoreError> {
        let file = self
            .environment_file(name)
            .ok_or_else(|| StoreError::NotFound(name.into()))?;
        let mut doc: DocumentMut = self
            .text(&file)
            .unwrap_or_default()
            .parse()
            .map_err(|e| StoreError::Invalid(format!("{e}")))?;
        let mut st = self.lock();
        let mut new = format!("{name} copy");
        let mut k = 2;
        while env_file_in(&st, &new).is_some() {
            new = format!("{name} copy {k}");
            k += 1;
        }
        doc["name"] = value(new.as_str());
        let rel = unique_rel(&st, &slugify(&new));
        put_doc(&mut st, rel, doc.to_string());
        notify_all(&mut st, Event::Changed);
        Ok(new)
    }

    pub fn delete_environment(&self, name: &str) -> Result<(), StoreError> {
        let file = self
            .environment_file(name)
            .ok_or_else(|| StoreError::NotFound(name.into()))?;
        let mut st = self.lock();
        let path = st.root.join(&file);
        st.docs.remove(&file);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        notify_all(&mut st, Event::Changed);
        Ok(())
    }

    /// Rename: sets `name`; the file follows the slug. `default_environment` follows too.
    pub fn rename_environment(&self, old: &str, new: &str) -> Result<(), StoreError> {
        let new = new.trim();
        if new.is_empty() {
            return Err(StoreError::Invalid("Name cannot be empty.".into()));
        }
        if old != new && self.environment_file(new).is_some() {
            return Err(StoreError::Invalid(format!(
                "Environment '{new}' already exists."
            )));
        }
        let file = self
            .environment_file(old)
            .ok_or_else(|| StoreError::NotFound(old.into()))?;
        self.edit(&file, |d| {
            d["name"] = value(new);
            Ok(())
        })?;
        if self.snapshot().manifest.default_environment.as_deref() == Some(old) {
            self.set_default_environment(Some(new))?;
        }
        self.flush()?;
        let mut st = self.lock();
        let rel = unique_rel(&st, &slugify(new));
        if rel == file || rel.replace("-2", "") == file {
            return Ok(());
        }
        std::fs::rename(st.root.join(&file), st.root.join(&rel))?;
        if let Some(d) = st.docs.remove(&file) {
            st.docs.insert(rel, d);
        }
        notify_all(&mut st, Event::Changed);
        Ok(())
    }

    pub fn set_default_environment(&self, name: Option<&str>) -> Result<(), StoreError> {
        self.edit(crate::store::MANIFEST, |d| {
            match name {
                Some(n) => d["default_environment"] = value(n),
                None => {
                    d.remove("default_environment");
                }
            }
            Ok(())
        })
    }

    /// Set (`Some`) or remove (`None`) a variable at a file scope. `owner` is the Environment
    /// name, Server ID, or Route ID; global needs none. Runtime scope is not stored in files.
    pub fn set_variable(
        &self,
        scope: Scope,
        owner: Option<&str>,
        key: &str,
        val: Option<&str>,
    ) -> Result<(), StoreError> {
        if key.is_empty()
            || !key
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
        {
            return Err(StoreError::Invalid(format!(
                "Invalid variable name '{key}'."
            )));
        }
        let need = |what: &str| {
            owner.ok_or_else(|| {
                StoreError::Invalid(format!("--scope {} needs a {what}.", scope.as_str()))
            })
        };
        let (file, tgt) = match scope {
            Scope::Global => (crate::store::MANIFEST.to_string(), None),
            Scope::Environment => {
                let n = need("environment name")?;
                (
                    self.environment_file(n)
                        .ok_or_else(|| StoreError::NotFound(n.into()))?,
                    None,
                )
            }
            Scope::Server => {
                let id = need("server")?;
                (
                    self.server_file(id)
                        .ok_or_else(|| StoreError::NotFound(id.into()))?,
                    Some(Target::Server(id.into())),
                )
            }
            Scope::Route => {
                let id = need("route")?;
                let t = Target::Route(id.into());
                (
                    self.file_of(&t)
                        .ok_or_else(|| StoreError::NotFound(id.into()))?,
                    Some(t),
                )
            }
            Scope::Runtime => {
                return Err(StoreError::Invalid(
                    "Runtime variables are not stored in files.".into(),
                ));
            }
        };
        self.edit(&file, |d| {
            let t: &mut Table = match &tgt {
                Some(t) => target(d, t).ok_or_else(|| StoreError::NotFound(format!("{t:?}")))?,
                None => d.as_table_mut(),
            };
            match val {
                Some(v) => {
                    if t.get("variables").and_then(Item::as_table).is_none() {
                        t["variables"] = Item::Table(Table::new());
                    }
                    t["variables"][key] = infer(v);
                }
                None => {
                    if let Some(vs) = t.get_mut("variables").and_then(Item::as_table_mut) {
                        vs.remove(key);
                    }
                }
            }
            Ok(())
        })
    }
}

impl Snapshot {
    pub fn environment(&self, name: &str) -> Option<&fauxfield_model::Environment> {
        self.environments
            .iter()
            .map(|(_, e)| e)
            .find(|e| e.name == name)
    }

    /// Dotenv values of an Environment (empty when unset, missing, or outside the workspace).
    pub fn dotenv(&self, env: &str) -> std::collections::BTreeMap<String, String> {
        let Some(p) = self.environment(env).and_then(|e| e.dotenv.as_deref()) else {
            return Default::default();
        };
        let path = std::path::Path::new(p);
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Default::default();
        }
        std::fs::read_to_string(self.root.join(path))
            .map(|t| parse_dotenv(&t))
            .unwrap_or_default()
    }

    /// Variables in scope for a Server (and optionally a Route), with scope and source.
    pub fn vars_in_scope(
        &self,
        env: Option<&str>,
        server_id: Option<&str>,
        route_id: Option<&str>,
        runtime: Option<&std::collections::BTreeMap<String, String>>,
    ) -> Vec<ResolvedVar> {
        let server =
            server_id.and_then(|id| self.servers.iter().map(|(_, s)| s).find(|s| s.id == id));
        let route = route_id.and_then(|id| {
            self.servers
                .iter()
                .flat_map(|(_, s)| s.routes.iter())
                .find(|r| r.id == id)
        });
        let e = env.and_then(|n| self.environment(n));
        let layers = Layers {
            global: Some(&self.manifest.variables),
            environment: e.map(|e| &e.variables),
            server: server.map(|s| &s.variables),
            route: route.map(|r| &r.variables),
            runtime,
            dotenv: env.map(|n| self.dotenv(n)).unwrap_or_default(),
        };
        resolve(&layers, &|n| std::env::var(n).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_and_variable_edits_keep_comments_and_raw() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("environments")).unwrap();
        std::fs::write(root.join("fauxfield.toml"), "schema = 1\nname = \"w\"\n").unwrap();
        std::fs::write(
            root.join("environments/local.toml"),
            "# keep me\nname = \"local\"\ndotenv = \".env\"\n[variables]\nk = \"${env:ZZ_NOPE}\"\n",
        )
        .unwrap();
        std::fs::write(root.join(".env"), "A=1\n").unwrap();
        let s = Store::open(root).unwrap();
        s.set_variable(Scope::Environment, Some("local"), "base", Some("http://x"))
            .unwrap();
        s.set_variable(Scope::Global, None, "g", Some("1")).unwrap();
        s.create_environment("staging").unwrap();
        assert!(s.create_environment("staging").is_err());
        s.flush().unwrap();
        let t = std::fs::read_to_string(root.join("environments/local.toml")).unwrap();
        assert!(t.contains("# keep me") && t.contains("${env:ZZ_NOPE}") && t.contains("base"));
        let snap = s.snapshot();
        assert_eq!(snap.dotenv("local").get("A").map(String::as_str), Some("1"));
        let v = snap.vars_in_scope(Some("local"), None, None, None);
        assert!(v.iter().any(|x| x.key == "g" && x.scope == Scope::Global));
        s.rename_environment("staging", "stage2").unwrap();
        assert!(root.join("environments/stage2.toml").exists());
        s.delete_environment("stage2").unwrap();
        assert!(
            s.set_variable(Scope::Global, None, "bad key", Some("1"))
                .is_err()
        );
    }
}
