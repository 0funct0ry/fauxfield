//! Column constraints of a Dataset (M10.07): type fit, `required`, `unique`, `default`.
//! Pure functions shared by the engine (enforcement) and the GUI (inline errors).

use crate::DatasetColumn;
use serde_json::Value;

/// Does `v` fit the column type? `null` fits every type; the `required` check is separate.
pub fn value_fits_type(ty: &str, v: &Value) -> bool {
    if v.is_null() {
        return true;
    }
    match ty {
        "" | "string" => v.is_string(),
        "int" => v.is_i64() || v.is_u64(),
        "float" => v.is_number(),
        "bool" => v.is_boolean(),
        "datetime" => v.as_str().is_some_and(looks_like_datetime),
        _ => true,
    }
}

fn looks_like_datetime(s: &str) -> bool {
    // `YYYY-MM-DD` optionally followed by `T` or space and a time.
    let b = s.as_bytes();
    b.len() >= 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b[..4]
            .iter()
            .chain(&b[5..7])
            .chain(&b[8..10])
            .all(u8::is_ascii_digit)
        && (b.len() == 10 || matches!(b[10], b'T' | b' '))
}

fn is_empty(v: &Value) -> bool {
    v.is_null() || v.as_str().is_some_and(str::is_empty)
}

/// The column default as JSON.
pub fn default_value(c: &DatasetColumn) -> Option<Value> {
    c.default
        .as_ref()
        .and_then(|d| serde_json::to_value(d).ok())
}

/// Does the column default fit its own type?
pub fn default_fits(c: &DatasetColumn) -> bool {
    default_value(c).is_none_or(|v| value_fits_type(&c.ty, &v))
}

/// Check one cell: type, `required`, `unique` (against every other row). `index` is the row
/// being edited (`None` for a row not yet inserted).
pub fn check_cell(
    cols: &[DatasetColumn],
    column: &str,
    val: &Value,
    rows: &[Value],
    index: Option<usize>,
) -> Result<(), String> {
    let Some(c) = cols.iter().find(|c| c.name == column) else {
        return Ok(());
    };
    if !value_fits_type(&c.ty, val) {
        return Err(format!(
            "`{column}` must be {}.",
            if c.ty.is_empty() { "string" } else { &c.ty }
        ));
    }
    if (c.required || c.primary_key) && is_empty(val) {
        return Err(format!("`{column}` is required."));
    }
    if (c.unique || c.primary_key)
        && !val.is_null()
        && rows
            .iter()
            .enumerate()
            .any(|(i, r)| Some(i) != index && r.get(column) == Some(val))
    {
        return Err(format!("`{column}` must be unique; {val} already exists."));
    }
    Ok(())
}

/// Fill missing columns of a new row from their defaults.
pub fn apply_defaults(cols: &[DatasetColumn], row: &mut Value) {
    if let Some(o) = row.as_object_mut() {
        for c in cols {
            if !o.contains_key(&c.name)
                && let Some(d) = default_value(c)
            {
                o.insert(c.name.clone(), d);
            }
        }
    }
}

/// Check a whole new row against the existing `rows` (type, required, unique).
pub fn check_row(cols: &[DatasetColumn], row: &Value, rows: &[Value]) -> Result<(), String> {
    for c in cols {
        let v = row.get(&c.name).unwrap_or(&Value::Null);
        check_cell(cols, &c.name, v, rows, None)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cols() -> Vec<DatasetColumn> {
        let mut id = DatasetColumn::new("id", "int");
        id.primary_key = true;
        let mut name = DatasetColumn::new("name", "string");
        name.required = true;
        let mut sku = DatasetColumn::new("sku", "string");
        sku.unique = true;
        let mut qty = DatasetColumn::new("qty", "int");
        qty.default = Some(toml::Value::Integer(1));
        vec![id, name, sku, qty]
    }

    #[test]
    fn types_required_unique() {
        let c = cols();
        let rows = vec![json!({"id":1,"name":"a","sku":"x"})];
        assert!(check_cell(&c, "qty", &json!("no"), &rows, Some(0)).is_err());
        assert!(check_cell(&c, "name", &json!(""), &rows, Some(0)).is_err());
        assert!(check_cell(&c, "sku", &json!("x"), &rows, Some(0)).is_ok());
        assert!(check_cell(&c, "sku", &json!("x"), &rows, None).is_err());
        assert!(check_cell(&c, "id", &json!(1), &rows, None).is_err());
        assert!(check_cell(&c, "qty", &Value::Null, &rows, None).is_ok());
    }

    #[test]
    fn defaults_and_datetime() {
        let c = cols();
        let mut r = json!({"id":2});
        apply_defaults(&c, &mut r);
        assert_eq!(r["qty"], json!(1));
        assert!(value_fits_type("datetime", &json!("2026-01-02T03:04:05Z")));
        assert!(!value_fits_type("datetime", &json!("yesterday")));
        assert!(default_fits(&c[3]));
    }
}
