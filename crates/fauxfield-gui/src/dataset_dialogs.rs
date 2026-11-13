//! The Export and Seed data modals of the Dataset tab (M10.07). Both are hand-built from
//! `TextInput` and `ComboBox` like the schema designer.

use fauxfield_engine::import::{ExportOptions, Format, estimate_export_size, export_with};
use fauxfield_model::{GeneratorField, GeneratorSeed, GeneratorSpec};
use gpui::{
    AnyElement, Context, Entity, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    ParentElement, StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::*, px,
};
use serde_json::Value;

use crate::combo_box::{
    ComboEvent, ComboHandler, ComboItem, ComboLook, ComboSite, ComboSize, ComboWidth,
};
use crate::shell::Shell;
use crate::text_input::{InputEvent, InputSize, TextInput};
use crate::theme::Theme;
use crate::ui::{eid, toolbar_button};

// ---- shared helpers ---------------------------------------------------------------------

fn pick(
    shell: &Shell,
    cx: &mut Context<Shell>,
    key: &str,
    items: &[&str],
    value: &str,
    w: f32,
    on: ComboHandler,
) -> AnyElement {
    shell.combo_field(
        cx,
        ComboSite {
            key: key.into(),
            items: items
                .iter()
                .map(|s| ComboItem::plain(s.to_string()))
                .collect(),
            value: Some(value.to_string().into()),
            fallback: Some(value.to_string().into()),
            color: None,
            tip: None,
            width: ComboWidth::Fixed(w),
            look: ComboLook::Field,
            size: ComboSize::Small,
            on_event: on,
        },
    )
}

fn small_input(cx: &mut Context<Shell>, ph: &str, text: &str) -> Entity<TextInput> {
    cx.new(|cx| {
        TextInput::new(cx)
            .size(InputSize::Small)
            .placeholder(ph.to_string())
            .text(text)
    })
}

fn btn(t: &Theme, id: String, text: String) -> gpui::Stateful<gpui::Div> {
    toolbar_button(t, eid(id)).h(px(22.)).px(px(8.)).child(text)
}

/// A spec as the text the generator ComboBox shows: `kind` or `fake:<kind>`.
pub fn spec_to_text(s: &GeneratorSpec) -> String {
    match (s.kind.as_str(), &s.fake) {
        ("fake", Some(f)) => format!("fake:{f}"),
        (k, _) => k.to_string(),
    }
}

pub fn human_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", bytes as f64 / 1024.0 / 1024.0)
    }
}

// ---- Export ------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    All,
    Selected,
    First,
}

pub struct ExportDialog {
    pub key: String,
    pub name: String,
    pub format: Format,
    pub scope: Scope,
    pub first_n: Entity<TextInput>,
    /// Column name and whether it is written, in output order.
    pub cols: Vec<(String, bool)>,
    pub pretty: bool,
    pub header: bool,
    pub delimiter: u8,
    pub clipboard: bool,
    pub path: Entity<TextInput>,
    pub selected: Vec<usize>,
    pub total: usize,
    pub error: Option<String>,
    _subs: Vec<Subscription>,
}

fn delim_label(d: u8) -> &'static str {
    match d {
        b';' => "semicolon",
        b'\t' => "tab",
        b'|' => "pipe",
        _ => "comma",
    }
}

fn delim_from(s: &str) -> u8 {
    match s {
        "semicolon" => b';',
        "tab" => b'\t',
        "pipe" => b'|',
        _ => b',',
    }
}

fn fmt_label(f: Format) -> &'static str {
    match f {
        Format::Json => "JSON",
        Format::Csv => "CSV",
        Format::Parquet => "Parquet",
    }
}

fn ext_of(f: Format) -> &'static str {
    match f {
        Format::Json => "json",
        Format::Csv => "csv",
        Format::Parquet => "parquet",
    }
}

impl ExportDialog {
    pub fn options(&self) -> ExportOptions {
        let mut o = ExportOptions::new(self.format);
        o.columns = self
            .cols
            .iter()
            .filter(|(_, on)| *on)
            .map(|(n, _)| n.clone())
            .collect();
        o.pretty = self.pretty;
        o.csv_header = self.header;
        o.delimiter = self.delimiter;
        o
    }

    /// The rows the scope picks out of `all`.
    pub fn scoped(&self, all: &[Value], first_n: usize) -> Vec<Value> {
        match self.scope {
            Scope::All => all.to_vec(),
            Scope::Selected => self
                .selected
                .iter()
                .filter_map(|i| all.get(*i).cloned())
                .collect(),
            Scope::First => all.iter().take(first_n).cloned().collect(),
        }
    }
}

