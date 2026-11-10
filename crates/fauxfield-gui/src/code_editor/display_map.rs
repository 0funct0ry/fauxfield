//! Offset <-> (row, column) mapping with soft wrap and tabs. Monospace: a column is one cell.
//! GPUI-free.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// One display row: a slice of one logical line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub start: usize,
    pub end: usize,
    /// True for the first display row of a logical line (the gutter shows its number).
    pub first: bool,
    pub line: usize,
}

pub struct DisplayMap {
    pub rows: Vec<Row>,
    tab: usize,
}

/// Cells a grapheme occupies when it starts at cell `col`.
fn cells(g: &str, col: usize, tab: usize) -> usize {
    if g == "\t" {
        tab - col % tab
    } else {
        UnicodeWidthStr::width(g).max(1)
    }
}

impl DisplayMap {
    /// `wrap` is the number of cells per row, or `None` for no wrapping.
    pub fn new(text: &str, tab: usize, wrap: Option<usize>) -> DisplayMap {
        let tab = tab.max(1);
        let mut rows = Vec::new();
        let mut off = 0;
        for (line, l) in text.split('\n').enumerate() {
            match wrap {
                None => rows.push(Row {
                    start: off,
                    end: off + l.len(),
                    first: true,
                    line,
                }),
                Some(w) => {
                    let w = w.max(1);
                    let (mut start, mut col, mut first) = (off, 0usize, true);
                    for (i, g) in l.grapheme_indices(true) {
                        let c = cells(g, col, tab);
                        if col + c > w && col > 0 {
                            rows.push(Row {
                                start,
                                end: off + i,
                                first,
                                line,
                            });
                            start = off + i;
                            col = 0;
                            first = false;
                        }
                        col += cells(g, col, tab);
                    }
                    rows.push(Row {
                        start,
                        end: off + l.len(),
                        first,
                        line,
                    });
                }
            }
            off += l.len() + 1;
        }
        DisplayMap { rows, tab }
    }

    /// True when this map was built for text of `len` bytes (a layout from before an edit is not).
    pub fn is_for_len(&self, len: usize) -> bool {
        self.rows.last().is_none_or(|r| r.end == len)
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Display row containing `offset` (the caret at a wrap point belongs to the later row).
    pub fn row_of(&self, offset: usize) -> usize {
        let i = self.rows.partition_point(|r| r.start <= offset);
        i.saturating_sub(1)
    }

    /// Cell column of `offset` within its display row.
    pub fn col_of(&self, text: &str, offset: usize) -> usize {
        let r = &self.rows[self.row_of(offset)];
        let mut col = 0;
        for (i, g) in text
            .get(r.start..r.end)
            .unwrap_or("")
            .grapheme_indices(true)
        {
            if r.start + i >= offset {
                break;
            }
            col += cells(g, col, self.tab);
        }
        col
    }

    /// Offset for a cell column in a row; clicks past the end land at the row end, and a click in
    /// the right half of a wide cell lands after it.
    pub fn offset_at(&self, text: &str, row: usize, col: usize) -> usize {
        let r = &self.rows[row.min(self.rows.len() - 1)];
        let mut c = 0;
        for (i, g) in text
            .get(r.start..r.end)
            .unwrap_or("")
            .grapheme_indices(true)
        {
            let w = cells(g, c, self.tab);
            if col < c + w.div_ceil(2) {
                return r.start + i;
            }
            c += w;
            if col < c {
                return r.start + i + g.len();
            }
        }
        r.end
    }

    pub fn row_width(&self, text: &str, row: usize) -> usize {
        let r = &self.rows[row];
        self.col_of(text, r.end).max(self.col_after(text, r))
    }

    fn col_after(&self, text: &str, r: &Row) -> usize {
        let mut c = 0;
        for g in text.get(r.start..r.end).unwrap_or("").graphemes(true) {
            c += cells(g, c, self.tab);
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_wrap_rows_match_lines() {
        let m = DisplayMap::new("ab\n\ncde", 4, None);
        assert_eq!(m.row_count(), 3);
        assert_eq!(m.row_of(3), 1);
        assert_eq!(m.row_of(7), 2);
    }

    #[test]
    fn soft_wrap_splits_long_lines() {
        let m = DisplayMap::new("abcdefgh", 4, Some(3));
        assert_eq!(m.row_count(), 3);
        assert!(!m.rows[1].first);
        assert_eq!(m.rows[1].start, 3);
        assert_eq!(m.row_of(3), 1);
    }

    #[test]
    fn tabs_expand_to_stops() {
        let t = "\tx";
        let m = DisplayMap::new(t, 4, None);
        assert_eq!(m.col_of(t, 1), 4);
        assert_eq!(m.offset_at(t, 0, 4), 1);
        assert_eq!(m.offset_at(t, 0, 1), 0);
        assert_eq!(m.offset_at(t, 0, 3), 1);
    }

    #[test]
    fn wide_characters_take_two_cells() {
        let t = "a日b";
        let m = DisplayMap::new(t, 4, None);
        assert_eq!(m.col_of(t, 1), 1);
        assert_eq!(m.col_of(t, 4), 3);
        assert_eq!(m.offset_at(t, 0, 1), 1);
        assert_eq!(m.offset_at(t, 0, 2), 4);
        assert_eq!(m.offset_at(t, 0, 3), 4);
        assert_eq!(m.offset_at(t, 0, 99), t.len());
    }

    #[test]
    fn multibyte_is_not_split() {
        let t = "é\u{301}x";
        let m = DisplayMap::new(t, 4, None);
        assert_eq!(m.offset_at(t, 0, 1), 4);
    }
}
