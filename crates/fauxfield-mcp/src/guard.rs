//! Guardrails (SPEC §11.1, M16): redaction of secret values in every result, the workspace path
//! jail, size caps, and the Agent activity log.

use std::{
    collections::VecDeque,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use serde_json::Value;

pub const MASK: &str = "***";
/// `get_traffic` never returns more than this many rows.
pub const TRAFFIC_LIMIT_MAX: usize = 500;
/// Bodies longer than this are cut and flagged `truncated: true`.
pub const BODY_CAP: usize = 64 * 1024;

/// Does a variable or key name look secret? Built-ins plus `[mcp] redact` globs (`*session*`).
pub fn is_secret_name(name: &str, extra: &[String]) -> bool {
    let n = name.to_ascii_lowercase();
    if ["token", "secret", "password", "key"]
        .iter()
        .any(|s| n.contains(s))
    {
        return true;
    }
    extra
        .iter()
        .any(|g| glob_match(&g.to_ascii_lowercase(), &n))
}

fn glob_match(glob: &str, text: &str) -> bool {
    let parts: Vec<&str> = glob.split('*').collect();
    if parts.len() == 1 {
        return glob == text;
    }
    let mut pos = 0usize;
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() {
            continue;
        }
        match text[pos..].find(p) {
            Some(at) => {
                if i == 0 && at != 0 {
                    return false;
                }
                pos += at + p.len();
            }
            None => return false,
        }
    }
    match parts.last() {
        Some(l) if !l.is_empty() => text.ends_with(l),
        _ => true,
    }
}

/// Supplies the current secret values (variables whose names look secret).
pub type SecretSource = Arc<dyn Fn() -> Vec<String> + Send + Sync>;

#[derive(Clone, Default)]
pub struct Redactor {
    source: Option<SecretSource>,
    pub extra: Vec<String>,
}

impl Redactor {
    pub fn new(source: SecretSource, extra: Vec<String>) -> Self {
        Self {
            source: Some(source),
            extra,
        }
    }
    fn secrets(&self) -> Vec<String> {
        let mut s: Vec<String> = self
            .source
            .as_ref()
            .map(|f| f())
            .unwrap_or_default()
            .into_iter()
            .filter(|v| v.len() >= 3)
            .collect();
        // Longest first so a secret containing another is masked whole.
        s.sort_by_key(|v| std::cmp::Reverse(v.len()));
        s.dedup();
        s
    }
    pub fn text(&self, s: &str) -> String {
        let mut out = s.to_owned();
        for sec in self.secrets() {
            if out.contains(&sec) {
                out = out.replace(&sec, MASK);
            }
        }
        out
    }
    /// Mask secret values in every string (keys included) of a JSON value.
    pub fn value(&self, v: Value) -> Value {
        let secrets = self.secrets();
        if secrets.is_empty() {
            return v;
        }
        mask(v, &secrets)
    }
}

fn mask_str(s: &str, secrets: &[String]) -> String {
    let mut out = s.to_owned();
    for sec in secrets {
        if out.contains(sec.as_str()) {
            out = out.replace(sec.as_str(), MASK);
        }
    }
    out
}

fn mask(v: Value, secrets: &[String]) -> Value {
    match v {
        Value::String(s) => Value::String(mask_str(&s, secrets)),
        Value::Array(a) => Value::Array(a.into_iter().map(|x| mask(x, secrets)).collect()),
        Value::Object(o) => Value::Object(
            o.into_iter()
                .map(|(k, x)| (mask_str(&k, secrets), mask(x, secrets)))
                .collect(),
        ),
        other => other,
    }
}

/// Cut `body` at [`BODY_CAP`] bytes on a char boundary. Returns whether it was cut.
pub fn cap_body(body: &mut String) -> bool {
    if body.len() <= BODY_CAP {
        return false;
    }
    let mut end = BODY_CAP;
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    body.truncate(end);
    true
}

/// Walk a result and cap every string field named `body` (recordings, responses).
pub fn cap_bodies(v: &mut Value) {
    match v {
        Value::Object(o) => {
            let mut cut = false;
            for (k, x) in o.iter_mut() {
                if k == "body"
                    && let Value::String(s) = x
                {
                    cut |= cap_body(s);
                    continue;
                }
                cap_bodies(x);
            }
            if cut {
                o.insert("truncated".into(), Value::Bool(true));
            }
        }
        Value::Array(a) => a.iter_mut().for_each(cap_bodies),
        _ => {}
    }
}

/// Resolve `arg` against the workspace root and refuse anything that leaves it: absolute
/// paths outside, `..` escapes and symlinks pointing out. The path need not exist yet.
pub fn jail(root: &Path, arg: &str) -> Result<PathBuf, String> {
    let canon_root = root
        .canonicalize()
        .map_err(|e| format!("workspace root unreadable: {e}"))?;
    let p = Path::new(arg);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        canon_root.join(p)
    };
    // Lexical normalisation first, so `a/../../x` is caught even when `a` does not exist.
    let mut norm = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                if !norm.pop() {
                    return Err(format!("path `{arg}` is outside the workspace"));
                }
            }
            Component::CurDir => {}
            other => norm.push(other.as_os_str()),
        }
    }
    // Then resolve symlinks of the longest existing prefix.
    let mut existing = norm.clone();
    let mut tail = Vec::new();
    while !existing.exists() {
        match existing.file_name() {
            Some(n) => tail.push(n.to_owned()),
            None => break,
        }
        if !existing.pop() {
            break;
        }
    }
    let mut resolved = existing
        .canonicalize()
        .map_err(|_| format!("path `{arg}` cannot be resolved"))?;
    resolved.extend(tail.into_iter().rev());
    if resolved.starts_with(&canon_root) {
        Ok(resolved)
    } else {
        Err(format!("path `{arg}` is outside the workspace"))
    }
}

