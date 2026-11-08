//! Scenarios (SPEC §11.4): YAML files of steps that drive the app through the tool layer, with
//! assertions, screenshot and audit checkpoints. The runner is GPUI-free and calls tools through
//! a `ToolCaller`, so the app (`ui.run_scenario`, in-process) and the shell
//! (`fauxfield mcp scenario`, over a socket) share one implementation.

use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{
    class::{AllowedClasses, ToolClass},
    condition::Condition,
    registry::{CallError, Observer, Registry, ToolDef},
};

/// `- click_element: rt_health` or `- click_element: {id: rt_health}`.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IdArg {
    Plain(String),
    Obj { id: String },
}

impl IdArg {
    pub fn id(&self) -> &str {
        match self {
            IdArg::Plain(s) | IdArg::Obj { id: s } => s,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct ScreenshotArgs {
    /// Label used in the file name and the report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Crop to this element id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    /// A named state or a list of steps to run first (as for `ui.screenshot`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<Value>,
}

/// One scenario step. In YAML each is a one-key map: `- key: secondary-k`.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// `ui.key`.
    Key(String),
    /// `ui.type_text`.
    TypeText(String),
    /// `ui.invoke_action` by action name.
    Invoke(String),
    /// `ui.command` object.
    Command(Map<String, Value>),
    /// `ui.click_element`.
    ClickElement(IdArg),
    /// `ui.open_tab`.
    Open { kind: String, id: String },
    /// `ui.open_workspace`.
    OpenWorkspace(String),
    /// `ui.set_theme`.
    SetTheme(String),
    /// `ui.resize`.
    Resize(String),
    /// `ui.wait_for_idle`; the value is the tool's arguments (`{}` for the default).
    WaitForIdle(Map<String, Value>),
    /// `ui.wait_for`.
    WaitFor {
        condition: Condition,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u64>,
    },
    /// Run a named state or list of steps from `fixtures/ui-states` (no capture), e.g.
    /// `state: traffic-10k` or `state: [reset]`.
    State(Value),
    /// `ui.find`; keeps the result for a following `assert_find`.
    Find(Map<String, Value>),
    /// Assert on `ui.state` with a JSON pointer and `equals` / `contains` / `matches` / `not`.
    Assert(Condition),
    /// Assert on the last `find` result (same keys as `assert`), and/or `ids: [...]` for the
    /// exact ids matched. Any `find` arguments (`label`, `role`, `text`) run a fresh find.
    AssertFind(Map<String, Value>),
    /// `ui.screenshot` checkpoint; saved by `--out` and returned by `ui.run_scenario`.
    Screenshot(ScreenshotArgs),
    /// `ui.audit` checkpoint; fails when errors exceed `max_errors` (default 0). Other keys are
    /// `ui.audit` arguments (`themes`, `sizes`, `states`, `rules`, `subtree`).
    Audit(Map<String, Value>),
    /// Wait for an event of `kind` in `ui.events` newer than the scenario start or last wait.
    WaitEvent {
        kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u64>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct Scenario {
    pub name: String,
    /// Workspace folder for `ui.open_workspace`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// `default`, `compact`, `min` or `WxH`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    /// Start from the baseline view (`reset` state). Default true.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub reset: bool,
    pub steps: Vec<Step>,
}

fn yes() -> bool {
    true
}
fn is_true(b: &bool) -> bool {
    *b
}

impl Scenario {
    /// Parses scenario YAML. Goes through JSON values because the YAML crate writes enums as tags.
    pub fn from_yaml(text: &str) -> Result<Self, String> {
        let v: Value = serde_yaml_ng::from_str(text).map_err(|e| format!("invalid YAML: {e}"))?;
        serde_json::from_value(v).map_err(|e| format!("invalid scenario: {e}"))
    }

    pub fn to_yaml(&self) -> Result<String, String> {
        let v = serde_json::to_value(self).map_err(|e| e.to_string())?;
        serde_yaml_ng::to_string(&v).map_err(|e| e.to_string())
    }
}

fn label(step: &Step) -> String {
    serde_json::to_value(step)
        .ok()
        .and_then(|v| v.as_object().and_then(|o| o.keys().next().cloned()))
        .unwrap_or_else(|| "step".into())
}

// ---------------------------------------------------------------------------------------------
// Running

pub type CallFuture<'a> = Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'a>>;

/// How the runner reaches tools. Results may carry a top-level `_images` array
/// (`[{data: base64, mime}]`), as `ui.screenshot` returns in-process.
pub trait ToolCaller: Send + Sync {
    fn call<'a>(&'a self, name: &'a str, args: Map<String, Value>) -> CallFuture<'a>;
}

/// Calls tools of a registry in-process (the registry is filled in after the tools that hold
/// this caller are registered, hence the `OnceLock`).
pub struct RegistryCaller {
    pub registry: Arc<OnceLock<Registry>>,
}

impl ToolCaller for RegistryCaller {
    fn call<'a>(&'a self, name: &'a str, args: Map<String, Value>) -> CallFuture<'a> {
        Box::pin(async move {
            let reg = self.registry.get().ok_or("registry not ready")?;
            reg.call(AllowedClasses::all().with(ToolClass::Ui), name, args)
                .await
                .map_err(|e| match e {
                    CallError::Tool(m) => m,
                    CallError::Unknown(n) => format!("unknown tool `{n}`"),
                    CallError::Refused { name, class } => {
                        format!("tool `{name}` ({class}) is not allowed")
                    }
                })
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RunOptions {
    /// `false` skips every image grab (hosts without screen capture).
    pub capture: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self { capture: true }
    }
}

#[derive(Debug, Serialize)]
pub struct StepReport {
    pub index: usize,
    pub step: String,
    pub ok: bool,
    pub ms: u64,
}

#[derive(Debug, Serialize)]
pub struct Failure {
    pub index: usize,
    pub step: Value,
    pub expected: Value,
    pub actual: Value,
    /// `ui.state` at the time of failure.
    pub state: Option<Value>,
    /// Label of the failure screenshot in `images`, if one was taken.
    pub screenshot: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Image {
    pub label: String,
    /// Base64 PNG.
    pub data: String,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub name: String,
    pub passed: bool,
    pub steps: Vec<StepReport>,
    pub failure: Option<Failure>,
    pub elapsed_ms: u64,
    /// Checkpoint and failure screenshots; excluded from the JSON (written to files or returned
    /// as image blocks instead).
    #[serde(skip)]
    pub images: Vec<Image>,
}

struct Miss {
    expected: Value,
    actual: Value,
}

fn miss(expected: impl Into<Value>, actual: impl Into<Value>) -> Miss {
    Miss {
        expected: expected.into(),
        actual: actual.into(),
    }
}

fn fail_err(e: String) -> Miss {
    miss("the step succeeds", e)
}

fn obj(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

const CONDITION_KEYS: [&str; 5] = ["pointer", "equals", "contains", "matches", "not"];

struct Run<'a> {
    caller: &'a dyn ToolCaller,
    opts: RunOptions,
    last_find: Option<Value>,
    event_cursor: Option<u64>,
    images: Vec<Image>,
}

impl Run<'_> {
    async fn tool(&self, name: &str, args: Map<String, Value>) -> Result<Value, Miss> {
        self.caller.call(name, args).await.map_err(fail_err)
    }

    /// Moves `_images` out of a tool result into the run's images.
    fn take_images(&mut self, v: &mut Value, label: &str) {
        let Some(arr) = v.as_object_mut().and_then(|o| o.remove("_images")) else {
            return;
        };
        for (i, img) in arr.as_array().into_iter().flatten().enumerate() {
            if let Some(data) = img.get("data").and_then(Value::as_str) {
                let label = if i == 0 {
                    label.to_string()
                } else {
                    format!("{label}-{i}")
                };
                self.images.push(Image {
                    label,
                    data: data.to_owned(),
                });
            }
        }
    }

    async fn step(&mut self, index: usize, step: &Step) -> Result<(), Miss> {
        match step {
            Step::Key(k) => self
                .tool("ui.key", obj(json!({ "keys": k })))
                .await
                .map(drop),
            Step::TypeText(t) => self
                .tool("ui.type_text", obj(json!({ "text": t })))
                .await
                .map(drop),
            Step::Invoke(n) => self
                .tool("ui.invoke_action", obj(json!({ "name": n })))
                .await
                .map(drop),
            Step::Command(m) => self.tool("ui.command", m.clone()).await.map(drop),
            Step::ClickElement(i) => self
                .tool("ui.click_element", obj(json!({ "id": i.id() })))
                .await
                .map(drop),
            Step::Open { kind, id } => self
                .tool("ui.open_tab", obj(json!({ "kind": kind, "id": id })))
                .await
                .map(drop),
            Step::OpenWorkspace(p) => self
                .tool("ui.open_workspace", obj(json!({ "path": p })))
                .await
                .map(drop),
            Step::SetTheme(n) => self
                .tool("ui.set_theme", obj(json!({ "name": n })))
                .await
                .map(drop),
            Step::Resize(s) => self
                .tool("ui.resize", obj(json!({ "size": s })))
                .await
                .map(drop),
            Step::WaitForIdle(m) => self.tool("ui.wait_for_idle", m.clone()).await.map(drop),
            Step::WaitFor {
                condition,
                timeout_ms,
            } => {
                let mut a = obj(json!({ "condition": condition }));
                if let Some(t) = timeout_ms {
                    a.insert("timeout_ms".into(), json!(t));
                }
                self.tool("ui.wait_for", a).await.map(drop)
            }
            Step::State(state) => self
                .tool(
                    "ui.screenshot",
                    obj(json!({ "state": state, "capture": false })),
                )
                .await
                .map(drop),
            Step::Find(m) => {
                self.settle().await;
                let r = self.tool("ui.find", m.clone()).await?;
                self.last_find = Some(r);
                Ok(())
            }
            Step::Assert(cond) => {
                let state = self.tool("ui.state", Map::new()).await?;
                let hit = cond.eval(&state).map_err(fail_err)?;
                if hit {
                    Ok(())
                } else {
                    Err(miss(
                        serde_json::to_value(cond).unwrap_or(Value::Null),
                        state.pointer(&cond.pointer).cloned().unwrap_or(Value::Null),
                    ))
                }
            }
            Step::AssertFind(m) => self.assert_find(m.clone()).await,
            Step::Screenshot(a) => {
                let mut args = Map::new();
                if let Some(e) = &a.element {
                    args.insert("element".into(), json!(e));
                }
                if let Some(s) = &a.state {
                    args.insert("state".into(), s.clone());
                }
                if !self.opts.capture {
                    args.insert("capture".into(), json!(false));
                }
                let mut r = self.tool("ui.screenshot", args).await?;
                let name = a.name.clone().unwrap_or_else(|| "shot".into());
                self.take_images(&mut r, &format!("{:02}-{name}", index + 1));
                Ok(())
            }
            Step::Audit(m) => {
                let mut m = m.clone();
                let max = m.remove("max_errors").and_then(|v| v.as_u64()).unwrap_or(0);
                let r = self.tool("ui.audit", m).await?;
                let errors = r["summary"]["errors"].as_u64().unwrap_or(0);
                if errors <= max {
                    return Ok(());
                }
                Err(miss(
                    format!("at most {max} audit errors"),
                    json!({ "summary": r["summary"], "errors": error_findings(&r) }),
                ))
            }
            Step::WaitEvent { kind, timeout_ms } => {
                let mut a = obj(json!({
                    "kinds": [kind],
                    "wait_ms": timeout_ms.unwrap_or(5000),
                }));
                if let Some(c) = self.event_cursor {
                    a.insert("since".into(), json!(c));
                }
                let r = self.tool("ui.events", a).await?;
                if let Some(l) = r["last_seq"].as_u64() {
                    self.event_cursor = Some(l);
                }
                if r["events"].as_array().is_some_and(|e| !e.is_empty()) {
                    Ok(())
                } else {
                    Err(miss(format!("an event of kind `{kind}`"), r))
                }
            }
        }
    }

    /// The tree reads the last painted frame, so let the UI paint what the last input did.
    async fn settle(&self) {
        let _ = self.caller.call("ui.wait_for_idle", Map::new()).await;
    }

    async fn assert_find(&mut self, mut m: Map<String, Value>) -> Result<(), Miss> {
        let ids = m.remove("ids");
        let mut cond = Map::new();
        for k in CONDITION_KEYS {
            if let Some(v) = m.remove(k) {
                cond.insert(k.into(), v);
            }
        }
        if !m.is_empty() {
            self.settle().await;
        }
        let result = if m.is_empty() && self.last_find.is_some() {
            self.last_find.clone().unwrap_or(Value::Null)
        } else {
            let r = self.tool("ui.find", m).await?;
            self.last_find = Some(r.clone());
            r
        };
        if let Some(ids) = ids {
            let got: Vec<Value> = result["matches"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|e| e["id"].clone())
                .collect();
            if Value::Array(got.clone()) != ids {
                return Err(miss(ids, Value::Array(got)));
            }
        }
        if !cond.is_empty() {
            let c: Condition = serde_json::from_value(Value::Object(cond))
                .map_err(|e| fail_err(format!("bad assert_find: {e}")))?;
            if !c.eval(&result).map_err(fail_err)? {
                return Err(miss(
                    serde_json::to_value(&c).unwrap_or(Value::Null),
                    result.pointer(&c.pointer).cloned().unwrap_or(Value::Null),
                ));
            }
        }
        Ok(())
    }
}

fn error_findings(audit: &Value) -> Vec<Value> {
    let is_err = |f: &&Value| {
        f["severity"]
            .as_str()
            .is_some_and(|s| s.eq_ignore_ascii_case("error"))
    };
    let mut out: Vec<Value> = audit["global"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(is_err)
        .cloned()
        .collect();
    for c in audit["cells"].as_array().into_iter().flatten() {
        out.extend(
            c["findings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(is_err)
                .map(|f| json!({ "cell": c["label"], "finding": f })),
        );
    }
    out.truncate(10);
    out
}

/// Runs a scenario. The first failing step stops the run and is reported with the expected and
/// actual value, `ui.state`, and (unless `capture` is off) a screenshot.
pub async fn run(sc: &Scenario, caller: &dyn ToolCaller, opts: RunOptions) -> Report {
    let started = Instant::now();
    let mut steps: Vec<Step> = Vec::new();
    if sc.reset {
        steps.push(Step::State(json!(["reset"])));
    }
    if let Some(w) = &sc.workspace {
        steps.push(Step::OpenWorkspace(w.clone()));
    }
    if let Some(s) = &sc.size {
        steps.push(Step::Resize(s.clone()));
    }
    steps.extend(sc.steps.iter().cloned());

    let mut run = Run {
        caller,
        opts,
        last_find: None,
        event_cursor: None,
        images: Vec::new(),
    };
    // The cursor for `wait_event`: only events after the scenario started count.
    if let Ok(r) = caller.call("ui.events", Map::new()).await {
        run.event_cursor = r["last_seq"].as_u64();
    }
    let mut reports = Vec::new();
    let mut failure = None;
    for (i, step) in steps.iter().enumerate() {
        let t = Instant::now();
        let res = run.step(i, step).await;
        let ok = res.is_ok();
        reports.push(StepReport {
            index: i + 1,
            step: label(step),
            ok,
            ms: t.elapsed().as_millis() as u64,
        });
        if let Err(m) = res {
            let state = caller.call("ui.state", Map::new()).await.ok();
            let mut shot = None;
            if opts.capture
                && let Ok(mut r) = caller.call("ui.screenshot", Map::new()).await
            {
                let before = run.images.len();
                run.take_images(&mut r, "failure");
                if run.images.len() > before {
                    shot = Some("failure".to_string());
                }
            }
            failure = Some(Failure {
                index: i + 1,
                step: serde_json::to_value(step).unwrap_or(Value::Null),
                expected: m.expected,
                actual: m.actual,
                state,
                screenshot: shot,
            });
            break;
        }
    }
    Report {
        name: sc.name.clone(),
        passed: failure.is_none(),
        steps: reports,
        failure,
        elapsed_ms: started.elapsed().as_millis() as u64,
        images: run.images,
    }
}

// ---------------------------------------------------------------------------------------------
// Recording

/// Collects state-changing calls between `ui.record` start and stop.
#[derive(Clone, Default)]
pub struct Recorder {
    steps: Arc<Mutex<Option<Vec<Step>>>>,
}

/// The step a successful tool call corresponds to, if it changes state.
pub fn step_from_call(tool: &str, a: &Map<String, Value>) -> Option<Step> {
    let s = |k: &str| a.get(k).and_then(Value::as_str).map(str::to_owned);
    Some(match tool {
        "ui.key" => Step::Key(s("keys")?),
        "ui.type_text" => Step::TypeText(s("text")?),
        "ui.invoke_action" => Step::Invoke(s("name")?),
        "ui.command" => Step::Command(a.clone()),
        "ui.click_element" => Step::ClickElement(IdArg::Plain(s("id")?)),
        "ui.open_tab" => Step::Open {
            kind: s("kind")?,
            id: s("id")?,
        },
        "ui.set_theme" => Step::SetTheme(s("name")?),
        "ui.resize" => Step::Resize(s("size")?),
        "ui.wait_for" => Step::WaitFor {
            condition: serde_json::from_value(a.get("condition")?.clone()).ok()?,
            timeout_ms: a.get("timeout_ms").and_then(Value::as_u64),
        },
        "ui.wait_for_idle" => Step::WaitForIdle(a.clone()),
        _ => return None,
    })
}

impl Recorder {
    pub fn observer(&self) -> Observer {
        let steps = self.steps.clone();
        Arc::new(move |tool, args| {
            let mut g = steps.lock().unwrap_or_else(|e| e.into_inner());
            if let (Some(list), Some(step)) = (g.as_mut(), step_from_call(tool, args)) {
                list.push(step);
            }
        })
    }
    pub fn start(&self) {
        *self.steps.lock().unwrap_or_else(|e| e.into_inner()) = Some(Vec::new());
    }
    pub fn stop(&self) -> Option<Vec<Step>> {
        self.steps.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

// ---------------------------------------------------------------------------------------------
// Files

/// `FAUXFIELD_SCENARIOS`, else `fixtures/scenarios` in the current directory or a parent.
pub fn find_scenarios_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FAUXFIELD_SCENARIOS") {
        return Some(PathBuf::from(p));
    }
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let c = dir.join("fixtures").join("scenarios");
        if c.is_dir() {
            return Some(c);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn load_named(dir: Option<&Path>, name: &str) -> Result<Scenario, String> {
    if !valid_name(name) {
        return Err(format!("invalid scenario name {name:?}"));
    }
    let dir = dir.ok_or("fixtures/scenarios not found (set FAUXFIELD_SCENARIOS)")?;
    let path = dir.join(format!("{name}.yaml"));
    let text = std::fs::read_to_string(&path).map_err(|e| {
        let known: Vec<String> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                e.path()
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
            })
            .collect();
        format!(
            "cannot read {}: {e} (known: {})",
            path.display(),
            known.join(", ")
        )
    })?;
    Scenario::from_yaml(&text)
}

// ---------------------------------------------------------------------------------------------
// Tools

#[derive(Deserialize, JsonSchema)]
pub struct RunScenarioInput {
    /// A scenario in `fixtures/scenarios/<name>.yaml`.
    #[serde(default)]
    pub name: Option<String>,
    /// Or an inline scenario as YAML.
    #[serde(default)]
    pub scenario: Option<String>,
    /// `false` skips screenshots (hosts without Screen Recording permission). Default true.
    #[serde(default)]
    pub capture: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct RecordInput {
    /// `start` begins collecting state-changing calls; `stop` writes them as a scenario.
    pub action: RecordAction,
    /// Scenario name for `stop` (the file stem).
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum RecordAction {
    Start,
    Stop,
}

pub struct ScenarioEnv {
    /// Filled with the finished registry after registration.
    pub registry: Arc<OnceLock<Registry>>,
    pub dir: Option<PathBuf>,
    pub recorder: Recorder,
}

pub fn register(registry: &mut Registry, env: ScenarioEnv) {
    let ScenarioEnv {
        registry: shared,
        dir,
        recorder,
    } = env;
    let d = dir.clone();
    registry.register(ToolDef::new::<RunScenarioInput>(
        "ui.run_scenario",
        "Run a scenario by name (fixtures/scenarios/<name>.yaml) or inline YAML. Returns a \
         pass/fail report; the first failing step stops the run and carries expected, actual, \
         ui.state and a screenshot. Checkpoint screenshots come back as images.",
        ToolClass::Ui,
        move |args: Map<String, Value>| {
            let shared = shared.clone();
            let d = d.clone();
            Box::pin(async move {
                let i: RunScenarioInput = serde_json::from_value(Value::Object(args))
                    .map_err(|e| format!("bad arguments: {e}"))?;
                let sc = match (&i.name, &i.scenario) {
                    (Some(n), None) => load_named(d.as_deref(), n)?,
                    (None, Some(y)) => Scenario::from_yaml(y)?,
                    _ => return Err("pass exactly one of `name` or `scenario`".into()),
                };
                let caller = RegistryCaller { registry: shared };
                let opts = RunOptions {
                    capture: i.capture.unwrap_or(true),
                };
                let report = run(&sc, &caller, opts).await;
                let mut v = serde_json::to_value(&report).map_err(|e| e.to_string())?;
                v["screenshots"] =
                    json!(report.images.iter().map(|i| &i.label).collect::<Vec<_>>());
                v["_images"] = json!(
                    report
                        .images
                        .iter()
                        .map(|i| json!({ "data": i.data, "mime": "image/png" }))
                        .collect::<Vec<_>>()
                );
                Ok(v)
            })
        },
    ));
    let rec = recorder;
    registry.register(ToolDef::new::<RecordInput>(
        "ui.record",
        "Record a session as a scenario: `start`, then drive the app with the ui input tools, \
         then `stop` with a `name` to write fixtures/scenarios/<name>.yaml (the YAML is also \
         returned). Only state-changing calls are kept.",
        ToolClass::Ui,
        move |args: Map<String, Value>| {
            let rec = rec.clone();
            let dir = dir.clone();
            Box::pin(async move {
                let i: RecordInput = serde_json::from_value(Value::Object(args))
                    .map_err(|e| format!("bad arguments: {e}"))?;
                match i.action {
                    RecordAction::Start => {
                        rec.start();
                        Ok(json!({ "recording": true }))
                    }
                    RecordAction::Stop => {
                        let name = i.name.ok_or("`name` is required to stop")?;
                        if !valid_name(&name) {
                            return Err(format!("invalid scenario name {name:?}"));
                        }
                        let steps = rec.stop().ok_or("not recording; call start first")?;
                        let sc = Scenario {
                            name: name.clone(),
                            workspace: None,
                            size: None,
                            reset: true,
                            steps,
                        };
                        let yaml = sc.to_yaml()?;
                        let written = match &dir {
                            Some(d) => {
                                let p = d.join(format!("{name}.yaml"));
                                std::fs::write(&p, &yaml)
                                    .map_err(|e| format!("cannot write {}: {e}", p.display()))?;
                                Some(p.display().to_string())
                            }
                            None => None,
                        };
                        Ok(json!({
                            "steps": sc.steps.len(),
                            "path": written,
                            "yaml": yaml,
                        }))
                    }
                }
            })
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    type Handler = Box<dyn Fn(&str, &Map<String, Value>) -> Result<Value, String> + Send + Sync>;

    struct Fake {
        calls: StdMutex<Vec<String>>,
        handler: Handler,
    }

    impl ToolCaller for Fake {
        fn call<'a>(&'a self, name: &'a str, args: Map<String, Value>) -> CallFuture<'a> {
            self.calls.lock().unwrap().push(name.to_string());
            let r = (self.handler)(name, &args);
            Box::pin(async move { r })
        }
    }

    fn fake(
        handler: impl Fn(&str, &Map<String, Value>) -> Result<Value, String> + Send + Sync + 'static,
    ) -> Fake {
        Fake {
            calls: StdMutex::new(Vec::new()),
            handler: Box::new(handler),
        }
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn the_draft_scenarios_parse() {
        for f in ["m01-input", "m014-tree"] {
            let p = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../fixtures/scenarios/{f}.yaml"));
            let sc = Scenario::from_yaml(&std::fs::read_to_string(&p).unwrap())
                .unwrap_or_else(|e| panic!("{f}: {e}"));
            assert_eq!(sc.name, f);
            assert!(sc.steps.len() > 5);
        }
    }

    #[test]
    fn yaml_round_trips() {
        let sc = Scenario::from_yaml(
            "name: x\nsteps:\n  - key: escape\n  - assert: {pointer: /a, equals: 1}\n  - wait_for_idle: {}\n",
        )
        .unwrap();
        let again = Scenario::from_yaml(&sc.to_yaml().unwrap()).unwrap();
        assert_eq!(again.steps.len(), 3);
    }

    #[test]
    fn a_passing_scenario_reports_every_step() {
        let f = fake(|name, _| match name {
            "ui.state" => Ok(json!({"a": {"open": true}})),
            _ => Ok(json!({})),
        });
        let sc = Scenario::from_yaml(
            "name: ok\nsteps:\n  - key: x\n  - assert: {pointer: /a/open, equals: true}\n",
        )
        .unwrap();
        let r = rt().block_on(run(&sc, &f, RunOptions::default()));
        assert!(r.passed, "{:?}", r.failure);
        assert_eq!(r.steps.len(), 3); // reset state, key, assert
        assert!(r.steps.iter().all(|s| s.ok));
    }

    #[test]
    fn the_first_failure_stops_and_reports_expected_actual_state_and_shot() {
        let f = fake(|name, _| match name {
            "ui.state" => Ok(json!({"a": {"open": false}})),
            "ui.screenshot" => Ok(json!({"_images": [{"data": "QUJD", "mime": "image/png"}]})),
            _ => Ok(json!({})),
        });
        let sc = Scenario::from_yaml(
            "name: bad\nsteps:\n  - assert: {pointer: /a/open, equals: true}\n  - key: never\n",
        )
        .unwrap();
        let r = rt().block_on(run(&sc, &f, RunOptions::default()));
        assert!(!r.passed);
        let fail = r.failure.unwrap();
        assert_eq!(fail.index, 2);
        assert_eq!(fail.actual, json!(false));
        assert_eq!(fail.expected["equals"], json!(true));
        assert_eq!(fail.state.unwrap()["a"]["open"], json!(false));
        assert_eq!(fail.screenshot.as_deref(), Some("failure"));
        assert_eq!(r.images.len(), 1);
        assert!(!f.calls.lock().unwrap().iter().any(|c| c == "ui.key"));
    }

    #[test]
    fn no_capture_skips_the_failure_screenshot() {
        let f = fake(|name, _| match name {
            "ui.state" => Ok(json!({})),
            _ => Ok(json!({})),
        });
        let sc =
            Scenario::from_yaml("name: b\nsteps:\n  - assert: {pointer: /x, equals: 1}\n").unwrap();
        let r = rt().block_on(run(&sc, &f, RunOptions { capture: false }));
        assert!(r.failure.unwrap().screenshot.is_none());
        assert!(r.images.is_empty());
    }

    #[test]
    fn tool_errors_fail_the_step() {
        let f = fake(|name, _| {
            if name == "ui.key" {
                Err("boom".into())
            } else {
                Ok(json!({}))
            }
        });
        let sc = Scenario::from_yaml("name: e\nsteps:\n  - key: x\n").unwrap();
        let r = rt().block_on(run(&sc, &f, RunOptions { capture: false }));
        assert_eq!(r.failure.unwrap().actual, json!("boom"));
    }

    #[test]
    fn assert_find_uses_the_last_find_or_a_fresh_one() {
        let f = fake(|name, a| match name {
            "ui.find" => Ok(
                json!({"matches": [{"id": a.get("role").map(|_| "quick-search").unwrap_or("x"), "total": 10000}]}),
            ),
            _ => Ok(json!({})),
        });
        let sc = Scenario::from_yaml(
            "name: f\nsteps:\n  - find: {label: Traffic}\n  - assert_find: {pointer: /matches/0/total, equals: 10000}\n  - assert_find: {role: modal, ids: [quick-search]}\n  - assert_find: {ids: [nope]}\n",
        )
        .unwrap();
        let r = rt().block_on(run(&sc, &f, RunOptions { capture: false }));
        let fail = r.failure.unwrap();
        assert_eq!(fail.index, 5, "{fail:?}");
        assert_eq!(fail.expected, json!(["nope"]));
    }

    #[test]
    fn audit_checkpoint_fails_over_max_errors() {
        let f = fake(|name, _| match name {
            "ui.audit" => Ok(
                json!({"summary": {"errors": 2}, "global": [], "cells": [{"label": "c", "findings": [{"severity": "error", "rule": "UX103"}]}]}),
            ),
            _ => Ok(json!({})),
        });
        let sc =
            Scenario::from_yaml("name: a\nsteps:\n  - audit: {max_errors: 5}\n  - audit: {}\n")
                .unwrap();
        let r = rt().block_on(run(&sc, &f, RunOptions { capture: false }));
        let fail = r.failure.unwrap();
        assert_eq!(fail.index, 3);
        assert_eq!(fail.actual["errors"][0]["finding"]["rule"], json!("UX103"));
    }

    #[test]
    fn recorder_keeps_only_state_changing_calls() {
        let rec = Recorder::default();
        let obs = rec.observer();
        obs("ui.key", &obj(json!({"keys": "a"}))); // not recording yet
        rec.start();
        obs("ui.key", &obj(json!({"keys": "escape"})));
        obs("ui.state", &Map::new());
        obs("ui.click_element", &obj(json!({"id": "x"})));
        let steps = rec.stop().unwrap();
        assert_eq!(steps.len(), 2);
        assert!(rec.stop().is_none());
    }

    #[test]
    fn run_scenario_and_record_tools_work_through_the_registry() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("hello.yaml"),
            "name: hello\nreset: false\nsteps:\n  - key: a\n",
        )
        .unwrap();
        let mut reg = Registry::new();
        reg.register(ToolDef::new::<crate::tools::NoInput>(
            "ui.key",
            "k",
            ToolClass::Ui,
            |_| Box::pin(async { Ok(json!({})) }),
        ));
        reg.register(ToolDef::new::<crate::tools::NoInput>(
            "ui.events",
            "e",
            ToolClass::Ui,
            |_| Box::pin(async { Ok(json!({"last_seq": 0, "events": []})) }),
        ));
        let rec = Recorder::default();
        reg.set_observer(rec.observer());
        let shared = Arc::new(OnceLock::new());
        register(
            &mut reg,
            ScenarioEnv {
                registry: shared.clone(),
                dir: Some(dir.path().to_path_buf()),
                recorder: rec,
            },
        );
        let _ = shared.set(reg.clone());
        let all = AllowedClasses::all().with(ToolClass::Ui);
        let call = |n: &'static str, a: Value| {
            let reg = reg.clone();
            async move { reg.call(all, n, obj(a)).await }
        };
        rt().block_on(async {
            let r = call("ui.run_scenario", json!({"name": "hello"}))
                .await
                .unwrap();
            assert_eq!(r["passed"], json!(true));
            assert!(
                call("ui.run_scenario", json!({"name": "../x"}))
                    .await
                    .is_err()
            );
            call("ui.record", json!({"action": "start"})).await.unwrap();
            call("ui.key", json!({"keys": "escape"})).await.unwrap();
            let r = call("ui.record", json!({"action": "stop", "name": "rec"}))
                .await
                .unwrap();
            assert_eq!(r["steps"], json!(1));
        });
        let text = std::fs::read_to_string(dir.path().join("rec.yaml")).unwrap();
        assert!(Scenario::from_yaml(&text).unwrap().steps.len() == 1);
    }
}