impl Shell {
    pub(crate) fn begin_export(&mut self, key: &str, w: &mut Window, cx: &mut Context<Self>) {
        let (Some(d), Some(e)) = (self.ws.find_dataset(key).cloned(), self.engine()) else {
            return self.notify("No host is running.".into(), cx);
        };
        let rows = match e.dataset_rows(&d.server_id, &d.name) {
            Ok(Value::Array(a)) => a,
            Ok(other) => vec![other],
            Err(err) => return self.notify(err.to_string(), cx),
        };
        let mut names: Vec<String> = d.columns.iter().map(|(n, _)| n.clone()).collect();
        for r in &rows {
            if let Some(o) = r.as_object() {
                for k in o.keys() {
                    if !names.contains(k) {
                        names.push(k.clone());
                    }
                }
            }
        }
        let selected = self
            .ds_tables
            .borrow()
            .get(key)
            .map(|t| t.read(cx).delegate().selection.indices())
            .unwrap_or_default();
        // Empty until a folder is chosen with Browse… (or a path is typed).
        let path_in = small_input(cx, "Choose a folder with Browse…", "");
        let first_in = small_input(cx, "100", "100");
        let mut subs = vec![];
        for input in [&path_in, &first_in] {
            subs.push(
                cx.subscribe_in(
                    input,
                    w,
                    |this: &mut Shell, _, ev: &InputEvent, w, cx| match ev {
                        InputEvent::PressEnter { .. } => this.export_op("go", "", w, cx),
                        InputEvent::Escape => this.export_op("cancel", "", w, cx),
                        InputEvent::Change => cx.notify(),
                        _ => {}
                    },
                ),
            );
        }
        self.export_dialog = Some(ExportDialog {
            key: key.into(),
            name: d.name,
            format: Format::Json,
            scope: if selected.is_empty() {
                Scope::All
            } else {
                Scope::Selected
            },
            first_n: first_in,
            cols: names.into_iter().map(|n| (n, true)).collect(),
            pretty: true,
            header: true,
            delimiter: b',',
            clipboard: false,
            path: path_in,
            selected,
            total: rows.len(),
            error: None,
            _subs: subs,
        });
        cx.notify();
    }

    pub(crate) fn export_op(
        &mut self,
        op: &str,
        arg: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let i: usize = arg.parse().unwrap_or(0);
        match op {
            "cancel" => {
                self.export_dialog = None;
                window_focus_back(self, w);
                cx.notify();
                return;
            }
            "go" => return self.run_export(w, cx),
            "browse" => return self.browse_export_folder(w, cx),
            _ => {}
        }
        let Some(d) = self.export_dialog.as_mut() else {
            return;
        };
        match op {
            "format" => {
                d.format = Format::parse(arg).unwrap_or(Format::Json);
                if d.format == Format::Parquet {
                    d.clipboard = false;
                }
            }
            "scope" => {
                d.scope = match arg {
                    "selected" => Scope::Selected,
                    "first" => Scope::First,
                    _ => Scope::All,
                }
            }
            "pretty" => d.pretty = !d.pretty,
            "header" => d.header = !d.header,
            "delim" => d.delimiter = delim_from(arg),
            "dest" => d.clipboard = arg == "clipboard" && d.format != Format::Parquet,
            "col" => {
                if let Some(c) = d.cols.get_mut(i) {
                    c.1 = !c.1;
                }
            }
            "up" if i > 0 && i < d.cols.len() => d.cols.swap(i, i - 1),
            "down" if i + 1 < d.cols.len() => d.cols.swap(i, i + 1),
            _ => {}
        }
        // The path follows the chosen format until the user typed another extension.
        if op == "format" {
            let ext = ext_of(d.format);
            let cur = d.path.read(cx).value().to_string();
            if let Some((stem, e)) = cur.rsplit_once('.')
                && ["json", "csv", "parquet"].contains(&e)
            {
                let next = format!("{stem}.{ext}");
                d.path.update(cx, |t, cx| t.set_value(&next, cx));
            }
        }
        cx.notify();
    }

