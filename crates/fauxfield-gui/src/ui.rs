//! Small shared widgets. Views build UI only through these and GPUI's element API, so the
//! primitives can be swapped later (docs/adr/002: no `gpui-component`). No colour literals.

use gpui::{
    Div, Hsla, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, div, px, svg,
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
