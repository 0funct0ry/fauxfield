//! GPUI tests: typing through the input handler, keys, undo grouping, IME, read-only, wrap.

use gpui::{Entity, EntityInputHandler, TestAppContext, VisualTestContext};

use super::{CodeEditor, EditorEvent, Language};
use crate::keymap::Keymap;
use crate::theme::Theme;

fn editor<'a>(
    cx: &'a mut TestAppContext,
    lang: Language,
    text: &str,
    f: impl FnOnce(CodeEditor) -> CodeEditor + 'static,
) -> (Entity<CodeEditor>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        cx.set_global(Theme::one_dark());
        crate::actions::install(cx, &Keymap::bundled());
    });
    let text = text.to_string();
    let (ed, cx) = cx.add_window_view(move |_, cx| {
        let mut e = f(CodeEditor::new(lang, cx));
        e.set_text(&text, cx);
        e
    });
    cx.update(|w, cx| {
        let fh = ed.read(cx).focus.clone();
        w.focus(&fh);
    });
    cx.run_until_parked();
    (ed, cx)
}

fn text(ed: &Entity<CodeEditor>, cx: &mut VisualTestContext) -> String {
    ed.read_with(cx, |e, _| e.text().to_string())
}

#[gpui::test]
fn typing_goes_through_the_input_handler_with_auto_close(cx: &mut TestAppContext) {
    let (ed, cx) = editor(cx, Language::Json, "", |e| e);
    cx.simulate_input("{");
    assert_eq!(text(&ed, cx), "{}");
    cx.simulate_input("\"");
    assert_eq!(text(&ed, cx), "{\"\"}");
    cx.simulate_input("a");
    cx.simulate_input("\"");
    assert_eq!(text(&ed, cx), "{\"a\"}");
    cx.simulate_keystrokes("backspace backspace backspace");
    assert_eq!(text(&ed, cx), "{}");
}

#[gpui::test]
fn enter_and_tab_indent(cx: &mut TestAppContext) {
    let (ed, cx) = editor(cx, Language::Json, "", |e| e);
    cx.simulate_input("{");
    cx.simulate_keystrokes("enter");
    assert_eq!(text(&ed, cx), "{\n  \n}");
    cx.simulate_keystrokes("tab");
    assert_eq!(text(&ed, cx), "{\n    \n}");
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(text(&ed, cx), "{\n  \n}");
}

#[gpui::test]
fn undo_groups_typing_and_redo_restores(cx: &mut TestAppContext) {
    let (ed, cx) = editor(cx, Language::Plain, "", |e| e);
    cx.simulate_input("abc");
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(text(&ed, cx), "");
    cx.simulate_keystrokes("secondary-shift-z");
    assert_eq!(text(&ed, cx), "abc");
}

#[gpui::test]
fn read_only_rejects_edits_and_ranges_are_protected(cx: &mut TestAppContext) {
    let (ed, cx) = editor(cx, Language::Plain, "abc", |e| e.read_only(true));
    cx.simulate_input("x");
    assert_eq!(text(&ed, cx), "abc");
    let (ed, cx) = editor(cx, Language::Plain, "head\nbody", |e| {
        e.read_only_ranges(std::iter::once(0..4).collect())
    });
    ed.update(cx, |e, cx| e.set_cursor(2, cx));
    cx.simulate_input("x");
    assert_eq!(text(&ed, cx), "head\nbody");
    ed.update(cx, |e, cx| e.set_cursor(9, cx));
    cx.simulate_input("!");
    assert_eq!(text(&ed, cx), "head\nbody!");
}

#[gpui::test]
fn comment_toggle_and_line_moves(cx: &mut TestAppContext) {
    let (ed, cx) = editor(cx, Language::Yaml, "a: 1\nb: 2", |e| e);
    cx.simulate_keystrokes("secondary-/");
    assert_eq!(text(&ed, cx), "# a: 1\nb: 2");
    cx.simulate_keystrokes("alt-down");
    assert_eq!(text(&ed, cx), "b: 2\n# a: 1");
}

#[gpui::test]
fn goal_column_is_kept_across_short_lines(cx: &mut TestAppContext) {
    let (ed, cx) = editor(cx, Language::Plain, "abcdef\nab\nabcdef", |e| e);
    ed.update(cx, |e, cx| e.set_cursor(5, cx));
    cx.simulate_keystrokes("down");
    assert_eq!(ed.read_with(cx, |e, _| e.line_col()), (1, 2));
    cx.simulate_keystrokes("down");
    assert_eq!(ed.read_with(cx, |e, _| e.line_col()), (2, 5));
}

#[gpui::test]
fn ime_marks_and_commits(cx: &mut TestAppContext) {
    let (ed, cx) = editor(cx, Language::Plain, "", |e| e);
    ed.update_in(cx, |e, w, cx| {
        e.replace_and_mark_text_in_range(None, "に", Some(0..1), w, cx);
    });
    assert_eq!(ed.read_with(cx, |e, _| e.marked.clone()), Some(0..3));
    ed.update_in(cx, |e, w, cx| {
        e.replace_text_in_range(None, "日本", w, cx);
    });
    assert_eq!(text(&ed, cx), "日本");
    assert_eq!(ed.read_with(cx, |e, _| e.marked.clone()), None);
}

