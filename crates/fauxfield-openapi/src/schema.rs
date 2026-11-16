//! JSON Schema helpers: column mapping and inference from example values.
use fauxfield_model::DatasetColumn;
use serde_json::{Map, Value, json};

pub fn column_schema(c: &DatasetColumn) -> Value {
    let mut m = Map::new();
    match c.ty.as_str() {
        "int" => m.insert("type".into(), "integer".into()),
        "float" => m.insert("type".into(), "number".into()),
        "bool" => m.insert("type".into(), "boolean".into()),
        "datetime" => {
            m.insert("type".into(), "string".into());
            m.insert("format".into(), "date-time".into())
        }
        "json" => None,
        _ => m.insert("type".into(), "string".into()),
    };
    if let Some(d) = &c.description {
        m.insert("description".into(), d.clone().into());
    }
    if let Some(d) = &c.default
        && let Ok(v) = serde_json::to_value(d)
    {
        m.insert("default".into(), v);
    }
    Value::Object(m)
}

fn guess_format(s: &str) -> Option<&'static str> {
    let b = s.as_bytes();
    if s.len() == 36 && b[8] == b'-' && b[13] == b'-' && b[18] == b'-' && b[23] == b'-' {
        return Some("uuid");
    }
    if s.len() >= 19 && b[4] == b'-' && b[7] == b'-' && b[10] == b'T' {
        return Some("date-time");
    }
    if s.len() == 10 && b[4] == b'-' && b[7] == b'-' {
        return Some("date");
    }
    if s.contains('@') && s.contains('.') && !s.contains(' ') {
        return Some("email");
    }
    None
}

/// Schema of one value, without its children's optionality.
pub fn infer(v: &Value) -> Value {
    match v {
        Value::Null => json!({"type": "null"}),
        Value::Bool(_) => json!({"type": "boolean"}),
        Value::Number(n) => {
            json!({"type": if n.is_i64() || n.is_u64() { "integer" } else { "number" }})
        }
        Value::String(s) => match guess_format(s) {
            Some(f) => json!({"type": "string", "format": f}),
            None => json!({"type": "string"}),
        },
        Value::Array(a) => {
            let items = infer_many(a.iter());
            json!({"type": "array", "items": items.unwrap_or_else(|| json!({}))})
        }
        Value::Object(o) => {
            let props: Map<String, Value> = o.iter().map(|(k, v)| (k.clone(), infer(v))).collect();
            json!({"type": "object", "properties": props})
        }
    }
}

/// Union of several samples: object keys are merged (keys missing somewhere are optional),
/// mixed types become `oneOf`.
pub fn infer_many<'a>(vals: impl Iterator<Item = &'a Value>) -> Option<Value> {
    let vals: Vec<&Value> = vals.collect();
    if vals.is_empty() {
        return None;
    }
    if vals.iter().all(|v| v.is_object()) {
        let mut keys: Vec<String> = Vec::new();
        for v in &vals {
            for k in v.as_object().unwrap().keys() {
                if !keys.contains(k) {
                    keys.push(k.clone());
                }
            }
        }
        let mut props = Map::new();
        let mut required = Vec::new();
        for k in &keys {
            let present: Vec<&Value> = vals.iter().filter_map(|v| v.get(k)).collect();
            if present.len() == vals.len() {
                required.push(Value::String(k.clone()));
            }
            props.insert(k.clone(), infer_many(present.into_iter()).unwrap());
        }
        let mut o = json!({"type": "object", "properties": props});
        if !required.is_empty() {
            o["required"] = Value::Array(required);
        }
        return Some(o);
    }
    let mut kinds: Vec<Value> = Vec::new();
    for v in &vals {
        let s = infer(v);
        if !kinds
            .iter()
            .any(|k| k["type"] == s["type"] && k["format"] == s["format"])
        {
            kinds.push(s);
        }
    }
    // integer is a subset of number
    if kinds.len() == 2 && kinds.iter().any(|k| k["type"] == "number") {
        kinds.retain(|k| k["type"] == "number");
    }
    Some(if kinds.len() == 1 {
        kinds.pop().unwrap()
    } else {
        json!({"oneOf": kinds})
    })
}
