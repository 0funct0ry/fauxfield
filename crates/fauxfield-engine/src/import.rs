//! Dataset import and export (M10.05): JSON, CSV and Parquet.
//!
//! One code path for the GUI, CLI and MCP. Import is a one-time conversion: the file is read into
//! JSON rows; the caller writes them to `data/<name>.json`. The original file is never referenced.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde_json::{Map, Number, Value};

/// Warn above this many rows (configurable per call).
pub const DEFAULT_SOFT_LIMIT: usize = 100_000;
/// Refuse above this many rows (configurable per call).
pub const DEFAULT_HARD_LIMIT: usize = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Csv,
    Parquet,
}

impl Format {
    pub fn parse(s: &str) -> Option<Format> {
        match s.to_ascii_lowercase().as_str() {
            "json" => Some(Format::Json),
            "csv" | "tsv" => Some(Format::Csv),
            "parquet" | "pq" => Some(Format::Parquet),
            _ => None,
        }
    }

    /// Format chosen from the file extension.
    pub fn detect(path: &Path) -> Option<Format> {
        Format::parse(path.extension()?.to_str()?)
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Json => "json",
            Format::Csv => "csv",
            Format::Parquet => "parquet",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DecimalAs {
    #[default]
    Number,
    String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportOptions {
    pub format: Option<Format>,
    /// CSV delimiter override; detected when `None`.
    pub delimiter: Option<u8>,
    /// Key column (default `id` when present).
    pub key: Option<String>,
    /// CSV column type overrides: `int`, `float`, `bool`, `string`, `null`.
    pub column_types: BTreeMap<String, String>,
    pub decimal_as: DecimalAs,
    pub soft_limit: usize,
    pub hard_limit: usize,
}

impl Default for ImportOptions {
    fn default() -> Self {
        ImportOptions {
            format: None,
            delimiter: None,
            key: None,
            column_types: BTreeMap::new(),
            decimal_as: DecimalAs::Number,
            soft_limit: DEFAULT_SOFT_LIMIT,
            hard_limit: DEFAULT_HARD_LIMIT,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnInfo {
    pub name: String,
    /// `int`, `float`, `bool`, `string`, `null`, `json` (nested or mixed).
    pub ty: String,
}

#[derive(Debug, Clone)]
pub struct ImportResult {
    pub format: Format,
    /// The converted rows (an object file becomes one row with `singleton` set).
    pub rows: Vec<Value>,
    /// A JSON object file: the Dataset is a singleton and the seed is the object itself.
    pub singleton: bool,
    pub columns: Vec<ColumnInfo>,
    /// Key column used (explicit, or `id` when every row has it).
    pub key: Option<String>,
    pub warnings: Vec<String>,
}

impl ImportResult {
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// First `n` rows for the preview.
    pub fn first_rows(&self, n: usize) -> &[Value] {
        &self.rows[..self.rows.len().min(n)]
    }

    /// The JSON written to the seed file.
    pub fn seed_value(&self) -> Value {
        if self.singleton {
            self.rows.first().cloned().unwrap_or(Value::Null)
        } else {
            Value::Array(self.rows.clone())
        }
    }
}

/// Read and convert `path`. Pure apart from reading the file; nothing is written.
pub fn read(path: &Path, opts: &ImportOptions) -> Result<ImportResult, String> {
    let format = opts
        .format
        .or_else(|| Format::detect(path))
        .ok_or_else(|| {
            format!(
                "{}: unknown format; pass --format json|csv|parquet",
                path.display()
            )
        })?;
    let mut warnings = Vec::new();
    let (rows, singleton, mut columns) = match format {
        Format::Json => {
            let text =
                std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            from_json(&text)?
        }
        Format::Csv => {
            let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            from_csv(&bytes, opts, &mut warnings)?
        }
        Format::Parquet => from_parquet(path, opts)?,
    };
    if rows.len() > opts.hard_limit {
        return Err(format!(
            "{} rows exceeds the hard limit of {}; split the file or raise the limit",
            rows.len(),
            opts.hard_limit
        ));
    }
    if rows.len() > opts.soft_limit {
        warnings.push(format!(
            "{} rows is above {}; the Dataset will be large",
            rows.len(),
            opts.soft_limit
        ));
    }
    if columns.is_empty() {
        columns = infer_columns(&rows);
    }
    let key = match &opts.key {
        Some(k) => {
            if !singleton && !columns.iter().any(|c| &c.name == k) {
                return Err(format!("key column `{k}` not found"));
            }
            Some(k.clone())
        }
        None => columns
            .iter()
            .any(|c| c.name == "id")
            .then(|| "id".to_string()),
    };
    if let (Some(k), false) = (&key, singleton) {
        let mut seen = HashSet::new();
        let mut dups = 0usize;
        let mut missing = 0usize;
        for r in &rows {
            match r.get(k) {
                None | Some(Value::Null) => missing += 1,
                Some(v) => {
                    if !seen.insert(v.to_string()) {
                        dups += 1;
                    }
                }
            }
        }
        if dups > 0 {
            warnings.push(format!("{dups} duplicate value(s) in key column `{k}`"));
        }
        if missing > 0 {
            warnings.push(format!(
                "{missing} row(s) have no `{k}`; keys follow the Dataset id_strategy"
            ));
        }
    }
    Ok(ImportResult {
        format,
        rows,
        singleton,
        columns,
        key,
        warnings,
    })
}

type Parsed = (Vec<Value>, bool, Vec<ColumnInfo>);

fn from_json(text: &str) -> Result<Parsed, String> {
    match serde_json::from_str::<Value>(text).map_err(|e| format!("not valid JSON: {e}"))? {
        Value::Array(rows) => {
            if let Some(i) = rows.iter().position(|r| !r.is_object()) {
                return Err(format!("row {} is not a JSON object", i + 1));
            }
            Ok((rows, false, Vec::new()))
        }
        v @ Value::Object(_) => Ok((vec![v], true, Vec::new())),
        _ => Err("the file must hold a JSON array of objects or one object".into()),
    }
}

/// Pick the delimiter whose count is most consistent over the first lines.
pub fn detect_delimiter(text: &str) -> u8 {
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).take(10).collect();
    let mut best = (b',', 0usize);
    for d in [b',', b';', b'\t', b'|'] {
        let counts: Vec<usize> = lines.iter().map(|l| count_outside_quotes(l, d)).collect();
        let Some(&first) = counts.first() else {
            continue;
        };
        if first == 0 {
            continue;
        }
        let consistent = counts.iter().filter(|c| **c == first).count();
        let score = consistent * 1000 + first;
        if score > best.1 {
            best = (d, score);
        }
    }
    best.0
}

fn count_outside_quotes(line: &str, d: u8) -> usize {
    let mut inq = false;
    let mut n = 0;
    for b in line.bytes() {
        if b == b'"' {
            inq = !inq;
        } else if b == d && !inq {
            n += 1;
        }
    }
    n
}

/// Infer the JSON type of one CSV cell.
pub fn infer_cell(s: &str) -> Value {
    let t = s.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("null") {
        return Value::Null;
    }
    if t.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if t.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if let Ok(i) = t.parse::<i64>() {
        // Leading zeros ("007") are identifiers, not numbers.
        if !(t.len() > 1 && t.trim_start_matches('-').starts_with('0')) {
            return Value::Number(i.into());
        }
    } else if let Ok(f) = t.parse::<f64>()
        && t.chars().any(|c| c.is_ascii_digit())
        && !t.contains(['n', 'N', 'i', 'I'])
        && let Some(n) = Number::from_f64(f)
    {
        return Value::Number(n);
    }
    Value::String(s.to_string())
}

fn convert_cell(s: &str, ty: &str) -> Result<Value, ()> {
    let t = s.trim();
    match ty {
        "string" => Ok(Value::String(s.to_string())),
        "auto_string" if t.is_empty() => Ok(Value::Null),
        "auto_string" => Ok(Value::String(s.to_string())),
        "null" => Ok(Value::Null),
        _ if t.is_empty() => Ok(Value::Null),
        "int" => t
            .parse::<i64>()
            .map(|i| Value::Number(i.into()))
            .map_err(|_| ()),
        "float" => t
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number)
            .ok_or(()),
        "bool" => match t.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(Value::Bool(true)),
            "false" | "0" | "no" => Ok(Value::Bool(false)),
            _ => Err(()),
        },
        _ => Ok(infer_cell(s)),
    }
}

fn from_csv(
    bytes: &[u8],
    opts: &ImportOptions,
    warnings: &mut Vec<String>,
) -> Result<Parsed, String> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let text = String::from_utf8_lossy(bytes);
    let delim = opts.delimiter.unwrap_or_else(|| detect_delimiter(&text));
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delim)
        .flexible(true)
        .has_headers(true)
        .from_reader(text.as_bytes());
    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| format!("CSV header: {e}"))?
        .iter()
        .map(|h| h.trim().to_string())
        .collect();
    if headers.is_empty() || headers.iter().all(|h| h.is_empty()) {
        return Err("CSV has no header row".into());
    }
    let mut records: Vec<Vec<String>> = Vec::new();
    let (mut ragged, mut first_ragged) = (0usize, 0usize);
    for (i, rec) in rdr.records().enumerate() {
        let rec = rec.map_err(|e| format!("CSV row {}: {e}", i + 2))?;
        if rec.len() != headers.len() {
            ragged += 1;
            if first_ragged == 0 {
                first_ragged = i + 2;
            }
        }
        records.push(rec.iter().map(str::to_string).collect());
    }
    if ragged > 0 {
        warnings.push(format!(
            "{ragged} ragged row(s) (first at line {first_ragged}): missing cells are null, extra cells are dropped"
        ));
    }
    // Column types: override, else the join of the inferred cell types.
    let mut types: Vec<String> = Vec::new();
    for (c, h) in headers.iter().enumerate() {
        if let Some(t) = opts.column_types.get(h) {
            types.push(t.clone());
            continue;
        }
        let mut ty = "null";
        for r in &records {
            let cell = r.get(c).map(String::as_str).unwrap_or("");
            ty = join_type(ty, value_type(&infer_cell(cell)));
        }
        types.push(ty.to_string());
    }
    let mut bad = 0usize;
    let mut rows = Vec::with_capacity(records.len());
    for r in &records {
        let mut o = Map::new();
        for (c, h) in headers.iter().enumerate() {
            let cell = r.get(c).map(String::as_str).unwrap_or("");
            // Mixed columns stay strings so no cell silently changes meaning.
            let overridden = opts.column_types.contains_key(h);
            let ty = match types[c].as_str() {
                "json" => "auto_string",
                "string" if !overridden => "auto_string",
                t => t,
            };
            let v = convert_cell(cell, ty).unwrap_or_else(|_| {
                bad += 1;
                Value::Null
            });
            o.insert(h.clone(), v);
        }
        rows.push(Value::Object(o));
    }
    if bad > 0 {
        warnings.push(format!(
            "{bad} cell(s) could not be parsed as their column type and became null"
        ));
    }
    let columns = headers
        .into_iter()
        .zip(types)
        .map(|(name, ty)| ColumnInfo {
            name,
            ty: if ty == "json" { "string".into() } else { ty },
        })
        .collect();
    Ok((rows, false, columns))
}