#[gpui::test]
fn set_text_is_silent_and_replace_all_is_one_step(cx: &mut TestAppContext) {
    let (ed, cx) = editor(cx, Language::Json, "{}", |e| e);
    let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let ev = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&ed, move |_, e: &EditorEvent, _| {
            ev.borrow_mut().push(e.clone())
        })
        .detach();
    });
    ed.update(cx, |e, cx| e.set_text("[]", cx));
    assert!(!events.borrow().contains(&EditorEvent::Changed));
    ed.update(cx, |e, cx| {
        e.replace_all("[1]", cx);
    });
    assert!(events.borrow().contains(&EditorEvent::Changed));
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(text(&ed, cx), "[]");
}

#[gpui::test]
fn format_body_is_one_undo_step_and_reports_errors(cx: &mut TestAppContext) {
    let (ed, cx) = editor(cx, Language::Json, "{\"a\":1}", |e| e);
    cx.simulate_keystrokes("secondary-shift-i");
    assert_eq!(text(&ed, cx), "{\n  \"a\": 1\n}");
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(text(&ed, cx), "{\"a\":1}");
    ed.update(cx, |e, cx| e.set_text("{]", cx));
    cx.simulate_keystrokes("secondary-shift-i");
    assert!(ed.read_with(cx, |e, _| e.last_error().is_some()));
}

#[gpui::test]
fn soft_wrap_toggle_changes_display_rows(cx: &mut TestAppContext) {
    let long = "word ".repeat(400);
    let (ed, cx) = editor(cx, Language::Plain, &long, |e| e);
    let rows = |ed: &Entity<CodeEditor>, cx: &mut VisualTestContext| {
        ed.read_with(cx, |e, _| e.layout.as_ref().map_or(0, |l| l.dm.row_count()))
    };
    assert_eq!(rows(&ed, cx), 1);
    ed.update(cx, |e, cx| e.toggle_soft_wrap(cx));
    cx.run_until_parked();
    cx.update(|w, _| w.refresh());
    cx.run_until_parked();
    assert!(rows(&ed, cx) > 1);
}

#[gpui::test]
fn one_mib_text_loads_and_edits(cx: &mut TestAppContext) {
    let big = format!("[{}]", "1,".repeat(500_000) + "1");
    let (ed, cx) = editor(cx, Language::Json, &big, |e| e);
    ed.update(cx, |e, cx| e.set_cursor(1, cx));
    cx.simulate_input("9");
    assert_eq!(ed.read_with(cx, |e, _| e.text().len()), big.len() + 1);
}

#[gpui::test]
fn enter_and_tab_both_accept_the_open_completion(cx: &mut TestAppContext) {
    for key in ["enter", "tab"] {
        let (ed, cx) = editor(cx, Language::Plain, "", |e| {
            e.completion_provider(super::tera_ctx::TeraCompletions {
                variables: vec!["user_name".into()],
            })
        });
        cx.simulate_input("{{");
        assert!(ed.read_with(cx, |e, _| e.comp.is_some()), "popup open");
        cx.simulate_keystrokes(key);
        assert!(ed.read_with(cx, |e, _| e.comp.is_none()), "{key} closes it");
        let t = text(&ed, cx);
        assert!(!t.contains('\n'), "{key} must not insert a newline: {t:?}");
        assert_ne!(t, "{{}}", "{key} inserted the selected item");
    }
}

#[cfg(target_os = "macos")]
#[gpui::test]
fn emacs_bindings_move_and_kill(cx: &mut TestAppContext) {
    let (ed, cx) = editor(cx, Language::Plain, "hello world", |e| e);
    ed.update(cx, |e, cx| e.set_cursor(0, cx));
    cx.simulate_keystrokes("ctrl-e");
    assert_eq!(ed.read_with(cx, |e, _| e.buf.head()), 11);
    cx.simulate_keystrokes("ctrl-a ctrl-f ctrl-f");
    assert_eq!(ed.read_with(cx, |e, _| e.buf.head()), 2);
    cx.simulate_keystrokes("ctrl-k");
    assert_eq!(text(&ed, cx), "he");
}

#[gpui::test]
fn arrows_navigate_the_completion_popup(cx: &mut TestAppContext) {
    let (ed, cx) = editor(cx, Language::Plain, "", |e| {
        e.completion_provider(super::tera_ctx::TeraCompletions {
            variables: vec!["alpha".into(), "beta".into()],
        })
    });
    cx.simulate_input("{{");
    let sel = |ed: &Entity<CodeEditor>, cx: &mut VisualTestContext| {
        ed.read_with(cx, |e, _| e.comp.as_ref().map(|c| c.selected))
    };
    assert_eq!(sel(&ed, cx), Some(0));
    cx.simulate_keystrokes("down");
    assert_eq!(sel(&ed, cx), Some(1));
    cx.simulate_keystrokes("up");
    assert_eq!(sel(&ed, cx), Some(0));
    cx.simulate_keystrokes("up");
    assert!(sel(&ed, cx).unwrap() > 0, "wraps to the last item");
}

#[gpui::test]
fn replacing_fake_kind_does_not_panic(cx: &mut TestAppContext) {
    let src = "{\n  \"id\": 1,\n  \"name\": \"{{fake(kind='name')}}\"\n}";
    let (ed, cx) = editor(cx, Language::Json, src, |e| {
        e.completion_provider(super::tera_ctx::TeraCompletions {
            variables: vec!["a".into()],
        })
    });
    let at = src.find("'name'").unwrap() + 1;
    ed.update(cx, |e, cx| {
        e.set_cursor(at, cx);
    });
    cx.simulate_keystrokes("shift-right shift-right shift-right shift-right");
    cx.simulate_input("c");
    cx.simulate_input("o");
    assert!(text(&ed, cx).contains("kind='co'"));
}
