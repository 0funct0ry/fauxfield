//! Dataset actions of a Response (SPEC §6.3): an ordered list of steps that run after the
//! Response is chosen and before the body renders, atomic as a group.

use crate::data::{self, DataStore};
use fauxfield_model::{DatasetAction, Server};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

/// What one step changed; recorded on the request (Traffic) and returned by `send_test`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatasetChange {
    pub dataset: String,
    pub op: String,
    /// Rows inserted, updated or deleted.
    pub rows: usize,
    /// Keys of the rows touched, when the Dataset has them.
    #[serde(default)]
    pub keys: Vec<String>,
}

/// The step that failed and why; the whole group was rolled back.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionError {
    pub step: usize,
    pub message: String,
}

pub(crate) struct Env<'a> {
    pub root: &'a Path,
    pub server_id: &'a str,
    pub server: &'a Server,
    pub route_id: &'a str,
    pub counter: u64,
}

fn eval(env: &Env<'_>, ctx: &Value, src: &str) -> Result<String, String> {
    let input = crate::template::Input {
        context: ctx.clone(),
        root: env.root,
        seed: env.server.fake_seed,
        route_id: env.route_id,
        counter: env.counter,
        seq: None,
    };
    crate::template::render(src, &input).map_err(|e| format!("`{src}`: {e}"))
}

/// A rendered expression as JSON when it parses (numbers, objects, `true`), else a string.
fn to_value(text: &str) -> Value {
    let t = text.trim();
    serde_json::from_str(t).unwrap_or_else(|_| Value::String(text.to_string()))
}

fn set_path(row: &mut Value, path: &str, v: Value) {
    let mut cur = row;
    let segs: Vec<&str> = path.split('.').collect();
    for (i, seg) in segs.iter().enumerate() {
        if !cur.is_object() {
            *cur = json!({});
        }
        let o = cur.as_object_mut().unwrap();
        if i + 1 == segs.len() {
            o.insert((*seg).into(), v);
            return;
        }
        cur = o.entry((*seg).to_string()).or_insert(json!({}));
    }
}

/// Run `actions` against `store`. On success returns the changes; on failure every Dataset is
/// restored to its state before the first step. `ctx` gains `action` (and a fresh `data`).
pub(crate) fn run(
    store: &mut DataStore,
    env: &Env<'_>,
    actions: &[DatasetAction],
    ctx: &mut Value,
) -> Result<Vec<DatasetChange>, ActionError> {
    let steps: Vec<(usize, &DatasetAction)> = actions
        .iter()
        .enumerate()
        .filter(|(_, a)| !a.disabled)
        .collect();
    if steps.is_empty() {
        return Ok(vec![]);
    }
    // Checkpoint every Dataset the group names.
    let mut saved: Vec<(String, Value, u64)> = vec![];
    for (_, a) in &steps {
        if !saved.iter().any(|(n, ..)| n == &a.dataset)
            && let Some(d) = store.get(&a.dataset)
        {
            saved.push((a.dataset.clone(), d.rows.clone(), d.counter));
        }
    }
    let mut results: Vec<Value> = vec![];
    let mut changes: Vec<DatasetChange> = vec![];
    let mut failed: Option<ActionError> = None;
    for (n, a) in &steps {
        ctx["action"] = Value::Array(results.clone());
        match step(store, env, a, ctx) {
            Ok((res, change)) => {
                if let Some(expr) = a.fail_if.as_deref() {
                    let mut c2 = ctx.clone();
                    c2["result"] = res.clone();
                    match eval(env, &c2, expr) {
                        Ok(t) if t.trim() == "true" => {
                            failed = Some(ActionError {
                                step: *n,
                                message: format!("fail_if `{expr}` was true"),
                            });
                            break;
                        }
                        Ok(_) => {}
                        Err(m) => {
                            failed = Some(ActionError {
                                step: *n,
                                message: m,
                            });
                            break;
                        }
                    }
                }
                results.push(res);
                changes.push(change);
                ctx["data"] = (*store.snapshot()).clone();
            }
            Err(message) => {
                failed = Some(ActionError { step: *n, message });
                break;
            }
        }
    }
    if let Some(e) = failed {
        for (name, rows, counter) in saved {
            if let Some(d) = store.get_mut(&name) {
                d.rows = rows;
                d.counter = counter;
                d.persist(env.root, env.server_id);
            }
        }
        ctx["data"] = (*store.snapshot()).clone();
        return Err(e);
    }
    for (name, ..) in &saved {
        if let Some(d) = store.get(name) {
            d.persist(env.root, env.server_id);
        }
    }
    ctx["action"] = Value::Array(results);
    Ok(changes)
}

