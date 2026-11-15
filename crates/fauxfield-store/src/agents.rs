//! `fauxfield agents init` (SPEC §11.3, M16): the file plan shared by the CLI and the GUI
//! "Agents: Set Up MCP…" dialog. [`plan`] reads and computes; [`apply`] writes atomically
//! (temp files, then renames, restoring everything if one step fails).

use std::{
    fmt,
    path::{Path, PathBuf},
};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Number;
use toml_edit::{Array, DocumentMut, Item, Table, value};

pub const MARK_BEGIN: &str = "<!-- fauxfield:begin -->";
pub const MARK_END: &str = "<!-- fauxfield:end -->";
pub const ENTRY: &str = "fauxfield";

/// Every tool the generated AGENTS.md section names. A test in `fauxfield-mcp` checks each
/// against the registry so the names cannot drift.
pub const TOOL_NAMES: &[&str] = &[
    "workspace_info",
    "list_servers",
    "get_server",
    "create_server",
    "update_server",
    "delete_server",
    "list_routes",
    "get_route",
    "upsert_route",
    "delete_route",
    "validate",
    "start_server",
    "stop_server",
    "restart_server",
    "get_traffic",
    "send_test_request",
    "match_path",
    "render_template",
    "set_variable",
    "upsert_dataset",
    "reset_state",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Client {
    Claude,
    Codex,
    Cursor,
}

impl Client {
    pub const ALL: [Client; 3] = [Client::Claude, Client::Codex, Client::Cursor];
    pub fn id(self) -> &'static str {
        match self {
            Client::Claude => "claude",
            Client::Codex => "codex",
            Client::Cursor => "cursor",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Client::Claude => "Claude Code",
            Client::Codex => "Codex",
            Client::Cursor => "Cursor",
        }
    }
    fn file(self) -> &'static str {
        match self {
            Client::Claude => ".mcp.json",
            Client::Codex => ".codex/config.toml",
            Client::Cursor => ".cursor/mcp.json",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Create,
    Merge,
    Append,
    Unchanged,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub clients: Vec<Client>,
    /// Tool classes for `--allow`: any of read, write, execute.
    pub allow: Vec<String>,
    /// Command written into the client config (`fauxfield`, or an absolute path).
    pub command: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            clients: vec![Client::Claude],
            allow: vec!["read".into(), "write".into(), "execute".into()],
            command: "fauxfield".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FilePlan {
    /// Workspace-relative path with `/` separators.
    pub path: String,
    pub action: Action,
    pub diff: String,
    #[serde(skip)]
    pub after: String,
    #[serde(skip)]
    before: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub files: Vec<FilePlan>,
}

impl Plan {
    /// Files that would change.
    pub fn changes(&self) -> impl Iterator<Item = &FilePlan> {
        self.files.iter().filter(|f| f.action != Action::Unchanged)
    }
    pub fn is_noop(&self) -> bool {
        self.changes().next().is_none()
    }
}

#[derive(Debug)]
pub struct AgentsError(pub String);
impl fmt::Display for AgentsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for AgentsError {}

fn err<T>(m: impl Into<String>) -> Result<T, AgentsError> {
    Err(AgentsError(m.into()))
}

/// Clients whose config is already present: `.mcp.json`, `.cursor/`, `~/.codex`.
pub fn detect_clients(root: &Path, home: Option<&Path>) -> Vec<Client> {
    let mut v = Vec::new();
    if root.join(".mcp.json").exists() {
        v.push(Client::Claude);
    }
    if root.join(".codex").exists() || home.is_some_and(|h| h.join(".codex").exists()) {
        v.push(Client::Codex);
    }
    if root.join(".cursor").exists() {
        v.push(Client::Cursor);
    }
    v
}

// ---- order-preserving JSON, so a merge keeps the user's key order ----

#[derive(Debug, Clone, PartialEq)]
enum J {
    Null,
    Bool(bool),
    Num(Number),
    Str(String),
    Arr(Vec<J>),
    Obj(IndexMap<String, J>),
}

impl<'de> Deserialize<'de> for J {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = J;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any JSON value")
            }
            fn visit_unit<E>(self) -> Result<J, E> {
                Ok(J::Null)
            }
            fn visit_bool<E>(self, b: bool) -> Result<J, E> {
                Ok(J::Bool(b))
            }
            fn visit_i64<E>(self, n: i64) -> Result<J, E> {
                Ok(J::Num(n.into()))
            }
            fn visit_u64<E>(self, n: u64) -> Result<J, E> {
                Ok(J::Num(n.into()))
            }
            fn visit_f64<E>(self, n: f64) -> Result<J, E> {
                Ok(Number::from_f64(n).map_or(J::Null, J::Num))
            }
            fn visit_str<E>(self, s: &str) -> Result<J, E> {
                Ok(J::Str(s.into()))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut a: A) -> Result<J, A::Error> {
                let mut v = Vec::new();
                while let Some(x) = a.next_element()? {
                    v.push(x);
                }
                Ok(J::Arr(v))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut a: A) -> Result<J, A::Error> {
                let mut m = IndexMap::new();
                while let Some((k, v)) = a.next_entry()? {
                    m.insert(k, v);
                }
                Ok(J::Obj(m))
            }
        }
        d.deserialize_any(V)
    }
}

