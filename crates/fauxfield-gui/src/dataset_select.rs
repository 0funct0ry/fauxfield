//! Row selection of the Dataset table (M10.07). The model is pure (live row indices, so the
//! selection survives sorting and filtering); the Shell actions below work on it.

use std::collections::BTreeSet;

use gpui::Context;
use serde_json::Value;

use crate::shell::Shell;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RowSelection {
    /// Live row indices.
    pub set: BTreeSet<usize>,
    /// Live index the next shift-click extends from.
    pub anchor: Option<usize>,
}

impl RowSelection {
    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub fn contains(&self, live: usize) -> bool {
        self.set.contains(&live)
    }

    pub fn clear(&mut self) {
        self.set.clear();
        self.anchor = None;
    }

    /// A click on displayed row `pos`. `display` maps displayed position to live index.
    /// Shift extends from the anchor (a range), Cmd/Ctrl toggles one row, a plain click selects
    /// only that row.
    pub fn click(&mut self, display: &[usize], pos: usize, shift: bool, secondary: bool) {
        let Some(&live) = display.get(pos) else {
            return;
        };
        if shift
            && let Some(a) = self
                .anchor
                .and_then(|a| display.iter().position(|d| *d == a))
        {
            let (lo, hi) = (a.min(pos), a.max(pos));
            if !secondary {
                self.set.clear();
            }
            self.set.extend(display[lo..=hi].iter().copied());
        } else if secondary {
            self.toggle(live);
            self.anchor = Some(live);
        } else {
            self.set.clear();
            self.set.insert(live);
            self.anchor = Some(live);
        }
    }

    /// Flip one row (the checkbox).
    pub fn toggle(&mut self, live: usize) {
        if !self.set.remove(&live) {
            self.set.insert(live);
        }
        self.anchor = Some(live);
    }

    /// Select every displayed row, or clear when they are all selected already.
    pub fn toggle_all(&mut self, display: &[usize]) {
        if !display.is_empty() && display.iter().all(|d| self.set.contains(d)) {
            self.clear();
        } else {
            self.set = display.iter().copied().collect();
        }
    }

    pub fn all_selected(&self, display: &[usize]) -> bool {
        !display.is_empty() && display.iter().all(|d| self.set.contains(d))
    }

    /// Selected indices ascending.
    pub fn indices(&self) -> Vec<usize> {
        self.set.iter().copied().collect()
    }

    /// Drop indices at or beyond `rows` (after a delete or a reload).
    pub fn retain_below(&mut self, rows: usize) {
        self.set.retain(|i| *i < rows);
        if self.anchor.is_some_and(|a| a >= rows) {
            self.anchor = None;
        }
    }
}

impl Shell {
    fn selection_of(&self, key: &str, cx: &gpui::App) -> Vec<usize> {
        self.ds_tables
            .borrow()
            .get(key)
            .map(|t| t.read(cx).delegate().selection.indices())
            .unwrap_or_default()
    }

    pub(crate) fn selected_rows(&self, cx: &gpui::App) -> Vec<usize> {
        self.active_dataset_key()
            .map(|k| self.selection_of(&k, cx))
            .unwrap_or_default()
    }

    pub(crate) fn select_all_rows(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.active_dataset_key() else {
            return;
        };
        if let Some(t) = self.ds_tables.borrow().get(&key).cloned() {
            t.update(cx, |s, cx| {
                s.delegate_mut().select_all();
                s.refresh(cx);
            });
        }
        cx.notify();
    }

    pub(crate) fn clear_row_selection(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.active_dataset_key() else {
            return;
        };
        if let Some(t) = self.ds_tables.borrow().get(&key).cloned() {
            t.update(cx, |s, cx| {
                s.delegate_mut().selection.clear();
                s.refresh(cx);
            });
        }
        cx.notify();
    }

