//! The Quick Search, Command Palette, theme selector and environment picker overlay. One view,
//! four kinds (SPEC §9.6, §9.7, §9.8).

use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::*, px, uniform_list,
};

use crate::keymap::display_keys;
use crate::search::{Hit, Kind};
use crate::shell::{OpenMode, PickerKind, Shell};
use crate::theme::Theme;
use crate::ui::{icon, keycap, method_badge};

pub const ITEM_H: f32 = 28.0;
pub const MAX_VISIBLE: usize = 10;

pub(crate) fn placeholder(kind: PickerKind) -> &'static str {
    match kind {
        PickerKind::Search => {
            "Search routes, servers, folders…   > commands  @ servers  / routes  # folders  $ variables"
        }
        PickerKind::Palette => "Type a command…",
        PickerKind::Theme => "Select a theme (Esc reverts)…",
        PickerKind::Environment => "Switch environment…",
        PickerKind::Network => "Switch network profile…",
        PickerKind::Prompt => "Type a value, Enter to confirm, Esc to cancel",
    }
}

impl Shell {
    /// Stable dev id and label for the open picker, by kind.
    #[cfg_attr(not(feature = "dev-mcp"), allow(dead_code))]
    pub(crate) fn picker_ids(&self) -> (&'static str, &'static str) {
        match self.picker.as_ref().map(|p| p.kind) {
            Some(PickerKind::Search) => ("quick-search", "Quick Search"),
            Some(PickerKind::Palette) => ("command-palette", "Command Palette"),
            Some(PickerKind::Theme) => ("theme-selector", "Theme selector"),
            Some(PickerKind::Prompt) => ("prompt", "Prompt"),
            Some(PickerKind::Network) => ("network-picker", "Network profile picker"),
            _ => ("environment-picker", "Environment picker"),
        }
    }

    pub(crate) fn render_picker(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.picker.as_ref().expect("picker is open");
        let n = p.hits.len();
        let list_h = (n.clamp(1, MAX_VISIBLE)) as f32 * ITEM_H;
        #[cfg(feature = "dev-mcp")]
        crate::dev_mcp::tree::Registry::set_total(cx, "picker-items", n);
        #[cfg(feature = "dev-mcp")]
        let query = p.query.clone();
        div()
            .id("picker-backdrop")
            .absolute()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(70.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, w, cx| this.close_picker(true, w, cx)),
            )
            .child(
                div()
                    .id("picker")
                    .key_context("Picker")
                    .track_focus(&self.picker_focus)
                    .on_key_down(
                        cx.listener(|this, ev: &KeyDownEvent, w, cx| this.on_picker_key(ev, w, cx)),
                    )
                    // Clicks inside must not reach the backdrop.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .w(px(580.))
                    .flex()
                    .flex_col()
                    .rounded(px(8.))
                    .bg(t.elevated)
                    .border_1()
                    .border_color(t.border)
                    .shadow_lg()
                    .overflow_hidden()
                    .child(
                        div()
                            .h(px(36.))
                            .px(px(12.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .border_b_1()
                            .border_color(t.border_variant)
                            .child(icon("search", 14., t.icon_muted))
                            .child(crate::dev_id_z!(
                                div().flex_1().h_full().child(p.input.clone()),
                                1,
                                "picker-input",
                                Input,
                                &query,
                                None
                            )),
                    )
                    .child(if n == 0 {
                        div()
                            .h(px(ITEM_H))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .text_color(t.text_muted)
                            .child("No results")
                            .into_any_element()
                    } else {
                        uniform_list(
                            "picker-items",
                            n,
                            cx.processor(
                                |this: &mut Shell, range: std::ops::Range<usize>, _w, cx| {
                                    let t = crate::theme::theme(cx).clone();
                                    let Some(p) = this.picker.as_ref() else {
                                        return Vec::new();
                                    };
                                    let sel = p.selected;
                                    let items: Vec<(usize, Hit)> = range
                                        .filter_map(|i| p.hits.get(i).map(|h| (i, h.clone())))
                                        .collect();
                                    items
                                        .into_iter()
                                        .map(|(i, h)| this.render_hit(&t, i, &h, i == sel, cx))
                                        .collect::<Vec<_>>()
                                },
                            ),
                        )
                        .track_scroll(self.picker_scroll.clone())
                        .h(px(list_h))
                        .into_any_element()
                    })
                    .children(
                        (p.kind == PickerKind::Theme).then(|| self.render_theme_footer(t, cx)),
                    ),
            )
    }

    fn render_hit(
        &self,
        t: &Theme,
        i: usize,
        h: &Hit,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mac = cfg!(target_os = "macos");
        let (hover, sel) = (t.element_hover, t.element_selected);
        let lead: AnyElement = match (&h.method, h.kind) {
            (Some(m), _) => method_badge(t, m).into_any_element(),
            (None, Kind::Server) => icon("server", 14., t.icon_muted).into_any_element(),
            (None, Kind::Folder) => icon("folder", 14., t.icon_muted).into_any_element(),
            (None, Kind::Environment) => icon("layers", 14., t.icon_muted).into_any_element(),
            (None, Kind::Dataset) => icon("server", 14., t.icon_muted).into_any_element(),
            (None, Kind::Variable) => icon("file-text", 14., t.icon_muted).into_any_element(),
            (None, Kind::Network) => icon("wifi", 14., t.icon_muted).into_any_element(),
            (None, Kind::Theme) => swatch_strip(t, &h.label),
            (None, _) => icon("terminal", 14., t.icon_muted).into_any_element(),
        };
        crate::dev_id_z!(
            div()
                .id(crate::ui::eid(format!("hit-{}", h.label)))
                .h(px(ITEM_H))
                .w_full()
                .px(px(12.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .cursor_pointer()
                .when(selected, |d| d.bg(sel))
                .hover(move |s| s.bg(hover))
                .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, w, cx| {
                    let mode = if ev.modifiers().secondary() {
                        OpenMode::Pinned
                    } else {
                        OpenMode::Preview
                    };
                    this.picker_select(i, mode, w, cx)
                }))
                .child(lead)
                .child(div().flex_1().truncate().child(h.label.clone()))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(t.text_muted)
                        .child(h.detail.clone()),
                )
                .children(h.binding.as_ref().map(|b| keycap(t, &display_keys(b, mac)))),
            1,
            format!("hit-{}", h.label),
            Row,
            &h.label,
            PickerHit(i)
        )
        .into_any_element()
    }
}

