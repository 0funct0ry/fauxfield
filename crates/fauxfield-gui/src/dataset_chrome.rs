//! The Dataset tab chrome (M10.07): the single status chip row, the details popover, the seed
//! problem banner, the filter box and the hamburger menu. No menu kind of its own: the
//! hamburger is a [`ComboBox`](crate::combo_box::ComboBox) of action entries.

use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, prelude::*, px,
};

use crate::shell::Shell;
use crate::text_input::{InputEvent, InputSize, TextInput};
use crate::theme::Theme;
use crate::ui::{eid, toolbar_button};
use crate::view::Dataset;

/// A boxed click handler (the selection bar's icon buttons).
type ClickHandler = Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App)>;

/// One entry of the hamburger: action id, label, and why it is disabled (if it is).
#[derive(Clone, Debug, PartialEq)]
pub struct HamburgerItem {
    pub action: &'static str,
    pub label: String,
    pub disabled: Option<&'static str>,
    /// Lucide icon name.
    pub icon: &'static str,
    /// The entry opens a new group (a separator is drawn above it).
    pub group: bool,
    /// Destructive (drawn in the delete colour).
    pub danger: bool,
}

/// The hamburger entries for a Dataset, in order. Pure, so `ui.state` and tests read it too.
pub fn hamburger_items(
    d: &Dataset,
    total: usize,
    has_host: bool,
    json_mode: bool,
    seed_mode: bool,
) -> Vec<HamburgerItem> {
    let no_host = (!has_host).then_some("No host is running");
    let item = |action, label: &str, disabled| HamburgerItem {
        action,
        label: label.into(),
        disabled,
        icon: "",
        group: false,
        danger: false,
    };
    let table = json_mode || seed_mode;
    let mut v = vec![
        item(
            "toggle_dataset_view",
            if table {
                "Switch to Table view"
            } else {
                "Switch to JSON view"
            },
            None,
        ),
        item("export_dataset", "Export…", no_host),
        item("generate_dataset_rows", "Seed data…", no_host),
        item("import_into_dataset", "Import…", no_host),
        item("add_dataset_column", "Add column…", None),
        item("edit_dataset_schema", "Edit schema…", None),
        item(
            "create_crud_routes",
            "Create CRUD Routes…",
            (!d.used_by.is_empty()).then_some("A Route already serves this Dataset"),
        ),
        item("open_dataset_seed", "Open seed file", None),
        item("reset_state", "Reset", no_host),
        item(
            "clear_dataset_data",
            "Clear data…",
            if !has_host {
                Some("No host is running")
            } else if total == 0 {
                Some("There are no rows")
            } else {
                None
            },
        ),
    ];
    // Icons, groups (view | data in and out | schema and Routes | destructive) and colour.
    let looks: [(&str, &'static str, bool); 10] = [
        (
            "toggle_dataset_view",
            if table { "table" } else { "braces" },
            false,
        ),
        ("export_dataset", "download", true),
        ("generate_dataset_rows", "dice", false),
        ("import_into_dataset", "upload", false),
        ("add_dataset_column", "columns-2", true),
        ("edit_dataset_schema", "pencil", false),
        ("create_crud_routes", "server", false),
        ("open_dataset_seed", "file-text", false),
        ("reset_state", "refresh-cw", true),
        ("clear_dataset_data", "trash-2", false),
    ];
    for it in &mut v {
        if let Some((_, icon, group)) = looks.iter().find(|(a, ..)| *a == it.action) {
            it.icon = icon;
            it.group = *group;
        }
        it.danger = it.action == "clear_dataset_data";
    }
    v
}

/// `N rows · M columns · source · used by K Routes`.
pub fn chip_text(d: &Dataset, total: usize) -> String {
    let source = match d.source.as_str() {
        "static" => "static seed",
        "generator" => "generator",
        "template" => "seed template",
        _ => "live rows only",
    };
    format!(
        "{total} {} · {} {} · {source}",
        if total == 1 { "row" } else { "rows" },
        d.columns.len(),
        if d.columns.len() == 1 {
            "column"
        } else {
            "columns"
        },
    )
}

pub fn used_by_text(d: &Dataset) -> String {
    format!(
        "used by {} {}",
        d.used_by.len(),
        if d.used_by.len() == 1 {
            "Route"
        } else {
            "Routes"
        }
    )
}

impl Shell {
    /// The filter input of the Dataset tab `key`, created on first use.
    pub(crate) fn ds_filter_input(
        &self,
        key: &str,
        cx: &mut Context<Self>,
    ) -> gpui::Entity<TextInput> {
        if let Some(e) = self.ds_filters.borrow().get(key) {
            return e.clone();
        }
        let e = cx.new(|cx| {
            TextInput::new(cx)
                .size(InputSize::Small)
                .placeholder("Filter rows")
                .cleanable(true)
        });
        let k = key.to_string();
        let sub = cx.subscribe(&e, move |this: &mut Shell, input, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                let text = input.read(cx).value().to_string();
                this.set_dataset_filter(&k, &text, cx);
            }
        });
        self.ds_filter_subs.borrow_mut().push(sub);
        self.ds_filters
            .borrow_mut()
            .insert(key.to_string(), e.clone());
        e
    }

    pub(crate) fn set_dataset_filter(&mut self, key: &str, text: &str, cx: &mut Context<Self>) {
        if let Some(t) = self.ds_tables.borrow().get(key).cloned() {
            let text = text.to_string();
            t.update(cx, |s, cx| {
                s.delegate_mut().set_filter(&text);
                s.refresh(cx);
            });
        }
        cx.notify();
    }

    /// The hamburger: a DropdownMenu of the Dataset actions with icons, shortcuts and groups.
    pub(crate) fn ds_hamburger(
        &self,
        d: &Dataset,
        total: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::dropdown_menu::{DropdownEvent, DropdownSite, MenuItem};
        let mac = crate::keymap::Platform::current() == crate::keymap::Platform::Macos;
        let mut items: Vec<MenuItem> = vec![];
        for i in hamburger_items(
            d,
            total,
            self.host.is_some(),
            self.ds_json_mode,
            self.ds_seed_mode,
        ) {
            if i.group {
                items.push(MenuItem::Separator);
            }
            let mut m = MenuItem::item(i.action, i.label)
                .icon(i.icon)
                .danger(i.danger)
                .disabled(i.disabled);
            if let Some(keys) = self.keymap.binding_for(i.action) {
                m = m.shortcut(crate::keymap::display_keys(keys, mac));
            }
            items.push(m);
        }
        self.dropdown_field(
            cx,
            DropdownSite {
                key: "ds-menu".into(),
                icon: "menu",
                items,
                tip: Some("Dataset menu".into()),
                min_width: 240.,
                on_event: std::rc::Rc::new(|this, ev, cx| {
                    if let DropdownEvent::Action(id) = ev {
                        this.defer_action(id, cx);
                    }
                }),
            },
        )
    }

    /// The one-line status row, the details popover and the seed problem banner.
    pub(crate) fn ds_status(
        &self,
        t: &Theme,
        d: &Dataset,
        total: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = d.key();
        let muted = t.text_muted;
        let unused = d.used_by.is_empty();
        let chip = |id: String| {
            toolbar_button(t, eid(id))
                .h(px(20.))
                .px(px(6.))
                .text_color(muted)
        };
        let k1 = key.clone();
        let stats_id = format!("ds-status-{key}");
        let stats = crate::dev_id!(
            chip(stats_id.clone())
                .child(chip_text(d, total))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.ds_popover = !this.ds_popover;
                    let _ = &k1;
                    cx.notify();
                })),
            stats_id,
            Button,
            chip_text(d, total),
            Press("ds-popover:toggle".into())
        );
        let used_id = format!("ds-used-by-chip-{key}");
        let used = if unused {
            crate::dev_id!(
                chip(used_id.clone())
                    .text_color(t.modified)
                    .child(format!("{} · create CRUD Routes…", used_by_text(d)))
                    .on_click(cx.listener(|this, _, w, cx| this.begin_crud_routes(w, cx))),
                used_id,
                Button,
                used_by_text(d),
                Action("create_crud_routes")
            )
            .into_any_element()
        } else {
            crate::dev_id!(
                chip(used_id.clone())
                    .child(used_by_text(d))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.ds_popover = !this.ds_popover;
                        cx.notify();
                    })),
                used_id,
                Button,
                used_by_text(d),
                Press("ds-popover:toggle".into())
            )
            .into_any_element()
        };
        let warn = d.error.as_ref().map(|e| {
            let _ = e;
            crate::dev_id!(
                div()
                    .id(eid(format!("ds-warning-{key}")))
                    .px(px(6.))
                    .text_size(px(12.))
                    .text_color(t.deleted)
                    .child("⚠ seed problem"),
                format!("ds-warning-{key}"),
                Text,
                format!("Seed problem: {e}"),
                None
            )
        });
        let row = div()
            .flex_none()
            .px(px(12.))
            .py(px(4.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .border_b_1()
            .border_color(t.border_variant)
            .text_size(px(12.))
            .child(stats)
            .child(div().text_color(muted).child("·"))
            .child(used)
            .children(warn);

        let popover = self.ds_popover.then(|| {
            let line = |k: &str, v: String| {
                div()
                    .flex()
                    .flex_row()
                    .gap(px(8.))
                    .child(div().w(px(90.)).text_color(muted).child(k.to_string()))
                    .child(div().flex_1().child(v))
            };
            let seed = match d.source.as_str() {
                "static" => d.seed.clone().unwrap_or_default(),
                "template" => d.seed_template.clone().unwrap_or_default(),
                "generator" => format!("generator: {}", d.generator_fields.join(", ")),
                _ => "none".into(),
            };
            let cols = if d.column_defs.is_empty() {
                if d.columns.is_empty() {
                    "none declared".to_string()
                } else {
                    d.columns
                        .iter()
                        .map(|(n, ty)| format!("{n}:{ty}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            } else {
                d.column_defs
                    .iter()
                    .map(|c| {
                        let mut f = String::new();
                        if c.primary_key {
                            f += " pk";
                        }
                        if c.unique {
                            f += " unique";
                        }
                        if c.required {
                            f += " required";
                        }
                        format!(
                            "{}:{}{}",
                            c.name,
                            if c.ty.is_empty() { "string" } else { &c.ty },
                            f
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let routes: Vec<AnyElement> = d
                .used_by
                .iter()
                .enumerate()
                .map(|(n, (rid, label))| {
                    let rid2 = rid.clone();
                    crate::dev_id!(
                        toolbar_button(t, eid(format!("ds-used-by-{rid}-{n}")))
                            .child(format!("Route {label}"))
                            .on_click(
                                cx.listener(move |this, _, _, cx| { this.click_row(&rid2, 2, cx) })
                            ),
                        format!("ds-used-by-{rid}-{n}"),
                        Button,
                        label.clone(),
                        Row(rid.clone())
                    )
                    .into_any_element()
                })
                .collect();
            crate::dev_id!(
                div()
                    .id(eid(format!("ds-popover-{key}")))
                    .flex_none()
                    .mx(px(12.))
                    .my(px(4.))
                    .p(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.elevated)
                    .text_size(px(12.))
                    .child(line("Source", d.source.clone()))
                    .child(line("Seed", seed))
                    .child(line("Key", format!("{} ({})", d.key, d.id_strategy)))
                    .child(line("Columns", cols))
                    .child(line(
                        "Persisted",
                        if d.persist { "yes" } else { "no" }.into()
                    ))
                    .when(!d.description.is_empty(), |c| {
                        c.child(line("Description", d.description.clone()))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap(px(6.))
                            .child(div().w(px(90.)).text_color(muted).child("Used by"))
                            .children(routes)
                            .when(d.used_by.is_empty(), |c| c.child("no Routes")),
                    ),
                format!("ds-popover-{key}"),
                Menu,
                "Dataset details",
                None
            )
        });

        let banner = d
            .error
            .as_ref()
            .filter(|_| !self.ds_banner_dismissed.contains(&key))
            .map(|e| {
                let (k_a, k_b, k_c) = (key.clone(), key.clone(), key.clone());
                let btn = |id: &str, text: &'static str| {
                    toolbar_button(t, eid(format!("{id}-{key}")))
                        .h(px(22.))
                        .child(text)
                };
                crate::dev_id!(
                    div()
                        .id(eid(format!("ds-banner-{key}")))
                        .flex_none()
                        .px(px(12.))
                        .py(px(4.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .bg(t.surface)
                        .border_b_1()
                        .border_color(t.border_variant)
                        .text_size(px(12.))
                        .child(
                            div()
                                .flex_1()
                                .text_color(t.deleted)
                                .child(format!("Seed problem: {e}")),
                        )
                        .child(crate::dev_id!(
                            btn("ds-banner-create", "Create file").on_click(
                                cx.listener(move |this, _, _, cx| this.create_seed_file(&k_a, cx))
                            ),
                            format!("ds-banner-create-{key}"),
                            Button,
                            "Create seed file",
                            None
                        ))
                        .child(crate::dev_id!(
                            btn("ds-banner-open", "Open seed").on_click(cx.listener(
                                move |this, _, _, cx| {
                                    let _ = &k_b;
                                    this.ds_seed_mode = true;
                                    cx.notify();
                                }
                            )),
                            format!("ds-banner-open-{key}"),
                            Button,
                            "Open seed",
                            Press("ds-view:seed".into())
                        ))
                        .child(crate::dev_id!(
                            btn("ds-banner-dismiss", "Dismiss").on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.ds_banner_dismissed.insert(k_c.clone());
                                    cx.notify();
                                }
                            )),
                            format!("ds-banner-dismiss-{key}"),
                            Button,
                            "Dismiss",
                            Press(format!("ds-banner-dismiss:{}", key.replace(':', "|")))
                        )),
                    format!("ds-banner-{key}"),
                    Text,
                    format!("Seed problem: {e}"),
                    None
                )
            });
        div()
            .flex_none()
            .flex()
            .flex_col()
            .child(row)
            .children(popover)
            .children(banner)
            .into_any_element()
    }

    /// The bar shown while rows are selected: count, Copy as JSON, Duplicate, Delete, Clear.
    pub(crate) fn ds_selection_bar(
        &self,
        t: &Theme,
        d: &Dataset,
        selected: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = d.key();
        let btn = |id: &str, text: &'static str| {
            toolbar_button(t, eid(format!("{id}-{key}")))
                .h(px(22.))
                .child(text)
        };
        // Icon-only actions with a tooltip naming them.
        let icon_btn = |id: &str,
                        name: &'static str,
                        tip: &'static str,
                        danger: bool,
                        action: &'static str,
                        on: ClickHandler| {
            let _ = action;
            let full = format!("{id}-{key}");
            let color = if danger { t.deleted } else { t.icon_muted };
            crate::dev_id_meta!(
                crate::ui::icon_button_tinted(t, eid(full.clone()), name, tip, color, false, true)
                    .on_click(on),
                crate::dev_mcp::tree::Meta {
                    tooltip: true,
                    ..Default::default()
                },
                full,
                Button,
                tip,
                Action(action)
            )
        };
        crate::dev_id!(
            div()
                .id(eid(format!("ds-selection-{key}")))
                .flex_none()
                .px(px(12.))
                .py(px(3.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .bg(t.surface)
                .border_b_1()
                .border_color(t.border_variant)
                .text_size(px(12.))
                .child(div().child(format!(
                    "{selected} {} selected",
                    if selected == 1 { "row" } else { "rows" }
                )))
                .child(icon_btn(
                    "ds-sel-copy",
                    "braces",
                    "Copy as JSON",
                    false,
                    "copy_dataset_row",
                    Box::new(cx.listener(|this, _, _, cx| this.copy_selected_rows(cx)))
                ))
                .child(icon_btn(
                    "ds-sel-dup",
                    "copy",
                    "Duplicate",
                    false,
                    "duplicate_dataset_rows",
                    Box::new(cx.listener(|this, _, _, cx| this.duplicate_selected_rows(cx)))
                ))
                .child(icon_btn(
                    "ds-sel-delete",
                    "trash-2",
                    "Delete",
                    true,
                    "delete_dataset_row",
                    Box::new(cx.listener(|this, _, _, cx| this.delete_selected_rows(cx)))
                ))
                .child(div().flex_1())
                .child(crate::dev_id!(
                    btn("ds-sel-clear", "Clear")
                        .on_click(cx.listener(|this, _, _, cx| this.clear_row_selection(cx))),
                    format!("ds-sel-clear-{key}"),
                    Button,
                    "Clear selection",
                    Action("clear_dataset_selection")
                )),
            format!("ds-selection-{key}"),
            Toolbar,
            format!("{selected} selected"),
            None
        )
        .into_any_element()
    }

    /// Banner action: write an empty static seed file for a Dataset whose seed is missing.
    pub(crate) fn create_seed_file(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(d) = self.ws.find_dataset(key).cloned() else {
            return;
        };
        let (sid, name) = (d.server_id.clone(), d.name.clone());
        if self
            .store_op(cx, |s| s.write_dataset_seed(&sid, &name, "[]"))
            .is_some()
        {
            self.ds_gen += 1;
            self.sync_running();
            self.notify(format!("Created the seed file of {name}"), cx);
        }
    }

    // ---- clear data ------------------------------------------------------------------------

    pub(crate) fn begin_clear_data(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.active_dataset_key() else {
            return;
        };
        let Some(d) = self.ws.find_dataset(&key).cloned() else {
            return;
        };
        self.lc.data_confirm = Some(crate::lifecycle::DataConfirm {
            kind: crate::lifecycle::DataConfirmKind::ClearData,
            key,
            name: d.name,
            server_id: d.server_id,
            indices: vec![],
        });
        cx.notify();
    }

    pub(crate) fn clear_dataset_now(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(d) = self.ws.find_dataset(key).cloned() else {
            return;
        };
        let Some(e) = self.engine() else {
            return self.notify("No host is running.".into(), cx);
        };
        self.ds_gen += 1;
        match e.dataset_clear(&d.server_id, &d.name) {
            Ok(()) => self.notify(
                format!("Cleared {}; Reset restores it from the seed", d.name),
                cx,
            ),
            Err(err) => self.notify(err.to_string(), cx),
        }
        self.sync_running();
        cx.notify();
    }

    pub(crate) fn toggle_dataset_view(&mut self, cx: &mut Context<Self>) {
        let table = self.ds_json_mode || self.ds_seed_mode;
        if self.ds_json_mode && !self.leave_json_view(cx) {
            return;
        }
        self.ds_json_mode = !table;
        self.ds_seed_mode = false;
        cx.notify();
    }

    pub(crate) fn open_dataset_seed(&mut self, cx: &mut Context<Self>) {
        self.ds_seed_mode = true;
        cx.notify();
    }

    /// Edit schema… for the active Dataset (action target).
    pub(crate) fn edit_schema_action(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self.active_dataset_key().is_none() {
            return self.notify("Open a dataset first.".into(), cx);
        }
        self.begin_edit_schema(w, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ds(used: usize, cols: usize, source: &str) -> Dataset {
        Dataset {
            name: "p".into(),
            server_id: "s".into(),
            server: "S".into(),
            rows: 0,
            key: "id".into(),
            id_strategy: "uuid".into(),
            persist: false,
            seed: None,
            seed_template: None,
            error: None,
            columns: (0..cols)
                .map(|i| (format!("c{i}"), "string".into()))
                .collect(),
            column_defs: vec![],
            description: String::new(),
            source: source.into(),
            generator_fields: vec![],
            generator_preview: vec![],
            used_by: (0..used)
                .map(|i| (format!("r{i}"), "GET /p".into()))
                .collect(),
        }
    }

    #[test]
    fn chip_is_one_line() {
        assert_eq!(
            chip_text(&ds(0, 2, "static"), 1),
            "1 row · 2 columns · static seed"
        );
        assert_eq!(used_by_text(&ds(0, 0, "none")), "used by 0 Routes");
        assert_eq!(used_by_text(&ds(1, 0, "none")), "used by 1 Route");
    }

    #[test]
    fn hamburger_labels_and_disabled_reasons() {
        let d = ds(0, 1, "static");
        let items = hamburger_items(&d, 0, true, false, false);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels[0], "Switch to JSON view");
        for want in [
            "Export…",
            "Seed data…",
            "Import…",
            "Add column…",
            "Edit schema…",
            "Create CRUD Routes…",
            "Reset",
            "Clear data…",
        ] {
            assert!(labels.contains(&want), "{want}");
        }
        let clear = items
            .iter()
            .find(|i| i.action == "clear_dataset_data")
            .unwrap();
        assert_eq!(clear.disabled, Some("There are no rows"));
        let items = hamburger_items(&ds(1, 1, "static"), 5, true, true, false);
        assert_eq!(items[0].label, "Switch to Table view");
        assert!(
            items
                .iter()
                .find(|i| i.action == "create_crud_routes")
                .unwrap()
                .disabled
                .is_some()
        );
        assert!(
            hamburger_items(&d, 5, false, false, false)[1]
                .disabled
                .is_some()
        );
    }
}
