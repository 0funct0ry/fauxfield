//! The reusable DatePicker (M10.07), modelled on the gpui-kit DatePicker and Calendar:
//!
//! * a trigger with the ComboBox field look (same height, border, focus colour) showing the
//!   date as `%Y/%m/%d` or a placeholder, a calendar icon, and a clear button (`cleanable`);
//!   typing does nothing;
//! * a popup drawn with `deferred` + `anchored` 6 px under the trigger (left edges aligned),
//!   224 px wide plus 12 px padding, with a calendar: previous and next buttons, a month and a
//!   year toggle that switch to a 3x4 month grid or a 5x4 page of 20 years (clicking the same
//!   toggle returns to days), a weekday row starting on Sunday, and 32 px day cells for as
//!   many weeks as the month needs; days of the next and previous month are muted and still
//!   pick (and move the view);
//! * day colours: selected = focus colour filled, today = selected-element background,
//!   hover = hover background; disabled days are not used here;
//! * keyboard: the trigger is a tab stop; Enter toggles the popup, Esc closes it, Delete or
//!   Backspace clears; a press outside closes it;
//! * events: [`DatePickerEvent::Change`] on pick and clear, `Open` and `Close`.
//!
//! Not included (not needed yet): range mode, presets, several months, time editing.

use std::cell::Cell;
use std::rc::Rc;

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use gpui::{
    AnyElement, Bounds, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, ParentElement, Pixels, Render,
    SharedString, StatefulInteractiveElement, Styled, Window, anchored, canvas, deferred, div,
    point, prelude::*, px,
};

use crate::theme::{Theme, theme};
use crate::ui::icon;