    /// Browse…: the native folder dialog; the file field becomes `<folder>/<name>.export.<ext>`.
    pub(crate) fn browse_export_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.export_dialog.as_ref() else {
            return;
        };
        let file = format!("{}.export.{}", d.name, ext_of(d.format));
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Export here".into()),
        });
        cx.spawn_in(window, async move |this, cx| match rx.await {
            Ok(Ok(Some(paths))) => {
                if let Some(dir) = paths.into_iter().next() {
                    this.update(cx, |this, cx| this.set_export_folder(&dir, &file, cx))
                        .ok();
                }
            }
            // Cancelled: the field keeps what it had.
            Ok(Ok(None)) => {}
            _ => {
                this.update(cx, |this, cx| {
                    this.export_error("No folder dialog is available here; type the path.", cx)
                })
                .ok();
            }
        })
        .detach();
    }

    pub(crate) fn set_export_folder(
        &mut self,
        dir: &std::path::Path,
        file: &str,
        cx: &mut Context<Self>,
    ) {
        if let Some(d) = self.export_dialog.as_mut() {
            let p = dir.join(file).to_string_lossy().into_owned();
            d.path.update(cx, |t, cx| t.set_value(&p, cx));
            d.error = None;
        }
        cx.notify();
    }

    fn export_rows(&self, cx: &gpui::App) -> Option<Vec<Value>> {
        let d = self.export_dialog.as_ref()?;
        let ds = self.ws.find_dataset(&d.key)?;
        let all = match self.engine()?.dataset_rows(&ds.server_id, &ds.name).ok()? {
            Value::Array(a) => a,
            other => vec![other],
        };
        let n = d.first_n.read(cx).value().trim().parse().unwrap_or(0);
        Some(d.scoped(&all, n))
    }

    fn run_export(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let Some(rows) = self.export_rows(cx) else {
            return;
        };
        let Some(d) = self.export_dialog.as_ref() else {
            return;
        };
        let opts = d.options();
        if d.cols.iter().all(|(_, on)| !on) {
            return self.export_error("Tick at least one column.", cx);
        }
        let bytes = match export_with(&rows, &opts) {
            Ok(b) => b,
            Err(m) => return self.export_error(&m, cx),
        };
        let n = rows.len();
        if d.clipboard {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            self.notify(format!("Copied {n} rows to the clipboard"), cx);
        } else {
            let p = d.path.read(cx).value().trim().to_string();
            if p.is_empty() {
                return self.export_error("Choose a destination folder with Browse….", cx);
            }
            let path = std::path::PathBuf::from(&p);
            let path = if path.is_absolute() {
                path
            } else {
                self.ws.root.join(path)
            };
            if let Err(e) = std::fs::write(&path, bytes) {
                return self.export_error(&format!("Cannot write {}: {e}", path.display()), cx);
            }
            self.notify(format!("Exported {n} rows to {}", path.display()), cx);
        }
        self.export_dialog = None;
        window_focus_back(self, w);
        cx.notify();
    }

    fn export_error(&mut self, m: &str, cx: &mut Context<Self>) {
        if let Some(d) = self.export_dialog.as_mut() {
            d.error = Some(m.to_string());
        }
        cx.notify();
    }

    pub(crate) fn render_export_dialog(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let d = self.export_dialog.as_ref().expect("dialog is open");
        let rows = self.export_rows(cx).unwrap_or_default();
        let opts = d.options();
        let est = estimate_export_size(&rows, &opts);
        let label = |s: &'static str| {
            div()
                .w(px(90.))
                .text_size(px(12.))
                .text_color(t.text_muted)
                .child(s)
        };
        let line = |l: AnyElement, c: AnyElement| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .child(l)
                .child(c)
        };
        let fmt = pick(
            self,
            cx,
            "export-format",
            &["JSON", "CSV", "Parquet"],
            fmt_label(d.format),
            120.,
            std::rc::Rc::new(|this, ev, cx| {
                if let ComboEvent::Change(v) = ev
                    && let Some(d) = this.export_dialog.as_mut()
                {
                    d.format = Format::parse(v).unwrap_or(Format::Json);
                    if d.format == Format::Parquet {
                        d.clipboard = false;
                    }
                    let ext = ext_of(d.format);
                    let cur = d.path.read(cx).value().to_string();
                    if let Some((stem, e)) = cur.rsplit_once('.')
                        && ["json", "csv", "parquet"].contains(&e)
                    {
                        let next = format!("{stem}.{ext}");
                        d.path.update(cx, |t, cx| t.set_value(&next, cx));
                    }
                }
            }),
        );
        let scope_label = match d.scope {
            Scope::All => "All rows",
            Scope::Selected => "Selected rows",
            Scope::First => "First N rows",
        };
        let scope = pick(
            self,
            cx,
            "export-scope",
            &["All rows", "Selected rows", "First N rows"],
            scope_label,
            140.,
            std::rc::Rc::new(|this, ev, _| {
                if let ComboEvent::Change(v) = ev
                    && let Some(d) = this.export_dialog.as_mut()
                {
                    d.scope = match v.as_ref() {
                        "Selected rows" => Scope::Selected,
                        "First N rows" => Scope::First,
                        _ => Scope::All,
                    };
                }
            }),
        );
        let dest = pick(
            self,
            cx,
            "export-dest",
            &["File", "Clipboard"],
            if d.clipboard { "Clipboard" } else { "File" },
            120.,
            std::rc::Rc::new(|this, ev, _| {
                if let ComboEvent::Change(v) = ev
                    && let Some(d) = this.export_dialog.as_mut()
                {
                    d.clipboard = v.as_ref() == "Clipboard" && d.format != Format::Parquet;
                }
            }),
        );
        let delim = pick(
            self,
            cx,
            "export-delim",
            &["comma", "semicolon", "tab", "pipe"],
            delim_label(d.delimiter),
            110.,
            std::rc::Rc::new(|this, ev, _| {
                if let ComboEvent::Change(v) = ev
                    && let Some(d) = this.export_dialog.as_mut()
                {
                    d.delimiter = delim_from(v);
                }
            }),
        );
        let toggle = |id: &str, on: bool, text: &str, op: &'static str| {
            let b = crate::ui::checkbox_row(t, eid(id.to_string()), on, text.to_string())
                .on_click(cx.listener(move |this, _, w, cx| this.export_op(op, "", w, cx)));
            crate::dev_id!(
                b,
                id.to_string(),
                Button,
                text.to_string(),
                Press(format!("export-{op}"))
            )
        };
        let cols: Vec<AnyElement> = d
            .cols
            .iter()
            .enumerate()
            .map(|(i, (n, on))| {
                let tid = format!("export-col-{i}");
                let tick = crate::ui::checkbox_row(t, eid(tid.clone()), *on, n.clone()).on_click(
                    cx.listener(move |this, _, w, cx| this.export_op("col", &i.to_string(), w, cx)),
                );
                let mv = |text: &'static str, op: &'static str| {
                    let id = format!("export-{op}-{i}");
                    let b =
                        btn(t, id.clone(), text.into()).on_click(cx.listener(
                            move |this, _, w, cx| this.export_op(op, &i.to_string(), w, cx),
                        ));
                    crate::dev_id!(
                        b,
                        id,
                        Button,
                        text.to_string(),
                        Press(format!("export-{op}:{i}"))
                    )
                };
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .child(div().w(px(200.)).child(crate::dev_id!(
                        tick,
                        tid,
                        Button,
                        n.clone(),
                        Press(format!("export-col:{i}"))
                    )))
                    .child(mv("↑", "up"))
                    .child(mv("↓", "down"))
                    .into_any_element()
            })
            .collect();
        let path_el = crate::dev_id!(
            div().flex_1().min_w(px(0.)).child(d.path.clone()),
            "export-path",
            Input,
            "Destination file",
            None
        );
        let browse = crate::dev_id!(
            btn(t, "export-browse".into(), "Browse…".into())
                .flex_none()
                .on_click(cx.listener(|this, _, w, cx| this.browse_export_folder(w, cx))),
            "export-browse",
            Button,
            "Browse…",
            Press("export-browse".into())
        );
        let first_el = crate::dev_id!(
            div().w(px(70.)).child(d.first_n.clone()),
            "export-first-n",
            Input,
            "First N rows",
            None
        );
        let go = btn(t, "export-go".into(), "Export".into())
            .h(px(26.))
            .px(px(12.))
            .bg(t.element_selected)
            .on_click(cx.listener(|this, _, w, cx| this.export_op("go", "", w, cx)));
        let cancel = btn(t, "export-cancel".into(), "Cancel".into())
            .h(px(26.))
            .px(px(12.))
            .on_click(cx.listener(|this, _, w, cx| this.export_op("cancel", "", w, cx)));
        crate::dev_id_z!(
            div()
                .id("export-backdrop")
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .items_center()
                .pt(px(50.))
                .bg(t.background.opacity(0.6))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .id("export-dialog")
                        .key_context("Dialog")
                        .track_focus(&self.export_dialog_focus)
                        .on_key_down(cx.listener(|this, ev: &KeyDownEvent, w, cx| {
                            match ev.keystroke.key.as_str() {
                                "escape" => this.export_op("cancel", "", w, cx),
                                "enter" => this.export_op("go", "", w, cx),
                                _ => return,
                            }
                            cx.stop_propagation();
                        }))
                        .w(px(520.))
                        .max_h(px(620.))
                        .p(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(t.border)
                        .bg(t.elevated)
                        .shadow_lg()
                        .child(div().text_size(px(14.)).child(format!("Export {}", d.name)))
                        .child(line(label("Format").into_any_element(), fmt))
                        .child(
                            line(label("Rows").into_any_element(), scope)
                                .when(d.scope == Scope::First, |r| r.child(first_el))
                        )
                        .when(d.scope == Scope::Selected && d.selected.is_empty(), |c| {
                            c.child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(t.deleted)
                                    .child("No rows are selected."),
                            )
                        })
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(t.text_muted)
                                .child("Columns (order is the output order)")
                        )
                        .child(
                            div()
                                .id("export-cols")
                                .max_h(px(150.))
                                .overflow_y_scroll()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .children(cols),
                        )
                        .when(d.format == Format::Json, |c| c.child(toggle(
                            "export-pretty",
                            d.pretty,
                            "Pretty-print",
                            "pretty"
                        )))
                        .when(d.format == Format::Csv, |c| {
                            c.child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(toggle(
                                        "export-header",
                                        d.header,
                                        "Header row",
                                        "header",
                                    ))
                                    .child(label("Delimiter"))
                                    .child(delim),
                            )
                        })
                        .child(line(label("Destination").into_any_element(), dest))
                        .when(!d.clipboard, |c| c.child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .child(label("File"))
                                .child(path_el)
                                .child(browse)
                        ))
                        .child(crate::dev_id!(
                            div()
                                .id("export-summary")
                                .text_size(px(11.))
                                .text_color(t.text_muted)
                                .child(format!("{} rows · about {}", rows.len(), human_size(est))),
                            "export-summary",
                            Text,
                            format!("{} rows · about {}", rows.len(), human_size(est)),
                            None
                        ),)
                        .when_some(d.error.clone(), |c, e| c
                            .child(div().text_size(px(11.)).text_color(t.deleted).child(e)))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .justify_end()
                                .gap(px(8.))
                                .child(crate::dev_id!(
                                    cancel,
                                    "export-cancel",
                                    Button,
                                    "Cancel",
                                    Press("export-cancel".into())
                                ))
                                .child(crate::dev_id!(
                                    go,
                                    "export-go",
                                    Button,
                                    "Export",
                                    Press("export-go".into())
                                )),
                        ),
                ),
            2,
            "export-dialog",
            Modal,
            "Export dataset",
            None
        )
    }
}

