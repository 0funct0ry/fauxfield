//! Dataset building (M10.04): columns, manual rows, pasted rows, generated rows (keep as
//! generator or bake to seed) and the seed editor. Live-data edits are runtime state; seed edits
//! (and the structural row operations on a Dataset whose rows are its static seed) are written
//! to the workspace files through the store.

use fauxfield_model::{DatasetColumn, GeneratorField, GeneratorSeed, GeneratorSpec};
use gpui::{
    AnyElement, Context, Entity, InteractiveElement, IntoElement, ParentElement, Styled, Window,
    div, prelude::*, px,
};
use serde_json::{Map, Value};

use crate::code_editor::{CodeEditor, EditorEvent, Language};
use crate::dataset_ui::parse_cell;
use crate::shell::{PromptAction, Shell};
use crate::theme::Theme;
use crate::ui::toolbar_button;
use crate::view::Dataset;

/// Seeds above this size are not loaded into the editor (the CodeEditor caps at 1 MiB).
pub const SEED_EDIT_MAX: usize = 1 << 20;
/// Row operations write through to the seed only up to this many rows.
pub const WRITE_THROUGH_MAX: usize = 20_000;

/// The seed editor of one Dataset.
pub struct SeedEd {
    pub editor: Entity<CodeEditor>,
    /// Text last read from or written to the seed file.
    pub saved: String,
    pub problem: Option<String>,
}

impl SeedEd {
    pub fn dirty(&self, cx: &gpui::App) -> bool {
        self.editor.read(cx).text() != self.saved
    }
}

/// `name:type, name:type` into columns (type defaults to `string`).
pub fn parse_columns(text: &str) -> Result<Vec<DatasetColumn>, String> {
    let mut out: Vec<DatasetColumn> = vec![];
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (n, t) = part.split_once(':').unwrap_or((part, "string"));
        let (n, t) = (n.trim(), t.trim());
        if n.is_empty() {
            return Err("A column needs a name.".into());
        }
        if !fauxfield_model::COLUMN_TYPES.contains(&t) {
            return Err(format!(
                "Column type must be one of {}.",
                fauxfield_model::COLUMN_TYPES.join(", ")
            ));
        }
        if out.iter().any(|c| c.name == n) {
            return Err(format!("Column `{n}` is listed twice."));
        }
        out.push(DatasetColumn::new(n, t));
    }
    if out.is_empty() {
        return Err("Name at least one column (name:type, ...).".into());
    }
    Ok(out)
}

/// Default generator for a column, inferred from its name and type (SPEC §20.26).
pub fn infer_generator(name: &str, ty: &str, is_key: bool) -> GeneratorSpec {
    let n = name.to_lowercase();
    let fake = |k: &str| {
        let mut s = GeneratorSpec::of("fake");
        s.fake = Some(k.into());
        s
    };
    if is_key {
        return if ty == "int" {
            GeneratorSpec::of("sequence")
        } else {
            GeneratorSpec::of("uuid")
        };
    }
    if n.contains("email") {
        return fake("internet.email");
    }
    if n == "name" || n.ends_with("_name") || n == "title" {
        return fake("name");
    }
    match ty {
        "int" => GeneratorSpec::of("int"),
        "float" => GeneratorSpec::of("float"),
        "bool" => GeneratorSpec::of("bool"),
        "datetime" => GeneratorSpec::of("datetime"),
        "json" => {
            let mut s = GeneratorSpec::of("const");
            s.value = Some(Value::Null);
            s
        }
        _ => {
            let mut s = GeneratorSpec::of("lorem");
            s.words = Some(2);
            s
        }
    }
}

/// `kind` or `fake:internet.email` into a generator spec.
pub fn spec_from_text(text: &str) -> Result<GeneratorSpec, String> {
    let (k, rest) = text.split_once(':').unwrap_or((text, ""));
    let k = k.trim();
    if !fauxfield_model::GENERATORS.iter().any(|(g, _)| *g == k) {
        return Err(format!("Unknown generator `{k}`."));
    }
    let mut s = GeneratorSpec::of(k);
    if k == "fake" {
        if rest.trim().is_empty() {
            return Err("`fake` needs a kind, e.g. fake:internet.email.".into());
        }
        s.fake = Some(rest.trim().into());
    }
    Ok(s)
}

/// Default JSON value for a new row's column.
pub fn default_value(ty: &str) -> Value {
    match ty {
        "string" => Value::String(String::new()),
        "int" => Value::from(0),
        "float" => Value::from(0.0),
        "bool" => Value::Bool(false),
        _ => Value::Null,
    }
}