impl Serialize for J {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            J::Null => s.serialize_unit(),
            J::Bool(b) => s.serialize_bool(*b),
            J::Num(n) => n.serialize(s),
            J::Str(x) => s.serialize_str(x),
            J::Arr(a) => a.serialize(s),
            J::Obj(m) => m.serialize(s),
        }
    }
}

fn entry_json(opts: &Options) -> J {
    let allow = opts.allow.join(",");
    J::Obj(IndexMap::from([
        ("command".to_owned(), J::Str(opts.command.clone())),
        (
            "args".to_owned(),
            J::Arr(
                ["mcp", ".", "--allow", allow.as_str()]
                    .iter()
                    .map(|s| J::Str((*s).into()))
                    .collect(),
            ),
        ),
    ]))
}

fn merge_json(before: Option<&str>, opts: &Options, file: &str) -> Result<String, AgentsError> {
    let mut root = match before {
        None => J::Obj(IndexMap::new()),
        Some(t) if t.trim().is_empty() => J::Obj(IndexMap::new()),
        Some(t) => serde_json::from_str::<J>(t)
            .map_err(|e| AgentsError(format!("{file} is not valid JSON: {e}")))?,
    };
    let J::Obj(top) = &mut root else {
        return err(format!("{file} must contain a JSON object"));
    };
    let servers = top
        .entry("mcpServers".into())
        .or_insert_with(|| J::Obj(IndexMap::new()));
    let J::Obj(servers) = servers else {
        return err(format!("`mcpServers` in {file} must be an object"));
    };
    servers.insert(ENTRY.into(), entry_json(opts));
    let mut out = serde_json::to_string_pretty(&root).unwrap_or_default();
    out.push('\n');
    Ok(out)
}

fn merge_toml(before: Option<&str>, opts: &Options, file: &str) -> Result<String, AgentsError> {
    let mut doc: DocumentMut = before
        .unwrap_or("")
        .parse()
        .map_err(|e| AgentsError(format!("{file} is not valid TOML: {e}")))?;
    let servers = doc
        .entry("mcp_servers")
        .or_insert_with(|| {
            let mut t = Table::new();
            t.set_implicit(true);
            Item::Table(t)
        })
        .as_table_mut()
        .ok_or_else(|| AgentsError(format!("`mcp_servers` in {file} must be a table")))?;
    let mut t = Table::new();
    t["command"] = value(opts.command.as_str());
    let mut args = Array::new();
    for a in ["mcp", ".", "--allow", opts.allow.join(",").as_str()] {
        args.push(a);
    }
    t["args"] = value(args);
    servers.insert(ENTRY, Item::Table(t));
    Ok(doc.to_string())
}

