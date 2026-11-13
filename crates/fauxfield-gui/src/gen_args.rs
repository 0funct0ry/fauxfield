//! The options a generator kind takes, as typed fields for the Seed data dialog (M10.07).
//! Pure: the dialog keeps one text per field and [`apply_args`] turns them into a spec.

use fauxfield_model::GeneratorSpec;
use serde_json::Value;

/// How a field's text is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgKind {
    /// A number (int or float).
    Number,
    /// A whole number of 0 or more.
    Count,
    /// A probability from 0 to 1.
    Probability,
    /// Free text.
    Text,
    /// Comma-separated values; each is JSON when it parses (`1`, `true`), else a string.
    List,
    /// Comma-separated numbers.
    Numbers,
    /// A JSON value, else the text as a string.
    Json,
    /// A Dataset of the same Server (drawn as a ComboBox).
    Dataset,
    /// A calendar date (`YYYY-MM-DD`), drawn as a DatePicker. For `datetime` the minimum is the
    /// start of the day and the maximum its end.
    Date,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArgDef {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: ArgKind,
    pub placeholder: &'static str,
    pub required: bool,
}

const fn arg(
    key: &'static str,
    label: &'static str,
    kind: ArgKind,
    placeholder: &'static str,
    required: bool,
) -> ArgDef {
    ArgDef {
        key,
        label,
        kind,
        placeholder,
        required,
    }
}

/// The fields a generator kind takes (`fake:<kind>` and argument-free kinds take none).
pub fn args_for(kind: &str) -> &'static [ArgDef] {
    use ArgKind::*;
    match kind {
        "int" => {
            const A: &[ArgDef] = &[
                arg("min", "min", Number, "0", false),
                arg("max", "max", Number, "1000", false),
                arg("step", "step", Number, "1", false),
            ];
            A
        }
        "float" => {
            const A: &[ArgDef] = &[
                arg("min", "min", Number, "0", false),
                arg("max", "max", Number, "1", false),
                arg("precision", "precision", Count, "2", false),
            ];
            A
        }
        "bool" => {
            const A: &[ArgDef] = &[arg(
                "probability",
                "true probability",
                Probability,
                "0.5",
                false,
            )];
            A
        }
        "sequence" => {
            const A: &[ArgDef] = &[
                arg("start", "start", Number, "1", false),
                arg("step", "step", Number, "1", false),
            ];
            A
        }
        "regex" => {
            const A: &[ArgDef] = &[arg("pattern", "pattern", Text, "[A-Z]{3}-\\d{4}", true)];
            A
        }
        "one_of" => {
            const A: &[ArgDef] = &[
                arg("values", "values", List, "USD, EUR, GBP", true),
                arg(
                    "weights",
                    "weights",
                    Numbers,
                    "0.6, 0.3, 0.1 (optional)",
                    false,
                ),
            ];
            A
        }
        "string" => {
            const A: &[ArgDef] = &[
                arg("length", "length", Count, "8", false),
                arg("charset", "charset", Text, "alphanumeric", false),
            ];
            A
        }
        "lorem" => {
            const A: &[ArgDef] = &[arg("words", "words", Count, "5", false)];
            A
        }
        "datetime" => {
            const A: &[ArgDef] = &[
                arg("min", "from", Date, "last 365 days", false),
                arg("max", "to", Date, "today", false),
                arg("format", "format", Text, "%Y-%m-%d %H:%M (optional)", false),
            ];
            A
        }
        "date" => {
            const A: &[ArgDef] = &[
                arg("min", "from", Date, "last 365 days", false),
                arg("max", "to", Date, "today", false),
            ];
            A
        }
        "const" => {
            const A: &[ArgDef] = &[arg("value", "value", Json, "\"fixed\" or 42", true)];
            A
        }
        "ref" => {
            const A: &[ArgDef] = &[
                arg("dataset", "dataset", Dataset, "", true),
                arg("field", "field", Text, "id", false),
            ];
            A
        }
        "null_or" => {
            const A: &[ArgDef] = &[arg(
                "null_probability",
                "null probability",
                Probability,
                "0.5",
                false,
            )];
            A
        }
        "template" => {
            const A: &[ArgDef] = &[arg(
                "expr",
                "expression",
                Text,
                "{{ row.name | upper }}",
                true,
            )];
            A
        }
        _ => &[],
    }
}

/// Kinds the dialog cannot describe with flat fields (they nest other generators).
pub fn needs_toml(kind: &str) -> bool {
    matches!(kind, "object" | "array")
}

fn json_or_string(t: &str) -> Value {
    serde_json::from_str(t).unwrap_or_else(|_| Value::String(t.to_string()))
}