impl Shell {
    pub(crate) fn active_dataset_key(&self) -> Option<String> {
        self.tabs
            .active()
            .filter(|t| t.kind == crate::tabs::TabKind::Dataset)
            .map(|t| t.id.clone())
    }

    /// Columns to build rows from: the declared ones, else those the table shows.
    pub(crate) fn dataset_columns_now(&self, d: &Dataset, cx: &gpui::App) -> Vec<(String, String)> {
        if !d.columns.is_empty() {
            return d.columns.clone();
        }
        let keys = self
            .ds_tables
            .borrow()
            .get(&d.key())
            .map(|e| e.read(cx).delegate().column_keys());
        keys.unwrap_or_default()
            .into_iter()
            .map(|k| (k, "string".to_string()))
            .collect()
    }

    /// Persist the live rows as the static seed for a Dataset built in the table: no seed yet, or
    /// a static seed with declared columns. Other Datasets keep live edits runtime-only (Reset
    /// restores their seed); `Save Rows to Seed` writes them explicitly.
    pub(crate) fn write_through(&mut self, d: &Dataset, cx: &mut Context<Self>) {
        let built_here = d.source == "none" || (d.source == "static" && !d.columns.is_empty());
        if !built_here {
            return;
        }
        let Some(e) = self.engine() else { return };
        let Ok((n, _)) = e.dataset_page(&d.server_id, &d.name, 0, 1) else {
            return;
        };
        if n > WRITE_THROUGH_MAX {
            self.notify(
                "Too many rows to write through; use Save rows to seed.".into(),
                cx,
            );
            return;
        }
        self.save_rows_to_seed_for(d, cx);
    }

    fn save_rows_to_seed_for(&mut self, d: &Dataset, cx: &mut Context<Self>) {
        let Some(e) = self.engine() else {
            self.notify("No host is running.".into(), cx);
            return;
        };
        let Ok(rows) = e.dataset_rows(&d.server_id, &d.name) else {
            return;
        };
        let text = serde_json::to_string_pretty(&rows).unwrap_or_default();
        let (sid, name) = (d.server_id.clone(), d.name.clone());
        self.store_op(cx, |s| s.write_dataset_seed(&sid, &name, &text));
        self.ds_gen += 1;
        self.sync_running();
        cx.notify();
    }

    /// Save rows to seed: writes the live rows to `data/<name>.json` (replaces a generator).
    pub(crate) fn save_rows_to_seed(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(d) = self.ws.find_dataset(key).cloned() {
            self.save_rows_to_seed_for(&d, cx);
            self.notify(format!("Saved {} rows to data/{}.json", d.rows, d.name), cx);
        }
    }

    // ---- rows ----------------------------------------------------------------------------

    pub(crate) fn add_dataset_row(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(d) = self.ws.find_dataset(key).cloned() else {
            return;
        };
        let Some(e) = self.engine() else {
            self.notify("No host is running.".into(), cx);
            return;
        };
        let cols = self.dataset_columns_now(&d, cx);
        let mut row = Map::new();
        for (n, t) in &cols {
            // A declared default is applied by the engine; constrained columns stay empty
            // (null) so a fresh row never collides; the cell editor fills them in.
            let decl = d.column_defs.iter().find(|c| &c.name == n);
            match decl {
                Some(c) if c.default.is_some() => continue,
                Some(c) if c.unique || c.required || c.primary_key => {
                    row.insert(n.clone(), Value::Null);
                }
                _ => {
                    row.insert(n.clone(), default_value(t));
                }
            }
        }
        let keyf = d.key.clone();
        let key_ty = cols
            .iter()
            .find(|(n, _)| *n == keyf)
            .map(|(_, t)| t.as_str());
        let next_int = || {
            let max = e
                .dataset_rows(&d.server_id, &d.name)
                .ok()
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default()
                .iter()
                .filter_map(|r| r.get(&keyf).and_then(Value::as_i64))
                .max()
                .unwrap_or(0);
            Value::from(max + 1)
        };
        if key_ty == Some("int") || d.id_strategy == "increment" {
            row.insert(keyf.clone(), next_int());
        } else if key_ty.is_none_or(|t| t == "string") {
            row.insert(keyf.clone(), Value::String(fauxfield_model::new_id("row")));
        }
        match e.dataset_insert(&d.server_id, &d.name, None, vec![Value::Object(row)]) {
            Ok(_) => {
                self.ds_gen += 1;
                self.write_through(&d, cx);
                self.edit_new_row(key, cx);
            }
            Err(err) => self.notify(err.to_string(), cx),
        }
        self.sync_running();
        cx.notify();
    }

