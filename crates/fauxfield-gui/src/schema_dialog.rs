//! The schema designer modal (M10.07): New Dataset and Edit schema. Layout and wiring only;
//! the draft, its validation and the TOML preview live in [`crate::schema_draft`].

use gpui::{
    Context, Entity, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::*, px,
};

use crate::combo_box::{ComboEvent, ComboItem, ComboLook, ComboSite, ComboSize, ComboWidth};
use crate::schema_draft::{
    ColDraft, Issue, STARTERS, SchemaDraft, SourceKind, Where, preview_toml, starter, to_columns,
    validate_schema_draft,
};
use crate::shell::Shell;
use crate::text_input::{InputEvent, InputSize, TextInput};
use crate::theme::Theme;
use crate::ui::{eid, toolbar_button};

pub(crate) struct ColRow {
    pub uid: usize,
    pub name: Entity<TextInput>,
    pub default: Entity<TextInput>,
    pub desc: Entity<TextInput>,
    pub ty: String,
    pub generator: String,
    pub primary_key: bool,
    pub unique: bool,
    pub required: bool,
}

pub struct SchemaDialog {
    /// `server:name` of the Dataset being edited; `None` for a new one.
    pub editing: Option<String>,
    pub name: Entity<TextInput>,
    pub description: Entity<TextInput>,
    pub rows: Entity<TextInput>,
    pub server: String,
    pub source: SourceKind,
    pub create_crud: bool,
    pub submitted: bool,
    pub(crate) cols: Vec<ColRow>,
    next_uid: usize,
    /// The input that has focus, for Tab order and the clipboard.
    pub(crate) focused: Option<Entity<TextInput>>,
    _subs: Vec<Subscription>,
}

fn small_input(cx: &mut Context<Shell>, ph: &str, text: &str) -> Entity<TextInput> {
    cx.new(|cx| {
        TextInput::new(cx)
            .size(InputSize::Small)
            .placeholder(ph.to_string())
            .text(text)
    })
}

/// Generator choices: inferred, every kind but `fake`, and `fake:<kind>`.
pub(crate) fn generator_items() -> Vec<ComboItem> {
    let mut v = vec![ComboItem::new("", "(infer)")];
    for (k, _) in fauxfield_model::GENERATORS
        .iter()
        .filter(|(k, _)| *k != "fake")
    {
        v.push(ComboItem::plain(*k));
    }
    for (k, _) in fauxfield_engine::template::FAKE_KINDS {
        v.push(ComboItem::plain(format!("fake:{k}")));
    }
    v
}

impl Shell {
    // ---- open / close ------------------------------------------------------------------

    pub(crate) fn begin_new_dataset(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self.store.is_none() {
            return self.notify("Open a workspace first.".into(), cx);
        }
        let Some(server) = self
            .current_server_id()
            .or_else(|| self.ws.servers.first().map(|s| s.id.clone()))
        else {
            return self.notify("Create a server first.".into(), cx);
        };
        self.open_schema_dialog(None, server, "", "", starter("id + name"), cx, w);
    }