const POPUP_GAP: f32 = 6.0;
const CELL: f32 = 32.0;
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const WEEKDAYS: [&str; 7] = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];
/// Years per page of the year view (5 columns x 4 rows).
const YEAR_PAGE: i32 = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DatePickerEvent {
    Change(Option<NaiveDate>),
    Open,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalendarView {
    Day,
    Month,
    Year,
}

// ---- pure calendar model -------------------------------------------------------------------

/// The days the month grid shows for `year`/`month`, whole weeks starting on `first`
/// (4 to 6 weeks, as many as the month needs).
pub fn month_grid(year: i32, month: u32, first: Weekday) -> Vec<NaiveDate> {
    let Some(start) = NaiveDate::from_ymd_opt(year, month, 1) else {
        return vec![];
    };
    let lead = (start.weekday().num_days_from_sunday() + 7 - first.num_days_from_sunday()) % 7;
    let first_cell = start - Duration::days(i64::from(lead));
    let days = days_in_month(year, month);
    let weeks = (lead + days).div_ceil(7);
    (0..weeks * 7)
        .map(|i| first_cell + Duration::days(i64::from(i)))
        .collect()
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(ny, nm, 1)
        .and_then(|d| d.pred_opt())
        .map_or(30, |d| d.day())
}

/// `(year, month)` moved by `delta` months.
pub fn add_months(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let m0 = year * 12 + month as i32 - 1 + delta;
    (m0.div_euclid(12), (m0.rem_euclid(12) + 1) as u32)
}

/// First year of the 20-year page that holds `year`, counted from `range_start`.
pub fn year_page_start(range_start: i32, year: i32) -> i32 {
    range_start + (year - range_start).div_euclid(YEAR_PAGE) * YEAR_PAGE
}

// ---- the entity ------------------------------------------------------------------------------

pub struct DatePicker {
    id: SharedString,
    focus: FocusHandle,
    value: Option<NaiveDate>,
    open: bool,
    view: CalendarView,
    /// Month on screen in the day view.
    shown: (i32, u32),
    /// First year of the page on screen in the year view.
    page: i32,
    today: NaiveDate,
    /// Years offered, half-open (`today - 50` to `today + 50` by default).
    year_range: (i32, i32),
    first_day: Weekday,
    placeholder: SharedString,
    format: &'static str,
    cleanable: bool,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl EventEmitter<DatePickerEvent> for DatePicker {}

impl Focusable for DatePicker {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl DatePicker {
    pub fn new(id: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        let today = chrono::Local::now().date_naive();
        DatePicker {
            id: id.into(),
            focus: cx.focus_handle(),
            value: None,
            open: false,
            view: CalendarView::Day,
            shown: (today.year(), today.month()),
            page: year_page_start(today.year() - 50, today.year()),
            today,
            year_range: (today.year() - 50, today.year() + 50),
            first_day: Weekday::Sun,
            placeholder: "Select a date".into(),
            format: "%Y/%m/%d",
            cleanable: false,
            bounds: Rc::new(Cell::new(None)),
        }
    }

    pub fn placeholder(mut self, p: impl Into<SharedString>) -> Self {
        self.placeholder = p.into();
        self
    }

    pub fn cleanable(mut self, on: bool) -> Self {
        self.cleanable = on;
        self
    }

    pub fn first_day_of_week(mut self, d: Weekday) -> Self {
        self.first_day = d;
        self
    }

    pub fn year_range(mut self, start: i32, end: i32) -> Self {
        self.year_range = (start, end.max(start + 1));
        self
    }

    pub fn date(mut self, d: Option<NaiveDate>) -> Self {
        self.set_date(d);
        self
    }

    pub fn id(&self) -> &SharedString {
        &self.id
    }

    pub fn value(&self) -> Option<NaiveDate> {
        self.value
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn view(&self) -> CalendarView {
        self.view
    }

    pub fn shown_month(&self) -> (i32, u32) {
        self.shown
    }

    /// Set the date without an event (programmatic).
    pub fn set_date(&mut self, d: Option<NaiveDate>) {
        self.value = d;
        if let Some(d) = d {
            self.shown = (d.year(), d.month());
        }
        self.open = false;
    }

    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            return;
        }
        self.open = true;
        self.view = CalendarView::Day;
        let base = self.value.unwrap_or(self.today);
        self.shown = (base.year(), base.month());
        self.page = year_page_start(self.year_range.0, base.year());
        self.focus.focus(window);
        cx.emit(DatePickerEvent::Open);
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            cx.emit(DatePickerEvent::Close);
            cx.notify();
        }
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.close(cx);
        } else {
            self.open(window, cx);
        }
    }

    /// Pick a day: the value changes, the popup closes, `Change` is emitted.
    pub fn pick(&mut self, d: NaiveDate, cx: &mut Context<Self>) {
        self.value = Some(d);
        self.shown = (d.year(), d.month());
        self.close(cx);
        cx.emit(DatePickerEvent::Change(Some(d)));
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if self.value.take().is_some() {
            cx.emit(DatePickerEvent::Change(None));
        }
        cx.notify();
    }

    /// Previous (`-1`) or next (`1`): a month in the day view, a page in the year view.
    pub fn step(&mut self, delta: i32, cx: &mut Context<Self>) {
        match self.view {
            CalendarView::Day => self.shown = add_months(self.shown.0, self.shown.1, delta),
            CalendarView::Year => {
                let next = self.page + delta * YEAR_PAGE;
                if next + YEAR_PAGE > self.year_range.0 && next < self.year_range.1 {
                    self.page = next;
                }
            }
            CalendarView::Month => {}
        }
        cx.notify();
    }

    /// The month or year toggle: opens that view, or returns to days when it is open.
    pub fn toggle_view(&mut self, v: CalendarView, cx: &mut Context<Self>) {
        self.view = if self.view == v { CalendarView::Day } else { v };
        if self.view == CalendarView::Year {
            self.page = year_page_start(self.year_range.0, self.shown.0);
        }
        cx.notify();
    }

    pub fn choose_month(&mut self, m: u32, cx: &mut Context<Self>) {
        self.shown.1 = m;
        self.view = CalendarView::Day;
        cx.notify();
    }

    pub fn choose_year(&mut self, y: i32, cx: &mut Context<Self>) {
        self.shown.0 = y;
        self.view = CalendarView::Day;
        cx.notify();
    }

    fn handle_key(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let k = &ev.keystroke;
        if k.modifiers.platform || k.modifiers.control || k.modifiers.alt {
            return false;
        }
        match k.key.as_str() {
            "enter" | "space" => self.toggle(window, cx),
            "escape" if self.open => self.close(cx),
            "backspace" | "delete" => self.clear(cx),
            "tab" => {
                self.close(cx);
                if k.modifiers.shift {
                    window.focus_prev();
                } else {
                    window.focus_next();
                }
            }
            _ => return false,
        }
        true
    }

    fn mouse_out(&mut self, ev: &MouseDownEvent, cx: &mut Context<Self>) {
        // A press on the trigger is its own toggle.
        if self.bounds.get().is_some_and(|b| b.contains(&ev.position)) {
            return;
        }
        self.close(cx);
    }
}

