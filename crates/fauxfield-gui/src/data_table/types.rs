//! Plain data types of the DataTable (internal-docs/data-table.md §2–§6). GPUI-free.

use std::ops::Range;

use serde::Serialize;

/// Cell size: drives row height and cell padding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Size {
    XSmall,
    Small,
    #[default]
    Medium,
    Large,
}

impl Size {
    pub fn row_height(self) -> f32 {
        match self {
            Size::XSmall => 20.0,
            Size::Small => 24.0,
            Size::Medium => 28.0,
            Size::Large => 32.0,
        }
    }

    /// Horizontal cell padding (each side).
    pub fn pad_x(self) -> f32 {
        match self {
            Size::XSmall => 4.0,
            Size::Small => 6.0,
            Size::Medium => 8.0,
            Size::Large => 10.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// `None` = not sortable; the others make the column sortable and give its state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnSort {
    #[default]
    None,
    Default,
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnFixed {
    Left,
    Right,
}

pub const DEFAULT_COL_WIDTH: f32 = 100.0;
pub const DEFAULT_MIN_WIDTH: f32 = 20.0;
pub const DEFAULT_MAX_WIDTH: f32 = 1200.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub key: String,
    pub name: String,
    pub align: Align,
    pub sort: ColumnSort,
    pub paddings: Option<f32>,
    pub width: f32,
    pub min_width: f32,
    pub max_width: f32,
    pub fixed: Option<ColumnFixed>,
    pub resizable: bool,
    pub movable: bool,
    pub selectable: bool,
}

impl Default for Column {
    fn default() -> Column {
        Column {
            key: String::new(),
            name: String::new(),
            align: Align::Left,
            sort: ColumnSort::None,
            paddings: None,
            width: DEFAULT_COL_WIDTH,
            min_width: DEFAULT_MIN_WIDTH,
            max_width: DEFAULT_MAX_WIDTH,
            fixed: None,
            resizable: true,
            movable: true,
            selectable: true,
        }
    }
}

impl Column {
    pub fn new(key: impl Into<String>, name: impl Into<String>) -> Column {
        Column {
            key: key.into(),
            name: name.into(),
            ..Column::default()
        }
    }

    pub fn sortable(mut self) -> Self {
        self.sort = ColumnSort::Default;
        self
    }

    pub fn ascending(mut self) -> Self {
        self.sort = ColumnSort::Ascending;
        self
    }

    pub fn descending(mut self) -> Self {
        self.sort = ColumnSort::Descending;
        self
    }

    pub fn text_center(mut self) -> Self {
        self.align = Align::Center;
        self
    }

    pub fn text_right(mut self) -> Self {
        self.align = Align::Right;
        self
    }

    pub fn paddings(mut self, p: f32) -> Self {
        self.paddings = Some(p);
        self
    }

    pub fn p_0(self) -> Self {
        self.paddings(0.0)
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }

    pub fn fixed_left(mut self) -> Self {
        self.fixed = Some(ColumnFixed::Left);
        self
    }

    pub fn fixed_right(mut self) -> Self {
        self.fixed = Some(ColumnFixed::Right);
        self
    }

    pub fn resizable(mut self, v: bool) -> Self {
        self.resizable = v;
        self
    }

    pub fn movable(mut self, v: bool) -> Self {
        self.movable = v;
        self
    }

    pub fn selectable(mut self, v: bool) -> Self {
        self.selectable = v;
        self
    }

    /// Raising the minimum bumps a smaller width up.
    pub fn min_width(mut self, w: f32) -> Self {
        self.min_width = w;
        if self.width < w {
            self.width = w;
        }
        self
    }

    /// Lowering the maximum pulls a larger width down.
    pub fn max_width(mut self, w: f32) -> Self {
        self.max_width = w;
        if self.width > w {
            self.width = w;
        }
        self
    }

    pub fn clamp_width(&self, w: f32) -> f32 {
        w.max(self.min_width)
            .min(self.max_width.max(self.min_width))
    }
}

/// A label spanning `span` leaf columns in a group-header row.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnGroup {
    pub label: String,
    pub span: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum TableSelection {
    #[default]
    None,
    Row(usize),
    Column(usize),
    Cell(usize, usize),
}

#[derive(Clone, Debug, PartialEq)]
pub enum TableEvent {
    SelectRow(usize),
    DoubleClickedRow(usize),
    SelectColumn(usize),
    SelectCell(usize, usize),
    DoubleClickedCell(usize, usize),
    ColumnWidthsChanged(Vec<f32>),
    MoveColumn(usize, usize),
    RightClickedRow(Option<usize>),
    RightClickedCell(usize, usize),
    ClearSelection,
}

/// Rows and columns currently visible.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct VisibleRange {
    pub rows: Range<usize>,
    pub cols: Range<usize>,
}

/// Modifier state of a click.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClickMods {
    /// Cmd on macOS, Ctrl elsewhere.
    pub secondary: bool,
    pub shift: bool,
    /// 1 for a single click, 2 for a double click.
    pub clicks: usize,
}

impl ClickMods {
    pub fn plain() -> ClickMods {
        ClickMods {
            clicks: 1,
            ..ClickMods::default()
        }
    }

    pub fn double() -> ClickMods {
        ClickMods {
            clicks: 2,
            ..ClickMods::default()
        }
    }

    pub fn is_modified(self) -> bool {
        self.secondary || self.shift
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavKey {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessRole {
    Grid,
    Table,
}

/// `ui.state` view model of one table.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TableViewModel {
    pub columns: Vec<String>,
    /// Widths in whole points.
    pub widths: Vec<u32>,
    pub sort: Option<(String, ColumnSort)>,
    pub selection_kind: &'static str,
    pub selection: Vec<usize>,
    pub visible_rows: Range<usize>,
    pub visible_cols: Range<usize>,
    pub row_count: usize,
    pub loading: bool,
    pub empty: bool,
}
