//! `TableState` (entity) and the `TableDelegate` contract (data-table.md §3, §5, §6, §9–§12).

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, Context, Div, DragMoveEvent, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement,
    Render, ScrollStrategy, ScrollWheelEvent, SharedString, Stateful, StatefulInteractiveElement,
    Styled, UniformListScrollHandle, Window, canvas, div, point, px, uniform_list,
};

use super::actions as act;
use super::model::*;
use super::types::*;
use crate::theme::Theme;
use crate::ui::{eid, icon};

/// Presentation options of the `DataTable` element (data-table.md §2).
#[derive(Clone, Debug)]
pub struct TableLook {
    pub stripe: bool,
    pub bordered: bool,
    pub scroll_bounce: bool,
    pub header_row_height: Option<f32>,
    pub fixed_cols_border: bool,
    pub scrollbar_v: bool,
    pub scrollbar_h: bool,
    pub size: Size,
    pub focus_ring: bool,
    pub label: Option<String>,
}

impl Default for TableLook {
    fn default() -> TableLook {
        TableLook {
            stripe: false,
            bordered: true,
            scroll_bounce: cfg!(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "android"
            )),
            header_row_height: None,
            fixed_cols_border: true,
            scrollbar_v: true,
            scrollbar_h: true,
            size: Size::Medium,
            focus_ring: true,
            label: None,
        }
    }
}

/// What the host implements (data-table.md §5). Optional methods have the documented defaults.
pub trait TableDelegate: Sized + 'static {
    fn columns_count(&self, cx: &App) -> usize;
    fn rows_count(&self, cx: &App) -> usize;
    /// Called when the table prepares or refreshes, not per frame.
    fn column(&self, col_ix: usize, cx: &App) -> Column;
    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> AnyElement;

    /// The table already cycled the column's sort state and reset the others to `Default`.
    fn perform_sort(
        &mut self,
        _col_ix: usize,
        _sort: ColumnSort,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) {
    }
    /// Content width (widest cell and header, sort icon included). Must not update the table.
    fn fit_column(&self, _col_ix: usize, _cx: &App) -> Option<f32> {
        None
    }
    fn group_headers(&self, _cx: &App) -> Vec<Vec<ColumnGroup>> {
        vec![]
    }
    fn render_group_th(
        &mut self,
        _row: usize,
        _group: &ColumnGroup,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> Option<AnyElement> {
        None
    }
    fn render_th(
        &mut self,
        _col_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> Option<AnyElement> {
        None
    }
    /// Row container: styling, ids and handlers go here.
    fn render_tr(
        &mut self,
        _row_ix: usize,
        tr: Stateful<Div>,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        tr
    }
    fn cell_label(&self, _row_ix: usize, _col_ix: usize, _cx: &App) -> Option<String> {
        None
    }
    fn row_label(&self, row_ix: usize, cx: &App) -> String {
        (0..self.columns_count(cx))
            .filter_map(|c| self.cell_label(row_ix, c, cx))
            .collect::<Vec<_>>()
            .join(", ")
    }
    /// The host reorders its own column list (remove then insert).
    fn move_column(&mut self, _col_ix: usize, _to_ix: usize, _cx: &mut Context<TableState<Self>>) {}
    fn render_empty(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> Option<AnyElement> {
        None
    }
    fn loading(&self, _cx: &App) -> bool {
        false
    }
    fn render_loading(
        &mut self,
        _size: Size,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> Option<AnyElement> {
        None
    }
    fn has_more(&self, _cx: &App) -> bool {
        false
    }
    fn load_more_threshold(&self) -> usize {
        20
    }
    fn load_more(&mut self, _window: &mut Window, _cx: &mut Context<TableState<Self>>) {}
    /// Must be fast: called often.
    fn visible_rows_changed(&mut self, _range: Range<usize>, _cx: &mut Context<TableState<Self>>) {}
    fn visible_columns_changed(
        &mut self,
        _range: Range<usize>,
        _cx: &mut Context<TableState<Self>>,
    ) {
    }
    /// Export text for the cell, formatted as it should appear in exported data.
    fn cell_text(&self, _row_ix: usize, _col_ix: usize, _cx: &App) -> String {
        String::new()
    }
}

/// Window-space geometry measured during paint (one frame behind).
#[derive(Clone, Copy, Debug, Default)]
pub struct Geometry {
    /// Width of the scrollable middle viewport.
    pub mid_w: f32,
    pub head_left: f32,
    pub head_w: f32,
    pub body_h: f32,
}

#[derive(Clone)]
struct DraggedCol {
    ix: usize,
    name: String,
    width: f32,
}

#[derive(Clone)]
struct DraggedResize;

struct ColGhost(String, f32);

impl Render for ColGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme::theme(cx).clone();
        div()
            .w(px(self.1.clamp(100.0, 450.0)))
            .h(px(24.))
            .px(px(8.))
            .flex()
            .items_center()
            .bg(t.elevated)
            .border_1()
            .border_color(t.border)
            .rounded(px(3.))
            .text_size(px(12.))
            .text_color(t.text)
            .child(self.0.clone())
    }
}

struct NoGhost;

impl Render for NoGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size(px(0.))
    }
}

#[derive(Clone, Copy)]
struct ResizeStart {
    col: usize,
    x: f32,
    w: f32,
}

pub const ROW_HEADER_W: f32 = 36.0;
const SORT_ICON_W: f32 = 12.0;

pub struct TableState<D: TableDelegate> {
    delegate: D,
    pub model: TableModel,
    id: SharedString,
    focus: FocusHandle,
    pub scroll_y: UniformListScrollHandle,
    pub scroll_x: f32,
    geo: Rc<Cell<Geometry>>,
    keyboard: bool,
    resize: Option<ResizeStart>,
    /// Horizontal scrollbar drag: (mouse x, `scroll_x`) at press.
    hbar_drag: Option<(f32, f32)>,
    resize_dirty: bool,
    suppress_drag: bool,
    gap: Option<usize>,
    size: Size,
    row_h: f32,
    groups: Vec<Vec<ColumnGroup>>,
    load_requested_for: Option<usize>,
}

