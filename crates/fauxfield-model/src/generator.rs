//! Declarative seed generators (SPEC §6.1.1, M10.02): a row count plus a field list, each field
//! naming a generator with its options. Options are one flat struct so TOML stays simple.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Stable generator names (also feeds the generated docs page).
pub const GENERATORS: &[(&str, &str)] = &[
    (
        "int",
        "Integer in `min`..=`max` (default 0..=1000), optional `step`",
    ),
    (
        "float",
        "Float in `min`..=`max` (default 0..1) rounded to `precision` decimals (default 2)",
    ),
    ("bool", "Boolean, true with `probability` (default 0.5)"),
    ("uuid", "UUID v4 string"),
    ("ulid", "ULID string"),
    (
        "sequence",
        "Row index based number: `start` (default 1) plus `step` (default 1) per row",
    ),
    ("regex", "String matching `pattern`"),
    ("one_of", "One of `values`, optionally picked by `weights`"),
    (
        "string",
        "Random string of `length` (default 8) from `charset` (default alphanumeric)",
    ),
    ("lorem", "`words` (default 5) lorem ipsum words"),
    (
        "fake",
        "Any `fake` kind named by `fake`, e.g. `internet.email`",
    ),
    (
        "datetime",
        "RFC 3339 / `format` timestamp between `min` and `max` (RFC 3339, default last 365 days; one bound alone spans 365 days from it)",
    ),
    ("date", "Date (`YYYY-MM-DD`) between `min` and `max`"),
    ("const", "The fixed `value`"),
    (
        "ref",
        "Key (`field`, default `id`) of a row in another Dataset of the same Server (`dataset`)",
    ),
    (
        "null_or",
        "`inner` generator, or null with `null_probability` (default 0.5)",
    ),
    (
        "template",
        "Tera expression `expr`; earlier fields of the row are in `row`",
    ),
    ("object", "Nested object built from `fields`"),
    (
        "array",
        "Array of `min`..=`max` (default 1..=3) items from `item`",
    ),
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorSeed {
    /// Number of rows (array Datasets).
    #[serde(default)]
    pub rows: u32,
    #[serde(default)]
    pub fields: Vec<GeneratorField>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorField {
    pub name: String,
    #[serde(flatten)]
    pub spec: GeneratorSpec,
}

/// One generator and its options; `kind` selects which options apply.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorSpec {
    #[serde(default)]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precision: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probability: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weights: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<u32>,
    /// `fake` kind, e.g. `internet.email`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fake: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    /// `ref`: the other Dataset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset: Option<String>,
    /// `ref`: the key field to pick (default `id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub null_probability: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inner: Option<Box<GeneratorSpec>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<GeneratorField>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<Box<GeneratorSpec>>,
}

impl GeneratorSpec {
    pub fn of(kind: &str) -> Self {
        Self {
            kind: kind.into(),
            ..Self::default()
        }
    }
}

fn num(v: &Option<Value>) -> Option<f64> {
    v.as_ref().and_then(Value::as_f64)
}

/// Static checks of one generator; `(field path, message)` per problem. `datasets` are the
/// Dataset names of the Server (for `ref`); `refs` collects `ref` targets for cycle checks.
pub fn check_spec(
    g: &GeneratorSpec,
    path: &str,
    datasets: &[&str],
    refs: &mut Vec<String>,
    out: &mut Vec<(String, String)>,
) {
    let mut bad = |m: String| out.push((path.to_string(), m));
    let need = |o: bool, what: &str| (!o).then(|| format!("`{}` needs `{what}`", g.kind));
    match g.kind.as_str() {
        "int" | "float" | "sequence" => {
            if let (Some(a), Some(b)) = (num(&g.min), num(&g.max))
                && a > b
            {
                bad("`min` is greater than `max`".into());
            }
            if g.kind == "int" && g.step.is_some_and(|s| s <= 0.0) {
                bad("`step` must be positive".into());
            }
        }
        "bool" => {
            if g.probability.is_some_and(|p| !(0.0..=1.0).contains(&p)) {
                bad("`probability` must be between 0 and 1".into());
            }
        }
        "uuid" | "ulid" | "lorem" | "date" | "datetime" | "string" => {
            if g.charset.as_deref() == Some("") {
                bad("`charset` is empty".into());
            }
        }
        "regex" => match g.pattern.as_deref() {
            None => bad("`regex` needs `pattern`".into()),
            Some(p) => {
                if let Err(e) = regex_syntax::ParserBuilder::new()
                    .unicode(false)
                    .utf8(false)
                    .build()
                    .parse(p)
                {
                    bad(format!("invalid regex: {e}"));
                }
            }
        },
        "one_of" => {
            match &g.values {
                None => bad("`one_of` needs `values`".into()),
                Some(v) if v.is_empty() => bad("`values` is empty".into()),
                Some(v) => {
                    if let Some(w) = &g.weights
                        && (w.len() != v.len()
                            || w.iter().any(|x| *x < 0.0)
                            || w.iter().sum::<f64>() <= 0.0)
                    {
                        bad("`weights` must match `values` in length, be non-negative and not all zero".into());
                    }
                }
            }
        }
        // The kind itself is resolved by the engine, which owns the kind list.
        "fake" => {
            if g.fake.as_deref().is_none_or(str::is_empty) {
                bad("`fake` needs the `fake` kind".into());
            }
        }
        "const" => {
            if let Some(m) = need(g.value.is_some(), "value") {
                bad(m);
            }
        }
        "ref" => match g.dataset.as_deref() {
            None => bad("`ref` needs `dataset`".into()),
            Some(d) if !datasets.contains(&d) => bad(format!("unknown dataset `{d}`")),
            Some(d) => refs.push(d.to_string()),
        },
        "null_or" => match &g.inner {
            None => bad("`null_or` needs `inner`".into()),
            Some(i) => check_spec(i, &format!("{path}.inner"), datasets, refs, out),
        },
        "template" => {
            if let Some(m) = need(g.expr.is_some(), "expr") {
                bad(m);
            }
        }
        "object" => match &g.fields {
            None => bad("`object` needs `fields`".into()),
            Some(f) => check_fields(f, &format!("{path}.fields"), datasets, refs, out),
        },
        "array" => match &g.item {
            None => bad("`array` needs `item`".into()),
            Some(i) => check_spec(i, &format!("{path}.item"), datasets, refs, out),
        },
        k => bad(format!("unknown generator `{k}`")),
    }
}

pub fn check_fields(
    fields: &[GeneratorField],
    path: &str,
    datasets: &[&str],
    refs: &mut Vec<String>,
    out: &mut Vec<(String, String)>,
) {
    for (j, f) in fields.iter().enumerate() {
        if f.name.trim().is_empty() {
            out.push((format!("{path}[{j}].name"), "field name is empty".into()));
        }
        check_spec(&f.spec, &format!("{path}[{j}]"), datasets, refs, out);
    }
}

/// Datasets a generator seed reads through `ref` (any depth).
pub fn ref_targets(g: &GeneratorSeed) -> Vec<String> {
    fn walk(s: &GeneratorSpec, out: &mut Vec<String>) {
        if s.kind == "ref"
            && let Some(d) = &s.dataset
        {
            out.push(d.clone());
        }
        if let Some(i) = &s.inner {
            walk(i, out);
        }
        if let Some(i) = &s.item {
            walk(i, out);
        }
        for f in s.fields.iter().flatten() {
            walk(&f.spec, out);
        }
    }
    let mut out = Vec::new();
    for f in &g.fields {
        walk(&f.spec, &mut out);
    }
    out
}

/// Topological load order of Datasets by `ref` (names; unknown targets ignored). `Err` carries
/// a cycle path such as `a -> b -> a`.
pub fn load_order(datasets: &[crate::Dataset]) -> Result<Vec<usize>, Vec<String>> {
    let idx = |n: &str| datasets.iter().position(|d| d.name == n);
    let deps: Vec<Vec<usize>> = datasets
        .iter()
        .map(|d| {
            d.generator
                .as_ref()
                .map(|g| ref_targets(g).iter().filter_map(|n| idx(n)).collect())
                .unwrap_or_default()
        })
        .collect();
    // 0 = new, 1 = visiting, 2 = done
    let mut state = vec![0u8; datasets.len()];
    let mut order = Vec::new();
    fn visit(
        i: usize,
        deps: &[Vec<usize>],
        state: &mut [u8],
        order: &mut Vec<usize>,
        stack: &mut Vec<usize>,
    ) -> Option<Vec<usize>> {
        match state[i] {
            2 => return None,
            1 => {
                let at = stack.iter().position(|x| *x == i).unwrap_or(0);
                let mut c = stack[at..].to_vec();
                c.push(i);
                return Some(c);
            }
            _ => {}
        }
        state[i] = 1;
        stack.push(i);
        for &d in &deps[i] {
            if let Some(c) = visit(d, deps, state, order, stack) {
                return Some(c);
            }
        }
        stack.pop();
        state[i] = 2;
        order.push(i);
        None
    }
    for i in 0..datasets.len() {
        if let Some(c) = visit(i, &deps, &mut state, &mut order, &mut Vec::new()) {
            return Err(c.into_iter().map(|j| datasets[j].name.clone()).collect());
        }
    }
    Ok(order)
}

pub fn ref_cycle(datasets: &[crate::Dataset]) -> Option<Vec<String>> {
    load_order(datasets).err()
}
