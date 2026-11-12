//! Seed generators (SPEC §6.1.1, M10.02): pure row generation from a `GeneratorSeed`.
//! Used by the engine, `fauxfield data generate`, the MCP `preview_generator` tool and the GUI.

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Duration, NaiveDate, SecondsFormat, TimeZone, Utc};
use fauxfield_model::{GeneratorField, GeneratorSeed, GeneratorSpec};
use rand::rngs::ChaCha8Rng;
use rand::{RngExt, SeedableRng};
use regex_syntax::hir::{Class, Hir, HirKind};
use serde_json::{Map, Value, json};

use crate::template::derive_seed;

/// A generation problem with the field path, e.g. `generator.fields[2]: invalid regex`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{path}: {message}")]
pub struct GenError {
    pub path: String,
    pub message: String,
}

fn err<T>(path: &str, m: impl Into<String>) -> Result<T, GenError> {
    Err(GenError {
        path: path.into(),
        message: m.into(),
    })
}

/// Rows of other Datasets of the same Server, by Dataset name (for `ref`).
pub type Refs = HashMap<String, Vec<Value>>;

struct Cx<'a> {
    rng: ChaCha8Rng,
    seeded: bool,
    root: &'a Path,
    route: String,
    refs: &'a Refs,
    index: u64,
}

/// Generate rows (`rows` overrides the declared count; `seed` overrides the Server `fake_seed`).
pub fn generate(
    name: &str,
    g: &GeneratorSeed,
    fake_seed: Option<u64>,
    rows: Option<u32>,
    refs: &Refs,
    root: &Path,
) -> Result<Value, GenError> {
    let route = format!("dataset:{name}");
    let rng = match fake_seed {
        Some(s) => ChaCha8Rng::seed_from_u64(derive_seed(s, &route, 0)),
        None => {
            let mut b = [0u8; 8];
            let _ = getrandom::fill(&mut b);
            ChaCha8Rng::seed_from_u64(u64::from_le_bytes(b))
        }
    };
    let mut cx = Cx {
        rng,
        seeded: fake_seed.is_some(),
        root,
        route,
        refs,
        index: 0,
    };
    let n = rows.unwrap_or(g.rows);
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..u64::from(n) {
        cx.index = i;
        out.push(Value::Object(row(&mut cx, &g.fields, "generator.fields")?));
    }
    Ok(Value::Array(out))
}

/// Sample values of one generator definition (no Dataset involved).
pub fn preview_spec(
    spec: &GeneratorSpec,
    fake_seed: Option<u64>,
    count: u32,
    refs: &Refs,
    root: &Path,
) -> Result<Vec<Value>, GenError> {
    let g = GeneratorSeed {
        rows: count,
        fields: vec![GeneratorField {
            name: "value".into(),
            spec: spec.clone(),
        }],
    };
    let v = generate("preview", &g, fake_seed, None, refs, root)?;
    Ok(v.as_array()
        .map(|a| a.iter().filter_map(|r| r.get("value").cloned()).collect())
        .unwrap_or_default())
}

fn row(cx: &mut Cx, fields: &[GeneratorField], path: &str) -> Result<Map<String, Value>, GenError> {
    let mut m = Map::new();
    for (j, f) in fields.iter().enumerate() {
        let p = format!("{path}[{j}]");
        let v = value(cx, &f.spec, &p, &m)?;
        m.insert(f.name.clone(), v);
    }
    Ok(m)
}

fn num(v: &Option<Value>) -> Option<f64> {
    v.as_ref().and_then(Value::as_f64)
}

fn parse_time(v: &Option<Value>, p: &str) -> Result<Option<DateTime<Utc>>, GenError> {
    let Some(v) = v else { return Ok(None) };
    if let Some(n) = v.as_i64() {
        return Ok(Utc.timestamp_opt(n, 0).single());
    }
    let s = v.as_str().unwrap_or_default();
    if let Ok(d) = DateTime::parse_from_rfc3339(s) {
        return Ok(Some(d.with_timezone(&Utc)));
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(d.and_hms_opt(0, 0, 0).map(|x| x.and_utc()));
    }
    err(p, format!("cannot parse date `{s}`"))
}

fn time_range(
    cx: &Cx,
    g: &GeneratorSpec,
    p: &str,
) -> Result<(DateTime<Utc>, DateTime<Utc>), GenError> {
    let (dmin, dmax) = if cx.seeded {
        (
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap(),
        )
    } else {
        (Utc::now() - Duration::days(365), Utc::now())
    };
    let lo = parse_time(&g.min, p)?.unwrap_or(dmin);
    let hi = parse_time(&g.max, p)?.unwrap_or(dmax);
    if lo > hi {
        return err(p, "`min` is after `max`");
    }
    Ok((lo, hi))
}