fn step(
    store: &mut DataStore,
    env: &Env<'_>,
    a: &DatasetAction,
    ctx: &Value,
) -> Result<(Value, DatasetChange), String> {
    let name = a.dataset.as_str();
    let mut keys: Vec<String> = vec![];
    let change = |rows: usize, keys: Vec<String>| DatasetChange {
        dataset: name.to_string(),
        op: a.op.clone(),
        rows,
        keys,
    };
    if a.op == "reset" {
        store.get(name).ok_or(format!("unknown Dataset `{name}`"))?;
        store.reset(env.root, env.server_id, env.server, Some(name));
        return Ok((json!({"reset": true}), change(0, keys)));
    }
    let key_expr = a.key.as_deref().map(|k| eval(env, ctx, k)).transpose()?;
    let ds = store
        .get_mut(name)
        .ok_or(format!("unknown Dataset `{name}`"))?;
    let key_name = ds.def.key_name().to_string();
    let is_array = ds.rows.is_array();
    // Build the object for insert/update.
    let build_row = || -> Result<Value, String> {
        let mut row = match a.row.as_deref() {
            Some(r) => to_value(&eval(env, ctx, r)?),
            None => ctx["request"]["json"].clone(),
        };
        if !row.is_object() {
            if a.fields.is_empty() && a.row.is_none() {
                return Err("request body is not a JSON object".into());
            }
            row = json!({});
        }
        for (f, e) in &a.fields {
            set_path(&mut row, f, to_value(&eval(env, ctx, e)?));
        }
        Ok(row)
    };
    match a.op.as_str() {
        "insert" => {
            let mut row = build_row()?;
            if !is_array {
                return Err("insert needs an array Dataset".into());
            }
            if row.get(&key_name).is_none_or(Value::is_null) {
                let id = ds.next_id();
                row[&key_name] = id;
            } else if let Some(id) = row.get(&key_name).and_then(data::scalar_text)
                && data::position(ds.rows.as_array().unwrap(), &key_name, &id).is_some()
            {
                return Err(format!("a row with {key_name} `{id}` already exists"));
            }
            if let Some(k) = row.get(&key_name).and_then(data::scalar_text) {
                keys.push(k);
            }
            ds.rows.as_array_mut().unwrap().push(row.clone());
            ds.counter = ds
                .counter
                .max(row.get(&key_name).and_then(Value::as_u64).unwrap_or(0));
            Ok((row, change(1, keys)))
        }
        "delete" => {
            let id = key_expr.ok_or("delete needs a key")?;
            let rows = ds
                .rows
                .as_array_mut()
                .ok_or("delete needs an array Dataset")?;
            let i = data::position(rows, &key_name, &id)
                .ok_or(format!("no row with {key_name} `{id}` in `{name}`"))?;
            rows.remove(i);
            Ok((json!({"count": 1}), change(1, vec![id])))
        }
        "delete_where" => {
            let mut filters = vec![];
            for (k, e) in &a.filter {
                filters.push((k.clone(), eval(env, ctx, e)?));
            }
            let idx: std::collections::BTreeSet<usize> =
                data::matching(&ds.rows, &filters).into_iter().collect();
            let rows = ds
                .rows
                .as_array_mut()
                .ok_or("delete_where needs an array Dataset")?;
            let mut i = 0;
            rows.retain(|r| {
                let hit = idx.contains(&i);
                if hit && let Some(k) = r.get(&key_name).and_then(data::scalar_text) {
                    keys.push(k);
                }
                i += 1;
                !hit
            });
            Ok((json!({"count": idx.len()}), change(idx.len(), keys)))
        }
        "update" | "set" | "increment" => {
            let target: &mut Value = if is_array {
                let id = key_expr.ok_or(format!("{} needs a key", a.op))?;
                let rows = ds.rows.as_array_mut().unwrap();
                let i = data::position(rows, &key_name, &id)
                    .ok_or(format!("no row with {key_name} `{id}` in `{name}`"))?;
                keys.push(id);
                &mut rows[i]
            } else {
                &mut ds.rows
            };
            match a.op.as_str() {
                "update" => {
                    let new = build_row()?;
                    if a.mode.as_deref() == Some("replace") {
                        let kv = target.get(&key_name).cloned();
                        *target = new;
                        if let (Some(kv), true) = (kv, is_array) {
                            target[&key_name] = kv;
                        }
                    } else {
                        data::merge_patch(target, &new);
                    }
                }
                "set" => {
                    let f = a.field.as_deref().ok_or("set needs a field")?;
                    let v = to_value(&eval(env, ctx, a.value.as_deref().unwrap_or(""))?);
                    set_path(target, f, v);
                }
                _ => {
                    let f = a.field.as_deref().ok_or("increment needs a field")?;
                    let by: f64 = match a.by.as_deref() {
                        Some(e) => eval(env, ctx, e)?
                            .trim()
                            .parse()
                            .map_err(|_| format!("`by` `{e}` is not a number"))?,
                        None => 1.0,
                    };
                    let cur = match data::field(target, f) {
                        None | Some(Value::Null) => 0.0,
                        Some(v) => v.as_f64().ok_or(format!("field `{f}` is not a number"))?,
                    };
                    let sum = cur + by;
                    let v = if sum.fract() == 0.0 && sum.abs() < 9e15 {
                        json!(sum as i64)
                    } else {
                        json!(sum)
                    };
                    set_path(target, f, v);
                }
            }
            let out = target.clone();
            Ok((out, change(1, keys)))
        }
        other => Err(format!("unknown action op `{other}`")),
    }
}