    /// Show the last row and start editing its first cell (after Add row).
    fn edit_new_row(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(ent) = self.ds_tables.borrow().get(key).cloned() else {
            return;
        };
        let generation = self.ds_gen;
        self.set_dataset_filter(key, "", cx);
        if let Some(f) = self.ds_filters.borrow().get(key).cloned() {
            f.update(cx, |t, cx| t.set_value("", cx));
        }
        ent.update(cx, |s, cx| {
            s.delegate_mut().invalidate(generation);
            s.refresh(cx);
            let last = s.delegate().shown().saturating_sub(1);
            s.delegate_mut().begin_edit(last, 1, cx);
        });
    }

    /// Paste tab-separated rows (clipboard) into the table; a first line naming the columns is
    /// skipped.
    pub(crate) fn paste_dataset_rows(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(d) = self.ws.find_dataset(key).cloned() else {
            return;
        };
        let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) else {
            self.notify("The clipboard has no text.".into(), cx);
            return;
        };
        let cols = self.dataset_columns_now(&d, cx);
        let rows = match paste_rows(&text, &cols, &d.key) {
            Ok(r) => r,
            Err(m) => return self.notify(m, cx),
        };
        let n = rows.len();
        let Some(e) = self.engine() else {
            return self.notify("No host is running.".into(), cx);
        };
        match e.dataset_insert(&d.server_id, &d.name, None, rows) {
            Ok(_) => {
                self.ds_gen += 1;
                self.write_through(&d, cx);
                self.notify(format!("Pasted {n} rows"), cx);
            }
            Err(err) => self.notify(err.to_string(), cx),
        }
        self.sync_running();
        cx.notify();
    }

    /// Delete the selected rows (or the right-clicked one) and write through.
    pub(crate) fn delete_dataset_rows(
        &mut self,
        key: &str,
        indices: &[usize],
        cx: &mut Context<Self>,
    ) {
        let Some(d) = self.ws.find_dataset(key).cloned() else {
            return;
        };
        let Some(e) = self.engine() else { return };
        match e.dataset_delete(&d.server_id, &d.name, indices) {
            Ok(_) => {
                self.ds_gen += 1;
                self.write_through(&d, cx);
            }
            Err(err) => self.notify(err.to_string(), cx),
        }
        self.sync_running();
        cx.notify();
    }

    /// Start editing the selected row's cell (the last clicked column, else the first).
    pub(crate) fn edit_selected_cell(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.active_dataset_key() else {
            return;
        };
        let Some(ent) = self.ds_tables.borrow().get(&key).cloned() else {
            return;
        };
        let Some(r) = ent.read(cx).selected_row() else {
            return self.notify("Select a row first.".into(), cx);
        };
        let col = ent
            .read(cx)
            .delegate()
            .shared
            .cell
            .get()
            .map_or(1, |(_, c)| c.max(1));
        ent.update(cx, |s, cx| s.delegate_mut().begin_edit(r, col, cx));
    }

    // ---- columns -------------------------------------------------------------------------

    fn column_target(&self, key: &str, cx: &Context<Self>) -> Option<String> {
        let ent = self.ds_tables.borrow().get(key).cloned()?;
        let st = ent.read(cx);
        let col = st.delegate().shared.cell.get().map(|(_, c)| c)?;
        st.delegate()
            .column_keys()
            .get(col.checked_sub(1)?)
            .cloned()
    }

    pub(crate) fn begin_rename_column(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if let Some(key) = self.active_dataset_key() {
            let from = self.column_target(&key, cx).unwrap_or_default();
            self.begin_prompt(
                PromptAction::RenameColumn {
                    key,
                    from: from.clone(),
                },
                &from,
                w,
                cx,
            );
        }
    }

    pub(crate) fn begin_remove_column(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if let Some(key) = self.active_dataset_key() {
            let c = self.column_target(&key, cx).unwrap_or_default();
            self.begin_prompt(PromptAction::RemoveColumn { key }, &c, w, cx);
        }
    }

    /// `name:type` or `name:type=generator` (e.g. `email:string=fake:internet.email`).
    pub(crate) fn add_column_now(&mut self, key: &str, text: &str, cx: &mut Context<Self>) {
        let (decl, gen_text) = match text.split_once('=') {
            Some((a, b)) => (a, Some(b)),
            None => (text, None),
        };
        let new = match parse_columns(decl) {
            Ok(mut c) if c.len() == 1 => c.remove(0),
            Ok(_) => return self.notify("Add one column at a time.".into(), cx),
            Err(m) => return self.notify(m, cx),
        };
        let spec = match gen_text {
            Some(g) => match spec_from_text(g) {
                Ok(s) => Some(s),
                Err(m) => return self.notify(m, cx),
            },
            None => None,
        };
        self.add_column_with(key, new, spec, cx);
    }

    /// Add a fully described column. `spec` fills existing rows (inferred when `None`).
    pub(crate) fn add_column_with(
        &mut self,
        key: &str,
        new: DatasetColumn,
        spec: Option<GeneratorSpec>,
        cx: &mut Context<Self>,
    ) {
        let Some(d) = self.ws.find_dataset(key).cloned() else {
            return;
        };
        let mut cols: Vec<DatasetColumn> = if d.columns.is_empty() {
            self.dataset_columns_now(&d, cx)
                .into_iter()
                .map(|(name, ty)| DatasetColumn::new(name, ty))
                .collect()
        } else {
            d.column_defs.clone()
        };
        if cols.iter().any(|c| c.name == new.name) {
            return self.notify(format!("Column `{}` already exists.", new.name), cx);
        }
        let spec = spec.unwrap_or_else(|| infer_generator(&new.name, &new.ty, false));
        cols.push(new.clone());
        let (sid, name) = (d.server_id.clone(), d.name.clone());
        let root = self.ws.root.clone();
        let source = d.source.clone();
        let snap_srv = self.store.as_ref().and_then(|s| {
            s.snapshot()
                .servers
                .iter()
                .map(|(_, x)| x.clone())
                .find(|x| x.id == sid)
        });
        let fake_seed = snap_srv.as_ref().and_then(|s| s.fake_seed);
        let generator = snap_srv
            .and_then(|s| s.datasets.into_iter().find(|x| x.name == name))
            .and_then(|x| x.generator);
        let col_name = new.name.clone();
        self.store_op(cx, |s| {
            s.set_dataset_columns(&sid, &name, &cols)?;
            match (source.as_str(), generator) {
                ("generator", Some(mut g)) => {
                    g.fields.push(GeneratorField {
                        name: col_name.clone(),
                        spec: spec.clone(),
                    });
                    s.set_dataset_generator(&sid, &name, Some(&g))?;
                }
                ("static", _) => {
                    let n = s
                        .read_dataset_seed(&sid, &name)?
                        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                        .and_then(|v| v.as_array().map(Vec::len))
                        .unwrap_or(0);
                    let vals = fauxfield_engine::generate::preview_spec(
                        &spec,
                        fake_seed,
                        n as u32,
                        &Default::default(),
                        &root,
                    )
                    .unwrap_or_default();
                    let mut it = vals.into_iter();
                    s.map_seed_rows(&sid, &name, |row| {
                        if let Some(o) = row.as_object_mut() {
                            o.insert(col_name.clone(), it.next().unwrap_or(Value::Null));
                        }
                    })?;
                }
                _ => {}
            }
            Ok(())
        });
        self.ds_gen += 1;
        self.sync_running();
        self.notify(format!("Added column {}", new.name), cx);
        cx.notify();
    }

    pub(crate) fn rename_column_now(
        &mut self,
        key: &str,
        from: &str,
        to: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(d) = self.ws.find_dataset(key).cloned() else {
            return;
        };
        let to = to.trim().to_string();
        if to.is_empty() || to == from {
            return;
        }
        let mut d = d;
        if d.columns.is_empty() {
            // Declare the inferred columns first so the rename has something to rewrite.
            let cols: Vec<DatasetColumn> = self
                .dataset_columns_now(&d, cx)
                .into_iter()
                .map(|(name, ty)| DatasetColumn::new(name, ty))
                .collect();
            let (sid, name) = (d.server_id.clone(), d.name.clone());
            self.store_op(cx, |s| s.set_dataset_columns(&sid, &name, &cols));
            d = self.ws.find_dataset(key).cloned().unwrap_or(d);
        }
        let (sid, name, f, t) = (
            d.server_id.clone(),
            d.name.clone(),
            from.to_string(),
            to.clone(),
        );
        if self
            .store_op(cx, |s| s.rename_dataset_column(&sid, &name, &f, &t))
            .is_some()
        {
            if let Some(l) = self.layout.dataset_tables.get_mut(key) {
                for o in l.order.iter_mut().filter(|o| *o == from) {
                    *o = to.clone();
                }
                if let Some(w) = l.widths.remove(from) {
                    l.widths.insert(to.clone(), w);
                }
            }
            // Live rows only change on a reload; edit them so a live-only Dataset follows too.
            if let Some(e) = self.engine()
                && let Ok(Value::Array(mut rows)) = e.dataset_rows(&d.server_id, &d.name)
            {
                for r in rows.iter_mut() {
                    if let Some(o) = r.as_object_mut()
                        && let Some(v) = o.remove(from)
                    {
                        o.insert(to.clone(), v);
                    }
                }
                let _ = e.dataset_set(&d.server_id, &d.name, Value::Array(rows));
            }
            self.save_layout();
            self.ds_gen += 1;
            self.sync_running();
            self.notify(format!("Renamed column {from} to {to}"), cx);
        }
        cx.notify();
    }

    pub(crate) fn remove_column_now(&mut self, key: &str, column: &str, cx: &mut Context<Self>) {
        let Some(d) = self.ws.find_dataset(key).cloned() else {
            return;
        };
        let column = column.trim().to_string();
        let declared = d.columns.iter().any(|(n, _)| *n == column);
        let (sid, name, c) = (d.server_id.clone(), d.name.clone(), column.clone());
        if declared {
            self.store_op(cx, |s| s.remove_dataset_column(&sid, &name, &c));
        }
        if let Some(e) = self.engine()
            && let Ok(Value::Array(mut rows)) = e.dataset_rows(&d.server_id, &d.name)
        {
            for r in rows.iter_mut() {
                if let Some(o) = r.as_object_mut() {
                    o.remove(&column);
                }
            }
            let _ = e.dataset_set(&d.server_id, &d.name, Value::Array(rows));
        }
        if let Some(l) = self.layout.dataset_tables.get_mut(key) {
            l.order.retain(|o| *o != column);
            l.widths.remove(&column);
        }
        self.save_layout();
        self.write_through(&d, cx);
        self.ds_gen += 1;
        self.sync_running();
        self.notify(format!("Removed column {column}"), cx);
        cx.notify();
    }

    // ---- New Dataset… from table ----------------------------------------------------------

    pub(crate) fn begin_new_dataset_from_table(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        self.begin_new_dataset(w, cx);
    }

    pub(crate) fn create_dataset_from_table(
        &mut self,
        server: &str,
        name: &str,
        columns: &str,
        cx: &mut Context<Self>,
    ) {
        let cols = match parse_columns(columns) {
            Ok(c) => c,
            Err(m) => return self.notify(m, cx),
        };
        let (sid, n) = (server.to_string(), name.to_string());
        if self
            .store_op(cx, |s| {
                s.create_dataset_with_columns(&sid, &n, None, None, None, &cols)?;
                // The table starts as an empty static seed so manual rows have a home.
                s.write_dataset_seed(&sid, &n, "[]")
            })
            .is_some()
        {
            self.sidebar_tab = crate::tree::SidebarTab::Datasets;
            self.layout.sidebar_tab = self.sidebar_tab.id().into();
            self.open_tab(
                &format!("{server}:{name}"),
                crate::tabs::TabKind::Dataset,
                name,
                true,
            );
            self.notify(
                format!("Created dataset {name} with {} columns", cols.len()),
                cx,
            );
        }
    }

    // ---- Generate rows… ------------------------------------------------------------------

    /// `N bake` (append rows to the static seed) or `N keep` (write a generator block).
    pub(crate) fn generate_rows_now(&mut self, key: &str, text: &str, cx: &mut Context<Self>) {
        let (n, keep) = match parse_generate(text) {
            Ok(x) => x,
            Err(m) => return self.notify(m, cx),
        };
        self.generate_rows_apply(key, n, keep, false, None, None, cx);
    }

    /// Generate `n` rows: `keep` writes a generator block, otherwise the rows are baked into the
    /// static seed (`replace` drops the existing rows first). `fields` overrides the inferred
    /// generators; `fake_seed` overrides the Server's.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn generate_rows_apply(
        &mut self,
        key: &str,
        n: u32,
        keep: bool,
        replace: bool,
        fields: Option<Vec<GeneratorField>>,
        fake_seed: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        let Some(d) = self.ws.find_dataset(key).cloned() else {
            return;
        };
        let cols = self.dataset_columns_now(&d, cx);
        if cols.is_empty() {
            return self.notify("Add a column first.".into(), cx);
        }
        let existing = if replace {
            vec![]
        } else {
            self.engine()
                .and_then(|e| e.dataset_rows(&d.server_id, &d.name).ok())
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default()
        };
        let start = existing
            .iter()
            .filter_map(|r| r.get(&d.key).and_then(Value::as_i64))
            .max()
            .map_or(1, |m| m + 1);
        let fields: Vec<GeneratorField> = fields.unwrap_or_else(|| {
            cols.iter()
                .map(|(name, ty)| {
                    let mut spec = infer_generator(name, ty, *name == d.key);
                    if spec.kind == "sequence" && !keep {
                        spec.start = Some(start as f64);
                    }
                    GeneratorField {
                        name: name.clone(),
                        spec,
                    }
                })
                .collect()
        });
        let mut fields = fields;
        if !keep {
            for f in fields
                .iter_mut()
                .filter(|f| f.spec.kind == "sequence" && f.spec.start.is_none())
            {
                f.spec.start = Some(start as f64);
            }
        }
        let g = GeneratorSeed { rows: n, fields };
        let names: Vec<&str> = self.ws.datasets.iter().map(|x| x.name.as_str()).collect();
        let (mut refs, mut problems) = (Vec::new(), Vec::new());
        fauxfield_model::generator::check_fields(
            &g.fields,
            "generator.fields",
            &names,
            &mut refs,
            &mut problems,
        );
        if let Some((p, m)) = problems.first() {
            return self.notify(format!("{p}: {m}"), cx);
        }
        let (sid, name) = (d.server_id.clone(), d.name.clone());
        if keep {
            self.store_op(cx, |s| {
                // The columns stay declared so the empty table keeps its shape.
                let decl: Vec<DatasetColumn> = cols
                    .iter()
                    .map(|(n, t)| {
                        d.column_defs
                            .iter()
                            .find(|c| &c.name == n)
                            .cloned()
                            .unwrap_or_else(|| DatasetColumn::new(n.clone(), t.clone()))
                    })
                    .collect();
                s.set_dataset_columns(&sid, &name, &decl)?;
                s.set_dataset_generator(&sid, &name, Some(&g))
            });
            self.ds_gen += 1;
            self.sync_running();
            self.notify(format!("{name} now generates {n} rows at each start"), cx);
            return cx.notify();
        }
        let Some(srv) = self.store.as_ref().and_then(|s| {
            s.snapshot()
                .servers
                .iter()
                .map(|(_, x)| x.clone())
                .find(|x| x.id == sid)
        }) else {
            return;
        };
        let mut datasets = srv.datasets.clone();
        if let Some(t) = datasets.iter_mut().find(|x| x.name == name) {
            t.generator = Some(g);
            t.seed = None;
            t.seed_template = None;
        }
        let rows = match fauxfield_engine::generate::generate_dataset(
            &datasets,
            &name,
            None,
            fake_seed.or(srv.fake_seed),
            &self.ws.root,
        ) {
            Ok(v) => v,
            Err(m) => return self.notify(m, cx),
        };
        let mut all = existing;
        all.extend(rows.as_array().cloned().unwrap_or_default());
        let text = serde_json::to_string_pretty(&all).unwrap_or_default();
        self.store_op(cx, |s| {
            s.set_dataset_generator(&sid, &name, None)?;
            s.write_dataset_seed(&sid, &name, &text)
        });
        self.ds_gen += 1;
        self.sync_running();
        self.notify(format!("Baked {n} rows into data/{name}.json"), cx);
        cx.notify();
    }

    // ---- seed editor ---------------------------------------------------------------------

    /// Body of the Seed section: a JSON editor over the seed file with validation.
    pub(crate) fn seed_editor_body(
        &self,
        t: &Theme,
        d: &Dataset,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = d.key();
        let muted = t.text_muted;
        let msg = |text: String| {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(12.))
                .text_color(muted)
                .child(text)
                .into_any_element()
        };
        let Some(store) = self.store.clone() else {
            return msg("Open a workspace first.".into());
        };
        let Some(disk) = d
            .seed
            .as_ref()
            .and_then(|_| store.read_workspace_file(d.seed.as_deref().unwrap_or("")))
        else {
            return msg(match d.source.as_str() {
                "generator" => "This Dataset is generated; bake it to get an editable seed.".into(),
                "template" => {
                    "This Dataset uses a seed template; edit files/ in your editor.".into()
                }
                _ => "No seed file yet. Add rows, or Generate rows… and bake them.".into(),
            });
        };
        if disk.len() > SEED_EDIT_MAX {
            return msg(format!(
                "The seed is {} KiB: too large for the editor. Edit {} in your editor.",
                disk.len() / 1024,
                d.seed.clone().unwrap_or_default()
            ));
        }
        let mut eds = self.seed_eds.borrow_mut();
        let st = self.editor_settings.clone();
        let entry = eds.entry(key.clone()).or_insert_with(|| {
            let ed = cx.new(|cx| CodeEditor::new(Language::Json, cx).line_numbers(true));
            ed.update(cx, |e, cx| e.set_text(&disk, cx));
            let k = key.clone();
            cx.subscribe(&ed, move |this: &mut Shell, _, ev: &EditorEvent, cx| {
                if matches!(ev, EditorEvent::Changed) {
                    this.seed_changed(&k, cx);
                }
            })
            .detach();
            SeedEd {
                editor: ed,
                saved: disk.clone(),
                problem: None,
            }
        });
        // Follow the file when it changed on disk and there are no unsaved edits.
        if !entry.dirty(cx) && entry.saved != disk {
            entry.saved = disk.clone();
            entry.editor.update(cx, |e, cx| e.set_text(&disk, cx));
        }
        entry.editor.update(cx, |e, cx| e.apply_settings(&st, cx));
        let dirty = entry.dirty(cx);
        let problem = entry.problem.clone();
        let ed = entry.editor.clone();
        drop(eds);
        let (k1, k2) = (key.clone(), key.clone());
        let bar = div()
            .flex_none()
            .h(px(32.))
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(8.))
            .border_b_1()
            .border_color(t.border_variant)
            .text_size(px(12.))
            .child(crate::dev_id!(
                div()
                    .id(crate::ui::eid(format!("ds-seed-state-{key}")))
                    .text_color(if problem.is_some() { t.deleted } else { muted })
                    .child(match (&problem, dirty) {
                        (Some(p), _) => p.clone(),
                        (None, true) => "Unsaved seed edits".to_string(),
                        (None, false) => format!("{} (saved)", d.seed.clone().unwrap_or_default()),
                    }),
                format!("ds-seed-state-{key}"),
                Text,
                "Seed state",
                None
            ))
            .child(div().flex_1())
            .child(crate::dev_id!(
                toolbar_button(t, "ds-seed-save")
                    .child("Save seed")
                    .on_click(cx.listener(move |this, _, _, cx| this.save_seed(&k1, cx))),
                "ds-seed-save",
                Button,
                "Save seed",
                Action("save_dataset_seed")
            ))
            .child(crate::dev_id!(
                toolbar_button(t, "ds-seed-reload")
                    .child("Reload from seed")
                    .on_click(cx.listener(move |this, _, _, cx| this.reload_from_seed(&k2, cx))),
                "ds-seed-reload",
                Button,
                "Reload from seed",
                Action("reload_dataset_from_seed")
            ));
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(bar)
            .child(div().flex_1().min_h_0().child(ed))
            .into_any_element()
    }

    fn seed_changed(&mut self, key: &str, cx: &mut Context<Self>) {
        let mut eds = self.seed_eds.borrow_mut();
        let Some(s) = eds.get_mut(key) else { return };
        let text = s.editor.read(cx).text().to_string();
        s.problem = match crate::code_editor::format::validate_json(&text) {
            Err(e) => Some(format!("{}:{}: {}", e.line, e.col, e.message)),
            Ok(()) => match serde_json::from_str::<Value>(&text) {
                Ok(Value::Array(a)) if a.iter().all(Value::is_object) => None,
                Ok(Value::Object(_)) => None,
                _ => Some("The seed must be an array of objects (or one object).".into()),
            },
        };
        drop(eds);
        cx.notify();
    }

    pub(crate) fn save_seed(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(d) = self.ws.find_dataset(key).cloned() else {
            return;
        };
        let text = {
            let eds = self.seed_eds.borrow();
            let Some(s) = eds.get(key) else { return };
            if let Some(p) = &s.problem {
                let p = p.clone();
                drop(eds);
                return self.notify(format!("Seed not saved: {p}"), cx);
            }
            s.editor.read(cx).text().to_string()
        };
        let (sid, name, t) = (d.server_id.clone(), d.name.clone(), text.clone());
        if self
            .store_op(cx, |s| s.write_dataset_seed(&sid, &name, &t))
            .is_some()
        {
            if let Some(s) = self.seed_eds.borrow_mut().get_mut(key) {
                s.saved = text;
            }
            self.ds_gen += 1;
            self.sync_running();
            self.notify(format!("Saved seed of {}", d.name), cx);
        }
        cx.notify();
    }

    /// Discard live edits and unsaved seed edits: the Dataset starts again from its seed file.
    pub(crate) fn reload_from_seed(&mut self, key: &str, cx: &mut Context<Self>) {
        let disk = self.ws.find_dataset(key).and_then(|d| {
            let rel = d.seed.clone()?;
            self.store.as_ref()?.read_workspace_file(&rel)
        });
        if let (Some(disk), Some(s)) = (disk, self.seed_eds.borrow_mut().get_mut(key)) {
            s.saved = disk.clone();
            s.editor.update(cx, |e, cx| e.set_text(&disk, cx));
            s.problem = None;
        }
        self.reset_dataset(key, cx);
    }

    #[cfg(feature = "dev-mcp")]
    /// Dirty-seed flags for `ui.state`: `(dataset key, dirty)`.
    pub(crate) fn seed_dirty(&self, key: &str, cx: &gpui::App) -> bool {
        self.seed_eds.borrow().get(key).is_some_and(|s| s.dirty(cx))
    }
}