fn value(
    cx: &mut Cx,
    g: &GeneratorSpec,
    p: &str,
    row_so_far: &Map<String, Value>,
) -> Result<Value, GenError> {
    Ok(match g.kind.as_str() {
        "int" => {
            let lo = num(&g.min).unwrap_or(0.0) as i64;
            let hi = num(&g.max).unwrap_or(1000.0) as i64;
            if lo > hi {
                return err(p, "`min` is greater than `max`");
            }
            match g.step.map(|s| s as i64).filter(|s| *s > 0) {
                Some(s) => json!(lo + cx.rng.random_range(0..=(hi - lo) / s) * s),
                None => json!(cx.rng.random_range(lo..=hi)),
            }
        }
        "float" => {
            let lo = num(&g.min).unwrap_or(0.0);
            let hi = num(&g.max).unwrap_or(1.0);
            if lo > hi {
                return err(p, "`min` is greater than `max`");
            }
            let x = if lo == hi {
                lo
            } else {
                cx.rng.random_range(lo..hi)
            };
            let f = 10f64.powi(g.precision.unwrap_or(2).min(12) as i32);
            json!((x * f).round() / f)
        }
        "bool" => json!(cx.rng.random_range(0.0..1.0) < g.probability.unwrap_or(0.5)),
        "uuid" => {
            let b: [u8; 16] = cx.rng.random();
            json!(uuid::Builder::from_random_bytes(b).into_uuid().to_string())
        }
        "ulid" => {
            let ts = if cx.seeded {
                0
            } else {
                Utc::now().timestamp_millis() as u64
            };
            let r: u128 = cx.rng.random::<u128>() & ((1u128 << 80) - 1);
            json!(ulid::Ulid::from_parts(ts, r).to_string())
        }
        "sequence" => {
            let v = g.start.unwrap_or(1.0) + g.step.unwrap_or(1.0) * cx.index as f64;
            if v.fract() == 0.0 {
                json!(v as i64)
            } else {
                json!(v)
            }
        }
        "regex" => {
            let pat = g.pattern.as_deref().unwrap_or_default();
            let hir = regex_syntax::ParserBuilder::new()
                .unicode(false)
                .utf8(false)
                .build()
                .parse(pat)
                .map_err(|e| GenError {
                    path: p.into(),
                    message: format!("invalid regex: {e}"),
                })?;
            let mut s = String::new();
            sample(&hir, &mut cx.rng, &mut s);
            json!(s)
        }
        "one_of" => {
            let vals = g.values.clone().unwrap_or_default();
            if vals.is_empty() {
                return err(p, "`values` is empty");
            }
            let i = match &g.weights {
                Some(w) if w.len() == vals.len() => {
                    let total: f64 = w.iter().sum();
                    if total <= 0.0 {
                        return err(p, "`weights` sum to zero");
                    }
                    let mut x = cx.rng.random_range(0.0..total);
                    let mut pick = vals.len() - 1;
                    for (i, w) in w.iter().enumerate() {
                        if x < *w {
                            pick = i;
                            break;
                        }
                        x -= w;
                    }
                    pick
                }
                _ => cx.rng.random_range(0..vals.len()),
            };
            vals[i].clone()
        }
        "string" => {
            let cs: Vec<char> = g
                .charset
                .clone()
                .unwrap_or_else(|| {
                    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".into()
                })
                .chars()
                .collect();
            if cs.is_empty() {
                return err(p, "`charset` is empty");
            }
            let n = g.length.unwrap_or(8);
            json!(
                (0..n)
                    .map(|_| cs[cx.rng.random_range(0..cs.len())])
                    .collect::<String>()
            )
        }
        "lorem" => {
            let n = g.words.unwrap_or(5).max(1);
            let w: Vec<String> = (0..n)
                .filter_map(|_| crate::template::fake_generate("lorem.word", &mut cx.rng))
                .collect();
            json!(w.join(" "))
        }
        "fake" => {
            let k = g.fake.as_deref().unwrap_or_default();
            match crate::template::fake_generate(k, &mut cx.rng) {
                Some(s) => json!(s),
                None => return err(p, format!("unknown fake kind `{k}`")),
            }
        }
        "datetime" => {
            let (lo, hi) = time_range(cx, g, p)?;
            let secs = cx.rng.random_range(0..=(hi - lo).num_seconds().max(0));
            let d = lo + Duration::seconds(secs);
            match &g.format {
                Some(f) => json!(d.format(f).to_string()),
                None => json!(d.to_rfc3339_opts(SecondsFormat::Secs, true)),
            }
        }
        "date" => {
            let (lo, hi) = time_range(cx, g, p)?;
            let days = cx.rng.random_range(0..=(hi - lo).num_days().max(0));
            let d = (lo + Duration::days(days)).date_naive();
            match &g.format {
                Some(f) => json!(d.format(f).to_string()),
                None => json!(d.format("%Y-%m-%d").to_string()),
            }
        }
        "const" => g.value.clone().unwrap_or(Value::Null),
        "ref" => {
            let d = g.dataset.as_deref().unwrap_or_default();
            let Some(rows) = cx.refs.get(d) else {
                return err(p, format!("unknown dataset `{d}`"));
            };
            if rows.is_empty() {
                return err(p, format!("dataset `{d}` has no rows to reference"));
            }
            let key = g.field.as_deref().unwrap_or("id");
            let r = &rows[cx.rng.random_range(0..rows.len())];
            match r.get(key) {
                Some(v) => v.clone(),
                None => return err(p, format!("dataset `{d}` rows have no `{key}`")),
            }
        }
        "null_or" => {
            let Some(inner) = &g.inner else {
                return err(p, "`null_or` needs `inner`");
            };
            // Always draw the inner value so a changed probability does not shift later fields.
            let v = value(cx, inner, &format!("{p}.inner"), row_so_far)?;
            if cx.rng.random_range(0.0..1.0) < g.null_probability.unwrap_or(0.5) {
                Value::Null
            } else {
                v
            }
        }
        "template" => {
            let src = g.expr.as_deref().unwrap_or_default();
            let wrapped = format!("{{{{ {src} }}}}");
            let input = crate::template::Input {
                context: json!({"row": row_so_far, "index": cx.index, "request": {}, "vars": {}, "data": {}, "state": {}}),
                root: cx.root,
                seed: cx
                    .seeded
                    .then(|| derive_seed(cx.rng.random(), &cx.route, cx.index)),
                route_id: &cx.route,
                counter: cx.index,
                seq: None,
            };
            let text = crate::template::render(&wrapped, &input).map_err(|e| GenError {
                path: p.into(),
                message: e.to_string(),
            })?;
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        }
        "object" => {
            let Some(f) = &g.fields else {
                return err(p, "`object` needs `fields`");
            };
            Value::Object(row(cx, f, &format!("{p}.fields"))?)
        }
        "array" => {
            let Some(item) = &g.item else {
                return err(p, "`array` needs `item`");
            };
            let lo = num(&g.min).unwrap_or(1.0).max(0.0) as usize;
            let hi = num(&g.max).unwrap_or(3.0).max(lo as f64) as usize;
            let n = cx.rng.random_range(lo..=hi);
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                v.push(value(cx, item, &format!("{p}.item"), row_so_far)?);
            }
            Value::Array(v)
        }
        k => return err(p, format!("unknown generator `{k}`")),
    })
}