// ---- render ------------------------------------------------------------------------------------

impl Render for DatePicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        if self.open && !self.focus.contains_focused(window, cx) {
            self.open = false;
            cx.emit(DatePickerEvent::Close);
        }
        let focused = self.focus.is_focused(window);
        let (label, color) = match self.value {
            Some(d) => (d.format(self.format).to_string(), t.text),
            None => (self.placeholder.to_string(), t.text_placeholder),
        };
        let id = self.id.clone();
        let bounds = self.bounds.clone();
        let clear = (self.cleanable && self.value.is_some()).then(|| {
            div()
                .id(SharedString::from(format!("{id}-clear")))
                .flex_none()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _, _, cx| this.clear(cx)))
                .child(icon("x", 12., t.icon_muted))
        });
        let trigger = div()
            .id(id.clone())
            .relative()
            .w_full()
            .h(px(22.))
            .pl(px(8.))
            .pr(px(6.))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(4.))
            .rounded(px(4.))
            .border_1()
            .border_color(if self.open || focused {
                t.border_focused
            } else {
                t.border_variant
            })
            .hover(|s| s.border_color(t.border))
            .cursor_pointer()
            .text_size(px(12.))
            .text_color(color)
            .track_focus(&self.focus)
            .key_context("DatePicker")
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if this.handle_key(ev, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.toggle(window, cx)),
            )
            .child(
                canvas(
                    move |b: Bounds<Pixels>, _, _| bounds.set(Some(b)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(div().min_w(px(0.)).truncate().child(label.clone()))
            .child(match clear {
                Some(c) => c.into_any_element(),
                None => icon("calendar", 12., t.icon_muted).into_any_element(),
            });
        let popup = self.open.then(|| self.render_popup(&t, cx));
        div()
            .id(SharedString::from(format!("{id}-picker")))
            .relative()
            .w_full()
            .child(crate::dev_id!(
                trigger,
                id.to_string(),
                Button,
                label,
                Press(format!("date:{id}"))
            ))
            .children(popup)
    }
}

impl DatePicker {
    fn nav_button(
        &self,
        t: &Theme,
        name: &'static str,
        delta: i32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let disabled = self.view == CalendarView::Month;
        let id = format!("{}-{}", self.id, if delta < 0 { "prev" } else { "next" });
        crate::dev_id!(
            div()
                .id(SharedString::from(id.clone()))
                .size(px(CELL))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .when(!disabled, |d| {
                    d.cursor_pointer()
                        .hover(|s| s.bg(t.element_hover))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.step(delta, cx);
                                cx.stop_propagation();
                            }),
                        )
                })
                .child(icon(
                    name,
                    16.,
                    if disabled { t.text_placeholder } else { t.text }
                )),
            id,
            Button,
            if delta < 0 { "Previous" } else { "Next" },
            Press(format!(
                "date-step:{}:{}",
                self.id,
                if delta < 0 { 0 } else { 1 }
            ))
        )
        .into_any_element()
    }

    fn view_toggle(
        &self,
        t: &Theme,
        text: String,
        v: CalendarView,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.view == v;
        let id = format!(
            "{}-{}",
            self.id,
            if v == CalendarView::Month {
                "month"
            } else {
                "year"
            }
        );
        crate::dev_id!(
            div()
                .id(SharedString::from(id.clone()))
                .h(px(28.))
                .px(px(8.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .cursor_pointer()
                .text_size(px(13.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(t.text)
                .when(active, |d| d.bg(t.element_selected))
                .hover(|s| s.bg(t.element_hover))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.toggle_view(v, cx);
                        cx.stop_propagation();
                    }),
                )
                .child(text.clone()),
            id,
            Button,
            text,
            Press(format!(
                "date-view:{}:{}",
                self.id,
                if v == CalendarView::Month { 0 } else { 1 }
            ))
        )
        .into_any_element()
    }

    /// A clickable cell of the month or year grid.
    #[allow(clippy::too_many_arguments)]
    fn grid_cell(
        &self,
        t: &Theme,
        id: String,
        text: String,
        active: bool,
        size: f32,
        press: String,
        on: impl Fn(&mut DatePicker, &mut Context<DatePicker>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let _ = &press;
        crate::dev_id!(
            div()
                .id(SharedString::from(id.clone()))
                .w_full()
                .h(px(CELL))
                .my(px(4.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .cursor_pointer()
                .text_size(px(size))
                .when(active, |d| d.bg(t.border_focused).text_color(t.background))
                .when(!active, |d| d
                    .text_color(t.text)
                    .hover(|s| s.bg(t.element_hover)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        on(this, cx);
                        cx.stop_propagation();
                    }),
                )
                .child(text.clone()),
            id,
            Button,
            text,
            Press(press)
        )
        .into_any_element()
    }

    fn day_grid(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let (y, m) = self.shown;
        let days = month_grid(y, m, self.first_day);
        let start = self.first_day.num_days_from_sunday() as usize;
        let head = div().flex().flex_row().children((0..7).map(|i| {
            div()
                .size(px(CELL))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.))
                .text_color(t.text_muted)
                .opacity(0.5)
                .child(WEEKDAYS[(start + i) % 7])
        }));
        let mut rows = vec![head.into_any_element()];
        for week in days.chunks(7) {
            let cells: Vec<AnyElement> = week
                .iter()
                .map(|&d| {
                    let selected = self.value == Some(d);
                    let today = d == self.today && !selected;
                    let other = d.month() != m;
                    let id = format!("{}-day-{}", self.id, d.format("%Y-%m-%d"));
                    crate::dev_id!(
                        div()
                            .id(SharedString::from(id.clone()))
                            .size(px(CELL))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .cursor_pointer()
                            .text_size(px(13.))
                            .text_color(if other { t.text_muted } else { t.text })
                            .when(!selected, |c| c
                                .hover(|s| s.bg(t.element_hover).text_color(t.text)))
                            .when(today, |c| c.bg(t.element_selected))
                            .when(selected, |c| c
                                .bg(t.border_focused)
                                .text_color(t.background))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.pick(d, cx);
                                    cx.stop_propagation();
                                }),
                            )
                            .child(d.day().to_string()),
                        id,
                        Button,
                        d.format("%Y-%m-%d").to_string(),
                        Press(format!("date-day:{}:{}", self.id, d.format("%Y%m%d")))
                    )
                    .into_any_element()
                })
                .collect();
            rows.push(div().flex().flex_row().children(cells).into_any_element());
        }
        div().flex().flex_col().children(rows).into_any_element()
    }

    fn month_grid_view(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let rows: Vec<AnyElement> = (0..4)
            .map(|r| {
                let cells: Vec<AnyElement> = (0..3)
                    .map(|c| {
                        let m = (r * 3 + c + 1) as u32;
                        let name = MONTHS[m as usize - 1][..3].to_string();
                        div()
                            .flex_1()
                            .child(self.grid_cell(
                                t,
                                format!("{}-m-{m}", self.id),
                                name,
                                self.shown.1 == m,
                                11.,
                                format!("date-m:{}:{m}", self.id),
                                move |p, cx| p.choose_month(m, cx),
                                cx,
                            ))
                            .into_any_element()
                    })
                    .collect();
                div()
                    .flex()
                    .flex_row()
                    .gap(px(4.))
                    .children(cells)
                    .into_any_element()
            })
            .collect();
        div().flex().flex_col().children(rows).into_any_element()
    }

    fn year_grid_view(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let rows: Vec<AnyElement> = (0..4)
            .map(|r| {
                let cells: Vec<AnyElement> = (0..5)
                    .map(|c| {
                        let y = self.page + r * 5 + c;
                        let in_range = y >= self.year_range.0 && y < self.year_range.1;
                        div()
                            .flex_1()
                            .when(in_range, |d| {
                                d.child(self.grid_cell(
                                    t,
                                    format!("{}-y-{y}", self.id),
                                    y.to_string(),
                                    self.shown.0 == y,
                                    13.,
                                    format!("date-y:{}:{y}", self.id),
                                    move |p, cx| p.choose_year(y, cx),
                                    cx,
                                ))
                            })
                            .into_any_element()
                    })
                    .collect();
                div()
                    .flex()
                    .flex_row()
                    .gap(px(4.))
                    .children(cells)
                    .into_any_element()
            })
            .collect();
        div().flex().flex_col().children(rows).into_any_element()
    }

    fn render_popup(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let at = self.bounds.get().map_or(point(px(0.), px(0.)), |b| {
            point(b.left(), b.bottom() + px(POPUP_GAP))
        });
        let (y, m) = self.shown;
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(self.nav_button(t, "chevron-left", -1, cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(self.view_toggle(
                        t,
                        MONTHS[m as usize - 1].to_string(),
                        CalendarView::Month,
                        cx,
                    ))
                    .child(self.view_toggle(t, y.to_string(), CalendarView::Year, cx)),
            )
            .child(self.nav_button(t, "chevron-right", 1, cx));
        let body = match self.view {
            CalendarView::Day => self.day_grid(t, cx),
            CalendarView::Month => self.month_grid_view(t, cx),
            CalendarView::Year => self.year_grid_view(t, cx),
        };
        let id = self.id.clone();
        deferred(
            anchored()
                .position(at)
                .snap_to_window_with_margin(px(8.))
                .child(crate::dev_id_z!(
                    div()
                        .id(SharedString::from(format!("{id}-calendar")))
                        .occlude()
                        .w(px(CELL * 7. + 24.))
                        .p(px(12.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .rounded(px(6.))
                        .bg(t.elevated)
                        .border_1()
                        .border_color(t.border)
                        .shadow_md()
                        .on_mouse_down_out(cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                            this.mouse_out(ev, cx)
                        }))
                        .child(header)
                        .child(body),
                    1,
                    format!("{id}-calendar"),
                    Menu,
                    "Calendar",
                    None
                )),
        )
        .with_priority(5)
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn grid_has_whole_weeks_starting_on_the_first_day() {
        // November 2026 starts on a Sunday and has 30 days: 5 weeks.
        let g = month_grid(2026, 11, Weekday::Sun);
        assert_eq!(g.len(), 35);
        assert_eq!(g[0], d(2026, 11, 1));
        // February 2026 starts on a Sunday and has 28 days: exactly 4 weeks.
        assert_eq!(month_grid(2026, 2, Weekday::Sun).len(), 28);
        // Starting on Monday, the grid leads with October 26.
        let g = month_grid(2026, 11, Weekday::Mon);
        assert_eq!(g[0], d(2026, 10, 26));
        assert_eq!(g.len(), 42);
        assert!(g.iter().all(|x| g[0] <= *x));
    }

    #[test]
    fn months_and_year_pages() {
        assert_eq!(add_months(2026, 12, 1), (2027, 1));
        assert_eq!(add_months(2026, 1, -1), (2025, 12));
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(year_page_start(1976, 2026), 2016);
        assert_eq!(year_page_start(1976, 1976), 1976);
    }
}
