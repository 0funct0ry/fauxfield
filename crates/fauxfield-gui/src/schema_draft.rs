//! Pure model of the schema designer (M10.07): the draft the modal edits, its live validation,
//! the TOML preview and the starter templates. No GPUI here, so it is unit tested.

use fauxfield_model::{COLUMN_TYPES, DatasetColumn};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SourceKind {
    /// `seed = "data/<name>.json"`, rows edited in the table.
    #[default]
    Static,
    /// `seed_template = "files/<name>.json.tera"`.
    Template,
    /// `[datasets.generator]`, rows regenerated at each start.
    Generator,
}

impl SourceKind {
    pub const ALL: [SourceKind; 3] = [Self::Static, Self::Template, Self::Generator];
    pub fn label(self) -> &'static str {
        match self {
            Self::Static => "Static seed file",
            Self::Template => "Seed template",
            Self::Generator => "Generator",
        }
    }
    pub fn from_label(s: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|k| k.label() == s)
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColDraft {
    pub name: String,
    pub ty: String,
    pub primary_key: bool,
    pub unique: bool,
    pub required: bool,
    pub default: String,
    /// `kind` or `fake:<kind>`; empty means "infer".
    pub generator: String,
    pub description: String,
}

impl ColDraft {
    pub fn new(name: &str, ty: &str) -> Self {
        Self {
            name: name.into(),
            ty: ty.into(),
            primary_key: false,
            unique: false,
            required: false,
            default: String::new(),
            generator: String::new(),
            description: String::new(),
        }
    }