/// `N bake` or `N keep` (mode defaults to bake).
pub fn parse_generate(text: &str) -> Result<(u32, bool), String> {
    let mut it = text.split_whitespace();
    let n: u32 = it
        .next()
        .and_then(|n| n.parse().ok())
        .filter(|n| (1..=1_000_000).contains(n))
        .ok_or("Row count must be 1 to 1000000.")?;
    match it.next() {
        None | Some("bake") => Ok((n, false)),
        Some("keep") => Ok((n, true)),
        Some(_) => Err("Use `bake` (write rows to the seed) or `keep` (keep as generator).".into()),
    }
}

/// Parse tab-separated clipboard text into rows for `cols`; a leading header line is skipped.
pub fn paste_rows(text: &str, cols: &[(String, String)], key: &str) -> Result<Vec<Value>, String> {
    if cols.is_empty() {
        return Err("Add a column before pasting rows.".into());
    }
    let mut lines: Vec<Vec<&str>> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split('\t').collect())
        .collect();
    if lines.first().is_some_and(|f| {
        f.len() == cols.len() && f.iter().zip(cols).all(|(a, (n, _))| a.trim() == n)
    }) {
        lines.remove(0);
    }
    if lines.is_empty() {
        return Err("Nothing to paste.".into());
    }
    let mut out = Vec::with_capacity(lines.len());
    for (ln, cells) in lines.iter().enumerate() {
        let mut row = Map::new();
        for (i, (n, t)) in cols.iter().enumerate() {
            let raw = cells.get(i).copied().unwrap_or("");
            let v =
                parse_cell(Some(t), raw).map_err(|e| format!("Row {}, column {n}: {e}", ln + 1))?;
            row.insert(n.clone(), v);
        }
        if row.get(key).is_none_or(Value::is_null) {
            row.insert(key.into(), Value::String(fauxfield_model::new_id("row")));
        }
        out.push(Value::Object(row));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_parse_and_reject() {
        let c = parse_columns("id:int, name, price:float").unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!(c[1].ty, "string");
        assert!(parse_columns("a:nope").is_err());
        assert!(parse_columns("a, a").is_err());
        assert!(parse_columns("").is_err());
    }

    #[test]
    fn generators_are_inferred_from_name_and_type() {
        assert_eq!(infer_generator("email", "string", false).kind, "fake");
        assert_eq!(infer_generator("id", "int", true).kind, "sequence");
        assert_eq!(infer_generator("id", "string", true).kind, "uuid");
        assert_eq!(infer_generator("price", "float", false).kind, "float");
        assert_eq!(
            spec_from_text("fake:internet.email")
                .unwrap()
                .fake
                .as_deref(),
            Some("internet.email")
        );
        assert!(spec_from_text("fake").is_err());
        assert!(spec_from_text("nope").is_err());
    }

    #[test]
    fn generate_text_parses() {
        assert_eq!(parse_generate("200 bake").unwrap(), (200, false));
        assert_eq!(parse_generate("5 keep").unwrap(), (5, true));
        assert_eq!(parse_generate("7").unwrap(), (7, false));
        assert!(parse_generate("0").is_err());
        assert!(parse_generate("5 maybe").is_err());
    }

    #[test]
    fn paste_maps_columns_skips_header_and_types_cells() {
        let cols = vec![
            ("id".to_string(), "int".to_string()),
            ("name".to_string(), "string".to_string()),
        ];
        let rows = paste_rows("id\tname\n1\tAda\n2\tBob\n", &cols, "id").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["id"], 1);
        assert_eq!(rows[1]["name"], "Bob");
        assert!(paste_rows("a\tb", &cols, "id").is_err(), "a is not an int");
        assert!(paste_rows("", &cols, "id").is_err());
    }
}