/// The AGENTS.md section, generated from one template so the tool names stay in sync.
pub fn agents_section() -> String {
    let tools = TOOL_NAMES
        .iter()
        .map(|t| format!("`{t}`"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{MARK_BEGIN}\n\
## Fauxfield mock APIs\n\n\
This folder is a Fauxfield **Workspace**: plain TOML files that define mock **Servers**.\n\n\
- `fauxfield.toml`: the Workspace manifest and global variables.\n\
- `servers/<slug>.toml`: one **Server** per file, holding **Folders**, **Routes**, **Responses**, **Rules** and **Callbacks**.\n\
- `environments/<name>.toml`: **Environments** with **Variables**.\n\
- `data/`: **Datasets** for stateful CRUD routes.\n\n\
Use the Fauxfield MCP tools instead of editing files by hand: {tools}.\n\n\
To mock an endpoint, loop:\n\n\
1. `upsert_route` once per Route (method, path, Responses).\n\
2. `start_server` for the Server.\n\
3. `send_test_request` for each Route to verify the Response.\n\
4. `get_traffic` to confirm the requests were recorded.\n\n\
Run headless with `fauxfield run`. For a read-only agent, start the server with `fauxfield mcp --read-only`.\n\
Use the Fauxfield vocabulary (Workspace, Server, Folder, Route, Response, Rule, Callback, Dataset, Environment, Variable, Recording, Template).\n\
{MARK_END}\n"
    )
}

fn merge_agents_md(before: Option<&str>) -> (String, Action) {
    let section = agents_section();
    match before {
        None => (section, Action::Create),
        Some(t) => {
            if let (Some(b), Some(e)) = (t.find(MARK_BEGIN), t.find(MARK_END))
                && e > b
            {
                let end = e + MARK_END.len();
                let end = if t[end..].starts_with('\n') {
                    end + 1
                } else {
                    end
                };
                (
                    format!("{}{}{}", &t[..b], section, &t[end..]),
                    Action::Merge,
                )
            } else {
                let sep = if t.is_empty() || t.ends_with("\n\n") {
                    ""
                } else if t.ends_with('\n') {
                    "\n"
                } else {
                    "\n\n"
                };
                (format!("{t}{sep}{section}"), Action::Append)
            }
        }
    }
}

fn merge_gitignore(before: Option<&str>) -> (String, Action) {
    match before {
        None => (".fauxfield/\n".into(), Action::Create),
        Some(t) => {
            if t.lines()
                .any(|l| matches!(l.trim(), ".fauxfield" | ".fauxfield/" | "/.fauxfield/"))
            {
                (t.to_owned(), Action::Unchanged)
            } else {
                let sep = if t.is_empty() || t.ends_with('\n') {
                    ""
                } else {
                    "\n"
                };
                (format!("{t}{sep}.fauxfield/\n"), Action::Append)
            }
        }
    }
}

fn read(root: &Path, rel: &str) -> Result<Option<String>, AgentsError> {
    match std::fs::read_to_string(root.join(rel)) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => err(format!("cannot read {rel}: {e}")),
    }
}

fn file_plan(rel: &str, before: Option<String>, after: String, hint: Option<Action>) -> FilePlan {
    let action = if before.as_deref() == Some(after.as_str()) {
        Action::Unchanged
    } else if let Some(h) = hint {
        h
    } else if before.is_none() {
        Action::Create
    } else {
        Action::Merge
    };
    let diff = if action == Action::Unchanged {
        String::new()
    } else {
        let old = before.clone().unwrap_or_default();
        let a = if before.is_some() {
            format!("a/{rel}")
        } else {
            "/dev/null".into()
        };
        similar::TextDiff::from_lines(old.as_str(), after.as_str())
            .unified_diff()
            .context_radius(3)
            .header(&a, &format!("b/{rel}"))
            .to_string()
    };
    FilePlan {
        path: rel.into(),
        action,
        diff,
        after,
        before,
    }
}

/// Compute what `agents init` would write. Nothing touches the disk.
pub fn plan(root: &Path, opts: &Options) -> Result<Plan, AgentsError> {
    let mut files = Vec::new();
    for c in &opts.clients {
        let rel = c.file();
        let before = read(root, rel)?;
        let after = match c {
            Client::Codex => merge_toml(before.as_deref(), opts, rel)?,
            _ => merge_json(before.as_deref(), opts, rel)?,
        };
        files.push(file_plan(rel, before, after, None));
    }
    let before = read(root, "AGENTS.md")?;
    let (after, hint) = merge_agents_md(before.as_deref());
    files.push(file_plan("AGENTS.md", before, after, Some(hint)));
    let before = read(root, ".gitignore")?;
    let (after, hint) = merge_gitignore(before.as_deref());
    files.push(file_plan(".gitignore", before, after, Some(hint)));
    Ok(Plan { files })
}