fn value_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_i64() || n.is_u64() => "int",
        Value::Number(_) => "float",
        Value::String(_) => "string",
        _ => "json",
    }
}

fn join_type(a: &'static str, b: &'static str) -> &'static str {
    match (a, b) {
        _ if a == b => a,
        ("null", x) | (x, "null") => x,
        ("int", "float") | ("float", "int") => "float",
        _ => "json",
    }
}

/// Column list (union of keys, first-seen order) with the joined type of its values.
pub fn infer_columns(rows: &[Value]) -> Vec<ColumnInfo> {
    let mut order: Vec<String> = Vec::new();
    let mut types: BTreeMap<String, &'static str> = BTreeMap::new();
    for r in rows {
        let Some(o) = r.as_object() else { continue };
        for (k, v) in o {
            let t = types.entry(k.clone()).or_insert_with(|| {
                order.push(k.clone());
                "null"
            });
            *t = join_type(t, value_type(v));
        }
    }
    order
        .into_iter()
        .map(|name| {
            let ty = types[&name].to_string();
            ColumnInfo { name, ty }
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Parquet read

fn from_parquet(path: &Path, opts: &ImportOptions) -> Result<Parsed, String> {
    use parquet::file::reader::{FileReader, SerializedFileReader};
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let reader = SerializedFileReader::new(file).map_err(|e| format!("{}: {e}", path.display()))?;
    let total = reader.metadata().file_metadata().num_rows();
    if total as usize > opts.hard_limit {
        return Err(format!(
            "{total} rows exceeds the hard limit of {}; split the file or raise the limit",
            opts.hard_limit
        ));
    }
    let iter = reader.get_row_iter(None).map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    for row in iter {
        let row = row.map_err(|e| e.to_string())?;
        let o: Map<String, Value> = row
            .get_column_iter()
            .map(|(k, f)| (k.clone(), field_json(f, opts.decimal_as)))
            .collect();
        rows.push(Value::Object(o));
    }
    Ok((rows, false, Vec::new()))
}

fn field_json(f: &parquet::record::Field, dec: DecimalAs) -> Value {
    use parquet::record::Field as F;
    let num = |x: f64| {
        Number::from_f64(x)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    };
    match f {
        F::Null => Value::Null,
        F::Bool(b) => Value::Bool(*b),
        F::Byte(v) => Value::from(*v),
        F::Short(v) => Value::from(*v),
        F::Int(v) => Value::from(*v),
        F::Long(v) => Value::from(*v),
        F::UByte(v) => Value::from(*v),
        F::UShort(v) => Value::from(*v),
        F::UInt(v) => Value::from(*v),
        F::ULong(v) => Value::from(*v),
        F::Float16(v) => num(f64::from(*v)),
        F::Float(v) => num(f64::from(*v)),
        F::Double(v) => num(*v),
        F::Decimal(d) => {
            let s = decimal_string(d.data(), d.scale());
            match dec {
                DecimalAs::String => Value::String(s),
                DecimalAs::Number => match s.parse::<f64>() {
                    Ok(x) if s.contains('.') => num(x),
                    _ => s
                        .parse::<i64>()
                        .map(Value::from)
                        .unwrap_or_else(|_| Value::String(s)),
                },
            }
        }
        F::Str(s) => Value::String(s.clone()),
        F::Bytes(b) => Value::String(hex(b.data())),
        F::Date(days) => chrono::DateTime::from_timestamp(i64::from(*days) * 86_400, 0)
            .map(|d| Value::String(d.format("%Y-%m-%d").to_string()))
            .unwrap_or(Value::Null),
        F::TimeMillis(ms) => Value::String(time_of_day(i64::from(*ms) * 1_000)),
        F::TimeMicros(us) => Value::String(time_of_day(*us)),
        F::TimestampMillis(ms) => iso(chrono::DateTime::from_timestamp_millis(*ms)),
        F::TimestampMicros(us) => iso(chrono::DateTime::from_timestamp_micros(*us)),
        F::Group(r) => Value::Object(
            r.get_column_iter()
                .map(|(k, v)| (k.clone(), field_json(v, dec)))
                .collect(),
        ),
        F::ListInternal(l) => {
            Value::Array(l.elements().iter().map(|e| field_json(e, dec)).collect())
        }
        F::MapInternal(m) => Value::Object(
            m.entries()
                .iter()
                .map(|(k, v)| {
                    let key = match field_json(k, dec) {
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    (key, field_json(v, dec))
                })
                .collect(),
        ),
    }
}

fn iso(d: Option<chrono::DateTime<chrono::Utc>>) -> Value {
    d.map(|d| Value::String(d.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)))
        .unwrap_or(Value::Null)
}

fn time_of_day(us: i64) -> String {
    let s = us / 1_000_000;
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Big-endian two's complement bytes and a scale to a decimal string.
fn decimal_string(bytes: &[u8], scale: i32) -> String {
    if bytes.is_empty() || bytes.len() > 16 {
        return hex(bytes);
    }
    let neg = bytes[0] & 0x80 != 0;
    let mut buf = [if neg { 0xFF } else { 0 }; 16];
    buf[16 - bytes.len()..].copy_from_slice(bytes);
    let v = i128::from_be_bytes(buf);
    let (sign, digits) = (if v < 0 { "-" } else { "" }, v.unsigned_abs().to_string());
    let scale = scale.max(0) as usize;
    if scale == 0 {
        return format!("{sign}{digits}");
    }
    let padded = format!("{digits:0>width$}", width = scale + 1);
    let (int, frac) = padded.split_at(padded.len() - scale);
    format!("{sign}{int}.{frac}")
}

// ---------------------------------------------------------------------------------------------
// Export

fn rows_of(value: &Value) -> Vec<Value> {
    match value {
        Value::Array(a) => a.clone(),
        Value::Null => Vec::new(),
        other => vec![other.clone()],
    }
}

/// Serialise rows in `format`.
pub fn export(value: &Value, format: Format) -> Result<Vec<u8>, String> {
    match format {
        Format::Json => Ok(serde_json::to_string_pretty(value)
            .map_err(|e| e.to_string())?
            .into_bytes()),
        Format::Csv => to_csv(&rows_of(value)),
        Format::Parquet => to_parquet(&rows_of(value)),
    }
}

/// Options of the GUI export modal (M10.07). Row scope is applied by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportOptions {
    pub format: Format,
    /// Columns to write, in order; empty means every column.
    pub columns: Vec<String>,
    /// JSON: pretty (default) or compact.
    pub pretty: bool,
    /// CSV: write the header row.
    pub csv_header: bool,
    /// CSV delimiter byte.
    pub delimiter: u8,
}

impl ExportOptions {
    pub fn new(format: Format) -> Self {
        Self {
            format,
            columns: vec![],
            pretty: true,
            csv_header: true,
            delimiter: b',',
        }
    }
}

/// Serialise `rows` with the modal's options: column subset and order, JSON style, CSV header
/// and delimiter.
pub fn export_with(rows: &[Value], o: &ExportOptions) -> Result<Vec<u8>, String> {
    let names: Vec<String> = if o.columns.is_empty() {
        infer_columns(rows).into_iter().map(|c| c.name).collect()
    } else {
        o.columns.clone()
    };
    let project = |r: &Value| -> Value {
        let mut m = serde_json::Map::new();
        for n in &names {
            if let Some(v) = r.get(n) {
                m.insert(n.clone(), v.clone());
            }
        }
        Value::Object(m)
    };
    match o.format {
        Format::Json => {
            let v = Value::Array(rows.iter().map(project).collect());
            let s = if o.pretty {
                serde_json::to_string_pretty(&v)
            } else {
                serde_json::to_string(&v)
            };
            Ok(s.map_err(|e| e.to_string())?.into_bytes())
        }
        Format::Csv => {
            let mut w = csv::WriterBuilder::new()
                .delimiter(o.delimiter)
                .from_writer(Vec::new());
            if o.csv_header {
                w.write_record(names.iter().map(String::as_str))
                    .map_err(|e| e.to_string())?;
            }
            for r in rows {
                w.write_record(names.iter().map(|n| cell_text(r.get(n))))
                    .map_err(|e| e.to_string())?;
            }
            w.into_inner().map_err(|e| e.to_string())
        }
        Format::Parquet => to_parquet(&rows.iter().map(project).collect::<Vec<_>>()),
    }
}

/// Estimated output size: exports up to 500 rows and scales to `rows.len()`.
pub fn estimate_export_size(rows: &[Value], o: &ExportOptions) -> usize {
    let n = rows.len().min(500);
    if n == 0 {
        return 0;
    }
    let sample = export_with(&rows[..n], o).map_or(0, |b| b.len());
    sample * rows.len() / n
}

fn cell_text(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// CSV with a header row; nested values are written as JSON text.
pub fn to_csv(rows: &[Value]) -> Result<Vec<u8>, String> {
    let cols = infer_columns(rows);
    let mut w = csv::Writer::from_writer(Vec::new());
    w.write_record(cols.iter().map(|c| c.name.as_str()))
        .map_err(|e| e.to_string())?;
    for r in rows {
        w.write_record(cols.iter().map(|c| cell_text(r.get(&c.name))))
            .map_err(|e| e.to_string())?;
    }
    w.into_inner().map_err(|e| e.to_string())
}

/// Flat Parquet: bool, int64, double and UTF-8 columns; nested or mixed columns hold JSON text.
pub fn to_parquet(rows: &[Value]) -> Result<Vec<u8>, String> {
    use parquet::basic::Compression;
    use parquet::data_type::{BoolType, ByteArray, ByteArrayType, DoubleType, Int64Type};
    use parquet::file::properties::WriterProperties;
    use parquet::file::writer::SerializedFileWriter;
    use std::sync::Arc;

    let cols = infer_columns(rows);
    if cols.is_empty() {
        return Err("nothing to export: no columns".into());
    }
    use parquet::basic::{ConvertedType, Repetition, Type as Physical};
    use parquet::schema::types::Type;
    let mut fields = Vec::new();
    for c in &cols {
        let (phys, utf8) = match c.ty.as_str() {
            "bool" => (Physical::BOOLEAN, false),
            "int" => (Physical::INT64, false),
            "float" => (Physical::DOUBLE, false),
            _ => (Physical::BYTE_ARRAY, true),
        };
        let mut b =
            Type::primitive_type_builder(&c.name, phys).with_repetition(Repetition::OPTIONAL);
        if utf8 {
            b = b.with_converted_type(ConvertedType::UTF8);
        }
        fields.push(Arc::new(b.build().map_err(|e| e.to_string())?));
    }
    let schema = Arc::new(
        Type::group_type_builder("fauxfield")
            .with_fields(fields)
            .build()
            .map_err(|e| e.to_string())?,
    );
    let props = Arc::new(
        WriterProperties::builder()
            .set_compression(Compression::SNAPPY)
            .build(),
    );
    let mut buf = Vec::new();
    let mut w = SerializedFileWriter::new(&mut buf, schema, props).map_err(|e| e.to_string())?;
    let mut rg = w.next_row_group().map_err(|e| e.to_string())?;
    for c in &cols {
        let mut cw = rg
            .next_column()
            .map_err(|e| e.to_string())?
            .ok_or("schema column count mismatch")?;
        let vals: Vec<Option<&Value>> = rows
            .iter()
            .map(|r| r.get(&c.name).filter(|v| !v.is_null()))
            .collect();
        let def: Vec<i16> = vals.iter().map(|v| i16::from(v.is_some())).collect();
        let err = |e: parquet::errors::ParquetError| e.to_string();
        match c.ty.as_str() {
            "bool" => {
                let d: Vec<bool> = vals.iter().flatten().filter_map(|v| v.as_bool()).collect();
                cw.typed::<BoolType>()
                    .write_batch(&d, Some(&def), None)
                    .map_err(err)?;
            }
            "int" => {
                let d: Vec<i64> = vals.iter().flatten().filter_map(|v| v.as_i64()).collect();
                cw.typed::<Int64Type>()
                    .write_batch(&d, Some(&def), None)
                    .map_err(err)?;
            }
            "float" => {
                let d: Vec<f64> = vals.iter().flatten().filter_map(|v| v.as_f64()).collect();
                cw.typed::<DoubleType>()
                    .write_batch(&d, Some(&def), None)
                    .map_err(err)?;
            }
            _ => {
                let d: Vec<ByteArray> = vals
                    .iter()
                    .flatten()
                    .map(|v| match v {
                        Value::String(s) if c.ty == "string" => ByteArray::from(s.as_str()),
                        other => ByteArray::from(other.to_string().as_str()),
                    })
                    .collect();
                cw.typed::<ByteArrayType>()
                    .write_batch(&d, Some(&def), None)
                    .map_err(err)?;
            }
        }
        cw.close().map_err(|e| e.to_string())?;
    }
    rg.close().map_err(|e| e.to_string())?;
    w.close().map_err(|e| e.to_string())?;
    Ok(buf)
}

/// Combine existing seed rows with imported rows (append mode).
pub fn append_rows(existing: &Value, imported: &[Value]) -> Value {
    let mut all = rows_of(existing);
    all.extend_from_slice(imported);
    Value::Array(all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn export_with_projects_columns_and_honours_csv_options() {
        let rows = vec![
            json!({"id":1,"name":"a","x":true}),
            json!({"id":2,"name":"b;c","x":false}),
        ];
        let mut o = ExportOptions::new(Format::Csv);
        o.columns = vec!["name".into(), "id".into()];
        o.delimiter = b';';
        let out = String::from_utf8(export_with(&rows, &o).unwrap()).unwrap();
        assert_eq!(out, "name;id\na;1\n\"b;c\";2\n");
        o.csv_header = false;
        assert!(
            !String::from_utf8(export_with(&rows, &o).unwrap())
                .unwrap()
                .contains("name;id")
        );
        let mut j = ExportOptions::new(Format::Json);
        j.pretty = false;
        j.columns = vec!["id".into()];
        assert_eq!(
            String::from_utf8(export_with(&rows, &j).unwrap()).unwrap(),
            r#"[{"id":1},{"id":2}]"#
        );
        assert!(estimate_export_size(&rows, &j) > 0);
    }

    fn tmp(name: &str, body: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(name);
        std::fs::write(&p, body).unwrap();
        (d, p)
    }

    #[test]
    fn json_array_and_singleton() {
        let (_d, p) = tmp("a.json", br#"[{"id":1},{"id":2}]"#);
        let r = read(&p, &ImportOptions::default()).unwrap();
        assert_eq!(
            (r.row_count(), r.singleton, r.key.as_deref()),
            (2, false, Some("id"))
        );
        let (_d, p) = tmp("o.json", br#"{"name":"x"}"#);
        let r = read(&p, &ImportOptions::default()).unwrap();
        assert!(r.singleton);
        assert_eq!(r.seed_value(), json!({"name":"x"}));
        let (_d, p) = tmp("bad.json", b"[1,2]");
        assert!(read(&p, &ImportOptions::default()).is_err());
    }

    #[test]
    fn csv_inference_and_delimiter() {
        let (_d, p) = tmp(
            "a.csv",
            b"id;name;price;ok;note\n1;\"a;b\";2.5;true;\n2;c;3;false;x\n",
        );
        let r = read(&p, &ImportOptions::default()).unwrap();
        assert_eq!(
            r.rows[0],
            json!({"id":1,"name":"a;b","price":2.5,"ok":true,"note":null})
        );
        let ty = |n: &str| r.columns.iter().find(|c| c.name == n).unwrap().ty.clone();
        assert_eq!(
            (ty("id").as_str(), ty("price").as_str(), ty("ok").as_str()),
            ("int", "float", "bool")
        );
        assert_eq!(detect_delimiter("a\tb\tc\n1\t2\t3\n"), b'\t');
    }

    #[test]
    fn csv_override_ragged_and_dups() {
        let (_d, p) = tmp("a.csv", b"id,zip\n1,02134\n1,9\n3\n");
        let mut o = ImportOptions::default();
        o.column_types.insert("id".into(), "string".into());
        let r = read(&p, &o).unwrap();
        assert_eq!(r.rows[0]["id"], json!("1"));
        assert_eq!(r.rows[0]["zip"], json!("02134"));
        assert!(r.warnings.iter().any(|w| w.contains("ragged")));
        assert!(r.warnings.iter().any(|w| w.contains("duplicate")));
        let (_d, p) = tmp("b.csv", b"n\nabc\n");
        let mut o = ImportOptions::default();
        o.column_types.insert("n".into(), "int".into());
        let r = read(&p, &o).unwrap();
        assert!(r.warnings.iter().any(|w| w.contains("could not be parsed")));
    }

    #[test]
    fn key_and_size_guard() {
        let (_d, p) = tmp("a.csv", b"sku,n\na,1\nb,2\n");
        let o = ImportOptions {
            key: Some("sku".into()),
            ..Default::default()
        };
        assert_eq!(read(&p, &o).unwrap().key.as_deref(), Some("sku"));
        let o = ImportOptions {
            key: Some("zzz".into()),
            ..Default::default()
        };
        assert!(read(&p, &o).is_err());
        let o = ImportOptions {
            soft_limit: 1,
            ..Default::default()
        };
        assert!(
            read(&p, &o)
                .unwrap()
                .warnings
                .iter()
                .any(|w| w.contains("above"))
        );
        let o = ImportOptions {
            hard_limit: 1,
            ..Default::default()
        };
        assert!(read(&p, &o).unwrap_err().contains("hard limit"));
    }

    #[test]
    fn round_trip_all_formats() {
        let rows = json!([
            {"id":1,"name":"a","price":1.5,"ok":true},
            {"id":2,"name":"b","price":2.5,"ok":false}
        ]);
        for (f, ext) in [
            (Format::Json, "json"),
            (Format::Csv, "csv"),
            (Format::Parquet, "parquet"),
        ] {
            let bytes = export(&rows, f).unwrap();
            let (_d, p) = tmp(&format!("x.{ext}"), &bytes);
            let r = read(&p, &ImportOptions::default()).unwrap();
            assert_eq!(r.seed_value(), rows, "{ext}");
        }
    }

    #[test]
    fn parquet_nested_stays_json_text_and_nulls() {
        let rows = json!([{"id":1,"tags":["a"],"x":null},{"id":2,"tags":["b"],"x":3}]);
        let bytes = export(&rows, Format::Parquet).unwrap();
        let (_d, p) = tmp("n.parquet", &bytes);
        let r = read(&p, &ImportOptions::default()).unwrap();
        assert_eq!(r.rows[1]["x"], json!(3));
        assert_eq!(r.rows[0]["x"], Value::Null);
    }

    #[test]
    fn decimals() {
        assert_eq!(decimal_string(&[0x04, 0xD2], 2), "12.34");
        assert_eq!(decimal_string(&[0xFB, 0x2E], 2), "-12.34");
        assert_eq!(decimal_string(&[5], 0), "5");
    }

    #[test]
    fn append() {
        let v = append_rows(&json!([{"id":1}]), &[json!({"id":2})]);
        assert_eq!(v, json!([{"id":1},{"id":2}]));
    }
}
