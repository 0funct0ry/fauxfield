//! Custom element: paints only the rows in view (gutter, active line, selection, guides,
//! whitespace, bracket box, text, caret, scrollbar) and registers the input handler.

use std::ops::Range;
use std::rc::Rc;

use gpui::{
    App, Bounds, Element, ElementId, ElementInputHandler, Entity, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, LayoutId, PaintQuad, Pixels, ShapedLine, SharedString, Style,
    TextRun, UnderlineStyle, Window, fill, point, px, relative, size,
};

use super::display_map::DisplayMap;
use super::entity::{CodeEditor, Layout};
use super::highlight::{Span, Tok, var_name};
use super::ops;
use crate::assets::BUFFER_FONT;
use crate::theme::{Theme, theme};

pub const FONT_SIZE: f32 = 13.0;
pub const LINE_H: f32 = 20.0;
const PAD_R: f32 = 12.0;

pub struct EditorElement {
    editor: Entity<CodeEditor>,
}

impl EditorElement {
    pub fn new(editor: Entity<CodeEditor>) -> EditorElement {
        EditorElement { editor }
    }
}

impl IntoElement for EditorElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

struct RowPaint {
    y: f32,
    line: Option<ShapedLine>,
    number: Option<ShapedLine>,
    active: bool,
}

pub struct Prepaint {
    rows: Vec<RowPaint>,
    quads: Vec<PaintQuad>,
    over: Vec<PaintQuad>,
    caret: Option<PaintQuad>,
    scrollbar: Option<PaintQuad>,
    gutter_bar: Vec<PaintQuad>,
    text_x: f32,
}

type Colour = Hsla;

fn tok_color(t: &Theme, k: Tok) -> Colour {
    match k {
        Tok::Key | Tok::Attribute => t.syntax_property,
        Tok::String => t.syntax_string,
        Tok::Number => t.syntax_number,
        Tok::Constant => t.syntax_constant,
        Tok::Comment => t.syntax_comment,
        Tok::Tag => t.syntax_type,
        Tok::Heading => t.syntax_keyword,
        Tok::Var => t.syntax_function,
    }
}

