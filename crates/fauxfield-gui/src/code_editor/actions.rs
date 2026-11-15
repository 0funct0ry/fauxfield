//! Editor actions (key context `CodeEditor`). Only go to line, toggle soft wrap and format body
//! are listed in the Command Palette; the rest are keymap-only (SPEC §9.8).

use gpui::{KeyBinding, actions};

actions!(
    code_editor,
    [
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        LineStart,
        LineEnd,
        SelectLineStart,
        SelectLineEnd,
        DocStart,
        DocEnd,
        SelectDocStart,
        SelectDocEnd,
        Backspace,
        Delete,
        DeleteWordLeft,
        DeleteWordRight,
        DeleteToLineStart,
        DeleteToLineEnd,
        Cut,
        Copy,
        Paste,
        SelectAll,
        Undo,
        Redo,
        ToggleComment,
        MoveLineUp,
        MoveLineDown,
        DuplicateLine,
        DeleteLine,
        GoToLine,
        MatchBracket,
        ShowCompletions,
        ToggleSoftWrap,
        FormatBody,
        Escape,
    ]
);

/// (action id, palette label or empty when hidden, default keys, constructor check). The keymap
/// TOML carries the keys; this table feeds `crate::actions::ACTIONS`.
pub const PALETTE_VISIBLE: &[&str] = &["ed_go_to_line", "ed_toggle_soft_wrap", "ed_format_body"];

/// Every editor action id (the keymap editor lists them).
pub const IDS: &[&str] = &[
    "ed_left",
    "ed_right",
    "ed_up",
    "ed_down",
    "ed_select_left",
    "ed_select_right",
    "ed_select_up",
    "ed_select_down",
    "ed_word_left",
    "ed_word_right",
    "ed_select_word_left",
    "ed_select_word_right",
    "ed_line_start",
    "ed_line_end",
    "ed_select_line_start",
    "ed_select_line_end",
    "ed_doc_start",
    "ed_doc_end",
    "ed_select_doc_start",
    "ed_select_doc_end",
    "ed_backspace",
    "ed_delete",
    "ed_delete_word_left",
    "ed_delete_word_right",
    "ed_delete_to_line_start",
    "ed_delete_to_line_end",
    "ed_cut",
    "ed_copy",
    "ed_paste",
    "ed_select_all",
    "ed_undo",
    "ed_redo",
    "ed_toggle_comment",
    "ed_move_line_up",
    "ed_move_line_down",
    "ed_duplicate_line",
    "ed_delete_line",
    "ed_go_to_line",
    "ed_match_bracket",
    "ed_show_completions",
    "ed_toggle_soft_wrap",
    "ed_format_body",
    "ed_escape",
];

/// Binding constructors for every editor action id (`ed_*`), used by `crate::actions::install`.
pub fn binder(id: &str) -> Option<fn(&str, Option<&str>) -> KeyBinding> {
    Some(match id {
        "ed_left" => |k, c| KeyBinding::new(k, Left, c),
        "ed_right" => |k, c| KeyBinding::new(k, Right, c),
        "ed_up" => |k, c| KeyBinding::new(k, Up, c),
        "ed_down" => |k, c| KeyBinding::new(k, Down, c),
        "ed_select_left" => |k, c| KeyBinding::new(k, SelectLeft, c),
        "ed_select_right" => |k, c| KeyBinding::new(k, SelectRight, c),
        "ed_select_up" => |k, c| KeyBinding::new(k, SelectUp, c),
        "ed_select_down" => |k, c| KeyBinding::new(k, SelectDown, c),
        "ed_word_left" => |k, c| KeyBinding::new(k, WordLeft, c),
        "ed_word_right" => |k, c| KeyBinding::new(k, WordRight, c),
        "ed_select_word_left" => |k, c| KeyBinding::new(k, SelectWordLeft, c),
        "ed_select_word_right" => |k, c| KeyBinding::new(k, SelectWordRight, c),
        "ed_line_start" => |k, c| KeyBinding::new(k, LineStart, c),
        "ed_line_end" => |k, c| KeyBinding::new(k, LineEnd, c),
        "ed_select_line_start" => |k, c| KeyBinding::new(k, SelectLineStart, c),
        "ed_select_line_end" => |k, c| KeyBinding::new(k, SelectLineEnd, c),
        "ed_doc_start" => |k, c| KeyBinding::new(k, DocStart, c),
        "ed_doc_end" => |k, c| KeyBinding::new(k, DocEnd, c),
        "ed_select_doc_start" => |k, c| KeyBinding::new(k, SelectDocStart, c),
        "ed_select_doc_end" => |k, c| KeyBinding::new(k, SelectDocEnd, c),
        "ed_backspace" => |k, c| KeyBinding::new(k, Backspace, c),
        "ed_delete" => |k, c| KeyBinding::new(k, Delete, c),
        "ed_delete_word_left" => |k, c| KeyBinding::new(k, DeleteWordLeft, c),
        "ed_delete_word_right" => |k, c| KeyBinding::new(k, DeleteWordRight, c),
        "ed_delete_to_line_start" => |k, c| KeyBinding::new(k, DeleteToLineStart, c),
        "ed_delete_to_line_end" => |k, c| KeyBinding::new(k, DeleteToLineEnd, c),
        "ed_cut" => |k, c| KeyBinding::new(k, Cut, c),
        "ed_copy" => |k, c| KeyBinding::new(k, Copy, c),
        "ed_paste" => |k, c| KeyBinding::new(k, Paste, c),
        "ed_select_all" => |k, c| KeyBinding::new(k, SelectAll, c),
        "ed_undo" => |k, c| KeyBinding::new(k, Undo, c),
        "ed_redo" => |k, c| KeyBinding::new(k, Redo, c),
        "ed_toggle_comment" => |k, c| KeyBinding::new(k, ToggleComment, c),
        "ed_move_line_up" => |k, c| KeyBinding::new(k, MoveLineUp, c),
        "ed_move_line_down" => |k, c| KeyBinding::new(k, MoveLineDown, c),
        "ed_duplicate_line" => |k, c| KeyBinding::new(k, DuplicateLine, c),
        "ed_delete_line" => |k, c| KeyBinding::new(k, DeleteLine, c),
        "ed_go_to_line" => |k, c| KeyBinding::new(k, GoToLine, c),
        "ed_match_bracket" => |k, c| KeyBinding::new(k, MatchBracket, c),
        "ed_show_completions" => |k, c| KeyBinding::new(k, ShowCompletions, c),
        "ed_toggle_soft_wrap" => |k, c| KeyBinding::new(k, ToggleSoftWrap, c),
        "ed_format_body" => |k, c| KeyBinding::new(k, FormatBody, c),
        "ed_escape" => |k, c| KeyBinding::new(k, Escape, c),
        _ => return None,
    })
}

#[cfg(test)]
mod ids_tests {
    #[test]
    fn ids_match_binder() {
        for id in super::IDS {
            assert!(super::binder(id).is_some(), "{id}");
        }
    }
}