fn window_focus_back(shell: &Shell, w: &mut Window) {
    w.focus(&shell.focus);
}

// ---- Seed data ---------------------------------------------------------------------------

pub struct SeedDialog {
    pub key: String,
    pub name: String,
    pub rows: Entity<TextInput>,
    pub fake_seed: Entity<TextInput>,
    pub replace: bool,
    pub keep: bool,
    pub cols: Vec<SeedCol>,
    /// Server of the Dataset (the `ref` Dataset choices).
    pub server_id: String,
    _subs: Vec<Subscription>,
}

/// One column of the Seed data dialog: its generator and the generator's options.
pub struct SeedCol {
    pub name: String,
    pub ty: String,
    /// `kind` or `fake:<kind>`.
    pub generator: String,
    /// One input per option of the kind ([`crate::gen_args::args_for`]), by option key.
    pub args: Vec<(String, Entity<TextInput>)>,
    /// Date options (`date` and `datetime` from/to), drawn as DatePickers.
    pub dates: Vec<(String, Entity<crate::date_picker::DatePicker>)>,
    /// The `ref` Dataset (drawn as a ComboBox, not an input).
    pub ref_dataset: String,
}

impl SeedCol {
    pub fn texts(&self, cx: &gpui::App) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = self
            .args
            .iter()
            .map(|(k, e)| (k.clone(), e.read(cx).value().to_string()))
            .collect();
        for (k, p) in &self.dates {
            let text = p
                .read(cx)
                .value()
                .map(|d| d.to_string())
                .unwrap_or_default();
            v.push((k.clone(), text));
        }
        v.push(("dataset".into(), self.ref_dataset.clone()));
        v
    }

    /// The column's generator with its options applied.
    pub fn spec(&self, cx: &gpui::App) -> Result<GeneratorSpec, String> {
        let kind = self.generator.split(':').next().unwrap_or("");
        if crate::gen_args::needs_toml(kind) {
            return Err(format!(
                "{}: `{kind}` nests generators; set it in the server file.",
                self.name
            ));
        }
        let spec = crate::dataset_edit::spec_from_text(&self.generator)
            .map_err(|m| format!("{}: {m}", self.name))?;
        crate::gen_args::apply_args(spec, &self.texts(cx), &self.name, &self.ty)
            .map_err(|m| format!("{}: {m}", self.name))
    }
}