/// Background, accent and three syntax colours of a theme (SPEC §9.2 swatch strip).
fn swatch_strip(t: &Theme, name: &str) -> AnyElement {
    let Some(tok) = crate::theme::tokens_named(name) else {
        return icon("palette", 14., t.icon_muted).into_any_element();
    };
    let to_hsla = |c: crate::theme_model::Rgba8| -> gpui::Hsla {
        gpui::Rgba {
            r: f32::from(c.r) / 255.0,
            g: f32::from(c.g) / 255.0,
            b: f32::from(c.b) / 255.0,
            a: 1.0,
        }
        .into()
    };
    let colors = [
        tok.editor,
        tok.text_accent,
        tok.syntax_string,
        tok.syntax_function,
        tok.syntax_keyword,
    ];
    div()
        .flex()
        .flex_row()
        .rounded(px(3.))
        .overflow_hidden()
        .border_1()
        .border_color(t.border_variant)
        .children(
            colors
                .into_iter()
                .map(|c| div().w(px(8.)).h(px(14.)).bg(to_hsla(c))),
        )
        .into_any_element()
}

impl Shell {
    /// Theme selector footer: the "Follow system appearance" switch.
    fn render_theme_footer(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let follow = self.theme_setting.mode == fauxfield_store::settings::ThemeMode::System;
        let box_ = div()
            .size(px(14.))
            .rounded(px(3.))
            .border_1()
            .border_color(t.border_focused)
            .when(follow, |d| d.bg(t.accent));
        crate::dev_id!(
            div()
                .id("theme-follow-system")
                .h(px(ITEM_H))
                .px(px(12.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .border_t_1()
                .border_color(t.border_variant)
                .cursor_pointer()
                .text_color(t.text_muted)
                .child(box_)
                .child("Follow system appearance")
                .on_click(cx.listener(move |this, _, _, cx| this.set_follow_system(!follow, cx))),
            "theme-follow-system",
            Button,
            "Follow system appearance",
            None
        )
        .into_any_element()
    }
}