    pub fn from_column(c: &DatasetColumn) -> Self {
        Self {
            name: c.name.clone(),
            ty: if c.ty.is_empty() {
                "string".into()
            } else {
                c.ty.clone()
            },
            primary_key: c.primary_key,
            unique: c.unique,
            required: c.required,
            default: match &c.default {
                Some(toml::Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
                None => String::new(),
            },
            generator: String::new(),
            description: c.description.clone().unwrap_or_default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SchemaDraft {
    pub name: String,
    pub server: String,
    pub description: String,
    pub source: SourceKind,
    /// Generator row count (text, validated).
    pub rows: String,
    pub columns: Vec<ColDraft>,
    pub create_crud: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Where {
    Name,
    Server,
    Rows,
    Column(usize, ColField),
    General,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColField {
    Name,
    Type,
    Default,
    Generator,
    Key,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub at: Where,
    pub code: &'static str,
    pub message: String,
}

pub const STARTERS: [&str; 3] = ["id + name", "Users", "Products"];

pub fn starter(label: &str) -> Vec<ColDraft> {
    let mk = |name: &str, ty: &str, f: &dyn Fn(&mut ColDraft)| {
        let mut c = ColDraft::new(name, ty);
        f(&mut c);
        c
    };
    match label {
        "Users" => vec![
            mk("id", "int", &|c| c.primary_key = true),
            mk("name", "string", &|c| c.required = true),
            mk("email", "string", &|c| {
                c.unique = true;
                c.required = true;
            }),
            mk("active", "bool", &|c| c.default = "true".into()),
            mk("created_at", "datetime", &|_| {}),
        ],
        "Products" => vec![
            mk("id", "int", &|c| c.primary_key = true),
            mk("name", "string", &|c| c.required = true),
            mk("sku", "string", &|c| c.unique = true),
            mk("price", "float", &|c| c.default = "0".into()),
            mk("in_stock", "bool", &|c| c.default = "true".into()),
        ],
        _ => vec![
            mk("id", "int", &|c| c.primary_key = true),
            mk("name", "string", &|_| {}),
        ],
    }
}

fn parse_default(c: &ColDraft) -> Result<Option<toml::Value>, String> {
    let t = c.default.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let bad = || format!("`{t}` is not a valid {}.", c.ty);
    Ok(Some(match c.ty.as_str() {
        "int" => toml::Value::Integer(t.parse().map_err(|_| bad())?),
        "float" => toml::Value::Float(t.parse().map_err(|_| bad())?),
        "bool" => toml::Value::Boolean(t.parse().map_err(|_| bad())?),
        "datetime" => {
            let v = serde_json::Value::String(t.into());
            if !fauxfield_model::columns::value_fits_type("datetime", &v) {
                return Err(bad());
            }
            toml::Value::String(t.into())
        }
        "json" => toml::Value::String(t.into()),
        _ => toml::Value::String(c.default.clone()),
    }))
}

/// Live validation. `existing` are the Dataset names already on the chosen Server; `editing`
/// is true for Edit schema (the name is then fixed and not checked for a clash).
pub fn validate_schema_draft(d: &SchemaDraft, existing: &[String], editing: bool) -> Vec<Issue> {
    let mut out = vec![];
    let mut push = |at, code, message: String| out.push(Issue { at, code, message });
    let n = d.name.trim();
    if n.is_empty() {
        push(Where::Name, "name", "A Dataset needs a name.".into());
    } else if !n
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        push(
            Where::Name,
            "E073",
            "Use letters, digits, `_` and `-` only.".into(),
        );
    } else if !editing && existing.iter().any(|e| e == n) {
        push(Where::Name, "name", format!("`{n}` already exists."));
    }
    if d.server.trim().is_empty() {
        push(Where::Server, "server", "Pick a Server.".into());
    }
    if d.source == SourceKind::Generator {
        match d.rows.trim().parse::<usize>() {
            Ok(r) if (1..=100_000).contains(&r) => {}
            _ => push(
                Where::Rows,
                "rows",
                "Rows is a number from 1 to 100000.".into(),
            ),
        }
    }
    let mut seen = std::collections::HashSet::new();
    for (i, c) in d.columns.iter().enumerate() {
        let cn = c.name.trim();
        if cn.is_empty() {
            push(
                Where::Column(i, ColField::Name),
                "E078",
                "Column name is empty.".into(),
            );
        } else if !seen.insert(cn.to_string()) {
            push(
                Where::Column(i, ColField::Name),
                "E078",
                format!("Column `{cn}` is listed twice."),
            );
        }
        if !COLUMN_TYPES.contains(&c.ty.as_str()) {
            push(
                Where::Column(i, ColField::Type),
                "E079",
                format!("Type must be one of {}.", COLUMN_TYPES.join(", ")),
            );
        }
        if let Err(m) = parse_default(c) {
            push(Where::Column(i, ColField::Default), "E099", m);
        }
        if !c.generator.trim().is_empty()
            && let Err(m) = crate::dataset_edit::spec_from_text(c.generator.trim())
        {
            push(Where::Column(i, ColField::Generator), "generator", m);
        }
    }
    let pks: Vec<usize> = d
        .columns
        .iter()
        .enumerate()
        .filter(|(_, c)| c.primary_key)
        .map(|(i, _)| i)
        .collect();
    if pks.len() > 1 {
        for i in pks {
            push(
                Where::Column(i, ColField::Key),
                "E098",
                "At most one column may be the primary key.".into(),
            );
        }
    }
    out
}

/// The declared columns the draft produces (call only when [`validate_schema_draft`] is clean).
pub fn to_columns(d: &SchemaDraft) -> Vec<DatasetColumn> {
    d.columns
        .iter()
        .map(|c| DatasetColumn {
            name: c.name.trim().into(),
            ty: c.ty.clone(),
            primary_key: c.primary_key,
            unique: c.unique,
            required: c.required,
            default: parse_default(c).ok().flatten(),
            description: Some(c.description.trim().to_string()).filter(|s| !s.is_empty()),
        })
        .collect()
}

/// Seed file the Dataset will use, for the preview.
pub fn seed_path(d: &SchemaDraft) -> Option<String> {
    let n = d.name.trim();
    match d.source {
        SourceKind::Static => Some(format!("data/{n}.json")),
        SourceKind::Template => Some(format!("files/{n}.json.tera")),
        SourceKind::Generator => None,
    }
}

/// The TOML the designer will write (preview only).
pub fn preview_toml(d: &SchemaDraft) -> String {
    let mut s = String::from("[[datasets]]\n");
    s += &format!("name = {:?}\n", d.name.trim());
    if !d.description.trim().is_empty() {
        s += &format!("description = {:?}\n", d.description.trim());
    }
    match d.source {
        SourceKind::Static => s += &format!("seed = {:?}\n", seed_path(d).unwrap_or_default()),
        SourceKind::Template => {
            s += &format!("seed_template = {:?}\n", seed_path(d).unwrap_or_default())
        }
        SourceKind::Generator => {}
    }
    if let Some(pk) = d.columns.iter().find(|c| c.primary_key) {
        s += &format!("key = {:?}\n", pk.name.trim());
    }
    for c in to_columns(d) {
        s += "\n[[datasets.columns]]\n";
        s += &format!("name = {:?}\ntype = {:?}\n", c.name, c.ty);
        for (k, on) in [
            ("primary_key", c.primary_key),
            ("unique", c.unique),
            ("required", c.required),
        ] {
            if on {
                s += &format!("{k} = true\n");
            }
        }
        if let Some(v) = &c.default {
            s += &format!("default = {v}\n");
        }
        if let Some(v) = &c.description {
            s += &format!("description = {v:?}\n");
        }
    }
    if d.source == SourceKind::Generator {
        s += &format!("\n[datasets.generator]\nrows = {}\n", d.rows.trim());
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> SchemaDraft {
        SchemaDraft {
            name: "products".into(),
            server: "srv_a".into(),
            rows: "10".into(),
            columns: starter("Products"),
            ..Default::default()
        }
    }

    #[test]
    fn starters_are_valid() {
        for s in STARTERS {
            let mut d = draft();
            d.columns = starter(s);
            assert!(validate_schema_draft(&d, &[], false).is_empty(), "{s}");
        }
    }

    #[test]
    fn reports_each_problem() {
        let mut d = draft();
        d.name = "".into();
        d.columns[1].name = "id".into();
        d.columns[2].primary_key = true;
        d.columns[3].default = "abc".into();
        d.columns[4].name = " ".into();
        let codes: Vec<_> = validate_schema_draft(&d, &[], false)
            .iter()
            .map(|i| i.code)
            .collect();
        for c in ["name", "E078", "E098", "E099"] {
            assert!(codes.contains(&c), "{c} in {codes:?}");
        }
        let mut d = draft();
        assert!(!validate_schema_draft(&d, &["products".into()], false).is_empty());
        assert!(validate_schema_draft(&d, &["products".into()], true).is_empty());
        d.source = SourceKind::Generator;
        d.rows = "0".into();
        assert!(
            validate_schema_draft(&d, &[], false)
                .iter()
                .any(|i| i.at == Where::Rows)
        );
    }

    #[test]
    fn preview_shows_constraints_and_seed_path() {
        let t = preview_toml(&draft());
        assert!(t.contains("seed = \"data/products.json\""), "{t}");
        assert!(t.contains("key = \"id\"") && t.contains("primary_key = true"));
        assert!(t.contains("default = 0.0"));
        let cols = to_columns(&draft());
        assert_eq!(cols.len(), 5);
        assert!(cols[0].primary_key && cols[2].unique);
    }
}
