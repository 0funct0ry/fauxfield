//! Small shared widgets. Views build UI only through these and GPUI's element API, so the
//! primitives can be swapped later (docs/adr/002: no `gpui-component`). No colour literals.

use gpui::{
    AppContext, Context, Div, Hsla, InteractiveElement, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px, svg,
};

use crate::assets::BUFFER_FONT;
use crate::theme::Theme;

/// Element id from a runtime string (`ElementId` has no `From<String>`).
pub fn eid(s: impl Into<String>) -> gpui::ElementId {
    gpui::ElementId::Name(SharedString::from(s.into()))
}

pub const TITLE_BAR_H: f32 = 26.0;
pub const TAB_BAR_H: f32 = 28.0;
pub const ROW_H: f32 = 24.0;
pub const STATUS_BAR_H: f32 = 22.0;

/// A Lucide line icon tinted by `color`.
pub fn icon(name: &str, size: f32, color: Hsla) -> impl IntoElement {
    svg()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(size))
        .flex_none()
        .text_color(color)
}

/// HTTP method badge: fixed width, monospaced, coloured from syntax tokens.
pub fn method_badge(t: &Theme, method: &str) -> Div {
    div()
        .w(px(40.))
        .flex_none()
        .font_family(BUFFER_FONT)
        .text_size(px(10.))
        .text_color(t.method_color(method))
        .child(method.to_string())
}

/// A key chip such as `⌘K`.
pub fn keycap(t: &Theme, text: &str) -> Div {
    div()
        .px(px(5.))
        .h(px(16.))
        .flex()
        .items_center()
        .rounded(px(3.))
        .border_1()
        .border_color(t.border_variant)
        .text_size(px(10.))
        .text_color(t.text_muted)
        .child(text.to_string())
}

/// Server status dot: filled when running, hollow otherwise.
pub fn status_dot(t: &Theme, running: bool) -> Div {
    let d = div().size(px(8.)).flex_none().rounded_full();
    if running {
        d.bg(t.created)
    } else {
        d.border_1().border_color(t.icon_muted)
    }
}

/// A flat toolbar button base: caller adds `.on_click(..)` and children.
pub fn toolbar_button(t: &Theme, id: impl Into<gpui::ElementId>) -> gpui::Stateful<Div> {
    let hover = t.element_hover;
    div()
        .id(id)
        .h(px(20.))
        .px(px(6.))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.))
        .rounded(px(4.))
        .text_size(px(12.))
        .text_color(t.text)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
}

/// Hairline separator (borders over shadows, SPEC §9.1).
pub fn hairline_v(t: &Theme) -> Div {
    div().w(px(1.)).h_full().flex_none().bg(t.border)
}

pub fn hairline_h(t: &Theme) -> Div {
    div().h(px(1.)).w_full().flex_none().bg(t.border)
}

/// Tags an element for the semantic UI tree (`ui.tree`, `ui.find`, `ui.click_element`).
/// `dev_id!(element, id, role, label, click)`; `dev_id_z!` adds a z layer for overlays.
/// Without `dev-mcp` it expands to the element itself: the arguments are never evaluated.
#[cfg(feature = "dev-mcp")]
#[macro_export]
macro_rules! dev_id {
    ($el:expr, $id:expr, $role:ident, $label:expr, $($click:tt)+) => {
        $crate::dev_id_z!($el, 0, $id, $role, $label, $($click)+)
    };
}

#[cfg(feature = "dev-mcp")]
#[macro_export]
macro_rules! dev_id_z {
    ($el:expr, $z:expr, $id:expr, $role:ident, $label:expr, $($click:tt)+) => {
        $crate::dev_mcp::tree::tag(
            $el,
            $crate::dev_mcp::tree::Spec {
                id: ($id).to_string(),
                role: $crate::dev_mcp::tree::Role::$role,
                label: ($label).to_string(),
                enabled: true,
                z: $z,
                click: $crate::dev_mcp::tree::Click::$($click)+,
                meta: ::std::default::Default::default(),
            },
        )
    };
}

/// Like `dev_id!` with audit metadata: `dev_id_meta!(element, Meta { tooltip: true, ..Default::default() }, id, role, label, click)`.
#[cfg(feature = "dev-mcp")]
#[macro_export]
macro_rules! dev_id_meta {
    ($el:expr, $meta:expr, $id:expr, $role:ident, $label:expr, $($click:tt)+) => {
        $crate::dev_mcp::tree::tag(
            $el,
            $crate::dev_mcp::tree::Spec {
                id: ($id).to_string(),
                role: $crate::dev_mcp::tree::Role::$role,
                label: ($label).to_string(),
                enabled: true,
                z: 0,
                click: $crate::dev_mcp::tree::Click::$($click)+,
                meta: $meta,
            },
        )
    };
}

