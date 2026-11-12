//! DataTable keyboard actions (key context `DataTable`, data-table.md §8). Keymap-only: they
//! are not listed in the Command Palette, like the code editor's cursor keys (SPEC §9.8).

use gpui::{KeyBinding, actions};

actions!(
    data_table,
    [Up, Down, Left, Right, Home, End, PageUp, PageDown, Escape]
);

pub fn binder(id: &str) -> Option<fn(&str, Option<&str>) -> KeyBinding> {
    Some(match id {
        "dt_up" => |k, c| KeyBinding::new(k, Up, c),
        "dt_down" => |k, c| KeyBinding::new(k, Down, c),
        "dt_left" => |k, c| KeyBinding::new(k, Left, c),
        "dt_right" => |k, c| KeyBinding::new(k, Right, c),
        "dt_home" => |k, c| KeyBinding::new(k, Home, c),
        "dt_end" => |k, c| KeyBinding::new(k, End, c),
        "dt_page_up" => |k, c| KeyBinding::new(k, PageUp, c),
        "dt_page_down" => |k, c| KeyBinding::new(k, PageDown, c),
        "dt_escape" => |k, c| KeyBinding::new(k, Escape, c),
        _ => return None,
    })
}
