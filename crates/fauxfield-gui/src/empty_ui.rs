//! The one empty-state view (SPEC §20.34): icon, title, what to do next, a primary button
//! with its live binding, and secondary links. All text comes from [`crate::copy`].

use gpui::{AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, div, px};
use gpui::{StatefulInteractiveElement, prelude::FluentBuilder};

use crate::shell::Shell;
use crate::theme::Theme;
use crate::ui::{icon, toolbar_button};

impl Shell {
    /// Renders the empty state `id` of the copy catalog. Element ids: `empty-<id>` for the
    /// block, `empty-<id>-primary` and `empty-<id>-link-<n>` for the buttons.
    #[allow(clippy::let_and_return)] // the dev id macro is the identity without dev-mcp
    pub(crate) fn empty_state(
        &self,
        id: &'static str,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let spec = crate::copy::empty(id);
        let mac = cfg!(target_os = "macos");
        let keys = |action: &str| {
            self.keymap
                .binding_for(action)
                .map(|k| crate::keymap::display_keys(k, mac))
        };
        let primary = spec.primary.map(|(label, action)| {
            let hint = keys(action);
            let b = toolbar_button(t, crate::ui::eid(format!("empty-{id}-primary")))
                .h(px(26.))
                .px(px(12.))
                .border_1()
                .border_color(t.border_focused)
                .child(label)
                .when_some(hint, |d, k| {
                    d.child(div().text_color(t.text_muted).text_size(px(11.)).child(k))
                })
                .on_click(cx.listener(move |this, _, w, cx| this.run_action_id(action, w, cx)));
            crate::dev_id!(
                b,
                format!("empty-{id}-primary"),
                Button,
                label,
                Action(action)
            )
        });
        let links = spec.links.iter().enumerate().map(|(n, (label, action))| {
            let action: &'static str = action;
            let b = toolbar_button(t, crate::ui::eid(format!("empty-{id}-link-{n}")))
                .text_color(t.text_muted)
                .child(*label)
                .on_click(cx.listener(move |this, _, w, cx| this.run_action_id(action, w, cx)));
            crate::dev_id!(
                b,
                format!("empty-{id}-link-{n}"),
                Button,
                *label,
                Action(action)
            )
        });
        crate::dev_id!(
            div()
                .id(crate::ui::eid(format!("empty-{id}")))
                .flex_1()
                .min_h(px(96.))
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .p(px(16.))
                .text_size(px(12.))
                .text_color(t.text_muted)
                .child(icon(spec.icon, 24., t.icon_muted))
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(t.text)
                        .child(spec.title)
                )
                .child(div().text_center().child(spec.hint))
                .children(primary)
                .when(!spec.links.is_empty(), |d| {
                    d.child(div().flex().flex_row().gap(px(8.)).children(links))
                }),
            format!("empty-{id}"),
            Text,
            spec.title,
            None
        )
        .into_any_element()
    }
}