impl SeedDialog {
    pub fn fields(&self, cx: &gpui::App) -> Result<Vec<GeneratorField>, String> {
        self.cols
            .iter()
            .map(|c| {
                c.spec(cx).map(|spec| GeneratorField {
                    name: c.name.clone(),
                    spec,
                })
            })
            .collect()
    }
}

/// The option widgets of one column.
struct ArgWidgets {
    inputs: Vec<(String, Entity<TextInput>)>,
    dates: Vec<(String, Entity<crate::date_picker::DatePicker>)>,
    ref_dataset: String,
}

/// Widgets for the options of `spec`'s kind, prefilled from `spec`; each edit redraws the
/// dialog. `col` keeps the DatePicker ids unique.
fn arg_inputs(
    spec: &GeneratorSpec,
    col: usize,
    subs: &mut Vec<Subscription>,
    cx: &mut Context<Shell>,
) -> ArgWidgets {
    use crate::date_picker::{DatePicker, DatePickerEvent};
    let texts = crate::gen_args::texts_of(spec);
    let mut out = vec![];
    let mut dates = vec![];
    let mut ref_dataset = String::new();
    for def in crate::gen_args::args_for(&spec.kind) {
        let text = texts
            .iter()
            .find(|(k, _)| k == def.key)
            .map(|(_, t)| t.clone())
            .unwrap_or_default();
        if def.kind == crate::gen_args::ArgKind::Dataset {
            ref_dataset = text;
            continue;
        }
        if def.kind == crate::gen_args::ArgKind::Date {
            let date = chrono::NaiveDate::parse_from_str(&text, "%Y-%m-%d").ok();
            let id = format!("seed-col-{col}-arg-{}", def.key);
            let p = cx.new(|cx| {
                DatePicker::new(id, cx)
                    .placeholder(def.placeholder)
                    .cleanable(true)
                    .date(date)
            });
            subs.push(
                cx.subscribe(&p, |_: &mut Shell, _, ev: &DatePickerEvent, cx| {
                    if matches!(ev, DatePickerEvent::Change(_)) {
                        cx.notify();
                    }
                }),
            );
            dates.push((def.key.to_string(), p));
            continue;
        }
        let e = small_input(cx, def.placeholder, &text);
        subs.push(cx.subscribe(&e, |_: &mut Shell, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                cx.notify();
            }
        }));
        out.push((def.key.to_string(), e));
    }
    ArgWidgets {
        inputs: out,
        dates,
        ref_dataset,
    }
}

impl Shell {
    /// Seed data… (replaces the Generate rows prompt).
    pub(crate) fn begin_generate_rows(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.active_dataset_key() else {
            return;
        };
        let Some(d) = self.ws.find_dataset(&key).cloned() else {
            return;
        };
        let cols_now = self.dataset_columns_now(&d, cx);
        if cols_now.is_empty() {
            return self.notify("Add a column first (Edit schema…).".into(), cx);
        }
        let mut subs = vec![];
        let cols = cols_now
            .into_iter()
            .enumerate()
            .map(|(i, (n, t))| {
                let spec = crate::dataset_edit::infer_generator(&n, &t, n == d.key);
                let w = arg_inputs(&spec, i, &mut subs, cx);
                SeedCol {
                    generator: spec_to_text(&spec),
                    name: n,
                    ty: t,
                    args: w.inputs,
                    dates: w.dates,
                    ref_dataset: w.ref_dataset,
                }
            })
            .collect();
        let rows = small_input(cx, "rows", "50");
        let seed = small_input(cx, "random", "");
        for input in [&rows, &seed] {
            subs.push(
                cx.subscribe_in(
                    input,
                    w,
                    |this: &mut Shell, _, ev: &InputEvent, w, cx| match ev {
                        InputEvent::PressEnter { .. } => this.seed_op("go", "", w, cx),
                        InputEvent::Escape => this.seed_op("cancel", "", w, cx),
                        InputEvent::Change => cx.notify(),
                        _ => {}
                    },
                ),
            );
        }
        rows.read(cx).focus(w);
        self.seed_dialog = Some(SeedDialog {
            key,
            name: d.name,
            rows,
            fake_seed: seed,
            replace: false,
            keep: false,
            cols,
            server_id: d.server_id.clone(),
            _subs: subs,
        });
        cx.notify();
    }