impl<D: TableDelegate> EventEmitter<TableEvent> for TableState<D> {}

impl<D: TableDelegate> Focusable for TableState<D> {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl<D: TableDelegate> TableState<D> {
    pub fn new(
        id: impl Into<SharedString>,
        delegate: D,
        opts: TableOptions,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut s = TableState {
            delegate,
            model: TableModel::new(opts),
            id: id.into(),
            focus: cx.focus_handle(),
            scroll_y: UniformListScrollHandle::new(),
            scroll_x: 0.0,
            geo: Rc::new(Cell::new(Geometry::default())),
            keyboard: false,
            resize: None,
            hbar_drag: None,
            resize_dirty: false,
            suppress_drag: false,
            gap: None,
            size: Size::Medium,
            row_h: Size::Medium.row_height(),
            groups: vec![],
            load_requested_for: None,
        };
        s.refresh(cx);
        s
    }

    pub fn id(&self) -> &SharedString {
        &self.id
    }

    pub fn delegate(&self) -> &D {
        &self.delegate
    }

    pub fn delegate_mut(&mut self) -> &mut D {
        &mut self.delegate
    }

    /// Rebuild column runtime info after columns or rows changed.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let n = self.delegate.columns_count(cx);
        let cols: Vec<Column> = (0..n).map(|i| self.delegate.column(i, cx)).collect();
        self.model.set_columns(cols);
        self.model.set_rows(self.delegate.rows_count(cx));
        self.refresh_header_layout(cx);
        cx.notify();
    }

    /// Recompute group headers after delegate state that affects them changed.
    pub fn refresh_header_layout(&mut self, cx: &mut Context<Self>) {
        self.groups = self.delegate.group_headers(cx);
        self.model.header_rows = 1 + self.groups.len();
    }

    fn emit(&mut self, evs: Vec<TableEvent>, cx: &mut Context<Self>) {
        for e in evs {
            cx.emit(e);
        }
        cx.notify();
    }

    // ---- public selection API ---------------------------------------------------------

    pub fn selection(&self) -> TableSelection {
        self.model.selection
    }

    pub fn selected_row(&self) -> Option<usize> {
        self.model.selected_row()
    }

    pub fn selected_col(&self) -> Option<usize> {
        self.model.selected_col()
    }

    pub fn selected_cell(&self) -> Option<(usize, usize)> {
        self.model.selected_cell()
    }

    pub fn set_selection(&mut self, s: TableSelection, cx: &mut Context<Self>) {
        let evs = self.model.set_selection(s);
        match s {
            TableSelection::Row(r) => self.scroll_to_row(r, ScrollStrategy::Center),
            TableSelection::Column(c) => self.scroll_to_col(c),
            TableSelection::Cell(r, c) => {
                self.scroll_to_row(r, ScrollStrategy::Center);
                self.scroll_to_col(c);
            }
            TableSelection::None => {}
        }
        self.emit(evs, cx);
    }

    pub fn set_selected_row(&mut self, r: usize, cx: &mut Context<Self>) {
        self.set_selection(TableSelection::Row(r), cx)
    }

    pub fn set_selected_col(&mut self, c: usize, cx: &mut Context<Self>) {
        self.set_selection(TableSelection::Column(c), cx)
    }