const REPEAT_CAP: u32 = 8;

fn sample(h: &Hir, rng: &mut ChaCha8Rng, out: &mut String) {
    match h.kind() {
        HirKind::Empty | HirKind::Look(_) => {}
        HirKind::Literal(l) => out.push_str(&String::from_utf8_lossy(&l.0)),
        HirKind::Class(Class::Unicode(c)) => {
            let r = c.ranges();
            if r.is_empty() {
                return;
            }
            let total: u32 = r
                .iter()
                .map(|x| x.end() as u32 - x.start() as u32 + 1)
                .sum();
            let mut n = rng.random_range(0..total);
            for x in r {
                let len = x.end() as u32 - x.start() as u32 + 1;
                if n < len {
                    out.push(char::from_u32(x.start() as u32 + n).unwrap_or('?'));
                    return;
                }
                n -= len;
            }
        }
        HirKind::Class(Class::Bytes(c)) => {
            // Printable ASCII only, so output stays valid UTF-8.
            let mut set: Vec<u8> = Vec::new();
            for x in c.ranges() {
                for b in x.start().max(0x20)..=x.end().min(0x7e) {
                    set.push(b);
                }
            }
            if !set.is_empty() {
                out.push(set[rng.random_range(0..set.len())] as char);
            }
        }
        HirKind::Repetition(r) => {
            let hi = r.max.unwrap_or(r.min + REPEAT_CAP).min(r.min + REPEAT_CAP);
            for _ in 0..rng.random_range(r.min..=hi) {
                sample(&r.sub, rng, out);
            }
        }
        HirKind::Capture(c) => sample(&c.sub, rng, out),
        HirKind::Concat(v) => v.iter().for_each(|x| sample(x, rng, out)),
        HirKind::Alternation(v) => {
            if !v.is_empty() {
                sample(&v[rng.random_range(0..v.len())], rng, out);
            }
        }
    }
}