    /// A new generator for column `i`: its option inputs are rebuilt for the new kind.
    pub(crate) fn set_seed_generator(&mut self, i: usize, value: &str, cx: &mut Context<Self>) {
        let Ok(spec) = crate::dataset_edit::spec_from_text(value) else {
            return;
        };
        let Some(mut d) = self.seed_dialog.take() else {
            return;
        };
        let w = arg_inputs(&spec, i, &mut d._subs, cx);
        if let Some(c) = d.cols.get_mut(i) {
            c.generator = value.to_string();
            c.args = w.inputs;
            c.dates = w.dates;
            c.ref_dataset = w.ref_dataset;
        }
        self.seed_dialog = Some(d);
        cx.notify();
    }

    pub(crate) fn seed_op(&mut self, op: &str, arg: &str, w: &mut Window, cx: &mut Context<Self>) {
        match op {
            "cancel" => {
                self.seed_dialog = None;
                window_focus_back(self, w);
                cx.notify();
            }
            "go" => self.run_seed(w, cx),
            _ => {}
        }
        let _ = arg;
    }

    fn run_seed(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.seed_dialog.as_ref() else {
            return;
        };
        let n: u32 = match d.rows.read(cx).value().trim().parse() {
            Ok(n) if (1..=100_000).contains(&n) => n,
            _ => return self.notify("Rows is a number from 1 to 100000.".into(), cx),
        };
        let seed_text = d.fake_seed.read(cx).value().trim().to_string();
        let fake_seed = if seed_text.is_empty() {
            None
        } else {
            match seed_text.parse::<u64>() {
                Ok(s) => Some(s),
                Err(_) => return self.notify("Fake seed is a whole number.".into(), cx),
            }
        };
        let fields = match d.fields(cx) {
            Ok(f) => f,
            Err(m) => return self.notify(m, cx),
        };
        let (key, keep, replace) = (d.key.clone(), d.keep, d.replace);
        self.seed_dialog = None;
        window_focus_back(self, w);
        self.generate_rows_apply(&key, n, keep, replace, Some(fields), fake_seed, cx);
    }

    /// Five sample rows with the dialog's current choices, as JSON lines (or the error).
    pub(crate) fn seed_preview(&self, cx: &gpui::App) -> Result<Vec<Value>, String> {
        let Some(d) = self.seed_dialog.as_ref() else {
            return Ok(vec![]);
        };
        let fields = d.fields(cx)?;
        let seed = d.fake_seed.read(cx).value().trim().parse::<u64>().ok();
        // `ref` columns pick from the live rows of the other Dataset.
        let mut refs = fauxfield_engine::generate::Refs::new();
        if let Some(e) = self.engine() {
            for c in d.cols.iter().filter(|c| !c.ref_dataset.is_empty()) {
                if let Ok(Value::Array(a)) = e.dataset_rows(&d.server_id, &c.ref_dataset) {
                    refs.insert(c.ref_dataset.clone(), a);
                }
            }
        }
        let g = GeneratorSeed { rows: 5, fields };
        match fauxfield_engine::generate::generate(
            "preview",
            &g,
            seed.or(Some(1)),
            None,
            &refs,
            &self.ws.root,
        ) {
            Ok(Value::Array(a)) => Ok(a),
            Ok(_) => Ok(vec![]),
            Err(e) => Err(e.to_string()),
        }
    }