    pub fn set_selected_cell(&mut self, r: usize, c: usize, cx: &mut Context<Self>) {
        self.set_selection(TableSelection::Cell(r, c), cx)
    }

    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        let evs = self.model.clear_selection();
        self.emit(evs, cx);
    }

    pub fn right_clicked_row(&self) -> Option<usize> {
        self.model.right_row
    }

    pub fn right_clicked_cell(&self) -> Option<(usize, usize)> {
        self.model.right_cell
    }

    /// Clear the row target when opening a header menu.
    pub fn set_right_clicked_row(&mut self, r: Option<usize>, cx: &mut Context<Self>) {
        self.model.right_row = r;
        cx.notify();
    }

    // ---- scrolling --------------------------------------------------------------------

    pub fn scroll_to_row(&mut self, ix: usize, strategy: ScrollStrategy) {
        self.scroll_y.scroll_to_item(ix, strategy);
    }

    fn region_bounds(&self) -> (usize, usize) {
        if !self.model.opts.col_fixed {
            return (0, 0);
        }
        let cols = &self.model.columns;
        let l = cols
            .iter()
            .take_while(|c| c.fixed == Some(ColumnFixed::Left))
            .count();
        let r = cols
            .iter()
            .rev()
            .take_while(|c| c.fixed == Some(ColumnFixed::Right))
            .count();
        (l, r.min(cols.len() - l.min(cols.len())))
    }

    fn max_scroll_x(&self) -> f32 {
        let (l, r) = self.region_bounds();
        let n = self.model.widths.len();
        let mid: f32 = self.model.widths[l.min(n)..n - r.min(n)].iter().sum();
        (mid - self.geo.get().mid_w).max(0.0)
    }

    /// Scroll a column into view. Columns pinned on either side are always in view.
    pub fn scroll_to_col(&mut self, ix: usize) {
        let (l, r) = self.region_bounds();
        let n = self.model.widths.len();
        if ix < l || ix >= n - r.min(n) || ix >= n {
            return;
        }
        let left: f32 = self.model.widths[l..ix].iter().sum();
        let right = left + self.model.widths[ix];
        let vp = self.geo.get().mid_w;
        if left < self.scroll_x {
            self.scroll_x = left;
        } else if right > self.scroll_x + vp {
            self.scroll_x = right - vp;
        }
        self.scroll_x = self.scroll_x.clamp(0.0, self.max_scroll_x());
    }

    // ---- widths, fit, visible, export -------------------------------------------------

    pub fn col_width(&self, ix: usize) -> f32 {
        self.model.widths.get(ix).copied().unwrap_or(0.0)
    }

    /// Fit a column to its content through the delegate.
    pub fn fit_col(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(content) = self.delegate.fit_column(ix, cx) else {
            return;
        };
        if self.model.fit(ix, content, self.size.pad_x()).is_some() {
            let w = self.model.widths.clone();
            self.emit(vec![TableEvent::ColumnWidthsChanged(w)], cx);
        }
    }

    pub fn visible_range(&self) -> VisibleRange {
        self.model.visible.clone().unwrap_or_default()
    }

    pub fn headers(&self) -> Vec<String> {
        self.model.columns.iter().map(|c| c.name.clone()).collect()
    }

    pub fn dump(&self, cx: &App) -> (Vec<String>, Vec<Vec<String>>) {
        (self.headers(), self.dump_range(0..self.model.rows, cx))
    }

    /// Export a clamped row range, so huge tables can be exported in batches.
    pub fn dump_range(&self, range: Range<usize>, cx: &App) -> Vec<Vec<String>> {
        let r = clamp_range(range, self.model.rows);
        r.map(|row| {
            (0..self.model.columns.len())
                .map(|c| self.delegate.cell_text(row, c, cx))
                .collect()
        })
        .collect()
    }

    pub fn view_model(&self, cx: &App) -> TableViewModel {
        self.model.view_model(self.delegate.loading(cx))
    }

    // ---- input handlers ---------------------------------------------------------------

    fn key(&mut self, k: NavKey, window: &mut Window, cx: &mut Context<Self>) {
        self.keyboard = true;
        let before = self.model.selection;
        let evs = self.model.nav(k);
        if evs.is_empty() {
            return;
        }
        let after = self.model.selection;
        let down = matches!(k, NavKey::Down | NavKey::PageDown | NavKey::End);
        let strat = if down {
            ScrollStrategy::Bottom
        } else {
            ScrollStrategy::Top
        };
        match after {
            TableSelection::Row(r) | TableSelection::Cell(r, _) => {
                if before != after {
                    self.scroll_to_row(r, strat);
                }
            }
            _ => {}
        }
        match after {
            TableSelection::Column(c) | TableSelection::Cell(_, c) => self.scroll_to_col(c),
            _ => {}
        }
        let _ = window;
        self.emit(evs, cx);
    }

    fn escape(&mut self, cx: &mut Context<Self>) {
        self.keyboard = true;
        let (handled, evs) = self.model.escape();
        if handled {
            self.emit(evs, cx);
        } else {
            cx.propagate();
        }
    }

    fn mods(ev: &ClickEvent) -> ClickMods {
        let m = ev.modifiers();
        ClickMods {
            secondary: m.secondary(),
            shift: m.shift,
            clicks: ev.click_count().max(1),
        }
    }

    /// Programmatic resize (dev tools and hosts): clamp, apply, report.
    pub fn resize_col(&mut self, ix: usize, width: f32, cx: &mut Context<Self>) {
        if self.model.resize(ix, width).is_some() {
            let w = self.model.widths.clone();
            self.emit(vec![TableEvent::ColumnWidthsChanged(w)], cx);
        }
    }

    /// Programmatic column move to the gap `gap` (same rules as a drag).
    pub fn move_col(&mut self, from: usize, gap: usize, cx: &mut Context<Self>) -> bool {
        match self.model.plan_move(from, gap) {
            Some(to) => {
                self.model.apply_move(from, to);
                self.delegate.move_column(from, to, cx);
                self.emit(vec![TableEvent::MoveColumn(from, to)], cx);
                true
            }
            None => false,
        }
    }

    fn flush_resize(&mut self, cx: &mut Context<Self>) {
        if self.resize_dirty {
            self.resize_dirty = false;
            let w = self.model.widths.clone();
            self.emit(vec![TableEvent::ColumnWidthsChanged(w)], cx);
        }
    }

    pub fn header_click(
        &mut self,
        c: usize,
        mods: ClickMods,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.keyboard = false;
        window.focus(&self.focus);
        let mut evs = self.model.click_header(c, mods);
        if let Some((col, sort)) = self.model.cycle_sort(c) {
            self.delegate.perform_sort(col, sort, window, cx);
        }
        if !evs.is_empty() {
            self.scroll_to_col(c);
        }
        evs.extend(vec![]);
        self.emit(evs, cx);
    }

    pub fn row_click(
        &mut self,
        r: usize,
        mods: ClickMods,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.keyboard = false;
        window.focus(&self.focus);
        let evs = self.model.click_row(r, mods);
        self.emit(evs, cx);
    }

    pub fn cell_click(
        &mut self,
        r: usize,
        c: usize,
        mods: ClickMods,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.keyboard = false;
        window.focus(&self.focus);
        let evs = self.model.click_cell(r, c, mods);
        if !evs.is_empty() {
            self.scroll_to_col(c);
        }
        self.emit(evs, cx);
    }

    fn drag_move_resize(&mut self, x: f32, cx: &mut Context<Self>) {
        let Some(st) = self.resize else { return };
        if self.suppress_drag {
            return;
        }
        if self.model.resize(st.col, st.w + (x - st.x)).is_some() {
            self.resize_dirty = true;
            // Scroll when the handle nears the viewport edge.
            let g = self.geo.get();
            let edge = 24.0;
            if x > g.head_left + g.head_w - edge {
                self.scroll_x = (self.scroll_x + 12.0).min(self.max_scroll_x());
            } else if x < g.head_left + edge {
                self.scroll_x = (self.scroll_x - 12.0).max(0.0);
            }
            cx.notify();
        }
    }

    /// Visual left edge of every column (and the right edge of the last) in window x.
    fn visual_edges(&self) -> Vec<f32> {
        let g = self.geo.get();
        let (l, r) = self.region_bounds();
        let n = self.model.widths.len();
        let lead = if self.model.opts.cell_selectable && self.model.opts.row_header {
            ROW_HEADER_W
        } else {
            0.0
        };
        let left_w: f32 = self.model.widths[..l].iter().sum();
        let right_w: f32 = self.model.widths[n - r..].iter().sum();
        let mut edges = Vec::with_capacity(n + 1);
        let mut x = 0.0;
        for i in 0..n {
            let w = self.model.widths[i];
            let vx = if i < l {
                g.head_left + lead + x
            } else if i >= n - r {
                g.head_left + g.head_w - right_w + (x - left_w - self.mid_total(l, r))
            } else {
                (g.head_left + lead + left_w + (x - left_w) - self.scroll_x).clamp(
                    g.head_left + lead + left_w,
                    g.head_left + g.head_w - right_w,
                )
            };
            edges.push(vx);
            x += w;
        }
        edges.push(g.head_left + g.head_w - right_w);
        let _ = n;
        edges
    }

    fn mid_total(&self, l: usize, r: usize) -> f32 {
        let n = self.model.widths.len();
        self.model.widths[l..n - r].iter().sum()
    }

    fn drag_move_col(&mut self, from: usize, x: f32, cx: &mut Context<Self>) {
        let edges = self.visual_edges();
        let centres: Vec<f32> = (0..self.model.widths.len())
            .map(|i| (edges[i] + edges[i + 1].max(edges[i])) / 2.0)
            .collect();
        let gap = gap_from_centres(&centres, x);
        let g = self.model.plan_move(from, gap).map(|_| gap);
        if g != self.gap {
            self.gap = g;
            cx.notify();
        }
    }

    fn drop_col(&mut self, from: usize, cx: &mut Context<Self>) {
        let gap = self.gap.take();
        if let Some(to) = gap.and_then(|g| self.model.plan_move(from, g)) {
            self.model.apply_move(from, to);
            self.delegate.move_column(from, to, cx);
            self.emit(vec![TableEvent::MoveColumn(from, to)], cx);
        } else {
            cx.notify();
        }
    }

    // ---- rendering --------------------------------------------------------------------

    fn mid_window(&self, l: usize, r: usize) -> (Range<usize>, f32) {
        let n = self.model.widths.len();
        let vp = self.geo.get().mid_w.max(200.0);
        let mut x = 0.0;
        let (mut first, mut last, mut first_off) = (l, l, 0.0);
        let mut found = false;
        for i in l..n - r {
            let w = self.model.widths[i];
            if !found && x + w > self.scroll_x {
                first = i;
                first_off = x;
                found = true;
            }
            if found && x < self.scroll_x + vp {
                last = i + 1;
            }
            x += w;
        }
        if !found {
            first = n - r;
            last = n - r;
        }
        (first..last, first_off)
    }

    fn report_cols(&mut self, mid: &Range<usize>, l: usize, r: usize, cx: &mut Context<Self>) {
        let n = self.model.widths.len();
        let start = if l > 0 { 0 } else { mid.start };
        let end = if r > 0 { n } else { mid.end };
        if let Some(v) = self.model.report_visible(
            self.model
                .visible
                .as_ref()
                .map(|v| v.rows.clone())
                .unwrap_or(0..0),
            start..end,
        ) {
            self.delegate.visible_columns_changed(v.cols, cx);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn region(
        &self,
        t: &Theme,
        look: &TableLook,
        left: Vec<AnyElement>,
        mid: Vec<AnyElement>,
        mid_first_off: f32,
        right: Vec<AnyElement>,
        lead: Option<AnyElement>,
        h: f32,
        l_w: f32,
        r_w: f32,
        probe: bool,
    ) -> Div {
        let geo = self.geo.clone();
        let border = t.border_variant;
        let scroll_x = self.scroll_x;
        let mut row = div().flex().flex_row().w_full().h(px(h)).flex_none();
        if let Some(lead) = lead {
            row = row.child(lead);
        }
        if !left.is_empty() {
            row = row.child(
                div()
                    .flex()
                    .flex_row()
                    .flex_none()
                    .w(px(l_w))
                    .h_full()
                    .when_fixed_border(look.fixed_cols_border, border, true)
                    .children(left),
            );
        }
        let mut vp = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .relative()
            .overflow_hidden()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .h_full()
                    .left(px(mid_first_off - scroll_x))
                    .flex()
                    .flex_row()
                    .children(mid),
            );
        if probe {
            vp = vp.child(
                canvas(
                    move |b, window, _| {
                        let mut g = geo.get();
                        let w = f32::from(b.size.width);
                        let changed = (g.mid_w - w).abs() > 0.5;
                        g.mid_w = w;
                        geo.set(g);
                        if changed {
                            window.refresh();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        }
        row = row.child(vp);
        if !right.is_empty() {
            row = row.child(
                div()
                    .flex()
                    .flex_row()
                    .flex_none()
                    .w(px(r_w))
                    .h_full()
                    .when_fixed_border(look.fixed_cols_border, border, false)
                    .children(right),
            );
        }
        row
    }

    fn sort_icon(&self, c: usize, t: &Theme) -> Option<AnyElement> {
        let col = &self.model.columns[c];
        match col.sort {
            ColumnSort::Ascending => {
                Some(icon("arrow-up", SORT_ICON_W, t.text_muted).into_any_element())
            }
            ColumnSort::Descending => {
                Some(icon("arrow-down", SORT_ICON_W, t.text_muted).into_any_element())
            }
            _ => None,
        }
    }

    fn header_cell(
        &mut self,
        c: usize,
        t: &Theme,
        look: &TableLook,
        _h: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let col = self.model.columns[c].clone();
        let w = self.model.widths[c];
        let pad = col.paddings.unwrap_or(look.size.pad_x());
        let selected = self.model.selected_col() == Some(c);
        let custom = self.delegate.render_th(c, window, cx);
        let sortable = self.model.opts.sortable && col.sort != ColumnSort::None;
        let key = col.key.clone();
        let id = self.id.clone();
        let label = custom.unwrap_or_else(|| {
            let mut d = div().flex().flex_row().items_center().gap(px(4.)).min_w_0();
            d = match col.align {
                Align::Left => d,
                Align::Center => d.justify_center().w_full(),
                Align::Right => d.justify_end().w_full(),
            };
            d.child(div().truncate().child(col.name.clone()))
                .children(self.sort_icon(c, t))
                .into_any_element()
        });
        let hover = t.element_hover;
        let mut cell = div()
            .id(eid(format!("dt-{id}-th-{key}")))
            .relative()
            .flex_none()
            .w(px(w))
            .h_full()
            .px(px(pad))
            .flex()
            .items_center()
            .overflow_hidden()
            .text_size(px(11.))
            .text_color(t.text_muted)
            .bg(if selected {
                t.element_selected
            } else {
                t.panel
            })
            .border_r_1()
            .border_color(t.border_variant)
            .when_cursor(sortable || self.model.opts.col_selectable)
            .hover(move |s| s.bg(hover))
            .on_click(cx.listener(move |this, ev: &ClickEvent, w, cx| {
                this.header_click(c, Self::mods(ev), w, cx)
            }))
            .child(label);
        if self.model.can_move(c) {
            let drag = DraggedCol {
                ix: c,
                name: col.name.clone(),
                width: w,
            };
            cell = cell.on_drag(drag, |d, _, _, cx| {
                cx.new(|_| ColGhost(d.name.clone(), d.width))
            });
        }
        let mut cell = cell;
        if self.model.can_resize(c) {
            let handle = div()
                .id(eid(format!("dt-{id}-rs-{key}")))
                .absolute()
                .top_0()
                .right(px(-4.))
                .w(px(9.))
                .h_full()
                .flex()
                .justify_center()
                .cursor(gpui::CursorStyle::ResizeLeftRight)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, _w, cx| {
                        this.flush_resize(cx);
                        cx.stop_propagation();
                        if ev.click_count >= 2 {
                            this.suppress_drag = true;
                            this.resize = None;
                            this.fit_col(c, cx);
                        } else {
                            this.suppress_drag = false;
                            this.resize = Some(ResizeStart {
                                col: c,
                                x: f32::from(ev.position.x),
                                w: this.model.widths[c],
                            });
                        }
                    }),
                )
                .on_drag(DraggedResize, |_, _, _, cx| cx.new(|_| NoGhost))
                .child(div().w(px(1.)).h_full().bg(t.border));
            cell = cell.child(crate::dev_id!(
                handle,
                format!("dt-{id}-rs-{key}"),
                Button,
                format!("Resize {}", col.name),
                None
            ));
        }
        crate::dev_id!(
            cell,
            format!("dt-{id}-th-{key}"),
            Button,
            col.name.clone(),
            DataTable(id.to_string(), crate::dev_mcp::tree::DtOp::Header(c))
        )
        .into_any_element()
    }

    fn group_row(
        &mut self,
        row: usize,
        t: &Theme,
        look: &TableLook,
        h: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (l, r) = self.region_bounds();
        let n = self.model.widths.len();
        let groups = self.groups[row].clone();
        let mut left = vec![];
        let mut mid = vec![];
        let mut right = vec![];
        let mut start = 0;
        let (mid_range, mid_off) = self.mid_window(l, r);
        let mut mid_first_off = mid_off;
        let mut mid_started = false;
        for g in groups {
            let end = (start + g.span).min(n);
            let custom = self.delegate.render_group_th(row, &g, window, cx);
            let mk = |from: usize, to: usize, custom: Option<AnyElement>| -> AnyElement {
                let w: f32 = self.model.widths[from..to].iter().sum();
                div()
                    .flex_none()
                    .w(px(w))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(px(look.size.pad_x()))
                    .overflow_hidden()
                    .bg(t.panel)
                    .border_r_1()
                    .border_color(t.border_variant)
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child(custom.unwrap_or_else(|| {
                        div().truncate().child(g.label.clone()).into_any_element()
                    }))
                    .into_any_element()
            };
            let mut custom = custom;
            // Left region part.
            let (a, b) = (start.max(0), end.min(l));
            if a < b {
                left.push(mk(a, b, custom.take()));
            }
            // Middle part (only the groups intersecting the visible window are emitted).
            let (a, b) = (start.max(l), end.min(n - r));
            if a < b && b > mid_range.start && a < mid_range.end {
                if !mid_started {
                    let off: f32 = self.model.widths[l..a].iter().sum();
                    mid_first_off = off;
                    mid_started = true;
                }
                mid.push(mk(a, b, custom.take()));
            }
            let (a, b) = (start.max(n - r), end);
            if a < b {
                right.push(mk(a, b, custom.take()));
            }
            start = end;
        }
        let (l_w, r_w) = self.fixed_widths(l, r);
        let lead = self.lead_cell(t, h, true);
        self.region(
            t,
            look,
            left,
            mid,
            mid_first_off,
            right,
            lead,
            h,
            l_w,
            r_w,
            false,
        )
        .into_any_element()
    }

    fn fixed_widths(&self, l: usize, r: usize) -> (f32, f32) {
        let n = self.model.widths.len();
        (
            self.model.widths[..l].iter().sum(),
            self.model.widths[n - r..].iter().sum(),
        )
    }

    fn lead_cell(&self, t: &Theme, h: f32, header: bool) -> Option<AnyElement> {
        (self.model.opts.cell_selectable && self.model.opts.row_header).then(|| {
            div()
                .flex_none()
                .w(px(ROW_HEADER_W))
                .h(px(h))
                .bg(t.panel)
                .border_r_1()
                .border_color(t.border_variant)
                .when(header, |d| d)
                .into_any_element()
        })
    }

    fn render_rows(
        &mut self,
        range: Range<usize>,
        t: &Theme,
        look: &TableLook,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let total = self.model.rows;
        let rh = look.size.row_height();
        let (l, r) = self.region_bounds();
        let n = self.model.widths.len();
        let (mid_range, mid_off) = self.mid_window(l, r);
        // Report visibility (never for filler rows) and request more rows when near the end.
        let vis = range.start.min(total)..range.end.min(total);
        if let Some(v) = self.model.report_visible(
            vis.clone(),
            self.model
                .visible
                .as_ref()
                .map(|v| v.cols.clone())
                .unwrap_or(0..n),
        ) {
            self.delegate.visible_rows_changed(v.rows.clone(), cx);
        }
        self.report_cols(&mid_range, l, r, cx);
        if needs_load_more(
            vis.end,
            total,
            self.delegate.has_more(cx),
            self.delegate.load_more_threshold(),
        ) && self.load_requested_for != Some(total)
        {
            self.load_requested_for = Some(total);
            cx.defer_in(window, |this, w, cx| this.delegate.load_more(w, cx));
        }
        let (l_w, _) = self.fixed_widths(l, r);
        let r_w = self.fixed_widths(l, r).1;
        let id = self.id.clone();
        let mut out = vec![];
        for row in range {
            if row >= total {
                // Filler rows complete the stripe pattern.
                out.push(
                    div()
                        .h(px(rh))
                        .w_full()
                        .when(look.stripe && row % 2 == 1, |d| d.bg(t.surface))
                        .into_any_element(),
                );
                continue;
            }
            let selected_row = self.model.selected_row() == Some(row);
            let cell_mode = self.model.opts.cell_selectable;
            let cells = |cols: Range<usize>,
                         this: &mut Self,
                         window: &mut Window,
                         cx: &mut Context<Self>| {
                cols.map(|c| this.body_cell(row, c, t, look, selected_row, window, cx))
                    .collect::<Vec<_>>()
            };
            let left = cells(0..l, self, window, cx);
            let mid = cells(mid_range.clone(), self, window, cx);
            let right = cells(n - r..n, self, window, cx);
            let lead = (cell_mode && self.model.opts.row_header).then(|| {
                let sel = selected_row;
                div()
                    .id(eid(format!("dt-{id}-rh-{row}")))
                    .flex_none()
                    .w(px(ROW_HEADER_W))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(10.))
                    .text_color(t.text_muted)
                    .bg(if sel { t.element_selected } else { t.panel })
                    .border_r_1()
                    .border_color(t.border_variant)
                    .on_click(cx.listener(move |this, ev: &ClickEvent, w, cx| {
                        this.keyboard = false;
                        w.focus(&this.focus);
                        let evs = this.model.click_row_header(row, Self::mods(ev));
                        this.emit(evs, cx);
                    }))
                    .child((row + 1).to_string())
                    .into_any_element()
            });
            let mut line = self.region(
                t, look, left, mid, mid_off, right, lead, rh, l_w, r_w, false,
            );
            let bg = if selected_row {
                Some(t.element_selected)
            } else if look.stripe && row % 2 == 1 {
                Some(t.surface)
            } else {
                None
            };
            if let Some(bg) = bg {
                line = line.bg(bg);
            }
            let hover = t.element_hover;
            let mut tr = div()
                .id(eid(format!("dt-{id}-row-{row}")))
                .w_full()
                .h(px(rh))
                .flex_none()
                .border_b_1()
                .border_color(t.border_variant)
                .hover(move |s| s.bg(hover))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, w, cx| {
                        this.keyboard = false;
                        w.focus(&this.focus);
                        let evs = this.model.right_click_row(Some(row));
                        this.emit(evs, cx);
                        cx.stop_propagation();
                    }),
                );
            if !cell_mode {
                tr = tr.on_click(cx.listener(move |this, ev: &ClickEvent, w, cx| {
                    this.row_click(row, Self::mods(ev), w, cx)
                }));
            }
            let tr = self.delegate.render_tr(row, tr, window, cx).child(line);
            out.push(
                crate::dev_id!(
                    tr,
                    format!("dt-{id}-row-{row}"),
                    Row,
                    format!("Row {}", row + 1),
                    DataTable(id.to_string(), crate::dev_mcp::tree::DtOp::Row(row))
                )
                .into_any_element(),
            );
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn body_cell(
        &mut self,
        row: usize,
        c: usize,
        t: &Theme,
        look: &TableLook,
        selected_row: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let col = self.model.columns[c].clone();
        let w = self.model.widths[c];
        let pad = col.paddings.unwrap_or(look.size.pad_x());
        let selected =
            self.model.selected_col() == Some(c) || self.model.selected_cell() == Some((row, c));
        let right_clicked = self.model.right_cell == Some((row, c));
        let content = self.delegate.render_td(row, c, window, cx);
        let id = self.id.clone();
        let cell_mode = self.model.opts.cell_selectable;
        let mut d = div()
            .id(eid(format!("dt-{id}-cell-{row}-{}", col.key)))
            .flex_none()
            .w(px(w))
            .h_full()
            .px(px(pad))
            .flex()
            .items_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_size(px(12.))
            .text_color(t.text)
            .when(selected && !selected_row, |d| d.bg(t.element_selected))
            .when(right_clicked, |d| d.border_1().border_color(t.border))
            .when(col.align == Align::Center, |d| d.justify_center())
            .when(col.align == Align::Right, |d| d.justify_end())
            .child(content);
        if cell_mode && col.selectable {
            d = d
                .on_click(cx.listener(move |this, ev: &ClickEvent, w, cx| {
                    this.cell_click(row, c, Self::mods(ev), w, cx)
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, _, cx| {
                        let evs = this.model.right_click_cell(row, c);
                        this.emit(evs, cx);
                        cx.stop_propagation();
                    }),
                );
        }
        crate::dev_id!(
            d,
            format!("dt-{id}-cell-{row}-{}", col.key),
            Text,
            format!(
                "{}",
                self.delegate.cell_label(row, c, cx).unwrap_or_default()
            ),
            DataTable(id.to_string(), crate::dev_mcp::tree::DtOp::Row(row))
        )
        .into_any_element()
    }

    fn skeleton(&self, t: &Theme, look: &TableLook) -> AnyElement {
        let rh = look.size.row_height();
        div()
            .size_full()
            .flex()
            .flex_col()
            .children((0..5).map(|i| {
                div()
                    .h(px(rh))
                    .px(px(look.size.pad_x()))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .h(px(8.))
                            .w(px(if i == 0 { 160. } else { 280. }))
                            .rounded(px(3.))
                            .bg(t.element_selected),
                    )
            }))
            .into_any_element()
    }

    fn scrollbars(
        &self,
        t: &Theme,
        look: &TableLook,
        hdr_h: f32,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let g = self.geo.get();
        let mut out = vec![];
        let total_h = self.model.rows as f32 * look.size.row_height();
        let view_h = g.body_h;
        if look.scrollbar_v && total_h > view_h && view_h > 0.0 {
            let off = -f32::from(self.scroll_y.0.borrow().base_handle.offset().y);
            let thumb = (view_h * view_h / total_h).max(20.0).min(view_h);
            let top = (off / (total_h - view_h)).clamp(0.0, 1.0) * (view_h - thumb);
            out.push(
                div()
                    .absolute()
                    .right(px(1.))
                    .top(px(hdr_h + top))
                    .w(px(5.))
                    .h(px(thumb))
                    .rounded(px(3.))
                    .bg(t.border)
                    .into_any_element(),
            );
        }
        let max = self.max_scroll_x();
        if look.scrollbar_h && max > 0.0 {
            let (l, r) = self.region_bounds();
            let mid_total = self.mid_total(l, r);
            let vp = g.mid_w;
            let thumb = (vp * vp / mid_total).max(20.0).min(vp);
            let left = (self.scroll_x / max).clamp(0.0, 1.0) * (vp - thumb);
            let (l_w, _) = self.fixed_widths(l, r);
            out.push(
                div()
                    .absolute()
                    .bottom(px(1.))
                    .left(px(l_w + left))
                    .h(px(7.))
                    .w(px(thumb))
                    .rounded(px(3.))
                    .bg(t.border)
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                            this.hbar_drag = Some((f32::from(ev.position.x), this.scroll_x));
                            cx.stop_propagation();
                        }),
                    )
                    .into_any_element(),
            );
        }
        out
    }

    /// Render the whole table; called by `DataTable`.
    pub(crate) fn render_table(
        &mut self,
        look: &TableLook,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = crate::theme::theme(cx).clone();
        self.size = look.size;
        self.row_h = look.size.row_height();
        let rh = look.size.row_height();
        let hdr_h = look.header_row_height.unwrap_or(rh);
        let loading = self.delegate.loading(cx);
        let (l, r) = self.region_bounds();
        let n = self.model.widths.len();
        let g = self.geo.get();
        if self.model.opts.stretch && self.resize.is_none() && g.mid_w > 1.0 && l < n - r.min(n) {
            let hi = n - r.min(n);
            let sum: f32 = self.model.widths[l..hi].iter().sum();
            let extra = g.mid_w - sum;
            if extra > 0.5 {
                let each = extra / (hi - l) as f32;
                for w in &mut self.model.widths[l..hi] {
                    *w += each;
                }
                cx.notify();
            }
        }
        self.scroll_x = self.scroll_x.clamp(0.0, self.max_scroll_x());
        self.model.page_rows = ((g.body_h / rh).floor() as usize).max(1);
        let (l_w, r_w) = self.fixed_widths(l, r);
        let (mid_range, mid_off) = self.mid_window(l, r);
        let id = self.id.clone();
        let focused = self.focus.contains_focused(window, cx);
        let ring = look.focus_ring && focused && self.keyboard;

        // Header rows.
        let mut header = div().flex().flex_col().flex_none().w_full().relative();
        for gi in 0..self.groups.len() {
            let el = self.group_row(gi, &t, look, hdr_h, window, cx);
            header = header.child(el);
        }
        let left: Vec<AnyElement> = (0..l)
            .map(|c| self.header_cell(c, &t, look, hdr_h, window, cx))
            .collect();
        let mid: Vec<AnyElement> = mid_range
            .clone()
            .map(|c| self.header_cell(c, &t, look, hdr_h, window, cx))
            .collect();
        let right: Vec<AnyElement> = (n - r..n)
            .map(|c| self.header_cell(c, &t, look, hdr_h, window, cx))
            .collect();
        let lead = self.lead_cell(&t, hdr_h, true);
        let geo = self.geo.clone();
        let leaf = self
            .region(
                &t, look, left, mid, mid_off, right, lead, hdr_h, l_w, r_w, true,
            )
            .bg(t.panel)
            .border_b_1()
            .border_color(t.border)
            .child(
                canvas(
                    move |b, _, _| {
                        let mut g = geo.get();
                        g.head_left = f32::from(b.origin.x);
                        g.head_w = f32::from(b.size.width);
                        geo.set(g);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        header = header.child(leaf);
        if let Some(gap) = self.gap {
            let edges = self.visual_edges();
            let x = edges.get(gap).copied().unwrap_or(0.0) - g.head_left;
            header = header.child(
                div()
                    .absolute()
                    .top_0()
                    .left(px(x - 1.))
                    .w(px(2.))
                    .h_full()
                    .bg(t.border_focused),
            );
        }
        let header = header
            .on_drag_move::<DraggedCol>(cx.listener(
                |this, ev: &DragMoveEvent<DraggedCol>, _, cx| {
                    let from = ev.drag(cx).ix;
                    this.drag_move_col(from, f32::from(ev.event.position.x), cx);
                },
            ))
            .on_drop(cx.listener(|this, d: &DraggedCol, _, cx| this.drop_col(d.ix, cx)));

        // Body.
        let st = cx.entity();
        let look2 = look.clone();
        let t2 = t.clone();
        let geo2 = self.geo.clone();
        let body: AnyElement = if loading {
            self.delegate
                .render_loading(look.size, window, cx)
                .unwrap_or_else(|| self.skeleton(&t, look))
        } else if self.model.rows == 0 {
            self.delegate.render_empty(window, cx).unwrap_or_else(|| {
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("table", 28., t.icon_muted))
                    .into_any_element()
            })
        } else {
            let stripe_fill = if look.stripe {
                (g.body_h / rh).ceil() as usize + 1
            } else {
                0
            };
            let count = self.model.rows + stripe_fill.min(self.model.rows + 64);
            uniform_list("dt-rows", count, move |range, window, cx| {
                st.update(cx, |s, cx| s.render_rows(range, &t2, &look2, window, cx))
            })
            .track_scroll(self.scroll_y.clone())
            .size_full()
            .into_any_element()
        };
        let body = div()
            .id(eid(format!("dt-{id}-body")))
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .child(body)
            .child(
                canvas(
                    move |b, window, _| {
                        let mut g = geo2.get();
                        let h = f32::from(b.size.height);
                        let changed = (g.body_h - h).abs() > 0.5;
                        g.body_h = h;
                        geo2.set(g);
                        if changed {
                            window.refresh();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| {
                    let evs = this.model.right_click_row(None);
                    this.emit(evs, cx);
                }),
            );

        let root = div()
            .id(eid(format!("dt-{id}")))
            .key_context("DataTable")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(t.editor)
            .text_color(t.text)
            .when(look.bordered, |d| {
                d.border_1().rounded(px(4.)).border_color(if ring {
                    t.border_focused
                } else {
                    t.border
                })
            })
            .when(!look.bordered && ring, |d| {
                d.border_1().border_color(t.border_focused)
            })
            .on_action(cx.listener(|this, _: &act::Up, w, cx| this.key(NavKey::Up, w, cx)))
            .on_action(cx.listener(|this, _: &act::Down, w, cx| this.key(NavKey::Down, w, cx)))
            .on_action(cx.listener(|this, _: &act::Left, w, cx| this.key(NavKey::Left, w, cx)))
            .on_action(cx.listener(|this, _: &act::Right, w, cx| this.key(NavKey::Right, w, cx)))
            .on_action(cx.listener(|this, _: &act::Home, w, cx| this.key(NavKey::Home, w, cx)))
            .on_action(cx.listener(|this, _: &act::End, w, cx| this.key(NavKey::End, w, cx)))
            .on_action(cx.listener(|this, _: &act::PageUp, w, cx| this.key(NavKey::PageUp, w, cx)))
            .on_action(cx.listener(|this, _: &act::PageDown, w, cx| {
                let _ = w;
                this.key(NavKey::PageDown, w, cx)
            }))
            .on_action(cx.listener(|this, _: &act::Escape, _, cx| this.escape(cx)))
            .on_drag_move::<DraggedResize>(cx.listener(
                |this, ev: &DragMoveEvent<DraggedResize>, _, cx| {
                    this.drag_move_resize(f32::from(ev.event.position.x), cx)
                },
            ))
            .on_drop(cx.listener(|this, _: &DraggedResize, _, cx| {
                this.resize = None;
                this.flush_resize(cx);
            }))
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.resize = None;
                    this.flush_resize(cx);
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                let Some((x0, s0)) = this.hbar_drag else {
                    return;
                };
                if !ev.dragging() {
                    this.hbar_drag = None;
                    return;
                }
                let (l, r) = this.region_bounds();
                let total = this.mid_total(l, r);
                let vp = this.geo.get().mid_w;
                let thumb = (vp * vp / total).max(20.0).min(vp);
                let track = (vp - thumb).max(1.0);
                let dx = (f32::from(ev.position.x) - x0) / track * this.max_scroll_x();
                this.scroll_x = (s0 + dx).clamp(0.0, this.max_scroll_x());
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.hbar_drag = None),
            )
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                let d = ev.delta.pixel_delta(px(this.row_h));
                // Shift + wheel on a mouse reports a vertical delta; treat it as horizontal.
                let dx = if ev.modifiers.shift && f32::from(d.x) == 0.0 {
                    f32::from(d.y)
                } else {
                    f32::from(d.x)
                };
                if ev.modifiers.shift || dx.abs() > f32::from(d.y).abs() {
                    this.scroll_x = (this.scroll_x - dx).clamp(0.0, this.max_scroll_x());
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .child(header)
            .child(body)
            .children(self.scrollbars(&t, look, hdr_h * self.model.header_rows as f32, cx));
        let _ = point(px(0.), px(0.));
        let root = crate::dev_id!(
            root,
            format!("dt-{id}"),
            Table,
            look.label.clone().unwrap_or_else(|| "Table".into()),
            None
        );
        root.into_any_element()
    }
}

/// Small styling helpers kept private to the table.
trait DivExt: Sized {
    fn when_fixed_border(self, on: bool, color: gpui::Hsla, left_side: bool) -> Self;
    fn when_cursor(self, on: bool) -> Self;
}

impl DivExt for Div {
    fn when_fixed_border(self, on: bool, color: gpui::Hsla, left_side: bool) -> Self {
        if !on {
            return self;
        }
        if left_side {
            self.border_r_1().border_color(color)
        } else {
            self.border_l_1().border_color(color)
        }
    }

    fn when_cursor(self, _on: bool) -> Self {
        self
    }
}

impl DivExt for Stateful<Div> {
    fn when_fixed_border(self, _on: bool, _color: gpui::Hsla, _left_side: bool) -> Self {
        self
    }

    fn when_cursor(self, on: bool) -> Self {
        if on { self.cursor_pointer() } else { self }
    }
}

use gpui::AppContext as _;
use gpui::prelude::FluentBuilder as _;
