//! Popover: a card anchored under a trigger, modelled on the gpui-kit `Popover`
//! (crates/component/src/popover.rs).
//!
//! * the trigger reports its bounds ([`Shell::popover_trigger_bounds`]); the card opens below it,
//!   left-aligned, 4 px away (the gpui-kit `TopLeft` anchor with the default 0.25 rem offset), and
//!   is clamped to the window margin rather than flipped (`snap_to_window_with_margin`);
//! * `occlude`s what is under it, 12 px padding (`p_3`), theme background, 1 px border, rounded,
//!   shadowed; the content decides the rest (no fixed height, a width is passed in);
//! * dismissed by a press outside it, except on its own trigger, which toggles it (the owner
//!   handles that press), and by Esc (the owner's key handler);
//! * open state is controlled by the owner, who is told about a dismissal (`on_open_change`
//!   semantics: the closure runs with `false`).
//! * the card fades in and slides 4 px down over 150 ms (the dropdown open motion of gpui-kit).

use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, Bounds, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, Pixels, Styled, Window, anchored, canvas, deferred, div, point,
    px,
};

use crate::shell::Shell;
use crate::theme::Theme;

/// Gap between trigger and card.
const OFFSET: f32 = 4.0;
/// Margin kept to the window edge.
const MARGIN: f32 = 8.0;

impl Shell {
    /// A zero-size child for a trigger: it records the trigger's bounds under `key` each frame.
    pub(crate) fn popover_trigger_bounds(&self, key: &str) -> AnyElement {
        let map = self.popover_bounds.clone();
        let key = key.to_string();
        canvas(
            move |b: Bounds<Pixels>, _, _| {
                map.borrow_mut().insert(key.clone(), b);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
        .into_any_element()
    }

    /// The card for the popover of `key`, to be added as a child of any element of the open
    /// screen. `on_dismiss` runs on a press outside the card and outside the trigger.
    pub(crate) fn popover_card(
        &self,
        t: &Theme,
        key: &str,
        width: f32,
        content: AnyElement,
        on_dismiss: impl Fn(&mut Window, &mut gpui::App) + 'static,
    ) -> AnyElement {
        let trigger = self.popover_bounds.borrow().get(key).copied();
        let at = trigger.map_or(point(px(0.), px(0.)), |b| {
            point(b.left(), b.bottom() + px(OFFSET))
        });
        let card = div()
            .id(gpui::SharedString::from(format!("{key}-popover")))
            .occlude()
            .w(px(width))
            .p(px(12.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .rounded(px(8.))
            .bg(t.elevated)
            .border_1()
            .border_color(t.border)
            .shadow_md()
            .text_color(t.text)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(move |ev: &MouseDownEvent, w, cx| {
                if trigger.is_some_and(|b| b.contains(&ev.position)) {
                    return; // the trigger's own press toggles
                }
                on_dismiss(w, cx);
            })
            .child(content)
            .with_animation(
                gpui::SharedString::from(format!("{key}-popover-open")),
                Animation::new(Duration::from_millis(150)).with_easing(gpui::ease_out_quint()),
                |d, k| d.opacity(k).mt(px(-4. * (1. - k))),
            );
        deferred(
            anchored()
                .position(at)
                .snap_to_window_with_margin(px(MARGIN))
                .child(card),
        )
        .with_priority(10)
        .into_any_element()
    }
}