    pub(crate) fn render_seed_dialog(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let d = self.seed_dialog.as_ref().expect("dialog is open");
        let columns: Vec<String> = d.cols.iter().map(|c| c.name.clone()).collect();
        let preview_el = preview_table(t, &columns, self.seed_preview(cx));
        let label = |s: &'static str| {
            div()
                .w(px(110.))
                .text_size(px(12.))
                .text_color(t.text_muted)
                .child(s)
        };
        let mode = pick(
            self,
            cx,
            "seed-mode",
            &["Replace rows", "Append rows"],
            if d.replace {
                "Replace rows"
            } else {
                "Append rows"
            },
            130.,
            std::rc::Rc::new(|this, ev, _| {
                if let ComboEvent::Change(v) = ev
                    && let Some(d) = this.seed_dialog.as_mut()
                {
                    d.replace = v.as_ref() == "Replace rows";
                }
            }),
        );
        let keep = pick(
            self,
            cx,
            "seed-keep",
            &["Bake into the seed file", "Keep as a generator"],
            if d.keep {
                "Keep as a generator"
            } else {
                "Bake into the seed file"
            },
            190.,
            std::rc::Rc::new(|this, ev, _| {
                if let ComboEvent::Change(v) = ev
                    && let Some(d) = this.seed_dialog.as_mut()
                {
                    d.keep = v.as_ref() == "Keep as a generator";
                }
            }),
        );
        let datasets: Vec<String> = self
            .ws
            .datasets
            .iter()
            .filter(|x| x.server_id == d.server_id && x.name != d.name)
            .map(|x| x.name.clone())
            .collect();
        let mut col_rows: Vec<AnyElement> = vec![];
        for (i, c) in d.cols.iter().enumerate() {
            let g = c.generator.clone();
            let combo = self.combo_field(
                cx,
                ComboSite {
                    key: format!("seed-col-{i}-gen"),
                    items: crate::schema_dialog::generator_items()
                        .into_iter()
                        .filter(|it| !it.value.is_empty())
                        .collect(),
                    value: Some(g.clone().into()),
                    fallback: Some(g.into()),
                    color: None,
                    tip: None,
                    width: ComboWidth::Fixed(210.),
                    look: ComboLook::Field,
                    size: ComboSize::Small,
                    on_event: std::rc::Rc::new(move |this, ev, cx| {
                        if let ComboEvent::Change(v) = ev {
                            this.set_seed_generator(i, v, cx);
                        }
                    }),
                },
            );
            let kind = c.generator.split(':').next().unwrap_or("").to_string();
            let mut opts: Vec<AnyElement> = vec![];
            for def in crate::gen_args::args_for(&kind) {
                let lab = div()
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child(def.label);
                if def.kind == crate::gen_args::ArgKind::Dataset {
                    let cur = c.ref_dataset.clone();
                    let el = self.combo_field(
                        cx,
                        ComboSite {
                            key: format!("seed-col-{i}-ref"),
                            items: datasets
                                .iter()
                                .map(|n| ComboItem::plain(n.clone()))
                                .collect(),
                            value: Some(cur.clone().into()),
                            fallback: Some(
                                if cur.is_empty() {
                                    "pick a Dataset".to_string()
                                } else {
                                    cur
                                }
                                .into(),
                            ),
                            color: None,
                            tip: None,
                            width: ComboWidth::Fixed(150.),
                            look: ComboLook::Field,
                            size: ComboSize::Small,
                            on_event: std::rc::Rc::new(move |this, ev, _| {
                                if let ComboEvent::Change(v) = ev
                                    && let Some(c) =
                                        this.seed_dialog.as_mut().and_then(|d| d.cols.get_mut(i))
                                {
                                    c.ref_dataset = v.to_string();
                                }
                            }),
                        },
                    );
                    opts.push(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .child(lab)
                            .child(el)
                            .into_any_element(),
                    );
                } else if let Some((_, p)) = c.dates.iter().find(|(k, _)| k == def.key) {
                    opts.push(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .child(lab)
                            .child(div().w(px(140.)).child(p.clone()))
                            .into_any_element(),
                    );
                } else if let Some((_, e)) = c.args.iter().find(|(k, _)| k == def.key) {
                    let wide = matches!(
                        def.kind,
                        crate::gen_args::ArgKind::List
                            | crate::gen_args::ArgKind::Numbers
                            | crate::gen_args::ArgKind::Text
                            | crate::gen_args::ArgKind::Json
                    );
                    let id = format!("seed-col-{i}-arg-{}", def.key);
                    let _ = &id;
                    opts.push(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .child(lab)
                            .child(crate::dev_id!(
                                div().w(px(if wide { 220. } else { 70. })).child(e.clone()),
                                id,
                                Input,
                                def.label,
                                None
                            ))
                            .into_any_element(),
                    );
                }
            }
            let err = c.spec(cx).err();
            col_rows.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .child(div().w(px(130.)).truncate().child(c.name.clone()))
                            .child(
                                div()
                                    .w(px(60.))
                                    .text_color(t.text_muted)
                                    .child(c.ty.clone()),
                            )
                            .child(combo),
                    )
                    .when(!opts.is_empty(), |d| {
                        d.child(
                            div()
                                .pl(px(198.))
                                .flex()
                                .flex_row()
                                .flex_wrap()
                                .items_center()
                                .gap(px(10.))
                                .children(opts),
                        )
                    })
                    .when_some(err, |d, e| {
                        d.child(
                            div()
                                .pl(px(198.))
                                .text_size(px(11.))
                                .text_color(t.deleted)
                                .child(e),
                        )
                    })
                    .into_any_element(),
            );
        }
        let rows_el = crate::dev_id!(
            div().w(px(90.)).child(d.rows.clone()),
            "seed-rows",
            Input,
            "Rows",
            None
        );
        let seed_el = crate::dev_id!(
            div().w(px(110.)).child(d.fake_seed.clone()),
            "seed-fake-seed",
            Input,
            "Fake seed",
            None
        );
        let go = btn(t, "seed-go".into(), "Generate".into())
            .h(px(26.))
            .px(px(12.))
            .bg(t.element_selected)
            .on_click(cx.listener(|this, _, w, cx| this.seed_op("go", "", w, cx)));
        let cancel = btn(t, "seed-cancel".into(), "Cancel".into())
            .h(px(26.))
            .px(px(12.))
            .on_click(cx.listener(|this, _, w, cx| this.seed_op("cancel", "", w, cx)));
        crate::dev_id_z!(
            div()
                .id("seed-backdrop")
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .items_center()
                .pt(px(50.))
                .bg(t.background.opacity(0.6))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .id("seed-dialog")
                        .key_context("Dialog")
                        .track_focus(&self.seed_dialog_focus)
                        .on_key_down(cx.listener(|this, ev: &KeyDownEvent, w, cx| {
                            match ev.keystroke.key.as_str() {
                                "escape" => this.seed_op("cancel", "", w, cx),
                                "enter" => this.seed_op("go", "", w, cx),
                                _ => return,
                            }
                            cx.stop_propagation();
                        }))
                        .w(px(760.))
                        .max_h(px(680.))
                        .p(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(t.border)
                        .bg(t.elevated)
                        .shadow_lg()
                        .child(
                            div()
                                .text_size(px(14.))
                                .child(format!("Seed data for {}", d.name))
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .child(label("Rows"))
                                .child(rows_el)
                                .child(label("Fake seed"))
                                .child(seed_el),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .child(label("Mode"))
                                .child(mode)
                                .child(keep)
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(t.text_muted)
                                .child("Generator per column")
                        )
                        .child(
                            div()
                                .id("seed-cols")
                                .max_h(px(300.))
                                .overflow_y_scroll()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .children(col_rows),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(t.text_muted)
                                .child("Preview (5 rows)")
                        )
                        .child(preview_el)
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .justify_end()
                                .gap(px(8.))
                                .child(crate::dev_id!(
                                    cancel,
                                    "seed-cancel",
                                    Button,
                                    "Cancel",
                                    Press("seed-cancel".into())
                                ))
                                .child(crate::dev_id!(
                                    go,
                                    "seed-go",
                                    Button,
                                    "Generate",
                                    Press("seed-go".into())
                                )),
                        ),
                ),
            2,
            "seed-dialog",
            Modal,
            "Seed data",
            None
        )
    }
}