/// Argument names that carry a file or folder path inside the workspace. `body_file` and `dir`
/// are checked only (they stay workspace-relative in the TOML); `file` (imports) is resolved to
/// an absolute path inside the workspace.
const JAILED_KEYS: [&str; 3] = ["body_file", "dir", "file"];

/// Check every path-like argument against the workspace jail; returns the arguments to run with.
pub fn jail_args(
    root: &Path,
    args: serde_json::Map<String, Value>,
) -> Result<serde_json::Map<String, Value>, String> {
    fn walk(root: &Path, v: &mut Value) -> Result<(), String> {
        match v {
            Value::Object(o) => {
                for (k, x) in o.iter_mut() {
                    if JAILED_KEYS.contains(&k.as_str())
                        && let Value::String(s) = x
                    {
                        let resolved = jail(root, s)?;
                        if k == "file" {
                            *s = resolved.to_string_lossy().into_owned();
                        }
                    } else {
                        walk(root, x)?;
                    }
                }
                Ok(())
            }
            Value::Array(a) => a.iter_mut().try_for_each(|x| walk(root, x)),
            _ => Ok(()),
        }
    }
    let mut v = Value::Object(args);
    walk(root, &mut v)?;
    match v {
        Value::Object(o) => Ok(o),
        _ => Ok(Default::default()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Created,
    Updated,
    Deleted,
    Started,
    Read,
    Denied,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct Activity {
    pub ts_ms: u64,
    pub client: String,
    pub tool: String,
    pub target: String,
    pub outcome: Outcome,
}

/// Where agent sessions and activity go besides the local log: the host (SPEC §11.1, M16).
pub trait AgentSink: Send + Sync {
    fn attach(&self, id: &str, name: &str);
    fn detach(&self, id: &str);
    fn record(&self, a: &Activity);
}

/// In-memory Agent activity log (cap 500 entries). Never holds secret values: callers pass
/// already-redacted targets.
#[derive(Clone, Default)]
pub struct ActivityLog {
    inner: Arc<Mutex<VecDeque<Activity>>>,
}

const LOG_CAP: usize = 500;

impl ActivityLog {
    pub fn record(&self, client: &str, tool: &str, target: String, outcome: Outcome) {
        let ts_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mut q = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if q.len() == LOG_CAP {
            q.pop_front();
        }
        q.push_back(Activity {
            ts_ms,
            client: client.into(),
            tool: tool.into(),
            target,
            outcome,
        });
    }
    pub fn snapshot(&self) -> Vec<Activity> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .cloned()
            .collect()
    }
}

/// Short human target for a tool call (`id`, `server`, `name` or `path` argument).
pub fn target_of(args: &serde_json::Map<String, Value>) -> String {
    for k in ["id", "route", "server", "name", "path", "key"] {
        if let Some(Value::String(s)) = args.get(k) {
            return s.clone();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn secret_names_and_extras() {
        assert!(is_secret_name("API_KEY", &[]));
        assert!(is_secret_name("auth_Token", &[]));
        assert!(!is_secret_name("host", &[]));
        assert!(is_secret_name("my_session_id", &["*session*".into()]));
        assert!(!is_secret_name("my_session_id", &[]));
    }

    #[test]
    fn redacts_values_everywhere() {
        let r = Redactor::new(Arc::new(|| vec!["s3cr3t-value".into()]), vec![]);
        let v = r.value(json!({"a": "x s3cr3t-value y", "b": [{"s3cr3t-value": 1}], "n": 3}));
        assert_eq!(v, json!({"a": "x *** y", "b": [{"***": 1}], "n": 3}));
        assert_eq!(r.text("boom s3cr3t-value"), "boom ***");
    }

    #[test]
    fn caps_bodies() {
        let mut v = json!({"rows": [{"body": "x".repeat(BODY_CAP + 10)}]});
        cap_bodies(&mut v);
        assert_eq!(v["rows"][0]["body"].as_str().unwrap().len(), BODY_CAP);
        assert_eq!(v["rows"][0]["truncated"], json!(true));
    }

    #[test]
    fn jail_args_checks_nested_paths() {
        let d = tempfile::tempdir().unwrap();
        let ok = json!({"responses": [{"body_file": "files/a.json"}], "file": "data/x.csv"});
        let out = jail_args(d.path(), ok.as_object().unwrap().clone()).unwrap();
        assert_eq!(out["responses"][0]["body_file"], "files/a.json");
        assert!(out["file"].as_str().unwrap().ends_with("data/x.csv"));
        assert!(out["file"].as_str().unwrap().starts_with('/') || cfg!(windows));
        for bad in [
            json!({"responses": [{"body_file": "../../etc/passwd"}]}),
            json!({"file": "/etc/passwd"}),
            json!({"dir": "../x"}),
        ] {
            assert!(jail_args(d.path(), bad.as_object().unwrap().clone()).is_err());
        }
    }

    #[test]
    fn jail_rejects_escapes() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::create_dir(root.join("sub")).unwrap();
        assert!(jail(root, "sub/file.json").is_ok());
        assert!(jail(root, "new/dir/file").is_ok());
        assert!(jail(root, "../x").is_err());
        assert!(jail(root, "sub/../../x").is_err());
        assert!(jail(root, "/etc/passwd").is_err());
        assert!(jail(root, &root.join("sub").to_string_lossy()).is_ok());
        #[cfg(unix)]
        {
            let out = tempfile::tempdir().unwrap();
            std::os::unix::fs::symlink(out.path(), root.join("link")).unwrap();
            assert!(jail(root, "link/secret").is_err());
        }
    }
}