/// Expands tabs for display and maps source byte offsets to display byte offsets.
fn expand(src: &str, tab: usize) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(src.len());
    let mut map = Vec::with_capacity(src.len() + 1);
    let mut col = 0;
    for (i, c) in src.char_indices() {
        while map.len() <= i {
            map.push(out.len());
        }
        if c == '\t' {
            let n = tab - col % tab;
            out.push_str(&" ".repeat(n));
            col += n;
        } else {
            out.push(c);
            col += unicode_width::UnicodeWidthChar::width(c)
                .unwrap_or(1)
                .max(1);
        }
    }
    while map.len() <= src.len() {
        map.push(out.len());
    }
    (out, map)
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepaint {
        let t = theme(cx).clone();
        let focused = self.editor.read(cx).focus.is_focused(window);
        let font = gpui::font(BUFFER_FONT);
        let fsize = px(FONT_SIZE);
        // One monospace cell, measured once per frame from a shaped reference glyph.
        let cell_w = {
            let run = TextRun {
                len: 1,
                font: font.clone(),
                color: t.text,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            f32::from(
                window
                    .text_system()
                    .shape_line("M".into(), fsize, &[run], None)
                    .width,
            )
        };
        let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let rows_visible = ((h / LINE_H).ceil() as usize).max(1);

        self.editor.update(cx, |ed, _cx| {
            ed.sync_highlight();
            let digits = ed.buf.line_count().to_string().len().max(2);
            let gutter_w = if ed.opts.line_numbers {
                (digits as f32 + 2.0) * cell_w
            } else {
                8.0
            };
            let text_x = gutter_w + 6.0;
            let wrap_cols = ed
                .opts
                .soft_wrap
                .then(|| (((w - text_x - PAD_R) / cell_w).floor() as usize).max(4));
            let dm = ed.display_map(wrap_cols);
            ed.layout = Some(Layout {
                bounds,
                cell_w,
                line_h: LINE_H,
                text_x,
                rows_visible,
                wrap_cols,
                dm: dm.clone(),
            });

            let text = ed.buf.text().to_string();
            let head = ed.buf.head();
            let head_row = dm.row_of(head);
            let head_col = dm.col_of(&text, head);
            // Scroll-into-view: keep the caret 3 rows inside the viewport.
            if ed.reveal {
                let margin = 3.0f32.min((rows_visible as f32 / 2.0).floor());
                let r = head_row as f32;
                if r < ed.scroll_row + margin {
                    ed.scroll_row = (r - margin).max(0.0);
                } else if r > ed.scroll_row + rows_visible as f32 - 1.0 - margin {
                    ed.scroll_row = r - (rows_visible as f32 - 1.0 - margin);
                }
                if wrap_cols.is_none() {
                    let cx_px = head_col as f32 * cell_w;
                    let view_w = w - text_x - PAD_R;
                    if cx_px < ed.scroll_x {
                        ed.scroll_x = cx_px;
                    } else if cx_px + cell_w > ed.scroll_x + view_w {
                        ed.scroll_x = cx_px + cell_w - view_w;
                    }
                }
                ed.reveal = false;
            }
            ed.clamp_scroll();
            let first = ed.scroll_row.floor() as usize;
            let frac = ed.scroll_row - first as f32;
            let last = (first + rows_visible + 1).min(dm.row_count());
            let vis_start = dm.rows[first.min(dm.row_count() - 1)].start;
            let vis_end = dm.rows[last.saturating_sub(1).min(dm.row_count() - 1)].end;
            let spans: Vec<Span> = ed.hl.spans(&text, vis_start..vis_end);
            let sel = ed.buf.selection();
            let bracket = if ed.buf.has_selection() {
                None
            } else {
                ops::matching_bracket(&text, ed.lang, head)
            };
            let tab = ed.opts.tab_size;
            let mut rows = Vec::new();
            let mut quads = Vec::new();
            let mut over = Vec::new();
            let mut gutter_bar = Vec::new();
            let x0 = bounds.origin.x;
            let y0 = bounds.origin.y;
            let sx = ed.scroll_x;
            let pos = |col: usize| x0 + px(text_x + col as f32 * cell_w - sx);
            let sel_color = if focused {
                t.element_selected
            } else {
                t.element_selected.opacity(0.5)
            };
            let ro_ranges = ed.read_only_ranges.clone();
            let mut ws_guard = 0usize;
            for r in first..last {
                let row = &dm.rows[r];
                let y = (r as f32 - first as f32 - frac) * LINE_H;
                let ry = y0 + px(y);
                let src = &text[row.start..row.end];
                let is_active = r == head_row && !ed.buf.has_selection();
                if is_active && focused {
                    quads.push(fill(
                        Bounds::new(point(x0, ry), size(bounds.size.width, px(LINE_H))),
                        t.editor_active_line,
                    ));
                }
                // Selection.
                if sel.start < row.end + 1 && sel.end > row.start && !sel.is_empty() {
                    let a = sel.start.max(row.start);
                    let b = sel.end.min(row.end);
                    let (ca, mut cb) = (dm.col_of(&text, a), dm.col_of(&text, b));
                    if sel.end > row.end
                        && r + 1 < dm.row_count()
                        && dm.rows[r + 1].line != row.line
                        || sel.end > row.end && r + 1 == dm.row_count()
                    {
                        cb += 1;
                    }
                    if cb > ca {
                        quads.push(fill(
                            Bounds::new(
                                point(pos(ca), ry),
                                size(px((cb - ca) as f32 * cell_w), px(LINE_H)),
                            ),
                            sel_color,
                        ));
                    }
                }
                // Bracket match box.
                if let Some((a, b)) = bracket {
                    for o in [a, b] {
                        if o >= row.start && o < row.end.max(row.start + 1) && o < row.end {
                            let c = dm.col_of(&text, o);
                            quads.push(fill(
                                Bounds::new(point(pos(c), ry), size(px(cell_w), px(LINE_H))),
                                t.editor_bracket_match,
                            ));
                        }
                    }
                }
                // Indent guides.
                if ed.opts.indent_guides && row.first {
                    let lead = src.len() - src.trim_start_matches([' ', '\t']).len();
                    let (_, _) = (lead, 0);
                    let cols = dm.col_of(&text, row.start + lead);
                    let blank = src.trim().is_empty();
                    let mut c = 0;
                    while !blank && c + tab <= cols.max(0) && c + tab < cols + 1 && c < cols {
                        quads.push(fill(
                            Bounds::new(point(pos(c), ry), size(px(1.), px(LINE_H))),
                            t.editor_indent_guide,
                        ));
                        c += tab;
                    }
                }
                // Read-only gutter bar.
                if ro_ranges
                    .iter()
                    .any(|ro| ro.start < row.end.max(row.start + 1) && row.start < ro.end)
                {
                    gutter_bar.push(fill(
                        Bounds::new(point(x0, ry), size(px(2.), px(LINE_H))),
                        t.border_focused,
                    ));
                }
                // Text with highlight runs.
                let (disp, map) = expand(src, tab);
                let mut runs: Vec<TextRun> = Vec::new();
                let base_font = font.clone();
                let mut cursor = 0usize;
                let marked = ed.marked.clone();
                let push = |runs: &mut Vec<TextRun>,
                            from: usize,
                            to: usize,
                            color: Hsla,
                            underline: bool,
                            wavy: bool| {
                    if to > from {
                        runs.push(TextRun {
                            len: map[to] - map[from],
                            font: base_font.clone(),
                            color,
                            background_color: None,
                            underline: underline.then_some(UnderlineStyle {
                                color: Some(color),
                                thickness: px(1.),
                                wavy,
                            }),
                            strikethrough: None,
                        });
                    }
                };
                let muted = |off: usize| ro_ranges.iter().any(|ro| ro.contains(&off));
                let mut boundaries: Vec<(Range<usize>, Hsla, bool)> = Vec::new();
                for s in spans
                    .iter()
                    .filter(|s| s.range.end > row.start && s.range.start < row.end)
                {
                    let (a, b) = (
                        s.range.start.max(row.start) - row.start,
                        s.range.end.min(row.end) - row.start,
                    );
                    let mut color = tok_color(&t, s.tok);
                    let mut underline = false;
                    if s.tok == Tok::Var {
                        let name = var_name(&text[s.range.clone()]);
                        let ok = ed.var_check.as_ref().is_none_or(|f| f(name));
                        if !ok {
                            color = t.deleted;
                            underline = true;
                        } else {
                            color = t.accent;
                        }
                    }
                    boundaries.push((a..b, color, underline));
                }
                for (rg, color, ul) in boundaries {
                    if rg.start < cursor {
                        continue;
                    }
                    let plain = if muted(row.start + cursor) {
                        t.text_muted
                    } else {
                        t.text
                    };
                    push(&mut runs, cursor, rg.start, plain, false, false);
                    push(
                        &mut runs,
                        rg.start,
                        rg.end,
                        if muted(row.start + rg.start) {
                            t.text_muted
                        } else {
                            color
                        },
                        ul,
                        false,
                    );
                    cursor = rg.end;
                }
                let plain = if muted(row.start + cursor) {
                    t.text_muted
                } else {
                    t.text
                };
                push(&mut runs, cursor, src.len(), plain, false, false);
                // IME marked range: underline.
                if let Some(m) = marked
                    && m.start < row.end
                    && m.end > row.start
                {
                    let (a, b) = (
                        m.start.max(row.start) - row.start,
                        m.end.min(row.end) - row.start,
                    );
                    let (da, db) = (map[a], map[b]);
                    let mut acc = 0;
                    let mut split: Vec<TextRun> = Vec::new();
                    for run in runs.drain(..) {
                        let (s, e) = (acc, acc + run.len);
                        acc = e;
                        let cuts = [s, da.clamp(s, e), db.clamp(s, e), e];
                        for w in cuts.windows(2) {
                            if w[1] > w[0] {
                                let inside = w[0] >= da && w[1] <= db;
                                let mut rr = run.clone();
                                rr.len = w[1] - w[0];
                                if inside {
                                    rr.underline = Some(UnderlineStyle {
                                        color: Some(run.color),
                                        thickness: px(1.),
                                        wavy: false,
                                    });
                                }
                                split.push(rr);
                            }
                        }
                    }
                    runs = split;
                }
                let line = (!disp.is_empty()).then(|| {
                    window
                        .text_system()
                        .shape_line(SharedString::from(disp), fsize, &runs, None)
                });
                // Whitespace markers.
                if ed.opts.show_whitespace && ws_guard < 4000 {
                    let mut c = 0;
                    for ch in src.chars() {
                        let wdt = if ch == '\t' { tab - c % tab } else { 1 };
                        if ch == ' ' || ch == '\t' {
                            ws_guard += 1;
                            let mid = pos(c) + px(if ch == ' ' { cell_w / 2.0 - 1.0 } else { 2.0 });
                            let wid = if ch == ' ' {
                                2.0
                            } else {
                                (wdt as f32 * cell_w - 6.0).max(2.0)
                            };
                            over.push(fill(
                                Bounds::new(
                                    point(mid, ry + px(LINE_H / 2.0)),
                                    size(px(wid), px(1.)),
                                ),
                                t.editor_invisible,
                            ));
                        }
                        c += wdt;
                    }
                }
                let number = (ed.opts.line_numbers && row.first).then(|| {
                    let s = (row.line + 1).to_string();
                    let color = if r == head_row {
                        t.text
                    } else {
                        t.text_placeholder
                    };
                    let run = TextRun {
                        len: s.len(),
                        font: font.clone(),
                        color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    window
                        .text_system()
                        .shape_line(SharedString::from(s), fsize, &[run], None)
                });
                rows.push(RowPaint {
                    y,
                    line,
                    number,
                    active: r == head_row,
                });
            }
            // Caret.
            let caret = (focused && ed.caret_on).then(|| {
                let y = (head_row as f32 - ed.scroll_row) * LINE_H;
                fill(
                    Bounds::new(
                        point(pos(head_col), y0 + px(y + 2.)),
                        size(px(1.5), px(LINE_H - 4.)),
                    ),
                    t.accent,
                )
            });
            // Overlay scrollbar.
            let total = dm.row_count() as f32;
            let scrollbar = (total > rows_visible as f32).then(|| {
                let thumb = (rows_visible as f32 / total * h).max(24.0);
                let top = ed.scroll_row / (total - rows_visible as f32).max(1.0) * (h - thumb);
                fill(
                    Bounds::new(
                        point(
                            x0 + bounds.size.width - px(6.),
                            y0 + px(top.clamp(0.0, h - thumb)),
                        ),
                        size(px(4.), px(thumb)),
                    ),
                    t.border.opacity(0.7),
                )
            });
            Prepaint {
                rows,
                quads,
                over,
                caret,
                scrollbar,
                gutter_bar,
                text_x,
            }
        })
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        pre: &mut Prepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.editor.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.editor.clone()),
            cx,
        );
        let sx = self.editor.read(cx).scroll_x;
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            for q in pre.quads.drain(..) {
                window.paint_quad(q);
            }
            for q in pre.gutter_bar.drain(..) {
                window.paint_quad(q);
            }
            for r in pre.rows.drain(..) {
                if let Some(n) = &r.number {
                    let x = bounds.origin.x + px(pre.text_x - 14.0) - n.width;
                    let _ = n.paint(point(x, bounds.origin.y + px(r.y)), px(LINE_H), window, cx);
                }
                if let Some(l) = &r.line {
                    let _ = l.paint(
                        point(
                            bounds.origin.x + px(pre.text_x - sx),
                            bounds.origin.y + px(r.y),
                        ),
                        px(LINE_H),
                        window,
                        cx,
                    );
                }
                let _ = r.active;
            }
            for q in pre.over.drain(..) {
                window.paint_quad(q);
            }
            if let Some(c) = pre.caret.take() {
                window.paint_quad(c);
            }
            if let Some(s) = pre.scrollbar.take() {
                window.paint_quad(s);
            }
        });
        let _: Option<Rc<DisplayMap>> = None;
    }
}