#[cfg(feature = "dev-mcp")]
impl Shell {
    /// `ui.state` view of the Export modal.
    pub(crate) fn export_dialog_state(
        &self,
        cx: &gpui::App,
    ) -> Option<crate::dev_mcp::state::ExportDialogState> {
        let d = self.export_dialog.as_ref()?;
        let rows = self.export_rows(cx).unwrap_or_default();
        Some(crate::dev_mcp::state::ExportDialogState {
            format: ext_of(d.format),
            scope: match d.scope {
                Scope::All => "all",
                Scope::Selected => "selected",
                Scope::First => "first",
            },
            columns: d.cols.clone(),
            rows: rows.len(),
            estimate_bytes: estimate_export_size(&rows, &d.options()),
            clipboard: d.clipboard,
            path: d.path.read(cx).value().to_string(),
            error: d.error.clone(),
        })
    }

    /// `ui.state` view of the Seed data modal.
    pub(crate) fn seed_dialog_state(
        &self,
        cx: &gpui::App,
    ) -> Option<crate::dev_mcp::state::SeedDialogState> {
        let d = self.seed_dialog.as_ref()?;
        Some(crate::dev_mcp::state::SeedDialogState {
            rows: d.rows.read(cx).value().to_string(),
            fake_seed: d.fake_seed.read(cx).value().to_string(),
            replace: d.replace,
            keep: d.keep,
            columns: d
                .cols
                .iter()
                .map(|c| (c.name.clone(), c.ty.clone(), c.generator.clone()))
                .collect(),
            errors: d.cols.iter().map(|c| c.spec(cx).err()).collect(),
        })
    }
}

/// The Seed data preview: the generated rows as a small static table (header, striped rows,
/// equal-width columns, truncated cells), or the reason there is nothing to show.
fn preview_table(t: &Theme, columns: &[String], rows: Result<Vec<Value>, String>) -> AnyElement {
    let rows = match rows {
        Ok(r) => r,
        Err(m) => {
            return crate::dev_id!(
                div()
                    .id("seed-preview")
                    .p(px(8.))
                    .rounded(px(4.))
                    .bg(t.surface)
                    .text_size(px(11.))
                    .text_color(t.deleted)
                    .child(m.clone()),
                "seed-preview",
                Text,
                m,
                None
            )
            .into_any_element();
        }
    };
    let cell = |text: String, muted: bool| {
        div()
            .flex_1()
            .min_w(px(0.))
            .px(px(8.))
            .h(px(24.))
            .flex()
            .items_center()
            .border_r_1()
            .border_color(t.border_variant)
            .child(
                div()
                    .min_w(px(0.))
                    .truncate()
                    .text_color(if muted { t.text_muted } else { t.text })
                    .child(text),
            )
    };
    let header = div()
        .flex()
        .flex_row()
        .bg(t.panel)
        .border_b_1()
        .border_color(t.border)
        .children(columns.iter().map(|c| cell(c.clone(), true)));
    let body = rows.iter().enumerate().map(|(i, r)| {
        div()
            .flex()
            .flex_row()
            .when(i % 2 == 1, |d| d.bg(t.surface))
            .when(i + 1 < rows.len(), |d| {
                d.border_b_1().border_color(t.border_variant)
            })
            .children(columns.iter().map(|c| {
                let v = r.get(c).unwrap_or(&Value::Null);
                let text = match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                cell(text, v.is_null())
            }))
    });
    let label = format!("{} preview rows", rows.len());
    let _ = &label;
    crate::dev_id!(
        div()
            .id("seed-preview")
            .rounded(px(4.))
            .border_1()
            .border_color(t.border_variant)
            .bg(t.editor)
            .overflow_hidden()
            .font_family(crate::assets::BUFFER_FONT)
            .text_size(px(11.))
            .flex()
            .flex_col()
            .child(header)
            .children(body),
        "seed-preview",
        Table,
        label,
        None
    )
    .into_any_element()
}