    /// Edit schema… for the active Dataset.
    pub(crate) fn begin_edit_schema(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.active_dataset_key() else {
            return;
        };
        let Some(d) = self.ws.find_dataset(&key).cloned() else {
            return;
        };
        let mut cols: Vec<ColDraft> = d.column_defs.iter().map(ColDraft::from_column).collect();
        if cols.is_empty() {
            cols = self
                .dataset_columns_now(&d, cx)
                .into_iter()
                .map(|(n, t): (String, String)| ColDraft::new(&n, &t))
                .collect();
            if let Some(c) = cols.iter_mut().find(|c| c.name == d.key) {
                c.primary_key = true;
            }
        } else if !cols.iter().any(|c| c.primary_key)
            && let Some(c) = cols.iter_mut().find(|c| c.name == d.key)
        {
            c.primary_key = true;
        }
        self.open_schema_dialog(Some(key), d.server_id, &d.name, &d.description, cols, cx, w);
        if let Some(sd) = self.schema_dialog.as_mut() {
            sd.source = SourceKind::from_label(match d.source.as_str() {
                "template" => SourceKind::Template.label(),
                "generator" => SourceKind::Generator.label(),
                _ => SourceKind::Static.label(),
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn open_schema_dialog(
        &mut self,
        editing: Option<String>,
        server: String,
        name: &str,
        description: &str,
        cols: Vec<ColDraft>,
        cx: &mut Context<Self>,
        window: &mut Window,
    ) {
        if self.picker.is_some() {
            self.close_picker(true, window, cx);
        }
        let name_in = cx.new(|cx| {
            TextInput::new(cx)
                .placeholder("e.g. products")
                .text_selected(name)
        });
        let desc_in = small_input(cx, "optional", description);
        let rows_in = small_input(cx, "10", "10");
        let mut sd = SchemaDialog {
            editing,
            name: name_in.clone(),
            description: desc_in,
            rows: rows_in,
            server,
            source: SourceKind::Static,
            create_crud: false,
            submitted: false,
            cols: vec![],
            next_uid: 0,
            focused: None,
            _subs: vec![],
        };
        for c in &cols {
            let row = self.new_col_row(&mut sd, c, window, cx);
            sd.cols.push(row);
        }
        let singles = [sd.name.clone(), sd.description.clone(), sd.rows.clone()];
        for input in singles {
            let sub = self.sub_schema_input(&input, window, cx);
            sd._subs.push(sub);
        }
        name_in.read(cx).focus(window);
        self.schema_dialog = Some(sd);
        cx.notify();
    }

    fn sub_schema_input(
        &self,
        input: &Entity<TextInput>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        let me = input.clone();
        cx.subscribe_in(
            input,
            window,
            move |this: &mut Shell, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::Tab { reverse } => {
                    this.schema_move_focus(if *reverse { -1 } else { 1 }, window, cx)
                }
                InputEvent::PressEnter { .. } => this.submit_schema_dialog(window, cx),
                InputEvent::Escape => this.close_schema_dialog(window, cx),
                InputEvent::Focus => {
                    if let Some(d) = this.schema_dialog.as_mut() {
                        d.focused = Some(me.clone());
                    }
                }
                InputEvent::Change => cx.notify(),
                _ => {}
            },
        )
    }

    fn new_col_row(
        &mut self,
        sd: &mut SchemaDialog,
        c: &ColDraft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ColRow {
        let uid = sd.next_uid;
        sd.next_uid += 1;
        let row = ColRow {
            uid,
            name: small_input(cx, "column", &c.name),
            default: small_input(cx, "default", &c.default),
            desc: small_input(cx, "description", &c.description),
            ty: c.ty.clone(),
            generator: c.generator.clone(),
            primary_key: c.primary_key,
            unique: c.unique,
            required: c.required,
        };
        for input in [&row.name, &row.default, &row.desc] {
            let sub = self.sub_schema_input(input, window, cx);
            sd._subs.push(sub);
        }
        row
    }

    pub(crate) fn close_schema_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.schema_dialog.take().is_some() {
            window.focus(&self.focus);
            cx.notify();
        }
    }

    // ---- reading the draft ---------------------------------------------------------------

    pub(crate) fn schema_draft(&self, cx: &gpui::App) -> Option<SchemaDraft> {
        let d = self.schema_dialog.as_ref()?;
        Some(SchemaDraft {
            name: d.name.read(cx).value().to_string(),
            server: d.server.clone(),
            description: d.description.read(cx).value().to_string(),
            source: d.source,
            rows: d.rows.read(cx).value().to_string(),
            create_crud: d.create_crud,
            columns: d
                .cols
                .iter()
                .map(|r| ColDraft {
                    name: r.name.read(cx).value().to_string(),
                    ty: r.ty.clone(),
                    primary_key: r.primary_key,
                    unique: r.unique,
                    required: r.required,
                    default: r.default.read(cx).value().to_string(),
                    generator: r.generator.clone(),
                    description: r.desc.read(cx).value().to_string(),
                })
                .collect(),
        })
    }

    pub(crate) fn schema_issues(&self, cx: &gpui::App) -> Vec<Issue> {
        let (Some(draft), Some(d)) = (self.schema_draft(cx), self.schema_dialog.as_ref()) else {
            return vec![];
        };
        let existing: Vec<String> = self
            .ws
            .datasets
            .iter()
            .filter(|x| x.server_id == draft.server)
            .map(|x| x.name.clone())
            .collect();
        validate_schema_draft(&draft, &existing, d.editing.is_some())
    }

    // ---- operations (clicks and dev presses share these) ---------------------------------

    pub(crate) fn schema_op(
        &mut self,
        op: &str,
        arg: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let i: usize = arg.parse().unwrap_or(0);
        match op {
            "create" => return self.submit_schema_dialog(window, cx),
            "cancel" => return self.close_schema_dialog(window, cx),
            "crud" => {
                if let Some(d) = self.schema_dialog.as_mut() {
                    d.create_crud = !d.create_crud;
                }
            }
            "add" => {
                let Some(mut sd) = self.schema_dialog.take() else {
                    return;
                };
                let n = sd.cols.len() + 1;
                let row = self.new_col_row(
                    &mut sd,
                    &ColDraft::new(&format!("column{n}"), "string"),
                    window,
                    cx,
                );
                row.name.update(cx, |t, cx| {
                    t.focus(window);
                    t.select_all(cx);
                });
                sd.cols.push(row);
                self.schema_dialog = Some(sd);
            }
            "starter" => {
                let Some(mut sd) = self.schema_dialog.take() else {
                    return;
                };
                sd.cols.clear();
                for c in starter(arg) {
                    let row = self.new_col_row(&mut sd, &c, window, cx);
                    sd.cols.push(row);
                }
                self.schema_dialog = Some(sd);
            }
            _ => {
                let Some(d) = self.schema_dialog.as_mut() else {
                    return;
                };
                if i >= d.cols.len() {
                    return;
                }
                match op {
                    "up" if i > 0 => d.cols.swap(i, i - 1),
                    "down" if i + 1 < d.cols.len() => d.cols.swap(i, i + 1),
                    "del" => {
                        d.cols.remove(i);
                    }
                    "pk" => {
                        let on = !d.cols[i].primary_key;
                        for c in d.cols.iter_mut() {
                            c.primary_key = false;
                        }
                        d.cols[i].primary_key = on;
                    }
                    "unique" => d.cols[i].unique = !d.cols[i].unique,
                    "required" => d.cols[i].required = !d.cols[i].required,
                    "dup" => {
                        let (draft, cols) = (self.schema_draft(cx), i);
                        let Some(mut c) = draft.and_then(|dr| dr.columns.get(cols).cloned()) else {
                            return;
                        };
                        c.name = format!("{}_copy", c.name.trim());
                        c.primary_key = false;
                        let Some(mut sd) = self.schema_dialog.take() else {
                            return;
                        };
                        let row = self.new_col_row(&mut sd, &c, window, cx);
                        sd.cols.insert(i + 1, row);
                        self.schema_dialog = Some(sd);
                    }
                    _ => {}
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn submit_schema_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(d) = self.schema_dialog.as_mut() {
            d.submitted = true;
        }
        let issues = self.schema_issues(cx);
        if !issues.is_empty() {
            cx.notify();
            return;
        }
        let (Some(draft), Some(d)) = (self.schema_draft(cx), self.schema_dialog.as_ref()) else {
            return;
        };
        let editing = d.editing.clone();
        let was_edit = editing.is_some();
        if let Some(key) = editing {
            self.apply_schema_edit(&key, &draft, cx);
        } else {
            self.create_dataset_from_draft(&draft, cx);
        }
        self.close_schema_dialog(window, cx);
        if draft.create_crud && !was_edit {
            self.begin_crud_dialog(&draft.server, draft.name.trim(), window, cx);
        }
    }

    fn apply_schema_edit(&mut self, key: &str, draft: &SchemaDraft, cx: &mut Context<Self>) {
        let Some(d) = self.ws.find_dataset(key).cloned() else {
            return;
        };
        let cols = to_columns(draft);
        let pk = draft
            .columns
            .iter()
            .find(|c| c.primary_key)
            .map(|c| c.name.trim().to_string());
        let desc = Some(draft.description.trim().to_string()).filter(|s| !s.is_empty());
        let (sid, name) = (d.server_id.clone(), d.name.clone());
        let ok = self.store_op(cx, |s| {
            s.set_dataset_columns(&sid, &name, &cols)?;
            if let Some(pk) = &pk {
                s.set_dataset_field(&sid, &name, "key", Some(pk))?;
            }
            s.set_dataset_field(&sid, &name, "description", desc.as_deref())
        });
        if ok.is_some() {
            self.ds_gen += 1;
            self.sync_running();
            self.notify(format!("Updated schema of {name}"), cx);
        }
    }

    fn create_dataset_from_draft(&mut self, draft: &SchemaDraft, cx: &mut Context<Self>) {
        let (sid, name) = (draft.server.clone(), draft.name.trim().to_string());
        let cols = to_columns(draft);
        let pk = draft
            .columns
            .iter()
            .find(|c| c.primary_key)
            .map(|c| c.name.trim().to_string());
        let desc = Some(draft.description.trim().to_string()).filter(|s| !s.is_empty());
        let source = draft.source;
        let rows: usize = draft.rows.trim().parse().unwrap_or(10);
        let fields: Vec<fauxfield_model::GeneratorField> = draft
            .columns
            .iter()
            .map(|c| fauxfield_model::GeneratorField {
                name: c.name.trim().into(),
                spec: if c.generator.trim().is_empty() {
                    crate::dataset_edit::infer_generator(c.name.trim(), &c.ty, c.primary_key)
                } else {
                    crate::dataset_edit::spec_from_text(c.generator.trim())
                        .unwrap_or_else(|_| fauxfield_model::GeneratorSpec::of("uuid"))
                },
            })
            .collect();
        let ok = self.store_op(cx, |s| {
            s.create_dataset_with_columns(&sid, &name, pk.as_deref(), None, None, &cols)?;
            match source {
                SourceKind::Static => s.write_dataset_seed(&sid, &name, "[]")?,
                SourceKind::Template => {
                    s.upsert_dataset(
                        &sid,
                        &fauxfield_store::DatasetSpec {
                            name: name.clone(),
                            seed_template: Some("[]".into()),
                            ..Default::default()
                        },
                    )?;
                }
                SourceKind::Generator => s.set_dataset_generator(
                    &sid,
                    &name,
                    Some(&fauxfield_model::GeneratorSeed {
                        rows: rows as u32,
                        fields,
                    }),
                )?,
            }
            if let Some(d) = &desc {
                s.set_dataset_field(&sid, &name, "description", Some(d))?;
            }
            Ok(())
        });
        if ok.is_some() {
            self.sidebar_tab = crate::tree::SidebarTab::Datasets;
            self.layout.sidebar_tab = self.sidebar_tab.id().into();
            self.open_tab(
                &format!("{sid}:{name}"),
                crate::tabs::TabKind::Dataset,
                &name,
                true,
            );
            self.ds_gen += 1;
            self.sync_running();
            self.notify(format!("Created dataset {name}"), cx);
        }
    }

    fn schema_inputs(&self) -> Vec<Entity<TextInput>> {
        let Some(d) = self.schema_dialog.as_ref() else {
            return vec![];
        };
        let mut v = vec![d.name.clone(), d.description.clone()];
        if d.source == SourceKind::Generator {
            v.push(d.rows.clone());
        }
        for r in &d.cols {
            v.extend([r.name.clone(), r.default.clone(), r.desc.clone()]);
        }
        v
    }

    fn schema_move_focus(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let all = self.schema_inputs();
        if all.is_empty() {
            return;
        }
        let cur = self
            .schema_dialog
            .as_ref()
            .and_then(|d| d.focused.as_ref())
            .and_then(|f| all.iter().position(|x| x == f))
            .unwrap_or(0) as isize;
        let next = all[(cur + delta).rem_euclid(all.len() as isize) as usize].clone();
        next.update(cx, |t, cx| {
            t.focus(window);
            t.select_all(cx);
        });
    }

    // ---- render --------------------------------------------------------------------------

    pub(crate) fn render_schema_dialog(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let d = self.schema_dialog.as_ref().expect("dialog is open");
        let issues = self.schema_issues(cx);
        let draft = self.schema_draft(cx).unwrap_or_default();
        // Empty-name complaints wait until the user has tried to create.
        let show = |i: &Issue| {
            d.submitted
                || !(i.message.starts_with("A Dataset needs")
                    || i.message == "Column name is empty.")
        };
        let at = |w: Where| -> Vec<String> {
            issues
                .iter()
                .filter(|i| i.at == w && show(i))
                .map(|i| i.message.clone())
                .collect()
        };
        let err_line = |msgs: Vec<String>| {
            div().children(
                msgs.into_iter()
                    .map(|m| div().text_size(px(11.)).text_color(t.deleted).child(m)),
            )
        };
        let label = |s: &'static str| div().text_size(px(12.)).text_color(t.text_muted).child(s);
        let editing = d.editing.is_some();

        // Server and source combos.
        let server_items: Vec<ComboItem> = self
            .ws
            .servers
            .iter()
            .map(|s| ComboItem::new(s.id.clone(), s.name.clone()))
            .collect();
        let server_name = self
            .ws
            .servers
            .iter()
            .find(|s| s.id == d.server)
            .map(|s| s.name.clone())
            .unwrap_or_default();
        let server_combo = self.combo_field(
            cx,
            ComboSite {
                key: "schema-server".into(),
                items: server_items,
                value: Some(d.server.clone().into()),
                fallback: Some(server_name.into()),
                color: None,
                tip: None,
                width: ComboWidth::Flex,
                look: ComboLook::Field,
                size: ComboSize::Medium,
                on_event: std::rc::Rc::new(|this, ev, _| {
                    if let ComboEvent::Change(v) = ev
                        && let Some(d) = this.schema_dialog.as_mut()
                        && d.editing.is_none()
                    {
                        d.server = v.to_string();
                    }
                }),
            },
        );
        let source_combo = self.combo_field(
            cx,
            ComboSite {
                key: "schema-source".into(),
                items: SourceKind::ALL
                    .iter()
                    .map(|k| ComboItem::plain(k.label()))
                    .collect(),
                value: Some(d.source.label().into()),
                fallback: Some(d.source.label().into()),
                color: None,
                tip: None,
                width: ComboWidth::Flex,
                look: ComboLook::Field,
                size: ComboSize::Medium,
                on_event: std::rc::Rc::new(|this, ev, _| {
                    if let ComboEvent::Change(v) = ev
                        && let Some(d) = this.schema_dialog.as_mut()
                        && d.editing.is_none()
                    {
                        d.source = SourceKind::from_label(v);
                    }
                }),
            },
        );

        let mk_btn = |id: String, text: SharedString| {
            toolbar_button(t, eid(id)).h(px(22.)).px(px(8.)).child(text)
        };
        let press = |id: &str, op: &'static str, arg: String| {
            let _ = (id, &arg);
            op
        };
        let _ = press;

        // Column grid.
        let n_cols = d.cols.len();
        let col_rows: Vec<_> = d
            .cols
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let uid = r.uid;
                let ty_combo = self.combo_field(
                    cx,
                    ComboSite {
                        key: format!("schema-col-{uid}-type"),
                        items: fauxfield_model::COLUMN_TYPES
                            .iter()
                            .map(|t| ComboItem::plain(*t))
                            .collect(),
                        value: Some(r.ty.clone().into()),
                        fallback: Some(r.ty.clone().into()),
                        color: None,
                        tip: None,
                        width: ComboWidth::Fixed(84.),
                        look: ComboLook::Field,
                        size: ComboSize::Small,
                        on_event: std::rc::Rc::new(move |this, ev, _| {
                            if let ComboEvent::Change(v) = ev
                                && let Some(r) = this
                                    .schema_dialog
                                    .as_mut()
                                    .and_then(|d| d.cols.iter_mut().find(|r| r.uid == uid))
                            {
                                r.ty = v.to_string();
                            }
                        }),
                    },
                );
                let gen_combo = self.combo_field(
                    cx,
                    ComboSite {
                        key: format!("schema-col-{uid}-gen"),
                        items: generator_items(),
                        value: Some(r.generator.clone().into()),
                        fallback: Some("(infer)".into()),
                        color: None,
                        tip: None,
                        width: ComboWidth::Fixed(150.),
                        look: ComboLook::Field,
                        size: ComboSize::Small,
                        on_event: std::rc::Rc::new(move |this, ev, _| {
                            if let ComboEvent::Change(v) = ev
                                && let Some(r) = this
                                    .schema_dialog
                                    .as_mut()
                                    .and_then(|d| d.cols.iter_mut().find(|r| r.uid == uid))
                            {
                                r.generator = v.to_string();
                            }
                        }),
                    },
                );
                let toggle = |on: bool, text: &'static str, op: &'static str, tip: &'static str| {
                    let _ = tip;
                    let id = format!("schema-{op}-{i}");
                    let b = toolbar_button(t, eid(id.clone()))
                        .h(px(22.))
                        .w(px(24.))
                        .justify_center()
                        .when(on, |b| b.bg(t.element_selected))
                        .when(!on, |b| b.text_color(t.text_muted))
                        .child(text)
                        .on_click(cx.listener(move |this, _, w, cx| {
                            this.schema_op(op, &i.to_string(), w, cx)
                        }));
                    crate::dev_id!(b, id, Button, tip, Press(format!("schema-{op}:{i}")))
                };
                let act = |text: &'static str, op: &'static str, tip: &'static str, dim: bool| {
                    let _ = tip;
                    let id = format!("schema-{op}-{i}");
                    let b = toolbar_button(t, eid(id.clone()))
                        .h(px(22.))
                        .w(px(22.))
                        .justify_center()
                        .when(dim, |b| b.opacity(0.4))
                        .child(text)
                        .on_click(cx.listener(move |this, _, w, cx| {
                            this.schema_op(op, &i.to_string(), w, cx)
                        }));
                    crate::dev_id!(b, id, Button, tip, Press(format!("schema-{op}:{i}")))
                };
                let msgs: Vec<String> = issues
                    .iter()
                    .filter(|x| matches!(x.at, Where::Column(c, _) if c == i) && show(x))
                    .map(|x| x.message.clone())
                    .collect();
                let name_el = crate::dev_id!(
                    div().flex_1().min_w(px(0.)).child(r.name.clone()),
                    format!("schema-col-name-{i}"),
                    Input,
                    "Column name",
                    None
                );
                let default_el = crate::dev_id!(
                    div().w(px(90.)).child(r.default.clone()),
                    format!("schema-col-default-{i}"),
                    Input,
                    "Default",
                    None
                );
                let desc_el = crate::dev_id!(
                    div().flex_1().min_w(px(0.)).child(r.desc.clone()),
                    format!("schema-col-desc-{i}"),
                    Input,
                    "Description",
                    None
                );
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .child(name_el)
                            .child(ty_combo)
                            .child(toggle(r.primary_key, "PK", "pk", "Primary key"))
                            .child(toggle(r.unique, "U", "unique", "Unique"))
                            .child(toggle(r.required, "R", "required", "Required"))
                            .child(default_el)
                            .child(gen_combo)
                            .child(desc_el)
                            .child(act("↑", "up", "Move up", i == 0))
                            .child(act("↓", "down", "Move down", i + 1 == n_cols))
                            .child(act("⧉", "dup", "Duplicate column", false))
                            .child(act("✕", "del", "Remove column", false)),
                    )
                    .child(err_line(msgs))
            })
            .collect();

        let starters: Vec<_> = STARTERS
            .iter()
            .map(|s| {
                let id = format!("schema-starter-{}", s.replace([' ', '+'], ""));
                let b = mk_btn(id.clone(), (*s).into()).on_click(
                    cx.listener(move |this, _, w, cx| this.schema_op("starter", s, w, cx)),
                );
                crate::dev_id!(b, id, Button, *s, Press(format!("schema-starter:{s}")))
            })
            .collect();

        let can_create = issues.is_empty();
        let create_label = if editing {
            "Apply schema"
        } else {
            "Create dataset"
        };
        let create = toolbar_button(t, eid("schema-create"))
            .h(px(26.))
            .px(px(12.))
            .when(can_create, |b| b.bg(t.element_selected))
            .when(!can_create, |b| b.opacity(0.5))
            .child(create_label)
            .on_click(cx.listener(|this, _, w, cx| this.schema_op("create", "", w, cx)));
        let cancel = toolbar_button(t, eid("schema-cancel"))
            .h(px(26.))
            .px(px(12.))
            .child("Cancel")
            .on_click(cx.listener(|this, _, w, cx| this.schema_op("cancel", "", w, cx)));
        let add = mk_btn("schema-add".into(), "+ Add column".into())
            .on_click(cx.listener(|this, _, w, cx| this.schema_op("add", "", w, cx)));
        let crud_on = d.create_crud;
        let crud = toolbar_button(t, eid("schema-crud"))
            .h(px(22.))
            .child(if crud_on {
                "☑ Also create CRUD Routes"
            } else {
                "☐ Also create CRUD Routes"
            })
            .when(editing, |b| b.opacity(0.4))
            .on_click(cx.listener(|this, _, w, cx| {
                if this
                    .schema_dialog
                    .as_ref()
                    .is_some_and(|d| d.editing.is_none())
                {
                    this.schema_op("crud", "", w, cx)
                }
            }));

        let preview = if editing {
            "Edit schema updates the columns and description; the seed source is unchanged."
                .to_string()
        } else {
            preview_toml(&draft)
        };
        let seed_line = crate::schema_draft::seed_path(&draft)
            .map(|p| format!("Seed file: {p}"))
            .unwrap_or_else(|| "No seed file: rows are generated at each start.".into());

        let name_el = crate::dev_id!(
            div().child(d.name.clone()),
            "schema-name",
            Input,
            "Dataset name",
            None
        );
        let desc_el = crate::dev_id!(
            div().child(d.description.clone()),
            "schema-description",
            Input,
            "Description",
            None
        );
        let rows_el = crate::dev_id!(
            div().w(px(90.)).child(d.rows.clone()),
            "schema-rows",
            Input,
            "Generator rows",
            None
        );

        crate::dev_id_z!(
            div()
                .id("schema-backdrop")
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .items_center()
                .pt(px(40.))
                .bg(t.background.opacity(0.6))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .id("schema-dialog")
                        .key_context("Dialog")
                        .track_focus(&self.schema_dialog_focus)
                        .on_key_down(cx.listener(|this, ev: &KeyDownEvent, w, cx| {
                            match ev.keystroke.key.as_str() {
                                "escape" => this.close_schema_dialog(w, cx),
                                "enter" => this.submit_schema_dialog(w, cx),
                                "tab" => this.schema_move_focus(1, w, cx),
                                _ => return,
                            }
                            cx.stop_propagation();
                        }))
                        .w(px(980.))
                        .max_h(px(640.))
                        .p(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(t.border)
                        .bg(t.elevated)
                        .shadow_lg()
                        .child(div().text_size(px(14.)).child(if editing {
                            "Edit schema"
                        } else {
                            "New dataset"
                        }))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .gap(px(4.))
                                        .child(label("Dataset name"))
                                        .child(name_el)
                                        .child(err_line(at(Where::Name))),
                                )
                                .child(
                                    div()
                                        .w(px(220.))
                                        .flex()
                                        .flex_col()
                                        .gap(px(4.))
                                        .child(label("Server"))
                                        .child(server_combo)
                                        .child(err_line(at(Where::Server))),
                                )
                                .child(
                                    div()
                                        .w(px(200.))
                                        .flex()
                                        .flex_col()
                                        .gap(px(4.))
                                        .child(label("Source"))
                                        .child(source_combo),
                                )
                                .when(d.source == SourceKind::Generator, |r| {
                                    r.child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(label("Rows"))
                                            .child(rows_el)
                                            .child(err_line(at(Where::Rows))),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(label("Description"))
                                .child(desc_el),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.))
                                .child(label("Columns"))
                                .child(div().flex_1())
                                .when(!editing, |r| {
                                    r.child(label("Starter:")).children(starters)
                                })
                                .child(add),
                        )
                        .child(
                            div()
                                .id("schema-columns")
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .max_h(px(230.))
                                .overflow_y_scroll()
                                .children(col_rows),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .p(px(8.))
                                .rounded(px(4.))
                                .bg(t.surface)
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(t.text_muted)
                                        .child(seed_line)
                                )
                                .child(
                                    div()
                                        .id("schema-preview")
                                        .max_h(px(110.))
                                        .overflow_y_scroll()
                                        .text_size(px(11.))
                                        .font_family("monospace")
                                        .child(preview),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .child(
                                    div()
                                        .flex_1()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(10.))
                                        .child(crud)
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .text_color(t.text_muted)
                                                .child(
                                                    "Tab next field · Enter create · Esc cancel"
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .gap(px(8.))
                                        .child(crate::dev_id!(
                                            cancel,
                                            "schema-cancel",
                                            Button,
                                            "Cancel",
                                            Press("schema-cancel".into())
                                        ))
                                        .child(crate::dev_id!(
                                            create,
                                            "schema-create",
                                            Button,
                                            create_label,
                                            Press("schema-create".into())
                                        ))
                                ),
                        ),
                ),
            2,
            "schema-dialog",
            Modal,
            "Dataset schema designer",
            None
        )
    }
}