    /// Selected rows as pretty JSON on the clipboard.
    pub(crate) fn copy_selected_rows(&mut self, cx: &mut Context<Self>) {
        let (Some(key), Some(e)) = (self.active_dataset_key(), self.engine()) else {
            return;
        };
        let Some(d) = self.ws.find_dataset(&key).cloned() else {
            return;
        };
        let sel = self.selection_of(&key, cx);
        if sel.is_empty() {
            return self.notify("Select rows first.".into(), cx);
        }
        let Ok(Value::Array(rows)) = e.dataset_rows(&d.server_id, &d.name) else {
            return;
        };
        let picked: Vec<&Value> = sel.iter().filter_map(|i| rows.get(*i)).collect();
        let text = serde_json::to_string_pretty(&picked).unwrap_or_default();
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        self.notify(format!("Copied {} rows as JSON", picked.len()), cx);
    }

    pub(crate) fn duplicate_selected_rows(&mut self, cx: &mut Context<Self>) {
        let (Some(key), Some(e)) = (self.active_dataset_key(), self.engine()) else {
            return;
        };
        let Some(d) = self.ws.find_dataset(&key).cloned() else {
            return;
        };
        let sel = self.selection_of(&key, cx);
        if sel.is_empty() {
            return self.notify("Select rows first.".into(), cx);
        }
        match e.dataset_duplicate(&d.server_id, &d.name, &sel) {
            Ok(new) => {
                self.ds_gen += 1;
                self.write_through(&d, cx);
                // The copies become the selection.
                if let Some(t) = self.ds_tables.borrow().get(&key).cloned() {
                    let n = new.clone();
                    t.update(cx, |s, _| {
                        let sel = &mut s.delegate_mut().selection;
                        sel.set = n.iter().copied().collect();
                        sel.anchor = n.first().copied();
                    });
                }
                self.notify(format!("Duplicated {} rows", new.len()), cx);
            }
            Err(err) => self.notify(err.to_string(), cx),
        }
        self.sync_running();
        cx.notify();
    }

    /// Delete the selection: one row at once, several after a confirmation.
    pub(crate) fn delete_selected_rows(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.active_dataset_key() else {
            return;
        };
        let Some(d) = self.ws.find_dataset(&key).cloned() else {
            return;
        };
        let sel = self.selection_of(&key, cx);
        match sel.len() {
            0 => self.notify("Select rows first.".into(), cx),
            1 => self.delete_rows_now(&key, &sel, cx),
            _ => {
                self.lc.data_confirm = Some(crate::lifecycle::DataConfirm {
                    kind: crate::lifecycle::DataConfirmKind::DeleteRows,
                    key,
                    name: d.name,
                    server_id: d.server_id,
                    indices: sel,
                });
                cx.notify();
            }
        }
    }

    /// Delete live rows, remembering them so the toast's Undo can put them back.
    pub(crate) fn delete_rows_now(&mut self, key: &str, indices: &[usize], cx: &mut Context<Self>) {
        let (Some(e), Some(d)) = (self.engine(), self.ws.find_dataset(key).cloned()) else {
            return;
        };
        let removed: Vec<(usize, Value)> = match e.dataset_rows(&d.server_id, &d.name) {
            Ok(Value::Array(rows)) => indices
                .iter()
                .filter_map(|i| rows.get(*i).map(|r| (*i, r.clone())))
                .collect(),
            _ => vec![],
        };
        self.delete_dataset_rows(key, indices, cx);
        if let Some(t) = self.ds_tables.borrow().get(key).cloned() {
            t.update(cx, |s, cx| {
                s.delegate_mut().selection.clear();
                s.refresh(cx);
            });
        }
        if !removed.is_empty() {
            let n = removed.len();
            self.ds_undo_delete = Some((key.to_string(), removed));
            self.notify_action(
                format!("Deleted {n} {}", if n == 1 { "row" } else { "rows" }),
                Some(crate::run::NoticeAction {
                    label: "Undo",
                    action: crate::run::ToastAction::UndoRowDelete,
                }),
                cx,
            );
        }
    }

