//! Import a JSON, CSV or Parquet file into a Dataset (M10.05).
//!
//! Flow: pick a file (OS picker or drag-and-drop) -> for a new Dataset ask for its name -> a
//! preview notice (row count, inferred types, warnings, first row) and an options prompt
//! (`replace|append key=COL delim=X decimal=string COL:TYPE`) -> commit writes `data/<name>.json`.
//! The original file is not referenced afterwards.

use std::path::{Path, PathBuf};

use fauxfield_engine::import::{self as imp, DecimalAs, Format, ImportOptions, ImportResult};
use gpui::{Context, Window};
use serde_json::Value;

use crate::shell::{PromptAction, Shell};

/// Parsed text of the options prompt.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImportChoice {
    pub append: bool,
    pub opts: ImportOptions,
}

/// `replace|append key=COL delim=X decimal=string format=csv COL:TYPE ...`
pub fn parse_import_choice(text: &str) -> Result<ImportChoice, String> {
    let mut c = ImportChoice::default();
    for tok in text.split_whitespace() {
        match tok {
            "replace" => c.append = false,
            "append" => c.append = true,
            _ => {
                if let Some(v) = tok.strip_prefix("key=") {
                    c.opts.key = Some(v.to_string());
                } else if let Some(v) = tok.strip_prefix("delim=") {
                    c.opts.delimiter = Some(match v {
                        "tab" => b'\t',
                        v if v.len() == 1 => v.as_bytes()[0],
                        _ => return Err("delim= takes one character or `tab`.".into()),
                    });
                } else if let Some(v) = tok.strip_prefix("decimal=") {
                    c.opts.decimal_as = match v {
                        "string" => DecimalAs::String,
                        "number" => DecimalAs::Number,
                        _ => return Err("decimal= is `number` or `string`.".into()),
                    };
                } else if let Some(v) = tok.strip_prefix("format=") {
                    c.opts.format =
                        Some(Format::parse(v).ok_or("format= is json, csv or parquet.")?);
                } else if let Some((n, t)) = tok.split_once(':') {
                    if !["int", "float", "bool", "string"].contains(&t) {
                        return Err(format!("`{tok}`: type is int, float, bool or string."));
                    }
                    c.opts.column_types.insert(n.to_string(), t.to_string());
                } else {
                    return Err(format!("Unknown option `{tok}`."));
                }
            }
        }
    }
    Ok(c)
}

/// One-notice preview: format, rows, columns with inferred types, warnings, first row.
pub fn preview_text(file: &Path, r: &ImportResult) -> String {
    let cols = r
        .columns
        .iter()
        .take(8)
        .map(|c| format!("{}:{}", c.name, c.ty))
        .collect::<Vec<_>>()
        .join(", ");
    let name = file
        .file_name()
        .map_or(String::new(), |n| n.to_string_lossy().into());
    let mut s = format!(
        "{name}: {} · {} row(s) · {cols}{}",
        r.format.name(),
        r.row_count(),
        if r.columns.len() > 8 { ", …" } else { "" }
    );
    if let Some(k) = &r.key {
        s.push_str(&format!(" · key {k}"));
    }
    if let Some(first) = r.first_rows(1).first() {
        let mut j = first.to_string();
        if j.len() > 120 {
            j.truncate(120);
            j.push('…');
        }
        s.push_str(&format!(" · first {j}"));
    }
    for w in r.warnings.iter().take(3) {
        s.push_str(&format!(" · warning: {w}"));
    }
    s
}

impl Shell {
    /// Dataset: Import File… (a new Dataset) opens the OS file picker.
    pub(crate) fn begin_import_new(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self
            .current_server_id()
            .or_else(|| self.ws.servers.first().map(|s| s.id.clone()))
            .is_none()
        {
            return self.notify("Create a server first.".into(), cx);
        }
        self.pick_import_file(None, w, cx);
    }

    /// Dataset tab Import (replace or append) into the open Dataset.
    pub(crate) fn begin_import_existing(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        match self.active_dataset_key() {
            Some(key) => self.pick_import_file(Some(key), w, cx),
            None => self.notify("Open a dataset first.".into(), cx),
        }
    }