#[cfg(feature = "dev-mcp")]
impl Shell {
    /// `ui.state` view of the schema designer.
    pub(crate) fn schema_dialog_state(
        &self,
        cx: &gpui::App,
    ) -> Option<crate::dev_mcp::state::SchemaDialogState> {
        use crate::dev_mcp::state::{SchemaColumnState, SchemaDialogState};
        let d = self.schema_dialog.as_ref()?;
        let draft = self.schema_draft(cx)?;
        let issues = self.schema_issues(cx);
        Some(SchemaDialogState {
            editing: d.editing.clone(),
            name: draft.name.clone(),
            server: draft.server.clone(),
            description: draft.description.clone(),
            source: match d.source {
                SourceKind::Static => "static",
                SourceKind::Template => "template",
                SourceKind::Generator => "generator",
            },
            rows: draft.rows.clone(),
            create_crud: d.create_crud,
            columns: draft
                .columns
                .iter()
                .map(|c| SchemaColumnState {
                    name: c.name.clone(),
                    ty: c.ty.clone(),
                    primary_key: c.primary_key,
                    unique: c.unique,
                    required: c.required,
                    default: c.default.clone(),
                    generator: c.generator.clone(),
                    description: c.description.clone(),
                })
                .collect(),
            issues: issues
                .iter()
                .map(|i| format!("{}: {}", i.code, i.message))
                .collect(),
            create_enabled: issues.is_empty(),
            preview: preview_toml(&draft),
        })
    }
}
