//! The stateless `DataTable` element (data-table.md §2): presentation options plus an entity.

use gpui::{AnyElement, App, Entity, IntoElement, RenderOnce, Window};

use super::state::{TableDelegate, TableLook, TableState};
use super::types::Size;

#[derive(IntoElement)]
pub struct DataTable<D: TableDelegate> {
    state: Entity<TableState<D>>,
    look: TableLook,
}

impl<D: TableDelegate> DataTable<D> {
    pub fn new(state: &Entity<TableState<D>>) -> Self {
        DataTable {
            state: state.clone(),
            look: TableLook::default(),
        }
    }

    pub fn stripe(mut self, v: bool) -> Self {
        self.look.stripe = v;
        self
    }

    pub fn bordered(mut self, v: bool) -> Self {
        self.look.bordered = v;
        self
    }

    pub fn scroll_bounce(mut self, v: bool) -> Self {
        self.look.scroll_bounce = v;
        self
    }

    pub fn header_row_height(mut self, h: f32) -> Self {
        self.look.header_row_height = Some(h);
        self
    }

    pub fn fixed_cols_border(mut self, v: bool) -> Self {
        self.look.fixed_cols_border = v;
        self
    }

    pub fn scrollbar_visible(mut self, vertical: bool, horizontal: bool) -> Self {
        self.look.scrollbar_v = vertical;
        self.look.scrollbar_h = horizontal;
        self
    }

    pub fn with_size(mut self, s: Size) -> Self {
        self.look.size = s;
        self
    }

    pub fn focus_ring(mut self, v: bool) -> Self {
        self.look.focus_ring = v;
        self
    }

    pub fn accessibility_label(mut self, text: impl Into<String>) -> Self {
        self.look.label = Some(text.into());
        self
    }
}

impl<D: TableDelegate> RenderOnce for DataTable<D> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let look = self.look;
        self.state.update(cx, |s, cx| -> AnyElement {
            s.render_table(&look, window, cx)
        })
    }
}