fn split(t: &str) -> Vec<&str> {
    t.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Apply the field texts (`(key, text)`, missing or empty means default) to `spec`.
/// `ty` is the column type: `null_or` wraps the generator inferred for it.
pub fn apply_args(
    mut spec: GeneratorSpec,
    args: &[(String, String)],
    column: &str,
    ty: &str,
) -> Result<GeneratorSpec, String> {
    let get = |k: &str| {
        args.iter()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.trim())
            .filter(|v| !v.is_empty())
    };
    for def in args_for(&spec.kind.clone()) {
        let Some(t) = get(def.key) else {
            if def.required {
                return Err(format!("`{}` needs {}.", spec.kind, def.label));
            }
            continue;
        };
        let num = || {
            t.parse::<f64>()
                .map_err(|_| format!("{} must be a number.", def.label))
        };
        match def.kind {
            ArgKind::Number => {
                let n = num()?;
                match def.key {
                    "min" => spec.min = Some(Value::from(n)),
                    "max" => spec.max = Some(Value::from(n)),
                    "step" => spec.step = Some(n),
                    "start" => spec.start = Some(n),
                    _ => {}
                }
            }
            ArgKind::Count => {
                let n: u32 = t
                    .parse()
                    .map_err(|_| format!("{} must be a whole number.", def.label))?;
                match def.key {
                    "precision" => spec.precision = Some(n),
                    "length" => spec.length = Some(n),
                    "words" => spec.words = Some(n),
                    _ => {}
                }
            }
            ArgKind::Probability => {
                let p = num()?;
                if !(0.0..=1.0).contains(&p) {
                    return Err(format!("{} must be between 0 and 1.", def.label));
                }
                match def.key {
                    "probability" => spec.probability = Some(p),
                    "null_probability" => spec.null_probability = Some(p),
                    _ => {}
                }
            }
            ArgKind::Date => {
                let d = chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d")
                    .map_err(|_| format!("{} must be a date like 2026-01-31.", def.label))?;
                let text = match (spec.kind.as_str(), def.key) {
                    ("datetime", "min") => format!("{d}T00:00:00Z"),
                    ("datetime", _) => format!("{d}T23:59:59Z"),
                    _ => d.to_string(),
                };
                match def.key {
                    "min" => spec.min = Some(Value::String(text)),
                    _ => spec.max = Some(Value::String(text)),
                }
            }
            ArgKind::Text | ArgKind::Dataset => {
                let s = Some(t.to_string());
                match def.key {
                    "pattern" => spec.pattern = s,
                    "charset" => spec.charset = s,
                    "format" => spec.format = s,
                    "expr" => spec.expr = s,
                    "dataset" => spec.dataset = s,
                    "field" => spec.field = s,
                    "min" => spec.min = Some(Value::String(t.to_string())),
                    "max" => spec.max = Some(Value::String(t.to_string())),
                    _ => {}
                }
            }
            ArgKind::List => spec.values = Some(split(t).into_iter().map(json_or_string).collect()),
            ArgKind::Numbers => {
                let w: Result<Vec<f64>, _> = split(t).into_iter().map(str::parse).collect();
                spec.weights = Some(w.map_err(|_| format!("{} must be numbers.", def.label))?);
            }
            ArgKind::Json => spec.value = Some(json_or_string(t)),
        }
    }
    if matches!(spec.kind.as_str(), "date" | "datetime")
        && let (Some(Value::String(a)), Some(Value::String(b))) = (&spec.min, &spec.max)
        && a > b
    {
        return Err("from is after to.".into());
    }
    if let (Some(v), Some(w)) = (&spec.values, &spec.weights)
        && v.len() != w.len()
    {
        return Err(format!(
            "weights has {} numbers for {} values.",
            w.len(),
            v.len()
        ));
    }
    if spec.kind == "null_or" && spec.inner.is_none() {
        spec.inner = Some(Box::new(crate::dataset_edit::infer_generator(
            column, ty, false,
        )));
    }
    Ok(spec)
}