    fn pick_import_file(
        &mut self,
        existing: Option<String>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        });
        cx.spawn_in(w, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(p) = paths.into_iter().next()
            {
                this.update_in(cx, |this, w, cx| {
                    this.import_file_chosen(existing, p, w, cx)
                })
                .ok();
            }
        })
        .detach();
    }

    /// Files dropped on the Datasets tab: into the open Dataset when one is active, else a new one.
    pub(crate) fn import_dropped(
        &mut self,
        paths: Vec<PathBuf>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_tab != crate::tree::SidebarTab::Datasets {
            return;
        }
        let Some(p) = paths.into_iter().next() else {
            return;
        };
        if Format::detect(&p).is_none() {
            return self.notify("Drop a .json, .csv or .parquet file.".into(), cx);
        }
        self.import_file_chosen(None, p, w, cx);
    }

    pub(crate) fn import_file_chosen(
        &mut self,
        existing: Option<String>,
        path: PathBuf,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path_s = path.to_string_lossy().to_string();
        match existing {
            Some(key) => {
                let Some(d) = self.ws.find_dataset(&key).cloned() else {
                    return;
                };
                self.show_import_options(&d.server_id, &d.name, true, &path_s, w, cx);
            }
            None => {
                let Some(server) = self
                    .current_server_id()
                    .or_else(|| self.ws.servers.first().map(|s| s.id.clone()))
                else {
                    return self.notify("Create a server first.".into(), cx);
                };
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().replace(' ', "_"))
                    .unwrap_or_default();
                self.begin_prompt(
                    PromptAction::ImportName {
                        server,
                        path: path_s,
                    },
                    &stem,
                    w,
                    cx,
                );
            }
        }
    }

    /// Read with default options, show the preview, then ask for the options.
    pub(crate) fn show_import_options(
        &mut self,
        server: &str,
        name: &str,
        existing: bool,
        path: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match imp::read(Path::new(path), &ImportOptions::default()) {
            Ok(r) => {
                let text = preview_text(Path::new(path), &r);
                self.import_preview = Some(text.clone());
                self.notify(text, cx);
            }
            Err(m) => {
                self.import_preview = None;
                return self.notify(m, cx);
            }
        }
        self.begin_prompt(
            PromptAction::ImportOptions {
                server: server.into(),
                name: name.into(),
                existing,
                path: path.into(),
            },
            "replace",
            w,
            cx,
        );
    }

    /// Read the file with the chosen options and write the seed.
    pub(crate) fn import_commit(
        &mut self,
        server: &str,
        name: &str,
        existing: bool,
        path: &str,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let choice = match parse_import_choice(text) {
            Ok(c) => c,
            Err(m) => return self.notify(m, cx),
        };
        let r = match imp::read(Path::new(path), &choice.opts) {
            Ok(r) => r,
            Err(m) => return self.notify(m, cx),
        };
        let mut rows = r.seed_value();
        if choice.append && !r.singleton && existing {
            let current = self
                .engine()
                .and_then(|e| e.dataset_rows(server, name).ok())
                .unwrap_or(Value::Null);
            rows = imp::append_rows(&current, &r.rows);
        }
        let total = rows.as_array().map_or(1, Vec::len);
        let (sid, n) = (server.to_string(), name.to_string());
        if !existing
            && self
                .store_op(cx, |s| s.create_dataset(&sid, &n, None, None, None))
                .is_none()
        {
            return;
        }
        let spec = fauxfield_store::DatasetSpec {
            name: name.into(),
            seed_json: Some(serde_json::to_string_pretty(&rows).unwrap_or_default()),
            key: choice
                .opts
                .key
                .clone()
                .or_else(|| r.key.clone().filter(|_| !existing)),
            ..Default::default()
        };
        if self
            .store_op(cx, |s| s.upsert_dataset(&sid, &spec))
            .is_none()
        {
            return;
        }
        self.ds_gen += 1;
        self.sync_running();
        if !existing {
            self.sidebar_tab = crate::tree::SidebarTab::Datasets;
            self.layout.sidebar_tab = self.sidebar_tab.id().into();
        }
        self.open_tab(
            &format!("{server}:{name}"),
            crate::tabs::TabKind::Dataset,
            name,
            true,
        );
        self.import_preview = None;
        let w = if r.warnings.is_empty() {
            String::new()
        } else {
            format!(" ({} warning(s): {})", r.warnings.len(), r.warnings[0])
        };
        self.notify(
            format!(
                "Imported {} row(s) into {name}; now {total}{w}",
                r.row_count()
            ),
            cx,
        );
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choice_grammar() {
        let c = parse_import_choice("append key=sku delim=tab zip:string decimal=string").unwrap();
        assert!(c.append);
        assert_eq!(c.opts.key.as_deref(), Some("sku"));
        assert_eq!(c.opts.delimiter, Some(b'\t'));
        assert_eq!(c.opts.column_types["zip"], "string");
        assert_eq!(c.opts.decimal_as, DecimalAs::String);
        assert!(!parse_import_choice("").unwrap().append);
        assert!(parse_import_choice("zip:uuid").is_err());
        assert!(parse_import_choice("bogus").is_err());
        assert!(parse_import_choice("delim=ab").is_err());
    }
}