/// Write every changed file: temp files first, then renames. If any step fails everything
/// already written is restored (or removed, for files that did not exist).
pub fn apply(root: &Path, plan: &Plan) -> Result<Vec<String>, AgentsError> {
    apply_with(root, plan, None)
}

pub fn apply_with(
    root: &Path,
    plan: &Plan,
    fail_at: Option<usize>,
) -> Result<Vec<String>, AgentsError> {
    let changes: Vec<&FilePlan> = plan.changes().collect();
    let mut temps: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut made_dirs: Vec<PathBuf> = Vec::new();
    let cleanup_temps = |temps: &[(PathBuf, PathBuf)]| {
        for (t, _) in temps {
            let _ = std::fs::remove_file(t);
        }
    };
    // Stage.
    for f in &changes {
        let dest = root.join(&f.path);
        if let Some(dir) = dest.parent()
            && !dir.exists()
        {
            if let Err(e) = std::fs::create_dir_all(dir) {
                cleanup_temps(&temps);
                return err(format!("cannot create {}: {e}", dir.display()));
            }
            made_dirs.push(dir.to_path_buf());
        }
        let tmp = dest.with_file_name(format!(
            ".{}.fauxfield-tmp",
            dest.file_name().and_then(|n| n.to_str()).unwrap_or("file")
        ));
        if let Err(e) = std::fs::write(&tmp, &f.after) {
            cleanup_temps(&temps);
            return err(format!("cannot write {}: {e}", f.path));
        }
        temps.push((tmp, dest));
    }
    // Commit.
    for (i, (tmp, dest)) in temps.iter().enumerate() {
        let r = if fail_at == Some(i) {
            Err(std::io::Error::other("injected failure"))
        } else {
            std::fs::rename(tmp, dest)
        };
        if let Err(e) = r {
            // Restore the ones already renamed.
            for f in changes.iter().take(i) {
                let d = root.join(&f.path);
                match &f.before {
                    Some(b) => {
                        let _ = std::fs::write(&d, b);
                    }
                    None => {
                        let _ = std::fs::remove_file(&d);
                    }
                }
            }
            cleanup_temps(&temps);
            for d in made_dirs.iter().rev() {
                let _ = std::fs::remove_dir(d);
            }
            return err(format!(
                "cannot write {}: {e}; nothing was changed",
                changes[i].path
            ));
        }
    }
    Ok(changes.iter().map(|f| f.path.clone()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(c: &[Client]) -> Options {
        Options {
            clients: c.to_vec(),
            ..Options::default()
        }
    }

    fn tree(root: &Path) -> Vec<(String, String)> {
        let mut v = Vec::new();
        fn walk(d: &Path, root: &Path, v: &mut Vec<(String, String)>) {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, root, v);
                } else {
                    v.push((
                        p.strip_prefix(root).unwrap().to_string_lossy().into(),
                        std::fs::read_to_string(&p).unwrap_or_default(),
                    ));
                }
            }
        }
        walk(root, root, &mut v);
        v.sort();
        v
    }

    #[test]
    fn fresh_workspace_creates_files_and_rerun_is_unchanged() {
        let d = tempfile::tempdir().unwrap();
        let o = opts(&[Client::Claude, Client::Codex, Client::Cursor]);
        let p = plan(d.path(), &o).unwrap();
        let acts: Vec<_> = p
            .files
            .iter()
            .map(|f| (f.path.as_str(), f.action))
            .collect();
        assert_eq!(
            acts,
            [
                (".mcp.json", Action::Create),
                (".codex/config.toml", Action::Create),
                (".cursor/mcp.json", Action::Create),
                ("AGENTS.md", Action::Create),
                (".gitignore", Action::Create),
            ]
        );
        assert!(p.files[0].diff.contains("+++ b/.mcp.json"));
        apply(d.path(), &p).unwrap();
        let mcp = std::fs::read_to_string(d.path().join(".mcp.json")).unwrap();
        assert!(mcp.contains("\"--allow\"") && mcp.contains("read,write,execute"));
        let again = plan(d.path(), &o).unwrap();
        assert!(
            again.is_noop(),
            "{:?}",
            again.files.iter().map(|f| f.action).collect::<Vec<_>>()
        );
    }

    #[test]
    fn json_merge_keeps_other_servers_and_key_order() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join(".mcp.json"),
            "{\n  \"zeta\": 1,\n  \"mcpServers\": {\n    \"other\": {\"command\": \"x\"}\n  },\n  \"alpha\": true\n}\n",
        )
        .unwrap();
        let p = plan(d.path(), &opts(&[Client::Claude])).unwrap();
        assert_eq!(p.files[0].action, Action::Merge);
        let after = &p.files[0].after;
        assert!(after.find("zeta") < after.find("mcpServers"));
        assert!(after.find("mcpServers") < after.find("alpha"));
        assert!(after.contains("\"other\"") && after.contains("\"fauxfield\""));
    }

    #[test]
    fn toml_merge_keeps_comments_and_other_servers() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join(".codex")).unwrap();
        std::fs::write(
            d.path().join(".codex/config.toml"),
            "# my config\nmodel = \"x\"\n\n[mcp_servers.other]\ncommand = \"y\"\n",
        )
        .unwrap();
        let p = plan(d.path(), &opts(&[Client::Codex])).unwrap();
        let after = &p.files[0].after;
        assert!(after.contains("# my config") && after.contains("[mcp_servers.other]"));
        assert!(after.contains("[mcp_servers.fauxfield]"));
        assert!(after.contains("args = [\"mcp\", \".\", \"--allow\", \"read,write,execute\"]"));
    }

    #[test]
    fn agents_md_section_is_replaced_in_place() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("AGENTS.md"),
            format!("# Mine\n\n{MARK_BEGIN}\nSTALE-TEXT\n{MARK_END}\n\nafter\n"),
        )
        .unwrap();
        let p = plan(d.path(), &opts(&[])).unwrap();
        let f = p.files.iter().find(|f| f.path == "AGENTS.md").unwrap();
        assert_eq!(f.action, Action::Merge);
        assert!(f.after.starts_with("# Mine\n\n") && f.after.ends_with("\n\nafter\n"));
        assert!(!f.after.contains("STALE-TEXT"));
        std::fs::write(d.path().join("AGENTS.md"), "# Plain\n").unwrap();
        let p = plan(d.path(), &opts(&[])).unwrap();
        let f = p.files.iter().find(|f| f.path == "AGENTS.md").unwrap();
        assert_eq!(f.action, Action::Append);
    }

    #[test]
    fn section_names_the_loop_tools_and_vocabulary() {
        let s = agents_section();
        for t in [
            "upsert_route",
            "start_server",
            "send_test_request",
            "get_traffic",
        ] {
            assert!(s.contains(&format!("`{t}`")), "{t}");
        }
        assert!(s.contains("Workspace") && s.contains("Recording"));
    }

    #[test]
    fn invalid_existing_json_is_an_error_not_a_clobber() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(".mcp.json"), "{ nope").unwrap();
        assert!(plan(d.path(), &opts(&[Client::Claude])).is_err());
    }

    #[test]
    fn failed_write_restores_everything() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(".gitignore"), "target/\n").unwrap();
        let before = tree(d.path());
        let o = opts(&[Client::Claude, Client::Cursor]);
        let p = plan(d.path(), &o).unwrap();
        // Fail on the 3rd rename (AGENTS.md), after .mcp.json and .cursor/mcp.json landed.
        assert!(apply_with(d.path(), &p, Some(2)).is_err());
        assert_eq!(tree(d.path()), before);
        assert!(!d.path().join(".cursor").exists());
    }

    #[test]
    fn detects_clients() {
        let d = tempfile::tempdir().unwrap();
        assert!(detect_clients(d.path(), None).is_empty());
        std::fs::write(d.path().join(".mcp.json"), "{}").unwrap();
        std::fs::create_dir(d.path().join(".cursor")).unwrap();
        assert_eq!(
            detect_clients(d.path(), None),
            [Client::Claude, Client::Cursor]
        );
    }
}