/// Field texts that show what `spec` already holds (to prefill the dialog).
pub fn texts_of(spec: &GeneratorSpec) -> Vec<(String, String)> {
    let num = |v: &Option<Value>| match v {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => String::new(),
    };
    args_for(&spec.kind)
        .iter()
        .map(|d| {
            let t = match d.key {
                "min" | "max" if d.kind == ArgKind::Date => {
                    let v = num(if d.key == "min" { &spec.min } else { &spec.max });
                    v.chars().take(10).collect()
                }
                "min" => num(&spec.min),
                "max" => num(&spec.max),
                "step" => spec.step.map(|n| n.to_string()).unwrap_or_default(),
                "start" => spec.start.map(|n| n.to_string()).unwrap_or_default(),
                "precision" => spec.precision.map(|n| n.to_string()).unwrap_or_default(),
                "length" => spec.length.map(|n| n.to_string()).unwrap_or_default(),
                "words" => spec.words.map(|n| n.to_string()).unwrap_or_default(),
                "probability" => spec.probability.map(|n| n.to_string()).unwrap_or_default(),
                "null_probability" => spec
                    .null_probability
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
                "pattern" => spec.pattern.clone().unwrap_or_default(),
                "charset" => spec.charset.clone().unwrap_or_default(),
                "format" => spec.format.clone().unwrap_or_default(),
                "expr" => spec.expr.clone().unwrap_or_default(),
                "dataset" => spec.dataset.clone().unwrap_or_default(),
                "field" => spec.field.clone().unwrap_or_default(),
                "values" => spec
                    .values
                    .as_ref()
                    .map(|v| {
                        v.iter()
                            .map(|x| match x {
                                Value::String(s) => s.clone(),
                                o => o.to_string(),
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default(),
                "weights" => spec
                    .weights
                    .as_ref()
                    .map(|w| w.iter().map(f64::to_string).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default(),
                "value" => spec
                    .value
                    .as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                _ => String::new(),
            };
            (d.key.to_string(), t)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn one_of_takes_values_and_matching_weights() {
        let s = apply_args(
            GeneratorSpec::of("one_of"),
            &a(&[("values", "USD, EUR, 3"), ("weights", "0.5, 0.25, 0.25")]),
            "currency",
            "string",
        )
        .unwrap();
        assert_eq!(
            s.values,
            Some(vec![Value::from("USD"), Value::from("EUR"), Value::from(3)])
        );
        assert!(apply_args(GeneratorSpec::of("one_of"), &[], "c", "string").is_err());
        assert!(
            apply_args(
                GeneratorSpec::of("one_of"),
                &a(&[("values", "a, b"), ("weights", "1")]),
                "c",
                "string"
            )
            .is_err()
        );
    }

    #[test]
    fn numbers_ranges_and_refs() {
        let s = apply_args(
            GeneratorSpec::of("float"),
            &a(&[("min", "0"), ("max", "500"), ("precision", "2")]),
            "amount",
            "float",
        )
        .unwrap();
        assert_eq!(s.max, Some(Value::from(500.0)));
        assert_eq!(s.precision, Some(2));
        assert!(apply_args(GeneratorSpec::of("int"), &a(&[("min", "x")]), "n", "int").is_err());
        assert!(
            apply_args(
                GeneratorSpec::of("bool"),
                &a(&[("probability", "2")]),
                "b",
                "bool"
            )
            .is_err()
        );
        let r = apply_args(
            GeneratorSpec::of("ref"),
            &a(&[("dataset", "users")]),
            "user_id",
            "string",
        )
        .unwrap();
        assert_eq!(r.dataset.as_deref(), Some("users"));
        let n = apply_args(GeneratorSpec::of("null_or"), &[], "note", "string").unwrap();
        assert!(n.inner.is_some());
    }

    #[test]
    fn date_ranges_cover_whole_days_and_refuse_reversed_ranges() {
        let s = apply_args(
            GeneratorSpec::of("datetime"),
            &a(&[("min", "2026-01-01"), ("max", "2026-01-31")]),
            "at",
            "datetime",
        )
        .unwrap();
        assert_eq!(s.min, Some(Value::from("2026-01-01T00:00:00Z")));
        assert_eq!(s.max, Some(Value::from("2026-01-31T23:59:59Z")));
        assert_eq!(texts_of(&s)[0].1, "2026-01-01");
        let d = apply_args(
            GeneratorSpec::of("date"),
            &a(&[("min", "2026-02-01")]),
            "d",
            "string",
        )
        .unwrap();
        assert_eq!(d.min, Some(Value::from("2026-02-01")));
        assert!(
            apply_args(
                GeneratorSpec::of("date"),
                &a(&[("min", "2026-03-01"), ("max", "2026-02-01")]),
                "d",
                "string"
            )
            .is_err()
        );
        assert!(
            apply_args(
                GeneratorSpec::of("date"),
                &a(&[("min", "soon")]),
                "d",
                "string"
            )
            .is_err()
        );
    }

    #[test]
    fn texts_round_trip() {
        let s = apply_args(
            GeneratorSpec::of("one_of"),
            &a(&[("values", "a, b")]),
            "c",
            "string",
        )
        .unwrap();
        assert_eq!(texts_of(&s)[0], ("values".into(), "a, b".into()));
        assert!(args_for("uuid").is_empty() && needs_toml("object"));
    }
}
