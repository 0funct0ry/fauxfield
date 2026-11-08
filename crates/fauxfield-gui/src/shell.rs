//! The window shell: state, behaviour and the root view. Rendering of each region lives in the
//! sibling view modules (`title_bar`, `left_dock`, `center`, `bottom_dock`, `status_bar`,
//! `overlay`), all as `impl Shell` blocks so they share this state.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui::{
    Context, FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    MouseMoveEvent, ParentElement, Render, Styled, UniformListScrollHandle, Window, div,
    prelude::*, px,
};

use crate::actions;
use crate::assets::UI_FONT;
use crate::chord::{CHORD_TIMEOUT, ChordState, Outcome};
use crate::keymap::Keymap;
use crate::layout::{BottomTab, Layout};
use crate::mock::{self, TrafficRow, Workspace};
use crate::search::{self, CommandEntry, Hit, Kind, Target};
use crate::tabs::{TabKind, TabSet};
use crate::theme::{self, Theme};
use crate::tree::{self, MenuAct, RowKind};
use crate::ui::STATUS_BAR_H;

pub const TRAFFIC_ROWS: usize = 10_000;
pub const ROUTE_SUBTABS: [&str; 6] = [
    "Responses",
    "Rules",
    "Headers",
    "Callbacks",
    "Docs",
    "Settings",
];
pub const SERVER_SECTIONS: [&str; 9] = [
    "General",
    "TLS",
    "CORS",
    "Network",
    "Rate limit",
    "Proxy",
    "Datasets",
    "Variables",
    "Docs",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerKind {
    Search,
    Palette,
    Theme,
    Environment,
}

pub struct Picker {
    pub kind: PickerKind,
    pub query: String,
    pub selected: usize,
    pub hits: Vec<Hit>,
}

/// An open tree-row context menu, positioned in window coordinates.
#[derive(Clone, Debug)]
pub struct RowMenu {
    pub row_id: String,
    pub kind: RowKind,
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resize {
    Left,
    Right,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenMode {
    Preview,
    Pinned,
    Reveal,
}

#[derive(Default)]
pub struct ShellOptions {
    /// Where the layout JSON lives; `None` disables persistence (tests).
    pub layout_path: Option<PathBuf>,
    /// Directory holding an optional user `keymap.toml`.
    pub config_dir: Option<PathBuf>,
}

pub struct Shell {
    pub(crate) ws: Workspace,
    pub(crate) tabs: TabSet,
    pub(crate) layout: Layout,
    layout_path: Option<PathBuf>,
    pub(crate) keymap: Keymap,
    pub(crate) commands: Vec<CommandEntry>,
    chord: ChordState,
    pub(crate) collapsed: HashSet<String>,
    pub(crate) selected: Option<String>,
    pub(crate) active_env: String,
    pub(crate) picker: Option<Picker>,
    theme_before_preview: Option<String>,
    recent: Vec<String>,
    pub(crate) picker_scroll: UniformListScrollHandle,
    pub(crate) traffic: Vec<TrafficRow>,
    pub(crate) traffic_scroll: UniformListScrollHandle,
    pub(crate) traffic_selected: Option<usize>,
    pub(crate) route_sub: HashMap<String, usize>,
    pub(crate) server_section: HashMap<String, usize>,
    pub(crate) reveal_secrets: bool,
    pub(crate) notice: Option<(u64, String)>,
    notice_gen: u64,
    pub(crate) resize: Option<Resize>,
    pub(crate) menu: Option<RowMenu>,
    pub(crate) problems: Vec<mock::Problem>,
    pub(crate) logs: Vec<String>,
    pub(crate) focus: FocusHandle,
    pub(crate) picker_focus: FocusHandle,
}

impl Focusable for Shell {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Shell {
    pub fn new(opts: ShellOptions, window: &mut Window, cx: &mut Context<Self>) -> Shell {
        let ws = mock::workspace();
        let layout = opts
            .layout_path
            .as_deref()
            .map(Layout::load)
            .unwrap_or_default();
        let (keymap, keymap_error) = Keymap::load(opts.config_dir.as_deref());
        let mut problems = actions::install(cx, &keymap);
        problems.extend(keymap_error);

        let os_dark = matches!(
            window.appearance(),
            gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark
        );
        let chosen = layout
            .theme
            .as_deref()
            .and_then(Theme::named)
            .unwrap_or_else(|| {
                if os_dark {
                    Theme::one_dark()
                } else {
                    Theme::one_light()
                }
            });
        cx.set_global(chosen);

        let saved: Vec<_> = layout
            .tabs
            .iter()
            .filter(|t| tab_exists(&ws, t.kind, &t.id))
            .cloned()
            .collect();
        let tabs = TabSet::from_saved(saved, layout.active_tab.as_deref());

        let chords: Vec<(String, String)> = keymap
            .chords()
            .filter_map(|b| {
                let keys: Option<Vec<String>> =
                    b.keys.split_whitespace().map(actions::canonical).collect();
                Some((keys?.join(" "), b.action.clone()))
            })
            .collect();
        let prefixes: Vec<&str> = chords
            .iter()
            .filter_map(|(k, _)| k.split_whitespace().next())
            .collect();
        let singles: Vec<(String, String)> = keymap
            .bindings
            .iter()
            .filter(|b| !b.is_chord())
            .filter_map(|b| Some((actions::canonical(&b.keys)?, b.action.clone())))
            .filter(|(k, _)| prefixes.contains(&k.as_str()))
            .collect();

        let focus = cx.focus_handle();
        window.focus(&focus);
        let mut shell = Shell {
            commands: actions::commands(&keymap),
            chord: ChordState::new(&chords, &singles),
            tabs,
            layout,
            layout_path: opts.layout_path,
            keymap,
            collapsed: HashSet::new(),
            selected: None,
            active_env: ws
                .environments
                .first()
                .map(|e| e.name.clone())
                .unwrap_or_default(),
            ws,
            picker: None,
            theme_before_preview: None,
            recent: Vec::new(),
            picker_scroll: UniformListScrollHandle::new(),
            traffic: mock::traffic(TRAFFIC_ROWS),
            traffic_scroll: UniformListScrollHandle::new(),
            traffic_selected: None,
            route_sub: HashMap::new(),
            server_section: HashMap::new(),
            reveal_secrets: false,
            notice: None,
            notice_gen: 0,
            resize: None,
            menu: None,
            problems: mock::problems(),
            logs: mock::logs(),
            focus,
            picker_focus: cx.focus_handle(),
        };
        if let Some(p) = problems.first() {
            shell.notify(p.clone(), cx);
        }
        shell
    }

    // ---- persistence -------------------------------------------------------------------

    pub(crate) fn save_layout(&mut self) {
        self.layout.tabs = self.tabs.tabs().to_vec();
        self.layout.active_tab = self.tabs.active().map(|t| t.id.clone());
        if let Some(path) = &self.layout_path
            && let Err(e) = self.layout.save(path)
        {
            eprintln!("fauxfield: could not save layout: {e:#}");
        }
    }

    // ---- notices -----------------------------------------------------------------------

    /// Shows a transient message in the status bar for four seconds.
    pub fn notify(&mut self, msg: String, cx: &mut Context<Self>) {
        self.notice_gen += 1;
        let id = self.notice_gen;
        self.notice = Some((id, msg));
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(4))
                .await;
            this.update(cx, |this, cx| {
                if this.notice.as_ref().is_some_and(|(g, _)| *g == id) {
                    this.notice = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    // ---- docks -------------------------------------------------------------------------

    pub fn toggle_left(&mut self, cx: &mut Context<Self>) {
        self.layout.left_open = !self.layout.left_open;
        self.save_layout();
        cx.notify();
    }

    pub fn toggle_right(&mut self, cx: &mut Context<Self>) {
        self.layout.right_open = !self.layout.right_open;
        self.save_layout();
        cx.notify();
    }

    pub fn toggle_bottom(&mut self, cx: &mut Context<Self>) {
        self.layout.bottom_open = !self.layout.bottom_open;
        self.save_layout();
        cx.notify();
    }

    pub fn show_bottom(&mut self, tab: BottomTab, cx: &mut Context<Self>) {
        self.layout.bottom_open = true;
        self.layout.bottom_tab = tab;
        self.save_layout();
        cx.notify();
    }

    fn on_resize_move(&mut self, ev: &MouseMoveEvent, window: &Window, cx: &mut Context<Self>) {
        let Some(r) = self.resize else { return };
        let size = window.viewport_size();
        let (x, y) = (f32::from(ev.position.x), f32::from(ev.position.y));
        match r {
            Resize::Left => self.layout.set_left_width(x),
            Resize::Right => self.layout.set_right_width(f32::from(size.width) - x),
            Resize::Bottom => self
                .layout
                .set_bottom_height(f32::from(size.height) - y - STATUS_BAR_H),
        }
        cx.notify();
    }

    fn end_resize(&mut self, cx: &mut Context<Self>) {
        if self.resize.take().is_some() {
            self.save_layout();
            cx.notify();
        }
    }

    pub(crate) fn begin_resize(&mut self, r: Resize, cx: &mut Context<Self>) {
        self.resize = Some(r);
        cx.notify();
    }

    // ---- tabs and tree -----------------------------------------------------------------

    pub(crate) fn tab_for_row(&self, id: &str) -> Option<(TabKind, String)> {
        if let Some((_, r)) = self.ws.find_route(id) {
            return Some((TabKind::Route, format!("{} {}", r.method, r.path)));
        }
        if let Some(s) = self.ws.find_server(id) {
            return Some((TabKind::Server, s.name.clone()));
        }
        let name = id.strip_prefix("env:")?;
        self.ws.find_environment(name)?;
        Some((TabKind::Environment, name.to_string()))
    }

    /// Opens a tab by its tab id (route id, server id, environment name).
    pub fn open_tab(&mut self, id: &str, kind: TabKind, title: &str, pinned: bool) {
        if pinned {
            self.tabs.open_pinned(id, kind, title);
        } else {
            self.tabs.open_preview(id, kind, title);
        }
        self.selected = Some(match kind {
            TabKind::Environment => tree::env_row_id(id),
            _ => id.to_string(),
        });
        self.save_layout();
    }

    /// Row click: routes, servers and environments open tabs; folders and sections fold.
    pub fn click_row(&mut self, row_id: &str, clicks: usize, cx: &mut Context<Self>) {
        self.selected = Some(row_id.to_string());
        if let Some((kind, title)) = self.tab_for_row(row_id) {
            let tab_id = row_id.strip_prefix("env:").unwrap_or(row_id).to_string();
            self.open_tab(&tab_id, kind, &title, clicks >= 2);
        } else if row_id.starts_with("ds:") {
            self.notify("Dataset tabs arrive in M8.".into(), cx);
        } else {
            self.toggle_row(row_id);
        }
        cx.notify();
    }

    pub fn toggle_row(&mut self, id: &str) {
        if !self.collapsed.remove(id) {
            self.collapsed.insert(id.to_string());
        }
    }

    /// Selects a row and expands its ancestors so it is visible.
    pub fn reveal_row(&mut self, id: &str) {
        self.layout.left_open = true;
        for s in &self.ws.servers {
            let in_folder = s
                .folders
                .iter()
                .find(|f| f.id == id || f.routes.iter().any(|r| r.id == id));
            let is_member =
                s.id == id || in_folder.is_some() || s.routes.iter().any(|r| r.id == id);
            if is_member {
                self.collapsed.remove(&s.id);
                if let Some(f) = in_folder {
                    self.collapsed.remove(&f.id);
                }
            }
        }
        if id.starts_with("env:") {
            self.collapsed.remove(tree::SEC_ENVIRONMENTS);
        }
        if id.starts_with("ds:") {
            self.collapsed.remove(tree::SEC_DATASETS);
        }
        self.selected = Some(id.to_string());
        self.save_layout();
    }

    pub fn activate_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.tabs.activate(index) {
            self.sync_selection_to_tab();
            self.save_layout();
            cx.notify();
        }
    }

    fn sync_selection_to_tab(&mut self) {
        if let Some(t) = self.tabs.active() {
            self.selected = Some(match t.kind {
                TabKind::Environment => tree::env_row_id(&t.id),
                _ => t.id.clone(),
            });
        }
    }

    pub fn close_tab_at(&mut self, index: usize, cx: &mut Context<Self>) {
        self.tabs.close(index);
        self.sync_selection_to_tab();
        self.save_layout();
        cx.notify();
    }

    pub fn move_tab(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        self.tabs.move_tab(from, to);
        self.save_layout();
        cx.notify();
    }

    pub fn pin_tab_at(&mut self, index: usize, cx: &mut Context<Self>) {
        self.tabs.pin(index);
        self.save_layout();
        cx.notify();
    }

    // ---- tree context menu -------------------------------------------------------------

    pub fn open_menu(&mut self, row_id: &str, x: f32, y: f32, cx: &mut Context<Self>) {
        let rows = tree::flatten(&self.ws, &self.collapsed, &self.active_env);
        let Some(row) = rows.into_iter().find(|r| r.id == row_id) else {
            return;
        };
        if tree::menu_items(&row.kind, false).is_empty() {
            return;
        }
        self.selected = Some(row_id.to_string());
        self.menu = Some(RowMenu {
            row_id: row_id.to_string(),
            kind: row.kind,
            x,
            y,
        });
        cx.notify();
    }

    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    pub fn run_menu(&mut self, act: MenuAct, cx: &mut Context<Self>) {
        let Some(m) = self.menu.take() else { return };
        match act {
            MenuAct::Open => self.click_row(&m.row_id, 1, cx),
            MenuAct::OpenPinned => self.click_row(&m.row_id, 2, cx),
            MenuAct::StartStop => {
                if let Some(i) = self.ws.servers.iter().position(|s| s.id == m.row_id) {
                    self.toggle_server_at(i, cx);
                }
            }
            MenuAct::SetActive => {
                if let Some(name) = m.row_id.strip_prefix("env:") {
                    self.set_environment(name, cx);
                }
            }
            MenuAct::NewRoute => self.stub("Creating routes", "M2", cx),
            MenuAct::Rename => self.stub("Renaming", "M2", cx),
            MenuAct::Duplicate => self.stub("Duplicating", "M2", cx),
            MenuAct::Delete => self.stub("Deleting", "M2", cx),
            MenuAct::ResetDataset => self.stub("Resetting dataset state", "M8", cx),
        }
        cx.notify();
    }

    // ---- servers and environments (mock state until M3 / M7) ----------------------------

    /// Server of the active tab, else of the selected tree row.
    fn current_server(&self) -> Option<usize> {
        let id = self
            .tabs
            .active()
            .map(|t| t.id.clone())
            .or_else(|| self.selected.clone())?;
        self.ws
            .servers
            .iter()
            .position(|s| s.id == id || s.all_routes().any(|r| r.id == id))
    }

    pub fn toggle_server_at(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(s) = self.ws.servers.get_mut(i) {
            s.running = !s.running;
            let msg = format!(
                "{} {} (mock: engine arrives in M3)",
                s.name,
                if s.running { "started" } else { "stopped" }
            );
            self.notify(msg, cx);
        }
    }

    pub fn run_all(&mut self, cx: &mut Context<Self>) {
        let start = self.ws.running_count() < self.ws.servers.len();
        for s in &mut self.ws.servers {
            s.running = start;
        }
        let msg = if start {
            "All servers started (mock: engine arrives in M3)"
        } else {
            "All servers stopped (mock: engine arrives in M3)"
        };
        self.notify(msg.into(), cx);
    }

    pub fn set_environment(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.ws.find_environment(name).is_some() {
            self.active_env = name.to_string();
            self.notify(format!("Environment: {name}"), cx);
        }
    }

    // ---- picker ------------------------------------------------------------------------

    fn picker_hits(&self, kind: PickerKind, query: &str) -> Vec<Hit> {
        match kind {
            PickerKind::Search => search::quick_search(query, &self.ws, &self.commands),
            PickerKind::Palette => {
                let needle = query.strip_prefix('>').unwrap_or(query).trim_start();
                let mut hits = search::command_hits(needle, &self.commands);
                if needle.is_empty() {
                    // Recently used commands float to the top (SPEC §9.7).
                    let rank = |h: &Hit| match &h.target {
                        Target::Action(id) => self.recent.iter().position(|r| r == id),
                        _ => None,
                    };
                    hits.sort_by_key(|h| rank(h).unwrap_or(usize::MAX));
                }
                hits
            }
            PickerKind::Theme => {
                let names: Vec<String> = theme::bundled().iter().map(|t| t.name.clone()).collect();
                search::filter_labels(query, &names)
                    .into_iter()
                    .map(|i| {
                        simple_hit(Kind::Theme, &names[i], "", Target::Theme(names[i].clone()))
                    })
                    .collect()
            }
            PickerKind::Environment => {
                let names: Vec<String> = self
                    .ws
                    .environments
                    .iter()
                    .map(|e| e.name.clone())
                    .collect();
                search::filter_labels(query, &names)
                    .into_iter()
                    .map(|i| {
                        let detail = if names[i] == self.active_env {
                            "active"
                        } else {
                            ""
                        };
                        simple_hit(
                            Kind::Environment,
                            &names[i],
                            detail,
                            Target::SwitchEnvironment(names[i].clone()),
                        )
                    })
                    .collect()
            }
        }
    }

    pub fn open_picker(&mut self, kind: PickerKind, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.as_ref().is_some_and(|p| p.kind == kind) {
            self.close_picker(true, window, cx);
            return;
        }
        if self.picker.is_some() {
            self.close_picker(true, window, cx);
        }
        let hits = self.picker_hits(kind, "");
        let mut selected = 0;
        if kind == PickerKind::Theme {
            let current = cx.global::<Theme>().name.clone();
            self.theme_before_preview = Some(current.clone());
            selected = hits
                .iter()
                .position(|h| matches!(&h.target, Target::Theme(n) if *n == current))
                .unwrap_or(0);
        }
        self.picker = Some(Picker {
            kind,
            query: String::new(),
            selected,
            hits,
        });
        self.picker_scroll
            .scroll_to_item(selected, gpui::ScrollStrategy::Top);
        window.focus(&self.picker_focus);
        cx.notify();
    }

    /// Closes the picker. `revert` restores the previewed theme (Esc).
    pub fn close_picker(&mut self, revert: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(name) = self.theme_before_preview.take()
            && revert
        {
            self.apply_theme(&name, cx);
        }
        if self.picker.take().is_some() {
            window.focus(&self.focus);
            cx.notify();
        }
    }

    fn apply_theme(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(t) = Theme::named(name) {
            cx.set_global(t);
            cx.notify();
        }
    }

    fn refresh_picker(&mut self, cx: &mut Context<Self>) {
        let Some(p) = &self.picker else { return };
        let (kind, query) = (p.kind, p.query.clone());
        let hits = self.picker_hits(kind, &query);
        if let Some(p) = &mut self.picker {
            p.hits = hits;
            p.selected = 0;
        }
        self.after_selection_change(cx);
    }

    pub fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(p) = &mut self.picker else { return };
        if p.hits.is_empty() {
            return;
        }
        let last = p.hits.len() as isize - 1;
        p.selected = (p.selected as isize + delta).clamp(0, last) as usize;
        self.after_selection_change(cx);
    }

    /// Scrolls the highlight into view and live-previews themes.
    fn after_selection_change(&mut self, cx: &mut Context<Self>) {
        let Some(p) = &self.picker else { return };
        self.picker_scroll
            .scroll_to_item(p.selected, gpui::ScrollStrategy::Center);
        if p.kind == PickerKind::Theme
            && let Some(Target::Theme(name)) = p.hits.get(p.selected).map(|h| h.target.clone())
        {
            self.apply_theme(&name, cx);
        }
        cx.notify();
    }

    pub fn picker_type(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.picker {
            p.query.push_str(text);
        }
        self.refresh_picker(cx);
    }

    pub fn picker_backspace(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.picker {
            p.query.pop();
        }
        self.refresh_picker(cx);
    }

    pub fn picker_select(
        &mut self,
        index: usize,
        mode: OpenMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(p) = &mut self.picker {
            p.selected = index;
        }
        self.run_picker_selection(mode, window, cx);
    }

    pub fn run_picker_selection(
        &mut self,
        mode: OpenMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(p) = &self.picker else { return };
        let Some(hit) = p.hits.get(p.selected).cloned() else {
            return;
        };
        match &hit.target {
            Target::Theme(name) => {
                self.apply_theme(name, cx);
                self.layout.theme = Some(name.clone());
                self.theme_before_preview = None;
                self.save_layout();
                self.close_picker(false, window, cx);
            }
            Target::SwitchEnvironment(name) => {
                self.close_picker(true, window, cx);
                self.set_environment(name, cx);
            }
            Target::Action(id) => {
                self.close_picker(true, window, cx);
                self.recent.retain(|r| r != id);
                self.recent.insert(0, id.clone());
                self.run_action_id(id, window, cx);
            }
            target => {
                self.close_picker(true, window, cx);
                self.open_target(target, mode, cx);
            }
        }
        cx.notify();
    }

    fn open_target(&mut self, target: &Target, mode: OpenMode, cx: &mut Context<Self>) {
        let (row, tab) = match target {
            Target::Route(id) => (
                id.clone(),
                self.ws.find_route(id).map(|(_, r)| {
                    (
                        id.clone(),
                        TabKind::Route,
                        format!("{} {}", r.method, r.path),
                    )
                }),
            ),
            Target::Server(id) => (
                id.clone(),
                self.ws
                    .find_server(id)
                    .map(|s| (id.clone(), TabKind::Server, s.name.clone())),
            ),
            Target::Environment(n) | Target::Variable { environment: n, .. } => (
                tree::env_row_id(n),
                Some((n.clone(), TabKind::Environment, n.clone())),
            ),
            Target::Folder(id) => (id.clone(), None),
            Target::Dataset(n) => (tree::dataset_row_id(n), None),
            _ => return,
        };
        self.reveal_row(&row);
        if mode == OpenMode::Reveal {
            return;
        }
        match tab {
            Some((id, kind, title)) => self.open_tab(&id, kind, &title, mode == OpenMode::Pinned),
            None if matches!(target, Target::Dataset(_)) => {
                self.notify("Dataset tabs arrive in M8.".into(), cx);
            }
            None => {}
        }
    }

    pub(crate) fn on_picker_key(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let m = ev.keystroke.modifiers;
        match ev.keystroke.key.as_str() {
            "escape" => self.close_picker(true, window, cx),
            "enter" => {
                let mode = if m.alt {
                    OpenMode::Reveal
                } else if m.secondary() {
                    OpenMode::Pinned
                } else {
                    OpenMode::Preview
                };
                self.run_picker_selection(mode, window, cx);
            }
            "up" => self.move_selection(-1, cx),
            "down" => self.move_selection(1, cx),
            "pageup" => self.move_selection(-8, cx),
            "pagedown" => self.move_selection(8, cx),
            "backspace" => self.picker_backspace(cx),
            _ => {
                if !m.secondary()
                    && !m.control
                    && !m.alt
                    && let Some(ch) = ev.keystroke.key_char.as_deref()
                {
                    self.picker_type(ch, cx);
                }
            }
        }
        cx.stop_propagation();
    }

    // ---- actions -----------------------------------------------------------------------

    pub fn run_action_id(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(info) = actions::find(id) {
            window.dispatch_action((info.build)(), cx);
        }
    }

    fn on_capture_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let stroke = ev.keystroke.unparse();
        match self.chord.feed(&stroke) {
            Outcome::Passthrough => {}
            Outcome::Pending(seq) => {
                cx.stop_propagation();
                cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor().timer(CHORD_TIMEOUT).await;
                    this.update_in(cx, |this, window, cx| {
                        if let Some(id) = this.chord.expire(seq) {
                            this.run_action_id(&id, window, cx);
                        }
                    })
                    .ok();
                })
                .detach();
            }
            Outcome::Run(id) => {
                cx.stop_propagation();
                self.run_action_id(&id, window, cx);
            }
            Outcome::Cancelled => cx.stop_propagation(),
        }
    }

    fn stub(&mut self, what: &str, milestone: &str, cx: &mut Context<Self>) {
        self.notify(format!("{what} arrives in {milestone}."), cx);
    }
}

fn simple_hit(kind: Kind, label: &str, detail: &str, target: Target) -> Hit {
    Hit {
        label: label.to_string(),
        detail: detail.to_string(),
        kind,
        method: None,
        binding: None,
        target,
        score: 0,
    }
}

fn tab_exists(ws: &Workspace, kind: TabKind, id: &str) -> bool {
    match kind {
        TabKind::Route => ws.find_route(id).is_some(),
        TabKind::Server => ws.find_server(id).is_some(),
        TabKind::Environment => ws.find_environment(id).is_some(),
    }
}

macro_rules! on_actions {
    ($root:expr, $cx:expr, { $($ty:ident => |$this:ident, $w:ident, $c:ident| $body:expr),* $(,)? }) => {{
        let mut root = $root;
        $(
            root = root.on_action($cx.listener(
                |$this: &mut Shell, _: &actions::$ty, $w: &mut Window, $c: &mut Context<Shell>| $body,
            ));
        )*
        root
    }};
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme::theme(cx).clone();
        let body_h = div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_row()
            .when(self.layout.left_open, |d| {
                d.child(self.render_left_dock(&t, cx))
                    .child(self.resize_handle_v(&t, Resize::Left, cx))
            })
            .child(self.render_center(&t, cx))
            .when(self.layout.right_open, |d| {
                d.child(self.resize_handle_v(&t, Resize::Right, cx))
                    .child(self.render_right_dock(&t, cx))
            });

        let root = div()
            .id("shell")
            .key_context("Shell")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .bg(t.background)
            .text_color(t.text)
            .font_family(UI_FONT)
            .text_size(px(13.))
            .capture_key_down(
                cx.listener(|this, ev: &KeyDownEvent, w, cx| this.on_capture_key(ev, w, cx)),
            )
            .on_mouse_move(
                cx.listener(|this, ev: &MouseMoveEvent, w, cx| this.on_resize_move(ev, w, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_resize(cx)),
            );

        let root = on_actions!(root, cx, {
            QuickSearch => |this, w, cx| this.open_picker(PickerKind::Search, w, cx),
            CommandPalette => |this, w, cx| this.open_picker(PickerKind::Palette, w, cx),
            SelectTheme => |this, w, cx| this.open_picker(PickerKind::Theme, w, cx),
            SwitchEnvironment => |this, w, cx| this.open_picker(PickerKind::Environment, w, cx),
            ToggleLeftDock => |this, _w, cx| this.toggle_left(cx),
            ToggleBottomDock => |this, _w, cx| this.toggle_bottom(cx),
            ToggleRightDock => |this, _w, cx| this.toggle_right(cx),
            ShowTraffic => |this, _w, cx| this.show_bottom(BottomTab::Traffic, cx),
            ShowProblems => |this, _w, cx| this.show_bottom(BottomTab::Problems, cx),
            ShowLogs => |this, _w, cx| this.show_bottom(BottomTab::Logs, cx),
            CloseTab => |this, _w, cx| {
                if let Some(i) = this.tabs.active_index() {
                    this.close_tab_at(i, cx);
                }
            },
            PinTab => |this, _w, cx| {
                if let Some(i) = this.tabs.active_index() {
                    this.pin_tab_at(i, cx);
                }
            },
            ActivateTab1 => |this, _w, cx| this.activate_tab(0, cx),
            ActivateTab2 => |this, _w, cx| this.activate_tab(1, cx),
            ActivateTab3 => |this, _w, cx| this.activate_tab(2, cx),
            ActivateTab4 => |this, _w, cx| this.activate_tab(3, cx),
            ActivateTab5 => |this, _w, cx| this.activate_tab(4, cx),
            ActivateTab6 => |this, _w, cx| this.activate_tab(5, cx),
            ActivateTab7 => |this, _w, cx| this.activate_tab(6, cx),
            ActivateTab8 => |this, _w, cx| this.activate_tab(7, cx),
            ActivateTab9 => |this, _w, cx| this.activate_tab(8, cx),
            RunAll => |this, _w, cx| this.run_all(cx),
            ToggleServer => |this, _w, cx| {
                match this.current_server() {
                    Some(i) => this.toggle_server_at(i, cx),
                    None => this.notify("Select a server or route first.".into(), cx),
                }
            },
            NewServer => |this, _w, cx| this.stub("Creating servers", "M2", cx),
            NewRoute => |this, _w, cx| this.stub("Creating routes", "M2", cx),
            SendTest => |this, w, cx| {
                // Cmd+Enter is also "open pinned" inside Quick Search.
                if this.picker.is_some() {
                    this.run_picker_selection(OpenMode::Pinned, w, cx);
                } else {
                    this.stub("Send test", "M3", cx);
                }
            },
            Duplicate => |this, _w, cx| this.stub("Duplicating", "M2", cx),
            ResetState => |this, _w, cx| this.stub("Resetting dataset state", "M8", cx),
            Quit => |_this, _w, cx| cx.quit(),
        });

        root.child(self.render_title_bar(&t, cx))
            .child(body_h_with_bottom(self, body_h, &t, cx))
            .child(self.render_status_bar(&t, cx))
            .when(self.picker.is_some(), |d| {
                d.child(self.render_picker(&t, cx))
            })
            .when(self.menu.is_some(), |d| {
                d.child(self.render_row_menu(&t, cx))
            })
    }
}

fn body_h_with_bottom(
    shell: &Shell,
    top: impl IntoElement,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(top)
        .when(shell.layout.bottom_open, |d| {
            d.child(shell.resize_handle_h(t, cx))
                .child(shell.render_bottom_dock(t, cx))
        })
}

impl Shell {
    /// A 1 px divider with a 5 px invisible grab area, so the pane edge looks hairline-thin
    /// (SPEC §9.1: borders over shadows) while staying easy to drag.
    pub(crate) fn resize_handle_v(
        &self,
        t: &Theme,
        r: Resize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(match r {
                Resize::Left => "resize-left",
                _ => "resize-right",
            })
            .relative()
            .w(px(1.))
            .h_full()
            .flex_none()
            .bg(t.border)
            .child(
                div()
                    .id(match r {
                        Resize::Left => "resize-left-grab",
                        _ => "resize-right-grab",
                    })
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(-2.))
                    .w(px(5.))
                    .cursor_col_resize()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.begin_resize(r, cx)),
                    ),
            )
    }

    pub(crate) fn resize_handle_h(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("resize-bottom")
            .relative()
            .h(px(1.))
            .w_full()
            .flex_none()
            .bg(t.border)
            .child(
                div()
                    .id("resize-bottom-grab")
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(-2.))
                    .h(px(5.))
                    .cursor_row_resize()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.begin_resize(Resize::Bottom, cx)),
                    ),
            )
    }
}
