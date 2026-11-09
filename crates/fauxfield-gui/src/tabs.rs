//! Pure tab model with Zed's preview semantics (SPEC §9.4): a single-click opens a preview tab
//! (italic) that the next single-click replaces; double-click or editing pins it.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabKind {
    Route,
    Server,
    Folder,
    Environment,
    /// A Dataset; the tab id is `<server id>:<name>`.
    Dataset,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tab {
    /// Semantic key (route id, server id, environment name); stable across frames.
    pub id: String,
    pub kind: TabKind,
    pub title: String,
    pub preview: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TabSet {
    tabs: Vec<Tab>,
    active: Option<usize>,
}

impl TabSet {
    pub fn from_saved(tabs: Vec<Tab>, active_id: Option<&str>) -> TabSet {
        let active = active_id
            .and_then(|id| tabs.iter().position(|t| t.id == id))
            .or(if tabs.is_empty() { None } else { Some(0) });
        TabSet { tabs, active }
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Tab> {
        self.tabs.iter()
    }

    pub fn active_index(&self) -> Option<usize> {
        self.active
    }

    pub fn active(&self) -> Option<&Tab> {
        self.active.and_then(|i| self.tabs.get(i))
    }

    pub fn position(&self, id: &str) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    /// Single-click: focus the tab if open, else open it as a preview, replacing the current
    /// preview tab in place.
    /// Update the title of the tab `(kind, id)`; true when it changed.
    pub fn retitle(&mut self, kind: TabKind, id: &str, title: &str) -> bool {
        match self.tabs.iter_mut().find(|t| t.kind == kind && t.id == id) {
            Some(t) if t.title != title => {
                t.title = title.to_string();
                true
            }
            _ => false,
        }
    }

    pub fn open_preview(&mut self, id: &str, kind: TabKind, title: &str) {
        if let Some(i) = self.position(id) {
            self.active = Some(i);
            return;
        }
        let tab = Tab {
            id: id.into(),
            kind,
            title: title.into(),
            preview: true,
        };
        if let Some(i) = self.tabs.iter().position(|t| t.preview) {
            self.tabs[i] = tab;
            self.active = Some(i);
        } else {
            self.tabs.push(tab);
            self.active = Some(self.tabs.len() - 1);
        }
    }

    /// Double-click / Cmd+Enter: open pinned, or pin the existing tab.
    pub fn open_pinned(&mut self, id: &str, kind: TabKind, title: &str) {
        if let Some(i) = self.position(id) {
            self.tabs[i].preview = false;
            self.active = Some(i);
        } else {
            self.tabs.push(Tab {
                id: id.into(),
                kind,
                title: title.into(),
                preview: false,
            });
            self.active = Some(self.tabs.len() - 1);
        }
    }

    pub fn pin(&mut self, index: usize) {
        if let Some(t) = self.tabs.get_mut(index) {
            t.preview = false;
        }
    }

    pub fn activate(&mut self, index: usize) -> bool {
        if index < self.tabs.len() {
            self.active = Some(index);
            true
        } else {
            false
        }
    }

    /// Closes a tab. Closing the active tab activates its left neighbour (or the new first).
    pub fn close(&mut self, index: usize) -> Option<Tab> {
        if index >= self.tabs.len() {
            return None;
        }
        let closed = self.tabs.remove(index);
        self.active = match self.active {
            _ if self.tabs.is_empty() => None,
            Some(a) if a == index => Some(index.saturating_sub(1)),
            Some(a) if a > index => Some(a - 1),
            other => other,
        };
        Some(closed)
    }

    /// Drag-reorder: moves a tab and keeps the same tab active.
    pub fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return;
        }
        let active_id = self.active().map(|t| t.id.clone());
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active = active_id.and_then(|id| self.position(&id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(t: &TabSet) -> Vec<&str> {
        t.tabs().iter().map(|t| t.id.as_str()).collect()
    }

    #[test]
    fn single_clicks_replace_the_preview_tab() {
        let mut t = TabSet::default();
        t.open_preview("a", TabKind::Route, "A");
        t.open_preview("b", TabKind::Route, "B");
        assert_eq!(ids(&t), ["b"]);
        assert!(t.tabs()[0].preview);
    }

    #[test]
    fn pinning_keeps_the_tab_and_next_preview_appends() {
        let mut t = TabSet::default();
        t.open_preview("a", TabKind::Route, "A");
        t.pin(0);
        t.open_preview("b", TabKind::Server, "B");
        assert_eq!(ids(&t), ["a", "b"]);
        assert!(!t.tabs()[0].preview && t.tabs()[1].preview);
        t.open_pinned("b", TabKind::Server, "B");
        assert!(!t.tabs()[1].preview);
        assert_eq!(t.active_index(), Some(1));
    }

    #[test]
    fn reopening_focuses_without_duplicating() {
        let mut t = TabSet::default();
        t.open_pinned("a", TabKind::Route, "A");
        t.open_pinned("b", TabKind::Route, "B");
        t.open_preview("a", TabKind::Route, "A");
        assert_eq!(ids(&t), ["a", "b"]);
        assert_eq!(t.active().unwrap().id, "a");
    }

    #[test]
    fn close_picks_left_neighbour_and_handles_empty() {
        let mut t = TabSet::default();
        for id in ["a", "b", "c"] {
            t.open_pinned(id, TabKind::Route, id);
        }
        t.activate(1);
        t.close(1);
        assert_eq!(t.active().unwrap().id, "a");
        t.close(1);
        assert_eq!(t.active().unwrap().id, "a");
        t.close(0);
        assert!(t.active().is_none() && t.is_empty());
        assert!(t.close(0).is_none());
    }

    #[test]
    fn closing_before_active_shifts_the_index() {
        let mut t = TabSet::default();
        for id in ["a", "b", "c"] {
            t.open_pinned(id, TabKind::Route, id);
        }
        t.close(0);
        assert_eq!(t.active().unwrap().id, "c");
    }

    #[test]
    fn move_keeps_the_active_tab() {
        let mut t = TabSet::default();
        for id in ["a", "b", "c"] {
            t.open_pinned(id, TabKind::Route, id);
        }
        t.activate(0);
        t.move_tab(0, 2);
        assert_eq!(ids(&t), ["b", "c", "a"]);
        assert_eq!(t.active().unwrap().id, "a");
        t.move_tab(0, 9);
        assert_eq!(ids(&t), ["b", "c", "a"]);
    }

    #[test]
    fn activate_by_index_is_bounds_checked() {
        let mut t = TabSet::default();
        t.open_pinned("a", TabKind::Route, "A");
        assert!(!t.activate(5));
        assert!(t.activate(0));
    }
}