    /// Undo (`redo` false) or redo the last cell edit of the active Dataset.
    pub(crate) fn step_cell_history(&mut self, redo: bool, cx: &mut Context<Self>) {
        let (Some(key), Some(e)) = (self.active_dataset_key(), self.engine()) else {
            return;
        };
        let Some(d) = self.ws.find_dataset(&key).cloned() else {
            return;
        };
        let Some(ent) = self.ds_tables.borrow().get(&key).cloned() else {
            return;
        };
        let shared = ent.read(cx).delegate().shared.clone();
        let op = if redo {
            shared.redo.borrow_mut().pop()
        } else {
            shared.undo.borrow_mut().pop()
        };
        let Some(op) = op else {
            return self.notify(
                if redo {
                    "Nothing to redo."
                } else {
                    "Nothing to undo."
                }
                .into(),
                cx,
            );
        };
        let val = if redo { op.new.clone() } else { op.old.clone() };
        match e.dataset_update_cell(&d.server_id, &d.name, op.live, &op.column, val) {
            Ok(()) => {
                if redo {
                    shared.undo.borrow_mut().push(op);
                } else {
                    shared.redo.borrow_mut().push(op);
                }
                self.ds_gen += 1;
            }
            Err(err) => self.notify(err.to_string(), cx),
        }
        self.sync_running();
        cx.notify();
    }

    /// A printable key with the Dataset table focused starts editing the selected row's cell.
    pub(crate) fn type_to_edit(
        &mut self,
        ch: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> bool {
        use gpui::Focusable;
        let Some(key) = self.active_dataset_key() else {
            return false;
        };
        let Some(ent) = self.ds_tables.borrow().get(&key).cloned() else {
            return false;
        };
        if !ent.focus_handle(cx).contains_focused(window, cx)
            || ent.read(cx).delegate().editing.is_some()
        {
            return false;
        }
        let Some(r) = ent.read(cx).selected_row() else {
            return false;
        };
        let col = ent
            .read(cx)
            .delegate()
            .shared
            .cell
            .get()
            .map_or(1, |(_, c)| c.max(1));
        let ch = ch.to_string();
        ent.update(cx, |s, cx| {
            s.delegate_mut().begin_edit_with(r, col, Some(&ch), cx)
        });
        true
    }

    /// Toast Undo: put the rows of the last delete back at their old positions.
    pub(crate) fn undo_row_delete(&mut self, cx: &mut Context<Self>) {
        let Some((key, mut removed)) = self.ds_undo_delete.take() else {
            return;
        };
        let (Some(e), Some(d)) = (self.engine(), self.ws.find_dataset(&key).cloned()) else {
            return;
        };
        removed.sort_by_key(|(i, _)| *i);
        for (i, row) in removed {
            let _ = e.dataset_insert(&d.server_id, &d.name, Some(i), vec![row]);
        }
        self.ds_gen += 1;
        self.write_through(&d, cx);
        self.sync_running();
        self.notify("Restored the deleted rows".into(), cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disp(n: usize) -> Vec<usize> {
        (0..n).collect()
    }

    #[test]
    fn click_toggle_and_range() {
        let d = disp(10);
        let mut s = RowSelection::default();
        s.click(&d, 2, false, false);
        assert_eq!(s.indices(), vec![2]);
        s.click(&d, 5, true, false);
        assert_eq!(s.indices(), vec![2, 3, 4, 5]);
        s.click(&d, 8, false, true);
        assert_eq!(s.indices(), vec![2, 3, 4, 5, 8]);
        s.click(&d, 3, false, true);
        assert_eq!(s.indices(), vec![2, 4, 5, 8]);
        s.click(&d, 0, false, false);
        assert_eq!(s.indices(), vec![0]);
    }

    #[test]
    fn select_all_toggles_and_follows_a_filtered_display() {
        let shown = vec![1, 4, 7];
        let mut s = RowSelection::default();
        s.toggle_all(&shown);
        assert_eq!(s.indices(), vec![1, 4, 7]);
        assert!(s.all_selected(&shown));
        s.toggle_all(&shown);
        assert!(s.is_empty());
        // A range in a sorted display uses display positions.
        let sorted = vec![9, 3, 5, 1];
        s.click(&sorted, 1, false, false);
        s.click(&sorted, 3, true, false);
        assert_eq!(s.indices(), vec![1, 3, 5]);
        s.retain_below(4);
        assert_eq!(s.indices(), vec![1, 3]);
    }
}