#[cfg(not(feature = "dev-mcp"))]
#[macro_export]
macro_rules! dev_id_meta {
    ($el:expr, $($rest:tt)*) => {
        $el
    };
}

#[cfg(not(feature = "dev-mcp"))]
#[macro_export]
macro_rules! dev_id {
    ($el:expr, $($rest:tt)*) => {
        $el
    };
}

#[cfg(not(feature = "dev-mcp"))]
#[macro_export]
macro_rules! dev_id_z {
    ($el:expr, $($rest:tt)*) => {
        $el
    };
}

/// A one-line tooltip view.
struct TextTip(SharedString);

impl Render for TextTip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme::theme(cx).clone();
        div()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(4.))
            .border_1()
            .border_color(t.border)
            .bg(t.elevated)
            .text_size(px(12.))
            .text_color(t.text)
            .child(self.0.clone())
    }
}

/// A tooltip builder (formerly `lifecycle::tip`) showing `text`.
pub(crate) fn tooltip(
    text: String,
) -> impl Fn(&mut Window, &mut gpui::App) -> gpui::AnyView + 'static {
    move |_, cx| {
        let s = SharedString::from(text.clone());
        cx.new(|_| TextTip(s.clone())).into()
    }
}

/// The reusable code editor (SPEC §20.18); every surface that needs a text editor uses these.
pub use crate::code_editor::{CodeEditor, EditorEvent, EditorSettings, Language as EditorLanguage};

/// The DataTable family (data-table.md): a virtualised, interactive grid.
pub use crate::data_table::{
    Align, Column, ColumnFixed, ColumnGroup, ColumnSort, DataTable, Size, TableDelegate,
    TableEvent, TableLook, TableOptions, TableSelection, TableState, TableViewModel, VisibleRange,
};

/// The checkbox of the app (the Dataset table selection column and dialog options): a 14 px
/// box, filled with the focus colour and a check mark when on.
pub fn checkbox(t: &Theme, on: bool) -> Div {
    div()
        .size(px(14.))
        .flex_none()
        .rounded(px(3.))
        .border_1()
        .border_color(if on { t.border_focused } else { t.border })
        .bg(if on {
            t.border_focused
        } else {
            gpui::transparent_black()
        })
        .flex()
        .items_center()
        .justify_center()
        .when(on, |d| d.child(icon("check", 10., t.background)))
}

/// A 28x16 on/off switch (Settings rows, the theme selector's follow-system option). The
/// caller adds the id, `on_click` and the dev id.
pub fn switch(t: &Theme, on: bool) -> Div {
    div()
        .w(px(28.))
        .h(px(16.))
        .flex_none()
        .rounded(px(8.))
        .border_1()
        .border_color(if on { t.border_focused } else { t.border })
        .bg(if on { t.border_focused } else { t.surface })
        .flex()
        .flex_row()
        .items_center()
        .px(px(2.))
        .when(on, |d| d.justify_end())
        .child(
            div()
                .size(px(10.))
                .rounded(px(5.))
                .bg(if on { t.background } else { t.text_muted }),
        )
}

/// A checkbox with a label: one clickable row (the caller adds `on_click` and the dev id).
pub fn checkbox_row(
    t: &Theme,
    id: impl Into<gpui::ElementId>,
    on: bool,
    label: impl Into<SharedString>,
) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .h(px(22.))
        .px(px(4.))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.))
        .rounded(px(4.))
        .cursor_pointer()
        .text_size(px(12.))
        .text_color(t.text)
        .hover(|d| d.bg(t.element_hover))
        .child(checkbox(t, on))
        .child(div().min_w(px(0.)).truncate().child(label.into()))
}

/// An icon-only toolbar button with a tooltip (the caller adds `on_click` and the dev id).
/// `active` draws the selected background (a segment that is on); `enabled` false dims it.
pub fn icon_button(
    t: &Theme,
    id: impl Into<gpui::ElementId>,
    name: &str,
    tip: &str,
    active: bool,
    enabled: bool,
) -> gpui::Stateful<Div> {
    let color = if active { t.text } else { t.icon_muted };
    icon_button_tinted(t, id, name, tip, color, active, enabled)
}

/// [`icon_button`] with the icon drawn in `color` (e.g. `t.deleted` for a destructive action).
pub fn icon_button_tinted(
    t: &Theme,
    id: impl Into<gpui::ElementId>,
    name: &str,
    tip: &str,
    color: Hsla,
    active: bool,
    enabled: bool,
) -> gpui::Stateful<Div> {
    toolbar_button(t, id)
        .h(px(22.))
        .w(px(24.))
        .justify_center()
        .when(active, |d| d.bg(t.element_selected))
        .when(!enabled, |d| d.opacity(0.4))
        .tooltip(tooltip(tip.to_string()))
        .child(icon(name, 14., color).into_any_element())
}
