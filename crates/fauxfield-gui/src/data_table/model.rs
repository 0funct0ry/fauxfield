//! GPUI-free behaviour of the DataTable: selection, keyboard, sort, resize, move, visible
//! ranges, load-more and export clamping (internal-docs/data-table.md §3, §7–§12).
//! `TableState` wraps this with the entity, scroll handles and events.

use std::ops::Range;

use super::types::*;

/// Behaviour options (data-table.md §3). Defaults match the contract.
#[derive(Clone, Debug, PartialEq)]
pub struct TableOptions {
    pub row_selectable: bool,
    pub col_selectable: bool,
    pub cell_selectable: bool,
    pub row_header: bool,
    pub select_on_modifier_click: bool,
    pub loop_selection: bool,
    pub sortable: bool,
    pub col_resizable: bool,
    pub col_movable: bool,
    pub col_fixed: bool,
    /// Grow the scrollable columns evenly to fill the viewport when they are narrower.
    pub stretch: bool,
}

impl Default for TableOptions {
    fn default() -> TableOptions {
        TableOptions {
            row_selectable: true,
            col_selectable: true,
            cell_selectable: false,
            row_header: true,
            select_on_modifier_click: true,
            loop_selection: true,
            sortable: true,
            col_resizable: true,
            col_movable: true,
            col_fixed: true,
            stretch: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct TableModel {
    pub opts: TableOptions,
    pub columns: Vec<Column>,
    pub widths: Vec<f32>,
    pub rows: usize,
    pub selection: TableSelection,
    pub right_row: Option<usize>,
    pub right_cell: Option<(usize, usize)>,
    pub visible: Option<VisibleRange>,
    /// Rows that fit the viewport (for Page Up/Down); at least one.
    pub page_rows: usize,
    pub header_rows: usize,
    pub loading: bool,
}

/// Visible-range filter (§5, last paragraph). `None` means "do not report".
pub fn filter_visible(
    range: Range<usize>,
    total: usize,
    last: Option<&Range<usize>>,
) -> Option<Range<usize>> {
    if total == 0 {
        return (last != Some(&(0..0))).then_some(0..0);
    }
    if range.is_empty() && range.start >= total {
        return None;
    }
    let r = range.start.min(total)..range.end.min(total);
    if r.len() == 1 && total > 1 {
        return None;
    }
    (last != Some(&r)).then_some(r)
}

/// Gap index for a pointer at `x` given the (visual) centre of every column.
pub fn gap_from_centres(centres: &[f32], x: f32) -> usize {
    centres.iter().rposition(|c| *c < x).map_or(0, |i| i + 1)
}

/// Clamp an export row range to `0..rows`.
pub fn clamp_range(range: Range<usize>, rows: usize) -> Range<usize> {
    let start = range.start.min(rows);
    start..range.end.min(rows).max(start)
}

/// Infinite loading: the visible end has reached `rows - threshold`.
pub fn needs_load_more(visible_end: usize, rows: usize, has_more: bool, threshold: usize) -> bool {
    has_more && rows > 0 && visible_end >= rows.saturating_sub(threshold)
}

impl TableModel {
    pub fn new(opts: TableOptions) -> TableModel {
        TableModel {
            opts,
            page_rows: 1,
            header_rows: 1,
            ..TableModel::default()
        }
    }

    pub fn set_columns(&mut self, columns: Vec<Column>) {
        self.widths = columns.iter().map(|c| c.clamp_width(c.width)).collect();
        self.columns = columns;
        match self.selection {
            TableSelection::Column(c) if c >= self.columns.len() => {
                self.selection = TableSelection::None
            }
            TableSelection::Cell(_, c) if c >= self.columns.len() => {
                self.selection = TableSelection::None
            }
            _ => {}
        }
    }

    pub fn set_rows(&mut self, rows: usize) {
        self.rows = rows;
        match self.selection {
            TableSelection::Row(r) | TableSelection::Cell(r, _) if r >= rows => {
                self.selection = TableSelection::None
            }
            _ => {}
        }
    }

    // ---- selection --------------------------------------------------------------------

    pub fn selected_row(&self) -> Option<usize> {
        match self.selection {
            TableSelection::Row(r) => Some(r),
            _ => None,
        }
    }

    pub fn selected_col(&self) -> Option<usize> {
        match self.selection {
            TableSelection::Column(c) => Some(c),
            _ => None,
        }
    }

    pub fn selected_cell(&self) -> Option<(usize, usize)> {
        match self.selection {
            TableSelection::Cell(r, c) => Some((r, c)),
            _ => None,
        }
    }

    fn col_ok(&self, c: usize) -> bool {
        self.columns.get(c).is_some_and(|c| c.selectable)
    }

    /// Set the selection and return exactly the event a user action would emit.
    pub fn set_selection(&mut self, s: TableSelection) -> Vec<TableEvent> {
        let mut ev = vec![];
        match s {
            TableSelection::None => return self.clear_selection(),
            TableSelection::Row(r) => {
                if r >= self.rows {
                    return ev;
                }
                self.selection = s;
                if self.right_row.take().is_some() {
                    ev.push(TableEvent::RightClickedRow(None));
                }
                self.right_cell = None;
                ev.push(TableEvent::SelectRow(r));
            }
            TableSelection::Column(c) => {
                if !self.col_ok(c) {
                    return ev;
                }
                self.selection = s;
                ev.push(TableEvent::SelectColumn(c));
            }
            TableSelection::Cell(r, c) => {
                if r >= self.rows || !self.col_ok(c) {
                    return ev;
                }
                self.selection = s;
                self.right_cell = None;
                ev.push(TableEvent::SelectCell(r, c));
            }
        }
        ev
    }

    pub fn clear_selection(&mut self) -> Vec<TableEvent> {
        self.selection = TableSelection::None;
        vec![TableEvent::ClearSelection]
    }

    /// Escape: `(handled, events)`; with nothing selected the key propagates to the host.
    pub fn escape(&mut self) -> (bool, Vec<TableEvent>) {
        if self.selection == TableSelection::None {
            return (false, vec![]);
        }
        (true, self.clear_selection())
    }

    /// A click on a body row (row mode).
    pub fn click_row(&mut self, r: usize, m: ClickMods) -> Vec<TableEvent> {
        let mut ev = vec![];
        if r >= self.rows {
            return ev;
        }
        let skip = m.is_modified() && !self.opts.select_on_modifier_click;
        if self.opts.row_selectable && !skip {
            ev.extend(self.set_selection(TableSelection::Row(r)));
        }
        if m.clicks >= 2 {
            ev.push(TableEvent::DoubleClickedRow(r));
        }
        ev
    }

    /// A click on a row-header cell (cell mode): selects the whole row.
    pub fn click_row_header(&mut self, r: usize, m: ClickMods) -> Vec<TableEvent> {
        let skip = m.is_modified() && !self.opts.select_on_modifier_click;
        if !self.opts.row_selectable || skip {
            return vec![];
        }
        self.set_selection(TableSelection::Row(r))
    }

    /// A click on a leaf header cell (column mode).
    pub fn click_header(&mut self, c: usize, m: ClickMods) -> Vec<TableEvent> {
        let skip = m.is_modified() && !self.opts.select_on_modifier_click;
        if !self.opts.col_selectable || skip || !self.col_ok(c) {
            return vec![];
        }
        self.set_selection(TableSelection::Column(c))
    }

    /// A click on a body cell in cell mode. Without a row header, clicking the already
    /// selected cell again escalates to its row (never on a double click).
    pub fn click_cell(&mut self, r: usize, c: usize, m: ClickMods) -> Vec<TableEvent> {
        let mut ev = vec![];
        if !self.opts.cell_selectable || r >= self.rows || !self.col_ok(c) {
            return ev;
        }
        let skip = m.is_modified() && !self.opts.select_on_modifier_click;
        if !skip {
            let already = self.selection == TableSelection::Cell(r, c);
            if already && m.clicks < 2 && !self.opts.row_header && self.opts.row_selectable {
                ev.extend(self.set_selection(TableSelection::Row(r)));
            } else if self.selection != TableSelection::Row(r) || m.clicks < 2 {
                ev.extend(self.set_selection(TableSelection::Cell(r, c)));
            }
        }
        if m.clicks >= 2 {
            ev.push(TableEvent::DoubleClickedCell(r, c));
        }
        ev
    }

    /// Right click on a row (`None` = the empty area): clears any right-clicked cell.
    pub fn right_click_row(&mut self, r: Option<usize>) -> Vec<TableEvent> {
        self.right_cell = None;
        self.right_row = r;
        vec![TableEvent::RightClickedRow(r)]
    }

    pub fn right_click_cell(&mut self, r: usize, c: usize) -> Vec<TableEvent> {
        self.right_row = None;
        self.right_cell = Some((r, c));
        vec![TableEvent::RightClickedCell(r, c)]
    }

    // ---- keyboard ---------------------------------------------------------------------

    fn selectable_cols(&self) -> Vec<usize> {
        (0..self.columns.len())
            .filter(|&c| self.col_ok(c))
            .collect()
    }

    fn step(&self, cur: usize, n: usize, fwd: bool) -> usize {
        if fwd {
            if cur + 1 < n {
                cur + 1
            } else if self.opts.loop_selection {
                0
            } else {
                cur
            }
        } else if cur > 0 {
            cur - 1
        } else if self.opts.loop_selection {
            n - 1
        } else {
            cur
        }
    }

    /// Keyboard movement (§8). Returns the events of the resulting selection change.
    pub fn nav(&mut self, key: NavKey) -> Vec<TableEvent> {
        let col_mode = matches!(self.selection, TableSelection::Column(_))
            || (!self.opts.row_selectable
                && !self.opts.cell_selectable
                && self.opts.col_selectable);
        if col_mode {
            return self.nav_column(key);
        }
        if self.rows == 0 {
            return vec![];
        }
        if self.opts.cell_selectable {
            return self.nav_cell(key);
        }
        if !self.opts.row_selectable {
            return vec![];
        }
        let n = self.rows;
        let page = self.page_rows.max(1);
        let cur = match self.selection {
            TableSelection::Row(r) => Some(r),
            _ => None,
        };
        let next = match (key, cur) {
            (
                NavKey::Up
                | NavKey::Down
                | NavKey::Home
                | NavKey::End
                | NavKey::PageUp
                | NavKey::PageDown,
                None,
            ) => 0,
            (NavKey::Up, Some(r)) => self.step(r, n, false),
            (NavKey::Down, Some(r)) => self.step(r, n, true),
            (NavKey::Home, _) => 0,
            (NavKey::End, _) => n - 1,
            (NavKey::PageUp, Some(r)) => r.saturating_sub(page),
            (NavKey::PageDown, Some(r)) => (r + page).min(n - 1),
            (NavKey::Left | NavKey::Right, _) => return vec![],
        };
        self.set_selection(TableSelection::Row(next))
    }

    fn nav_cell(&mut self, key: NavKey) -> Vec<TableEvent> {
        let cols = self.selectable_cols();
        if cols.is_empty() {
            return vec![];
        }
        let n = self.rows;
        let page = self.page_rows.max(1);
        let (r, c) = match self.selection {
            TableSelection::Cell(r, c) => (r, c),
            TableSelection::Row(r) => (r, cols[0]),
            _ => return self.set_selection(TableSelection::Cell(0, cols[0])),
        };
        let ci = cols.iter().position(|&x| x == c).unwrap_or(0);
        let (nr, nc) = match key {
            NavKey::Up => (self.step(r, n, false), c),
            NavKey::Down => (self.step(r, n, true), c),
            NavKey::Left => (r, cols[self.step(ci, cols.len(), false)]),
            NavKey::Right => (r, cols[self.step(ci, cols.len(), true)]),
            NavKey::Home => (r, cols[0]),
            NavKey::End => (r, *cols.last().unwrap()),
            NavKey::PageUp => (r.saturating_sub(page), c),
            NavKey::PageDown => ((r + page).min(n - 1), c),
        };
        self.set_selection(TableSelection::Cell(nr, nc))
    }

    fn nav_column(&mut self, key: NavKey) -> Vec<TableEvent> {
        let cols = self.selectable_cols();
        if cols.is_empty() {
            return vec![];
        }
        let ci = self
            .selected_col()
            .and_then(|c| cols.iter().position(|&x| x == c));
        let next = match (key, ci) {
            (NavKey::Home, _) => 0,
            (NavKey::End, _) => cols.len() - 1,
            (NavKey::Left | NavKey::Right, None) => 0,
            (NavKey::Left, Some(i)) => self.step(i, cols.len(), false),
            (NavKey::Right, Some(i)) => self.step(i, cols.len(), true),
            _ => return vec![],
        };
        self.set_selection(TableSelection::Column(cols[next]))
    }

    // ---- sort -------------------------------------------------------------------------

    /// Header click on a sortable column: cycle Default → Descending → Ascending → Default and
    /// reset every other sortable column. Returns the column and its new state.
    pub fn cycle_sort(&mut self, c: usize) -> Option<(usize, ColumnSort)> {
        if !self.opts.sortable {
            return None;
        }
        let cur = self.columns.get(c)?.sort;
        let next = match cur {
            ColumnSort::None => return None,
            ColumnSort::Default => ColumnSort::Descending,
            ColumnSort::Descending => ColumnSort::Ascending,
            ColumnSort::Ascending => ColumnSort::Default,
        };
        for (i, col) in self.columns.iter_mut().enumerate() {
            if col.sort != ColumnSort::None {
                col.sort = if i == c { next } else { ColumnSort::Default };
            }
        }
        Some((c, next))
    }

    // ---- resize -----------------------------------------------------------------------

    pub fn can_resize(&self, c: usize) -> bool {
        self.opts.col_resizable && self.columns.get(c).is_some_and(|c| c.resizable)
    }

    /// Set a column width clamped to its min and max. Returns the applied width.
    pub fn resize(&mut self, c: usize, w: f32) -> Option<f32> {
        if !self.can_resize(c) {
            return None;
        }
        let w = self.columns[c].clamp_width(w);
        self.widths[c] = w;
        Some(w)
    }

    /// Fit to content: the delegate's content width plus cell padding, clamped.
    pub fn fit(&mut self, c: usize, content: f32, pad_x: f32) -> Option<f32> {
        let pad = self.columns.get(c)?.paddings.unwrap_or(pad_x);
        self.resize(c, content + 2.0 * pad)
    }

    // ---- move -------------------------------------------------------------------------

    pub fn can_move(&self, c: usize) -> bool {
        self.opts.col_movable && self.columns.get(c).is_some_and(|c| c.movable)
    }

    /// Gap index (0..=n) for a pointer at `x` (content coordinates): the position after the
    /// last column whose centre is left of the pointer.
    pub fn drop_gap(&self, x: f32) -> usize {
        let mut left = 0.0;
        let centres: Vec<f32> = self
            .widths
            .iter()
            .map(|w| {
                let c = left + w / 2.0;
                left += w;
                c
            })
            .collect();
        gap_from_centres(&centres, x)
    }

    fn region(f: Option<ColumnFixed>) -> u8 {
        match f {
            Some(ColumnFixed::Left) => 0,
            None => 1,
            Some(ColumnFixed::Right) => 2,
        }
    }

    /// Destination index (after removal) for dragging `from` to gap `gap`, or `None` when it
    /// would not move or would cross into another region (left-fixed, scrollable, right-fixed).
    pub fn plan_move(&self, from: usize, gap: usize) -> Option<usize> {
        if !self.can_move(from) || from >= self.columns.len() {
            return None;
        }
        let to = if gap > from { gap - 1 } else { gap }.min(self.columns.len() - 1);
        if to == from {
            return None;
        }
        let mut regions: Vec<u8> = self.columns.iter().map(|c| Self::region(c.fixed)).collect();
        let r = regions.remove(from);
        regions.insert(to, r);
        regions.windows(2).all(|w| w[0] <= w[1]).then_some(to)
    }

    pub fn apply_move(&mut self, from: usize, to: usize) {
        let col = self.columns.remove(from);
        self.columns.insert(to, col);
        let w = self.widths.remove(from);
        self.widths.insert(to, w);
        self.selection = match self.selection {
            TableSelection::Column(c) => TableSelection::Column(remap(c, from, to)),
            TableSelection::Cell(r, c) => TableSelection::Cell(r, remap(c, from, to)),
            s => s,
        };
    }

    // ---- ranges, export, a11y ---------------------------------------------------------

    /// Offer a new visible range; returns it when the host should be told.
    pub fn report_visible(
        &mut self,
        rows: Range<usize>,
        cols: Range<usize>,
    ) -> Option<VisibleRange> {
        let last_rows = self.visible.as_ref().map(|v| &v.rows);
        let last_cols = self.visible.as_ref().map(|v| &v.cols);
        let nr = filter_visible(rows, self.rows, last_rows);
        let nc = filter_visible(cols, self.columns.len(), last_cols);
        if nr.is_none() && nc.is_none() {
            return None;
        }
        let cur = self.visible.clone().unwrap_or_default();
        let v = VisibleRange {
            rows: nr.unwrap_or(cur.rows),
            cols: nc.unwrap_or(cur.cols),
        };
        self.visible = Some(v.clone());
        Some(v)
    }

    pub fn role(&self) -> AccessRole {
        if self.opts.row_selectable || self.opts.cell_selectable {
            AccessRole::Grid
        } else {
            AccessRole::Table
        }
    }

    /// Row count for assistive tech, header rows included.
    pub fn aria_rows(&self) -> usize {
        self.rows + self.header_rows
    }

    pub fn aria_cols(&self) -> usize {
        self.columns.len() + usize::from(self.opts.cell_selectable && self.opts.row_header)
    }

    pub fn view_model(&self, loading: bool) -> TableViewModel {
        let (kind, sel) = match self.selection {
            TableSelection::None => ("none", vec![]),
            TableSelection::Row(r) => ("row", vec![r]),
            TableSelection::Column(c) => ("column", vec![c]),
            TableSelection::Cell(r, c) => ("cell", vec![r, c]),
        };
        let v = self.visible.clone().unwrap_or_default();
        TableViewModel {
            columns: self.columns.iter().map(|c| c.key.clone()).collect(),
            widths: self.widths.iter().map(|w| w.round() as u32).collect(),
            sort: self
                .columns
                .iter()
                .find(|c| matches!(c.sort, ColumnSort::Ascending | ColumnSort::Descending))
                .map(|c| (c.key.clone(), c.sort)),
            selection_kind: kind,
            selection: sel,
            visible_rows: v.rows,
            visible_cols: v.cols,
            row_count: self.rows,
            loading,
            empty: self.rows == 0 && !loading,
        }
    }
}

fn remap(c: usize, from: usize, to: usize) -> usize {
    if c == from {
        to
    } else if from < to && c > from && c <= to {
        c - 1
    } else if to < from && c >= to && c < from {
        c + 1
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(rows: usize, cols: usize) -> TableModel {
        let mut m = TableModel::new(TableOptions::default());
        m.set_columns(
            (0..cols)
                .map(|i| Column::new(format!("c{i}"), format!("C{i}")).sortable())
                .collect(),
        );
        m.set_rows(rows);
        m.page_rows = 5;
        m
    }

    fn sel(m: &TableModel) -> TableSelection {
        m.selection
    }

    #[test]
    fn row_click_selects_and_double_click_reports() {
        let mut m = model(10, 3);
        assert_eq!(
            m.click_row(2, ClickMods::plain()),
            vec![TableEvent::SelectRow(2)]
        );
        assert_eq!(
            m.click_row(3, ClickMods::double()),
            vec![TableEvent::SelectRow(3), TableEvent::DoubleClickedRow(3)]
        );
        assert_eq!(m.selected_row(), Some(3));
        assert_eq!(m.selected_cell(), None);
    }

    #[test]
    fn modifier_click_option() {
        let mut m = model(10, 3);
        m.opts.select_on_modifier_click = false;
        m.click_row(1, ClickMods::plain());
        let ev = m.click_row(
            4,
            ClickMods {
                secondary: true,
                clicks: 1,
                shift: false,
            },
        );
        assert!(ev.is_empty());
        assert_eq!(m.selected_row(), Some(1));
        let ev = m.click_row(
            4,
            ClickMods {
                shift: true,
                clicks: 2,
                secondary: false,
            },
        );
        assert_eq!(ev, vec![TableEvent::DoubleClickedRow(4)]);
        assert_eq!(m.selected_row(), Some(1));
    }

    #[test]
    fn column_mode_and_unselectable_columns() {
        let mut m = model(3, 3);
        m.columns[1].selectable = false;
        assert_eq!(
            m.click_header(0, ClickMods::plain()),
            vec![TableEvent::SelectColumn(0)]
        );
        assert!(m.click_header(1, ClickMods::plain()).is_empty());
        m.nav(NavKey::Right);
        assert_eq!(sel(&m), TableSelection::Column(2));
        m.nav(NavKey::Right);
        assert_eq!(sel(&m), TableSelection::Column(0));
        m.nav(NavKey::End);
        assert_eq!(sel(&m), TableSelection::Column(2));
        m.nav(NavKey::Home);
        assert_eq!(sel(&m), TableSelection::Column(0));
        m.opts.col_selectable = false;
        assert!(m.click_header(2, ClickMods::plain()).is_empty());
    }

    #[test]
    fn cell_mode_clicks_and_escalation() {
        let mut m = model(5, 3);
        m.opts.cell_selectable = true;
        assert_eq!(
            m.click_cell(1, 1, ClickMods::plain()),
            vec![TableEvent::SelectCell(1, 1)]
        );
        // Row header present: second click keeps the cell.
        m.click_cell(1, 1, ClickMods::plain());
        assert_eq!(m.selected_cell(), Some((1, 1)));
        assert_eq!(
            m.click_row_header(2, ClickMods::plain()),
            vec![TableEvent::SelectRow(2)]
        );
        assert_eq!(m.selected_cell(), None);
        // Without a row header the second click escalates, a double click never does.
        m.opts.row_header = false;
        m.click_cell(1, 1, ClickMods::plain());
        assert_eq!(
            m.click_cell(1, 1, ClickMods::plain()),
            vec![TableEvent::SelectRow(1)]
        );
        m.click_cell(2, 2, ClickMods::plain());
        let ev = m.click_cell(2, 2, ClickMods::double());
        assert_eq!(
            ev,
            vec![
                TableEvent::SelectCell(2, 2),
                TableEvent::DoubleClickedCell(2, 2)
            ]
        );
        assert_eq!(m.selected_cell(), Some((2, 2)));
    }

    #[test]
    fn row_keys() {
        let mut m = model(12, 3);
        m.nav(NavKey::Down);
        assert_eq!(sel(&m), TableSelection::Row(0));
        m.nav(NavKey::Up);
        assert_eq!(sel(&m), TableSelection::Row(11)); // wraps
        m.nav(NavKey::Down);
        assert_eq!(sel(&m), TableSelection::Row(0));
        m.nav(NavKey::PageDown);
        assert_eq!(sel(&m), TableSelection::Row(5));
        m.nav(NavKey::PageDown);
        m.nav(NavKey::PageDown);
        assert_eq!(sel(&m), TableSelection::Row(11)); // clamped
        m.nav(NavKey::PageUp);
        assert_eq!(sel(&m), TableSelection::Row(6));
        m.nav(NavKey::Home);
        assert_eq!(sel(&m), TableSelection::Row(0));
        m.nav(NavKey::PageUp);
        assert_eq!(sel(&m), TableSelection::Row(0)); // no wrap
        m.nav(NavKey::End);
        assert_eq!(sel(&m), TableSelection::Row(11));
        assert!(m.nav(NavKey::Left).is_empty());
        m.opts.loop_selection = false;
        m.nav(NavKey::Down);
        assert_eq!(sel(&m), TableSelection::Row(11));
    }

    #[test]
    fn no_selection_move_selects_first_and_empty_ignores() {
        let mut m = model(0, 3);
        assert!(m.nav(NavKey::Down).is_empty());
        let mut m = model(4, 3);
        m.nav(NavKey::PageUp);
        assert_eq!(sel(&m), TableSelection::Row(0));
        let mut m = model(4, 3);
        m.opts.cell_selectable = true;
        m.nav(NavKey::End);
        assert_eq!(sel(&m), TableSelection::Cell(0, 0));
    }

    #[test]
    fn cell_keys() {
        let mut m = model(6, 3);
        m.opts.cell_selectable = true;
        m.nav(NavKey::Down);
        m.nav(NavKey::Right);
        m.nav(NavKey::Down);
        assert_eq!(sel(&m), TableSelection::Cell(1, 1));
        m.nav(NavKey::End);
        assert_eq!(sel(&m), TableSelection::Cell(1, 2));
        m.nav(NavKey::Right);
        assert_eq!(sel(&m), TableSelection::Cell(1, 0)); // wraps
        m.nav(NavKey::Left);
        assert_eq!(sel(&m), TableSelection::Cell(1, 2));
        m.nav(NavKey::Home);
        assert_eq!(sel(&m), TableSelection::Cell(1, 0));
        m.nav(NavKey::PageDown);
        assert_eq!(sel(&m), TableSelection::Cell(5, 0));
        m.nav(NavKey::PageUp);
        assert_eq!(sel(&m), TableSelection::Cell(0, 0));
        m.nav(NavKey::Up);
        assert_eq!(sel(&m), TableSelection::Cell(5, 0));
    }

    #[test]
    fn escape_clears_or_propagates() {
        let mut m = model(3, 2);
        assert_eq!(m.escape(), (false, vec![]));
        m.click_row(1, ClickMods::plain());
        assert_eq!(m.escape(), (true, vec![TableEvent::ClearSelection]));
        assert_eq!(m.selection, TableSelection::None);
    }

    #[test]
    fn right_click_targets_and_clearing() {
        let mut m = model(3, 2);
        m.opts.cell_selectable = true;
        m.right_click_row(Some(1));
        assert_eq!(m.right_row, Some(1));
        m.right_click_cell(0, 1);
        assert_eq!((m.right_row, m.right_cell), (None, Some((0, 1))));
        m.right_click_row(Some(2));
        assert_eq!(m.right_cell, None);
        let ev = m.set_selection(TableSelection::Row(0));
        assert_eq!(
            ev,
            vec![TableEvent::RightClickedRow(None), TableEvent::SelectRow(0)]
        );
    }

    #[test]
    fn sort_cycles_and_single_column() {
        let mut m = model(3, 3);
        assert_eq!(m.cycle_sort(0), Some((0, ColumnSort::Descending)));
        assert_eq!(m.cycle_sort(1), Some((1, ColumnSort::Descending)));
        assert_eq!(m.columns[0].sort, ColumnSort::Default);
        assert_eq!(m.cycle_sort(1), Some((1, ColumnSort::Ascending)));
        assert_eq!(m.cycle_sort(1), Some((1, ColumnSort::Default)));
        m.columns[2].sort = ColumnSort::None;
        assert_eq!(m.cycle_sort(2), None);
        m.opts.sortable = false;
        assert_eq!(m.cycle_sort(0), None);
    }

    #[test]
    fn resize_clamps_and_fit() {
        let mut m = model(1, 2);
        m.columns[0] = Column::new("a", "A").min_width(50.0).max_width(200.0);
        m.widths[0] = 100.0;
        assert_eq!(m.resize(0, 10.0), Some(50.0));
        assert_eq!(m.resize(0, 900.0), Some(200.0));
        assert_eq!(m.fit(0, 120.0, 8.0), Some(136.0));
        assert_eq!(m.fit(0, 1000.0, 8.0), Some(200.0));
        m.columns[1].resizable = false;
        assert_eq!(m.resize(1, 40.0), None);
        m.opts.col_resizable = false;
        assert_eq!(m.resize(0, 40.0), None);
    }

    #[test]
    fn column_bounds_setters() {
        let c = Column::new("a", "A").min_width(150.0);
        assert_eq!(c.width, 150.0);
        let c = Column::new("a", "A").width(500.0).max_width(300.0);
        assert_eq!(c.width, 300.0);
        let c = Column::new("a", "A").max_width(300.0).width(500.0);
        assert_eq!(c.width, 500.0); // not limited by a later initial width
    }

    #[test]
    fn move_gap_and_regions() {
        let mut m = model(1, 5);
        for w in m.widths.iter_mut() {
            *w = 100.0;
        }
        assert_eq!(m.drop_gap(10.0), 0);
        assert_eq!(m.drop_gap(140.0), 1);
        assert_eq!(m.drop_gap(1000.0), 5);
        assert_eq!(m.plan_move(0, 3), Some(2));
        assert_eq!(m.plan_move(3, 1), Some(1));
        assert_eq!(m.plan_move(2, 2), None); // same place
        assert_eq!(m.plan_move(2, 3), None);
        m.columns[0].fixed = Some(ColumnFixed::Left);
        m.columns[4].fixed = Some(ColumnFixed::Right);
        assert_eq!(m.plan_move(2, 0), None); // into the left-fixed region
        assert_eq!(m.plan_move(2, 5), None); // into the right-fixed region
        assert_eq!(m.plan_move(2, 4), Some(3));
        assert_eq!(m.plan_move(0, 3), None); // fixed column out of its region
        m.columns[2].movable = false;
        assert_eq!(m.plan_move(2, 4), None);
        m.apply_move(1, 3);
        assert_eq!(
            m.columns.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(),
            ["c0", "c2", "c3", "c1", "c4"]
        );
    }

    #[test]
    fn move_remaps_column_selection() {
        let mut m = model(2, 4);
        m.set_selection(TableSelection::Column(2));
        m.apply_move(0, 3);
        assert_eq!(m.selection, TableSelection::Column(1));
        m.apply_move(1, 0);
        assert_eq!(m.selection, TableSelection::Column(0));
    }

    #[test]
    fn visible_range_rules() {
        assert_eq!(filter_visible(0..0, 0, None), Some(0..0));
        assert_eq!(filter_visible(0..0, 0, Some(&(0..0))), None);
        assert_eq!(filter_visible(3..4, 100, None), None); // len 1 ignored
        assert_eq!(filter_visible(0..1, 1, None), Some(0..1)); // at most one item
        assert_eq!(filter_visible(120..120, 100, None), None); // stale
        assert_eq!(filter_visible(90..130, 100, None), Some(90..100)); // clamped
        assert_eq!(filter_visible(0..10, 100, Some(&(0..10))), None);
        let mut m = model(100, 4);
        let v = m.report_visible(0..20, 0..4).unwrap();
        assert_eq!((v.rows, v.cols), (0..20, 0..4));
        assert!(m.report_visible(0..20, 0..4).is_none());
        assert_eq!(m.report_visible(5..25, 0..4).unwrap().rows, 5..25);
    }

    #[test]
    fn load_more_threshold() {
        assert!(!needs_load_more(70, 100, false, 20));
        assert!(!needs_load_more(79, 100, true, 20));
        assert!(needs_load_more(80, 100, true, 20));
        assert!(!needs_load_more(0, 0, true, 20));
        assert!(needs_load_more(5, 10, true, 20));
    }

    #[test]
    fn dump_range_clamps() {
        assert_eq!(clamp_range(5..20, 10), 5..10);
        assert_eq!(clamp_range(15..20, 10), 10..10);
        assert_eq!(clamp_range(0..3, 10), 0..3);
    }

    #[test]
    fn accessibility_counts_and_role() {
        let mut m = model(10, 3);
        assert_eq!(m.role(), AccessRole::Grid);
        assert_eq!((m.aria_rows(), m.aria_cols()), (11, 3));
        m.header_rows = 2;
        m.opts.cell_selectable = true;
        assert_eq!((m.aria_rows(), m.aria_cols()), (12, 4));
        m.opts.row_selectable = false;
        m.opts.cell_selectable = false;
        assert_eq!(m.role(), AccessRole::Table);
    }

    #[test]
    fn view_model_reports_state() {
        let mut m = model(0, 2);
        assert!(m.view_model(false).empty);
        assert!(!m.view_model(true).empty);
        m.set_rows(5);
        m.set_selection(TableSelection::Cell(1, 1));
        m.cycle_sort(0);
        let v = m.view_model(false);
        assert_eq!(
            (v.selection_kind, v.selection.clone()),
            ("cell", vec![1, 1])
        );
        assert_eq!(v.sort, Some(("c0".to_string(), ColumnSort::Descending)));
    }
}
