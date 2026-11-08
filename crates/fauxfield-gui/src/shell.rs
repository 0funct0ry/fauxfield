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
use crate::search::{self, CommandEntry, Hit, Kind, Target};
use crate::tabs::{TabKind, TabSet};
use crate::theme::{self, Theme};
use crate::traffic::TrafficRow;
use crate::tree::{self, MenuAct, RowKind};
use crate::ui::STATUS_BAR_H;
use crate::view::{Problem, Workspace};
use fauxfield_store::{Background, Event as StoreEvent, Store};

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
    /// A one-line text prompt (rename, new item, field edit).
    Prompt,
}

/// What a confirmed Prompt does (M2). The value is the picker query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptAction {
    NewServer,
    ServerPort {
        server: String,
    },
    OpenFolder,
    CreateWorkspace,
    RenameWorkspace,
    ServerHost {
        server: String,
    },
    ServerPrefix {
        server: String,
    },
    ResponseBody {
        response: String,
    },
    NewFolder {
        server: String,
    },
    /// Value is `METHOD /path`.
    NewRoute {
        server: String,
        folder: Option<String>,
    },
    Rename {
        id: String,
    },
    RouteMethod {
        route: String,
    },
    RoutePath {
        route: String,
    },
    ResponseStatus {
        response: String,
    },
    ResponseName {
        response: String,
    },
    ResponseContentType {
        response: String,
    },
    ResponseDelay {
        response: String,
    },
    ResponseHeader {
        response: String,
    },
}

/// Which field of the current route or response a field-edit action changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldEdit {
    RouteMethod,
    RoutePath,
    ResponseStatus,
    ResponseName,
    ResponseContentType,
    ResponseDelay,
    ResponseHeader,
    ResponseBody,
}

impl PromptAction {
    /// One line of help shown under the prompt while no error is showing.
    pub fn help(&self) -> &'static str {
        match self {
            PromptAction::RenameWorkspace => {
                "Changes the name in fauxfield.toml. The folder on disk is not renamed."
            }
            PromptAction::ServerHost { .. } => {
                "0.0.0.0 or :: makes the server reachable from your network."
            }
            PromptAction::ServerPrefix { .. } => "Starts with /, no trailing /. Empty removes it.",
            _ => "",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            PromptAction::NewServer => "New server name",
            PromptAction::ServerPort { .. } => "Port (1-65535)",
            PromptAction::OpenFolder => "Open workspace folder",
            PromptAction::CreateWorkspace => "Create workspace in folder",
            PromptAction::RenameWorkspace => "Workspace name",
            PromptAction::ServerHost { .. } => "Host (IP address or hostname)",
            PromptAction::ServerPrefix { .. } => "Prefix (empty removes it)",
            PromptAction::NewFolder { .. } => "New folder name",
            PromptAction::NewRoute { .. } => "New route (METHOD /path)",
            PromptAction::Rename { .. } => "New name",
            PromptAction::RouteMethod { .. } => "Method",
            PromptAction::RoutePath { .. } => "Path",
            PromptAction::ResponseStatus { .. } => "Status",
            PromptAction::ResponseName { .. } => "Response name",
            PromptAction::ResponseContentType { .. } => "Content type",
            PromptAction::ResponseDelay { .. } => "Delay (ms)",
            PromptAction::ResponseHeader { .. } => "Header (name: value, empty value removes)",
            PromptAction::ResponseBody { .. } => "Body",
        }
    }
}

pub struct Picker {
    pub kind: PickerKind,
    pub query: String,
    /// Select All was pressed: the next typed text or Backspace replaces the whole query.
    pub all_selected: bool,
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
    /// Workspace folder to open; `None` shows the Welcome view.
    pub workspace: Option<PathBuf>,
    /// File holding the recent-workspaces list; `None` disables it (tests).
    pub recent_path: Option<PathBuf>,
    /// Added to every server port when binding (tests run servers side by side).
    pub port_offset: u16,
}

pub struct Shell {
    pub(crate) ws: Workspace,
    /// The open workspace; `None` shows the Welcome view.
    pub(crate) store: Option<Store>,
    pub(crate) store_events: Option<std::sync::mpsc::Receiver<StoreEvent>>,
    pub(crate) _background: Option<Background>,
    pub(crate) recent_path: Option<PathBuf>,
    pub(crate) recents: Vec<PathBuf>,
    /// Files whose external edit collides with unsaved GUI edits (merge banner).
    pub(crate) conflicts: Vec<String>,
    /// What the open Prompt will do with the typed value.
    pub(crate) prompt: Option<PromptAction>,
    /// Selected response index per route id (Responses sub-tab).
    pub(crate) resp_sel: HashMap<String, usize>,
    pub(crate) tabs: TabSet,
    pub(crate) layout: Layout,
    layout_path: Option<PathBuf>,
    pub(crate) keymap: Keymap,
    pub(crate) commands: Vec<CommandEntry>,
    chord: ChordState,
    pub(crate) collapsed: HashSet<String>,
    pub(crate) sidebar_tab: tree::SidebarTab,
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
    /// Dev-only: render volatile text (Traffic time and duration) as fixed placeholders so
    /// repeated screenshots compare equal.
    #[cfg(feature = "dev-mcp")]
    pub(crate) mask_volatile: bool,
    pub(crate) notice: Option<(u64, String)>,
    notice_gen: u64,
    pub(crate) resize: Option<Resize>,
    pub(crate) menu: Option<RowMenu>,
    pub(crate) problems: Vec<Problem>,
    pub(crate) logs: Vec<String>,
    /// The host for the open workspace (engine, control socket). Declared before `rt` so it
    /// drops first.
    pub(crate) host: Option<fauxfield_control::Host>,
    pub(crate) rt: Option<tokio::runtime::Runtime>,
    pub(crate) engine_events:
        Option<tokio::sync::broadcast::Receiver<fauxfield_engine::EngineEvent>>,
    pub(crate) notice_action: Option<crate::run::NoticeAction>,
    pub(crate) quit_confirm: bool,
    pub(crate) quit_confirmed: bool,
    pub(crate) select_next_test: bool,
    pub(crate) port_offset: u16,
    pub(crate) focus: FocusHandle,
    pub(crate) picker_focus: FocusHandle,
    pub(crate) lc: crate::lifecycle::Lifecycle,
    /// M3.2: where the menu is drawn, the last context, the last native model, and the seams.
    pub(crate) menu_style: crate::menu::MenuStyle,
    pub(crate) menu_ctx: Option<crate::menu::MenuContext>,
    pub(crate) menu_last: Option<Vec<crate::menu::MenuDef>>,
    pub(crate) menu_sink: Box<dyn crate::menu_shell::MenuSink>,
    pub(crate) url_opener: Box<dyn crate::menu_shell::UrlOpener>,
    pub(crate) about_open: bool,
    pub(crate) bar: crate::menubar::Bar,
    /// Dev-mcp actions waiting for the frames that re-register their handlers.
    #[cfg(feature = "dev-mcp")]
    pub(crate) dev_pending: u32,
    /// Last item run through the menu (sequence, label path), for `ui.state`/`ui.events`.
    pub(crate) menu_invoked: Option<(u64, Vec<String>)>,
}

impl Focusable for Shell {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Shell {
    pub fn new(opts: ShellOptions, window: &mut Window, cx: &mut Context<Self>) -> Shell {
        let recents = opts
            .recent_path
            .as_deref()
            .map(fauxfield_store::load_recent)
            .unwrap_or_default();
        let mut open_error = None;
        let (store, ws, store_problems) = match opts.workspace.as_deref().map(Store::open) {
            Some(Ok(store)) => {
                let snap = store.snapshot();
                if let Some(p) = &opts.recent_path {
                    let _ = fauxfield_store::push_recent(p, &snap.root);
                }
                let ws = Workspace::from_snapshot(&snap);
                let problems = Workspace::problems(&snap);
                (Some(store), ws, problems)
            }
            Some(Err(e)) => {
                open_error = Some(e.to_string());
                (None, Workspace::empty(), vec![])
            }
            None => (None, Workspace::empty(), vec![]),
        };
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
        let layout_sidebar_tab = layout.sidebar_tab.clone();
        let mut shell = Shell {
            commands: actions::commands(&keymap),
            chord: ChordState::new(&chords, &singles),
            tabs,
            layout,
            layout_path: opts.layout_path,
            keymap,
            collapsed: HashSet::new(),
            sidebar_tab: tree::SidebarTab::from_id(&layout_sidebar_tab),
            selected: None,
            active_env: ws
                .environments
                .first()
                .map(|e| e.name.clone())
                .unwrap_or_default(),
            ws,
            store,
            store_events: None,
            _background: None,
            recent_path: opts.recent_path,
            recents,
            conflicts: Vec::new(),
            prompt: None,
            resp_sel: HashMap::new(),
            picker: None,
            theme_before_preview: None,
            recent: Vec::new(),
            picker_scroll: UniformListScrollHandle::new(),
            // Real rows come from the engine; synthetic ones feed tests and dev-mcp scenarios.
            traffic: initial_traffic(),
            traffic_scroll: UniformListScrollHandle::new(),
            traffic_selected: None,
            route_sub: HashMap::new(),
            server_section: HashMap::new(),
            reveal_secrets: false,
            #[cfg(feature = "dev-mcp")]
            mask_volatile: false,
            notice: None,
            notice_gen: 0,
            resize: None,
            menu: None,
            problems: store_problems,
            logs: Vec::new(),
            host: None,
            rt: None,
            engine_events: None,
            notice_action: None,
            quit_confirm: false,
            quit_confirmed: false,
            select_next_test: false,
            port_offset: opts.port_offset,
            focus,
            picker_focus: cx.focus_handle(),
            lc: Default::default(),
            menu_style: crate::menu::menu_style(),
            menu_ctx: None,
            menu_last: None,
            menu_sink: Box::new(crate::menu_shell::NativeSink),
            url_opener: Box::new(crate::menu_shell::SystemOpener),
            about_open: false,
            bar: Default::default(),
            #[cfg(feature = "dev-mcp")]
            dev_pending: 0,
            menu_invoked: None,
        };
        if let Some(p) = problems.first() {
            shell.notify(p.clone(), cx);
        }
        if let Some(e) = open_error {
            shell.notify(format!("Could not open workspace: {e}"), cx);
        }
        // Saved tab titles may be stale (files edited while the app was closed).
        shell.refresh_tab_titles();
        shell.attach_store(cx);
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |_w, cx| {
            weak.update(cx, |s, cx| s.should_close(cx)).unwrap_or(true)
        });
        shell
    }

    // ---- workspace store ---------------------------------------------------------------

    /// Start autosave and the watcher, and poll store events into the view.
    fn attach_store(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        #[cfg(feature = "dev-mcp")]
        crate::dev_mcp::host::publish_store(Some(store.clone()));
        self.store_events = Some(store.subscribe());
        self._background = store
            .start_background(std::time::Duration::from_millis(300))
            .map_err(|e| eprintln!("fauxfield: watcher unavailable: {e}"))
            .ok();
        self.attach_engine(cx);
        self.lc.poll_gen += 1;
        let gen_id = self.lc.poll_gen;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(60))
                    .await;
                // A newer workspace (or a close) retires this loop.
                let alive = this
                    .update(cx, |this, cx| {
                        if this.lc.poll_gen != gen_id {
                            return false;
                        }
                        this.poll_store(cx);
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    fn poll_store(&mut self, cx: &mut Context<Self>) {
        self.poll_engine(cx);
        let Some(rx) = &self.store_events else { return };
        let mut changed = false;
        while let Ok(ev) = rx.try_recv() {
            let _ = ev;
            changed = true;
        }
        if changed {
            self.reload(cx);
        }
    }

    /// Rebuild the view from the store (after an edit, undo, or external reload).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let snap = store.snapshot();
        let running: HashSet<String> = self
            .ws
            .servers
            .iter()
            .filter(|s| s.running)
            .map(|s| s.id.clone())
            .collect();
        self.ws = Workspace::from_snapshot(&snap);
        for s in &mut self.ws.servers {
            s.running = running.contains(&s.id);
        }
        self.sync_running();
        // The active environment may have been renamed or deleted on disk.
        if self.ws.find_environment(&self.active_env).is_none() {
            self.active_env = self
                .ws
                .environments
                .first()
                .map(|e| e.name.clone())
                .unwrap_or_default();
        }
        self.problems = Workspace::problems(&snap);
        self.conflicts = store.conflicts();
        let ws = &self.ws;
        let before = self.tabs.tabs().len();
        let gone: Vec<usize> = self
            .tabs
            .tabs()
            .iter()
            .enumerate()
            .filter(|(_, t)| !tab_exists(ws, t.kind, &t.id))
            .map(|(i, _)| i)
            .collect();
        for i in gone.into_iter().rev() {
            self.tabs.close(i);
        }
        let retitled = self.refresh_tab_titles();
        if self.tabs.tabs().len() != before {
            self.sync_selection_to_tab();
            self.save_layout();
        } else if retitled {
            self.save_layout();
        }
        cx.notify();
    }

    /// Open a workspace folder (Welcome, `ui.open_workspace`, palette).
    pub fn open_workspace(
        &mut self,
        path: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let store = Store::open(path).map_err(|e| e.to_string())?;
        // Switching: release the previous workspace first (servers, host.json, socket).
        if self.store.is_some() {
            self.teardown();
        }
        let snap = store.snapshot();
        if let Some(p) = &self.recent_path {
            let _ = fauxfield_store::push_recent(p, &snap.root);
            self.recents = fauxfield_store::load_recent(p);
        }
        self.lc
            .opened_at
            .insert(snap.root.clone(), std::time::SystemTime::now());
        self.ws = Workspace::from_snapshot(&snap);
        self.problems = Workspace::problems(&snap);
        self.store = Some(store);
        self.tabs = TabSet::from_saved(vec![], None);
        self.selected = None;
        self.active_env = self
            .ws
            .environments
            .first()
            .map(|e| e.name.clone())
            .unwrap_or_default();
        self.attach_store(cx);
        cx.notify();
        Ok(())
    }

    /// Create a workspace (SPEC §4.1) in `path` and open it.
    pub fn create_workspace(
        &mut self,
        path: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        fauxfield_store::init_workspace(path).map_err(|e| e.to_string())?;
        self.open_workspace(path, cx)
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
        self.notice_action = None;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(4))
                .await;
            this.update(cx, |this, cx| {
                if this.notice.as_ref().is_some_and(|(g, _)| *g == id) {
                    this.notice = None;
                    this.notice_action = None;
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

    /// Title a tab of `kind` and `id` should show right now (derived from the files).
    fn tab_title(&self, kind: TabKind, id: &str) -> Option<String> {
        match kind {
            TabKind::Route => self
                .ws
                .find_route(id)
                .map(|(_, r)| format!("{} {}", r.method, r.path)),
            TabKind::Server => self.ws.find_server(id).map(|s| s.name.clone()),
            TabKind::Environment => self.ws.find_environment(id).map(|e| e.name.clone()),
        }
    }

    /// Keep tab titles in step with the workspace: a verb, path or name edit (GUI or external)
    /// changes the title. Returns true when any title changed.
    pub(crate) fn refresh_tab_titles(&mut self) -> bool {
        let wanted: Vec<(TabKind, String, String)> = self
            .tabs
            .tabs()
            .iter()
            .filter_map(|t| {
                self.tab_title(t.kind, &t.id)
                    .map(|title| (t.kind, t.id.clone(), title))
            })
            .collect();
        let mut changed = false;
        for (kind, id, title) in wanted {
            changed |= self.tabs.retitle(kind, &id, &title);
        }
        changed
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
        if let Some(sel) = &self.selected {
            let tab = tree::SidebarTab::of_row(sel);
            self.sidebar_tab = tab;
            self.layout.sidebar_tab = tab.id().into();
        }
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
        self.sidebar_tab = tree::SidebarTab::of_row(id);
        self.layout.sidebar_tab = self.sidebar_tab.id().into();
        self.selected = Some(id.to_string());
        self.save_layout();
    }

    /// Selects a sidebar tab, showing the left dock if hidden. Re-selecting the active tab
    /// only reveals a hidden dock; it never collapses it.
    pub fn set_sidebar_tab(&mut self, tab: tree::SidebarTab, cx: &mut Context<Self>) {
        if self.sidebar_tab == tab && self.layout.left_open {
            return;
        }
        self.sidebar_tab = tab;
        self.layout.sidebar_tab = tab.id().into();
        self.layout.left_open = true;
        self.save_layout();
        cx.notify();
    }

    /// The selected row, only while it belongs to the visible sidebar tab.
    pub(crate) fn selected_visible(&self) -> Option<String> {
        self.selected
            .clone()
            .filter(|id| tree::SidebarTab::of_row(id) == self.sidebar_tab)
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

    /// Test helper: run a server-menu item by its action key (`duplicate`, `delete`).
    #[cfg(test)]
    pub(crate) fn run_menu_for(&mut self, key: &str, id: &str, cx: &mut Context<Self>) {
        match key {
            "duplicate" => self.duplicate_row(id, cx),
            "delete" => self.delete_row(id, cx),
            _ => {}
        }
    }

    pub fn open_menu(&mut self, row_id: &str, x: f32, y: f32, cx: &mut Context<Self>) {
        if row_id == tree::WORKSPACE_ROW {
            self.menu = Some(RowMenu {
                row_id: row_id.to_string(),
                kind: RowKind::Workspace,
                x,
                y,
            });
            cx.notify();
            return;
        }
        let rows = tree::flatten(
            &self.ws,
            &self.collapsed,
            &self.active_env,
            self.sidebar_tab,
        );
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

    pub fn run_menu(&mut self, act: MenuAct, window: &mut Window, cx: &mut Context<Self>) {
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
            MenuAct::NewRoute => self.new_route_at(&m.row_id, window, cx),
            MenuAct::NewFolder => self.new_folder_at(&m.row_id, window, cx),
            MenuAct::NewResponse => self.new_response_for(&m.row_id, cx),
            MenuAct::Rename => self.rename_row(&m.row_id, window, cx),
            MenuAct::Duplicate => self.duplicate_row(&m.row_id, cx),
            MenuAct::Delete => self.delete_row(&m.row_id, cx),
            MenuAct::ResetDataset => self.stub("Resetting dataset state", "M8", cx),
            MenuAct::Restart => self.restart_server_id(&m.row_id, cx),
            MenuAct::ToggleEnabled => self.toggle_enabled(&m.row_id, cx),
            MenuAct::ChangePort => self.change_port(&m.row_id, window, cx),
            MenuAct::ChangeHost => self.change_host(&m.row_id, window, cx),
            MenuAct::ChangePrefix => self.change_prefix(&m.row_id, window, cx),
            MenuAct::NewServer => self.begin_prompt(PromptAction::NewServer, "", window, cx),
            MenuAct::RenameWorkspace => self.rename_workspace(window, cx),
            MenuAct::OpenFolder => self.pick_folder(false, window, cx),
            MenuAct::CreateWorkspace => self.pick_folder(true, window, cx),
            MenuAct::CloseWorkspace => self.guard(crate::lifecycle::Intent::Close, cx),
            MenuAct::ClearRecent => self.clear_recent(cx),
            MenuAct::OpenRecent(i) => {
                if let Some(p) = self.recents.get(i).cloned() {
                    self.open_recent(&p, cx);
                }
            }
        }
        cx.notify();
    }

    // ---- servers and environments (mock state until M3 / M7) ----------------------------

    /// Server of the active tab, else of the selected tree row.
    pub(crate) fn current_server(&self) -> Option<usize> {
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
            PickerKind::Prompt => {
                let title = self
                    .prompt
                    .as_ref()
                    .map(PromptAction::title)
                    .unwrap_or("Value");
                // An inline error replaces the help line while the value is invalid.
                let detail = match (&self.lc.prompt_error, self.prompt.as_ref()) {
                    (Some(e), _) => e.clone(),
                    (None, Some(p)) if !p.help().is_empty() => p.help().to_string(),
                    _ => "Enter to confirm".to_string(),
                };
                vec![simple_hit(
                    Kind::Command,
                    &format!("{title}: {query}"),
                    &detail,
                    Target::Prompt,
                )]
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
            all_selected: false,
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
            self.prompt = None;
            window.focus(&self.focus);
            cx.notify();
        }
    }

    pub(crate) fn apply_theme(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(t) = Theme::named(name) {
            cx.set_global(t);
            cx.notify();
        }
    }

    pub(crate) fn refresh_picker(&mut self, cx: &mut Context<Self>) {
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
            if std::mem::take(&mut p.all_selected) {
                p.query.clear();
            }
            p.query.push_str(text);
        }
        self.refresh_picker(cx);
    }

    pub fn picker_backspace(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.picker {
            if std::mem::take(&mut p.all_selected) {
                p.query.clear();
            } else {
                p.query.pop();
            }
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
            Target::Prompt => {
                let value = self
                    .picker
                    .as_ref()
                    .map(|p| p.query.clone())
                    .unwrap_or_default();
                // Invalid input keeps the prompt open with an inline message, not a toast.
                if let Some(a) = &self.prompt
                    && let Err(e) = Shell::validate_prompt(a, value.trim())
                {
                    self.lc.prompt_error = Some(e);
                    self.refresh_picker(cx);
                    return;
                }
                let action = self.prompt.take();
                self.close_picker(true, window, cx);
                if let Some(action) = action {
                    self.apply_prompt(action, value.trim(), cx);
                }
            }
            Target::Action(id) => {
                self.close_picker(true, window, cx);
                self.recent.retain(|r| r != id);
                self.recent.insert(0, id.clone());
                // The picker was the modal that greyed the menu; dispatch only after the next
                // frame has re-registered the handlers for the now-modal-free state.
                self.run_action_id_next_frame(id, window, cx);
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

    /// True while the first key of a chord waits for its second key or the 400 ms timeout.
    #[cfg(feature = "dev-mcp")]
    pub(crate) fn chord_pending(&self) -> bool {
        self.chord.is_pending()
    }

    pub fn run_action_id_next_frame(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(info) = actions::find(id) {
            let build = info.build;
            // Frame callbacks run before the frame is drawn, so wait one more frame: the
            // second one runs after a render that saw the closed picker.
            #[cfg(not(test))]
            {
                let this = cx.entity();
                #[cfg(feature = "dev-mcp")]
                {
                    self.dev_pending += 1;
                }
                window.on_next_frame(move |w, _| {
                    w.on_next_frame(move |w, cx| {
                        w.dispatch_action(build(), cx);
                        #[cfg(feature = "dev-mcp")]
                        this.update(cx, |s, _| s.dev_pending = s.dev_pending.saturating_sub(1));
                        #[cfg(not(feature = "dev-mcp"))]
                        let _ = &this;
                    });
                    w.refresh();
                });
            }
            // The test platform draws at the end of each effect cycle and runs no frame
            // callbacks, so a spawned task (which runs after that draw) plays their role.
            #[cfg(test)]
            cx.spawn_in(window, async move |this, cx| {
                this.update_in(cx, |_, w, cx| w.dispatch_action(build(), cx))
                    .ok();
            })
            .detach();
        }
        cx.notify();
    }

    pub fn run_action_id(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(info) = actions::find(id) {
            window.dispatch_action((info.build)(), cx);
        }
    }

    fn on_capture_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.bar.open.is_some() && self.bar_key(ev, window, cx) {
            cx.stop_propagation();
            return;
        }
        if self.bar_mnemonic(ev, cx) {
            cx.stop_propagation();
            return;
        }
        if self.about_open && matches!(ev.keystroke.key.as_str(), "escape" | "enter") {
            self.close_about(cx);
            cx.stop_propagation();
            return;
        }
        if self.any_dialog_open() && ev.keystroke.key == "escape" {
            self.cancel_dialog(cx);
            cx.stop_propagation();
            return;
        }
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

    // ---- workspace editing (M2) ----------------------------------------------------------

    /// Open a one-line prompt; confirming applies `action` with the typed value.
    pub fn begin_prompt(
        &mut self,
        action: PromptAction,
        initial: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.store.is_none()
            && !matches!(
                action,
                PromptAction::OpenFolder | PromptAction::CreateWorkspace
            )
        {
            self.notify("Open a workspace first.".into(), cx);
            return;
        }
        if self.picker.is_some() {
            self.close_picker(true, window, cx);
        }
        self.lc.prompt_error = None;
        self.prompt = Some(action);
        self.open_picker(PickerKind::Prompt, window, cx);
        if let Some(p) = &mut self.picker {
            p.query = initial.to_string();
        }
        self.refresh_picker(cx);
    }

    /// Run a store mutation, then rebuild the view; errors become notices.
    pub(crate) fn store_op<R>(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&Store) -> Result<R, fauxfield_store::StoreError>,
    ) -> Option<R> {
        let Some(store) = self.store.clone() else {
            self.notify("Open a workspace first.".into(), cx);
            return None;
        };
        // Any edit made here targets a server file; Undo follows it again.
        self.lc.undo_file = None;
        match f(&store) {
            Ok(r) => {
                self.reload(cx);
                Some(r)
            }
            Err(e) => {
                self.notify(e.to_string(), cx);
                None
            }
        }
    }

    pub(crate) fn current_route_id(&self) -> Option<String> {
        let id = self
            .tabs
            .active()
            .filter(|t| t.kind == TabKind::Route)
            .map(|t| t.id.clone())
            .or_else(|| self.selected.clone())?;
        self.ws.find_route(&id).map(|_| id)
    }

    pub(crate) fn current_server_id(&self) -> Option<String> {
        self.current_server().map(|i| self.ws.servers[i].id.clone())
    }

    pub(crate) fn current_response_id(&self) -> Option<String> {
        let rid = self.current_route_id()?;
        let (_, r) = self.ws.find_route(&rid)?;
        let i = self
            .resp_sel
            .get(&rid)
            .copied()
            .unwrap_or(0)
            .min(r.responses.len().saturating_sub(1));
        r.responses.get(i).map(|x| x.id.clone())
    }

    fn selected_is_route(&self) -> bool {
        self.selected_visible()
            .is_some_and(|id| self.ws.find_route(&id).is_some())
    }

    fn folder_server(&self, folder: &str) -> Option<String> {
        self.ws
            .servers
            .iter()
            .find(|s| s.folders.iter().any(|f| f.id == folder))
            .map(|s| s.id.clone())
    }

    fn new_route_at(&mut self, row: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (server, folder) = if self.ws.find_server(row).is_some() {
            (Some(row.to_string()), None)
        } else {
            (self.folder_server(row), Some(row.to_string()))
        };
        match server {
            Some(server) => self.begin_prompt(
                PromptAction::NewRoute { server, folder },
                "GET /",
                window,
                cx,
            ),
            None => self.notify("Select a server or folder first.".into(), cx),
        }
    }

    fn new_folder_at(&mut self, row: &str, window: &mut Window, cx: &mut Context<Self>) {
        match self.ws.find_server(row) {
            Some(s) => {
                let server = s.id.clone();
                self.begin_prompt(PromptAction::NewFolder { server }, "", window, cx)
            }
            None => self.notify("Select a server first.".into(), cx),
        }
    }

    fn new_response_for(&mut self, route: &str, cx: &mut Context<Self>) {
        if self.ws.find_route(route).is_none() {
            return self.notify("Select a route first.".into(), cx);
        }
        let route = route.to_string();
        if let Some(id) = self.store_op(cx, |s| s.create_response(&route)) {
            let n = self
                .ws
                .find_route(&route)
                .map(|(_, r)| r.responses.len())
                .unwrap_or(1);
            self.resp_sel.insert(route, n.saturating_sub(1));
            self.notify(format!("Created response {id}"), cx);
        }
    }

    fn row_target(&self, id: &str) -> Option<fauxfield_store::Target> {
        use fauxfield_store::Target as T;
        if self.ws.find_server(id).is_some() {
            Some(T::Server(id.into()))
        } else if self.ws.find_route(id).is_some() {
            Some(T::Route(id.into()))
        } else if self.folder_server(id).is_some() {
            Some(T::Folder(id.into()))
        } else {
            None
        }
    }

    pub(crate) fn rename_server_row(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.rename_row(id, window, cx)
    }

    fn rename_row(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if id.starts_with("env:") {
            return self.stub("Renaming environments", "M7", cx);
        }
        let current = if let Some(s) = self.ws.find_server(id) {
            s.name.clone()
        } else if let Some((_, r)) = self.ws.find_route(id) {
            r.name.clone()
        } else if let Some(f) = self
            .ws
            .servers
            .iter()
            .flat_map(|s| &s.folders)
            .find(|f| f.id == id)
        {
            f.name.clone()
        } else {
            return self.notify("Nothing to rename here.".into(), cx);
        };
        self.begin_prompt(PromptAction::Rename { id: id.into() }, &current, window, cx);
    }

    fn duplicate_row(&mut self, id: &str, cx: &mut Context<Self>) {
        if id.starts_with("env:") {
            return self.stub("Duplicating environments", "M7", cx);
        }
        let target = self.row_target(id).or_else(|| {
            self.ws
                .servers
                .iter()
                .flat_map(|s| s.all_routes())
                .flat_map(|r| &r.responses)
                .find(|x| x.id == id)
                .map(|_| fauxfield_store::Target::Response(id.into()))
        });
        match target {
            Some(t) => {
                if let Some(new) = self.store_op(cx, |s| s.duplicate(&t)) {
                    self.reveal_row(&new);
                    self.notify(format!("Duplicated as {new}"), cx);
                }
            }
            None => self.notify("Nothing to duplicate here.".into(), cx),
        }
    }

    fn delete_row(&mut self, id: &str, cx: &mut Context<Self>) {
        if id.starts_with("env:") {
            return self.stub("Deleting environments", "M7", cx);
        }
        if self.ws.find_server(id).is_some() {
            // Deleting a server asks first (modal `delete-confirm`).
            return self.request_delete_server(id, cx);
        }
        match self.row_target(id) {
            Some(t) => {
                if self.store_op(cx, |s| s.delete(&t)).is_some() {
                    self.selected = None;
                    self.notify("Deleted (undo with Edit: Undo)".into(), cx);
                }
            }
            None => self.notify("Nothing to delete here.".into(), cx),
        }
    }

    pub(crate) fn file_for_edit(&self) -> Option<String> {
        let store = self.store.as_ref()?;
        let id = self
            .tabs
            .active()
            .map(|t| t.id.clone())
            .or_else(|| self.selected.clone())?;
        let s = self.ws.servers.iter().find(|s| {
            s.id == id || s.all_routes().any(|r| r.id == id) || s.folders.iter().any(|f| f.id == id)
        })?;
        let _ = store;
        Some(s.file.clone())
    }

    fn undo_redo(&mut self, undo: bool, cx: &mut Context<Self>) {
        let Some(file) = self.lc.undo_file.clone().or_else(|| self.file_for_edit()) else {
            return self.notify("Nothing to undo here.".into(), cx);
        };
        let Some(store) = self.store.clone() else {
            return;
        };
        let done = if undo {
            store.undo(&file)
        } else {
            store.redo(&file)
        };
        if done {
            self.reload(cx);
        } else {
            self.notify(
                format!("Nothing to {}.", if undo { "undo" } else { "redo" }),
                cx,
            );
        }
    }

    fn resolve_merge(&mut self, keep_mine: bool, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        for f in store.conflicts() {
            let _ = store.resolve_conflict(&f, keep_mine);
        }
        self.reload(cx);
    }

    fn edit_field(&mut self, field: FieldEdit, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(route), resp) = (self.current_route_id(), self.current_response_id()) else {
            return self.notify("Open a route first.".into(), cx);
        };
        let Some((_, r)) = self.ws.find_route(&route) else {
            return;
        };
        let cur_resp = resp
            .as_ref()
            .and_then(|id| r.responses.iter().find(|x| &x.id == id));
        let (action, initial) = match field {
            FieldEdit::RouteMethod => (PromptAction::RouteMethod { route }, r.method.clone()),
            FieldEdit::RoutePath => (PromptAction::RoutePath { route }, r.path.clone()),
            f => {
                let (Some(response), Some(x)) = (resp, cur_resp) else {
                    return self.notify("This route has no response yet.".into(), cx);
                };
                match f {
                    FieldEdit::ResponseStatus => (
                        PromptAction::ResponseStatus { response },
                        x.status.to_string(),
                    ),
                    FieldEdit::ResponseName => {
                        (PromptAction::ResponseName { response }, x.name.clone())
                    }
                    FieldEdit::ResponseContentType => (
                        PromptAction::ResponseContentType { response },
                        x.content_type.clone(),
                    ),
                    FieldEdit::ResponseDelay => (
                        PromptAction::ResponseDelay { response },
                        x.delay_ms.to_string(),
                    ),
                    FieldEdit::ResponseHeader => {
                        (PromptAction::ResponseHeader { response }, String::new())
                    }
                    _ => (
                        PromptAction::ResponseBody { response },
                        x.body.replace('\n', " "),
                    ),
                }
            }
        };
        self.begin_prompt(action, &initial, window, cx);
    }

    /// Apply a confirmed Prompt.
    fn apply_prompt(&mut self, action: PromptAction, value: &str, cx: &mut Context<Self>) {
        use fauxfield_store::Target as T;
        let v = value.to_string();
        match action {
            PromptAction::OpenFolder => self.open_folder_chosen(PathBuf::from(&v), cx),
            PromptAction::CreateWorkspace => self.create_folder_chosen(PathBuf::from(&v), cx),
            PromptAction::RenameWorkspace => {
                if self.store_op(cx, |s| s.set_workspace_name(&v)).is_some() {
                    // Undo/Redo now act on fauxfield.toml.
                    self.lc.undo_file = Some("fauxfield.toml".into());
                }
            }
            PromptAction::ServerHost { server } => {
                let Some(old) = self.ws.find_server(&server).cloned() else {
                    return;
                };
                let host = v.clone();
                let sid = server.clone();
                if self
                    .store_op(cx, |s| s.set_str(&T::Server(sid), "host", Some(&host)))
                    .is_some()
                    && host != old.host
                {
                    let reachable = fauxfield_model::host_is_network_reachable(&host);
                    let msg = match (old.running, reachable) {
                        (true, true) => format!(
                            "{} restarted on {host}:{}. Reachable from your network.",
                            old.name, old.port
                        ),
                        (true, false) => format!("{} restarted on {host}:{}", old.name, old.port),
                        (false, true) => "Reachable from your network.".to_string(),
                        (false, false) => return,
                    };
                    self.notify(msg, cx);
                }
            }
            PromptAction::ServerPrefix { server } => {
                let prefix = fauxfield_model::validate_prefix(&v).ok().flatten();
                self.store_op(cx, |s| {
                    s.set_str(&T::Server(server.clone()), "prefix", prefix.as_deref())
                });
            }
            PromptAction::ServerPort { server } => match v.parse::<u16>() {
                Ok(p) if p > 0 => {
                    let old = self.ws.find_server(&server).cloned();
                    self.store_op(cx, |s| {
                        s.set_int(&T::Server(server.clone()), "port", Some(p.into()))
                    });
                    if let Some(o) = old
                        && o.running
                        && o.port != p
                    {
                        self.notify(format!("{} restarted on {}:{p}", o.name, o.host), cx);
                    }
                }
                _ => self.notify("A port is a number from 1 to 65535.".into(), cx),
            },
            PromptAction::NewServer => {
                if v.is_empty() {
                    return self.notify("A server needs a name.".into(), cx);
                }
                let port = self
                    .ws
                    .servers
                    .iter()
                    .map(|s| s.port)
                    .max()
                    .map(|p| p + 1)
                    .unwrap_or(3001);
                if let Some(id) = self.store_op(cx, |s| s.create_server(&v, port)) {
                    self.reveal_row(&id);
                }
            }
            PromptAction::NewFolder { server } => {
                if v.is_empty() {
                    return self.notify("A folder needs a name.".into(), cx);
                }
                if let Some(id) = self.store_op(cx, |s| s.create_folder(&server, &v)) {
                    self.reveal_row(&id);
                }
            }
            PromptAction::NewRoute { server, folder } => {
                let (method, path) = v
                    .split_once(char::is_whitespace)
                    .map(|(m, p)| (m.trim(), p.trim()))
                    .unwrap_or(("GET", v.as_str()));
                if !path.starts_with('/') {
                    return self.notify("A route path starts with `/`.".into(), cx);
                }
                let (method, path) = (method.to_string(), path.to_string());
                if let Some(id) = self.store_op(cx, |s| {
                    s.create_route(&server, folder.as_deref(), &method, &path)
                }) {
                    self.reveal_row(&id);
                    if let Some((kind, title)) = self.tab_for_row(&id) {
                        self.open_tab(&id, kind, &title, true);
                    }
                }
            }
            PromptAction::Rename { id } => {
                if v.is_empty() {
                    return self.notify("Names cannot be empty.".into(), cx);
                }
                match self.row_target(&id) {
                    Some(T::Server(sid)) => {
                        self.store_op(cx, |s| s.rename_server(&sid, &v));
                    }
                    Some(t) => {
                        self.store_op(cx, |s| s.set_str(&t, "name", Some(&v)));
                    }
                    None => {}
                }
            }
            PromptAction::RouteMethod { route } => {
                self.store_op(cx, |s| {
                    s.set_str(&T::Route(route), "method", Some(&v.to_ascii_uppercase()))
                });
            }
            PromptAction::RoutePath { route } => {
                self.store_op(cx, |s| s.set_str(&T::Route(route), "path", Some(&v)));
            }
            PromptAction::ResponseStatus { response } => match v.parse::<i64>() {
                Ok(n) => {
                    self.store_op(cx, |s| s.set_int(&T::Response(response), "status", Some(n)));
                }
                Err(_) => self.notify("Status is a number such as 200.".into(), cx),
            },
            PromptAction::ResponseDelay { response } => match v.parse::<i64>() {
                Ok(n) if n >= 0 => {
                    self.store_op(cx, |s| {
                        s.set_int(&T::Response(response), "delay_ms", (n > 0).then_some(n))
                    });
                }
                _ => self.notify("Delay is a number of milliseconds.".into(), cx),
            },
            PromptAction::ResponseName { response } => {
                self.store_op(cx, |s| s.set_str(&T::Response(response), "name", Some(&v)));
            }
            PromptAction::ResponseContentType { response } => {
                self.store_op(cx, |s| {
                    s.set_str(
                        &T::Response(response),
                        "content_type",
                        (!v.is_empty()).then_some(v.as_str()),
                    )
                });
            }
            PromptAction::ResponseBody { response } => {
                self.store_op(cx, |s| {
                    s.set_str(
                        &T::Response(response),
                        "body",
                        (!v.is_empty()).then_some(v.as_str()),
                    )
                });
            }
            PromptAction::ResponseHeader { response } => {
                let (name, val) = v
                    .split_once(':')
                    .map(|(n, x)| (n.trim(), x.trim()))
                    .unwrap_or((v.as_str(), ""));
                if name.is_empty() {
                    return self.notify("Header format is `name: value`.".into(), cx);
                }
                let (name, val) = (name.to_string(), val.to_string());
                self.store_op(cx, |s| {
                    s.set_header(&response, &name, (!val.is_empty()).then_some(val.as_str()))
                });
            }
        }
    }

    fn stub(&mut self, what: &str, milestone: &str, cx: &mut Context<Self>) {
        self.notify(format!("{what} arrives in {milestone}."), cx);
    }
}

/// Synthetic rows in test and dev-mcp builds only; a release build starts empty.
#[cfg(any(test, feature = "dev-mcp"))]
pub(crate) fn initial_traffic() -> Vec<TrafficRow> {
    crate::traffic::synthetic(TRAFFIC_ROWS)
}

#[cfg(not(any(test, feature = "dev-mcp")))]
pub(crate) fn initial_traffic() -> Vec<TrafficRow> {
    Vec::new()
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

/// Registers one handler per action, but only while the action is enabled in the menu model
/// (`Shell::gated`), so a greyed native menu item and its key both do nothing (SPEC §20.14).
macro_rules! on_actions {
    ($shell:expr, $root:expr, $cx:expr, { $($ty:ident => |$this:ident, $w:ident, $c:ident| $body:expr),* $(,)? }) => {{
        let mut root = $root;
        $(
            if $shell.gated(stringify!($ty)) {
                root = root.on_action($cx.listener(
                    |$this: &mut Shell, _: &actions::$ty, $w: &mut Window, $c: &mut Context<Shell>| $body,
                ));
            }
        )*
        root
    }};
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_window_title(window);
        let w = f32::from(window.viewport_size().width);
        if (self.bar.win_w - w).abs() > 0.5 {
            self.bar.win_w = w;
        }
        self.sync_menus(window, cx);
        #[cfg(feature = "dev-mcp")]
        crate::dev_mcp::tree::Registry::begin_frame(
            cx,
            self.picker.as_ref().map(|_| "picker-input"),
        );
        let t = theme::theme(cx).clone();
        let body_h = div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_row()
            .when(self.layout.left_open, |d| {
                d.child(crate::dev_id!(
                    self.render_left_dock(&t, cx),
                    "left-dock",
                    Dock,
                    "Left dock",
                    None
                ))
                .child(self.resize_handle_v(&t, Resize::Left, cx))
            })
            .child(crate::dev_id!(
                self.render_center(&t, cx),
                "center",
                Panel,
                "Center pane",
                None
            ))
            .when(self.layout.right_open, |d| {
                d.child(self.resize_handle_v(&t, Resize::Right, cx))
                    .child(crate::dev_id!(
                        self.render_right_dock(&t, cx),
                        "right-dock",
                        Dock,
                        "Right dock",
                        None
                    ))
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

        let root = on_actions!(self, root, cx, {
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
            NewServer => |this, w, cx| this.begin_prompt(PromptAction::NewServer, "", w, cx),
            NewRoute => |this, w, cx| match this.current_server_id() {
                Some(server) => this.begin_prompt(PromptAction::NewRoute { server, folder: None }, "GET /", w, cx),
                None => this.notify("Select a server first.".into(), cx),
            },
            NewFolder => |this, w, cx| match this.current_server_id() {
                Some(server) => this.begin_prompt(PromptAction::NewFolder { server }, "", w, cx),
                None => this.notify("Select a server first.".into(), cx),
            },
            NewResponse => |this, _w, cx| match this.current_route_id() {
                Some(r) => this.new_response_for(&r, cx),
                None => this.notify("Open a route first.".into(), cx),
            },
            Rename => |this, w, cx| match this.selected_visible() {
                Some(id) => this.rename_row(&id, w, cx),
                None => this.notify("Select something to rename.".into(), cx),
            },
            Delete => |this, _w, cx| match this.selected_visible() {
                Some(id) => this.delete_row(&id, cx),
                None => this.notify("Select something to delete.".into(), cx),
            },
            Undo => |this, _w, cx| this.undo_redo(true, cx),
            Redo => |this, _w, cx| this.undo_redo(false, cx),
            OpenFolder => |this, w, cx| this.pick_folder(false, w, cx),
            CreateWorkspace => |this, w, cx| this.pick_folder(true, w, cx),
            OpenFolderByPath => |this, w, cx| this.begin_prompt(PromptAction::OpenFolder, "", w, cx),
            CreateWorkspaceByPath => |this, w, cx| this.begin_prompt(PromptAction::CreateWorkspace, "", w, cx),
            ClearRecent => |this, _w, cx| this.clear_recent(cx),
            ConfirmDialog => |this, _w, cx| this.confirm_dialog(cx),
            CancelDialog => |this, _w, cx| this.cancel_dialog(cx),
            ToastAction => |this, w, cx| this.run_toast_action(w, cx),
            EditServerHost => |this, w, cx| match this.current_server_id() {
                Some(id) => this.change_host(&id, w, cx),
                None => this.notify("Select a server first.".into(), cx),
            },
            EditServerPrefix => |this, w, cx| match this.current_server_id() {
                Some(id) => this.change_prefix(&id, w, cx),
                None => this.notify("Select a server first.".into(), cx),
            },
            ToggleServerEnabled => |this, _w, cx| match this.current_server_id() {
                Some(id) => this.toggle_enabled(&id, cx),
                None => this.notify("Select a server first.".into(), cx),
            },
            ToggleUnmatchedHint => |this, _w, cx| match this.current_server_id() {
                Some(id) => this.toggle_unmatched_hint(&id, cx),
                None => this.notify("Select a server first.".into(), cx),
            },
            MergeKeepMine => |this, _w, cx| this.resolve_merge(true, cx),
            MergeLoadDisk => |this, _w, cx| this.resolve_merge(false, cx),
            EditRouteMethod => |this, _w, cx| this.open_method_menu(None, cx),
            EditRouteMethodCustom => |this, w, cx| this.edit_field(FieldEdit::RouteMethod, w, cx),
            EditRoutePath => |this, w, cx| this.edit_field(FieldEdit::RoutePath, w, cx),
            EditResponseStatus => |this, w, cx| this.edit_field(FieldEdit::ResponseStatus, w, cx),
            EditResponseName => |this, w, cx| this.edit_field(FieldEdit::ResponseName, w, cx),
            EditResponseContentType => |this, w, cx| this.edit_field(FieldEdit::ResponseContentType, w, cx),
            EditResponseDelay => |this, w, cx| this.edit_field(FieldEdit::ResponseDelay, w, cx),
            EditResponseHeader => |this, w, cx| this.edit_field(FieldEdit::ResponseHeader, w, cx),
            EditResponseBody => |this, w, cx| this.edit_field(FieldEdit::ResponseBody, w, cx),
            SetDefaultResponse => |this, _w, cx| match this.current_response_id() {
                Some(r) => {
                    this.store_op(cx, |s| s.set_default_response(&r));
                }
                None => this.notify("Open a route first.".into(), cx),
            },
            SendTest => |this, w, cx| {
                // Cmd+Enter is also "open pinned" inside Quick Search.
                if this.picker.is_some() {
                    this.run_picker_selection(OpenMode::Pinned, w, cx);
                } else {
                    this.send_test(cx);
                }
            },
            Duplicate => |this, _w, cx| match this.current_response_id().filter(|_| this.selected_is_route()).or(this.selected_visible()) {
                Some(id) => this.duplicate_row(&id, cx),
                None => this.notify("Select something to duplicate.".into(), cx),
            },
            ResetState => |this, _w, cx| this.stub("Resetting dataset state", "M8", cx),
            RestartServer => |this, _w, cx| this.restart_current(cx),
            ClearTraffic => |this, _w, cx| this.clear_traffic(cx),
            CopyAsCurl => |this, _w, cx| this.copy_curl(cx),
            EditServerPort => |this, w, cx| match this.notice_action.as_ref().and_then(|a| match &a.action { crate::run::ToastAction::ChangePort(id) => Some(id.clone()), _ => None }).or_else(|| this.current_server().and_then(|i| this.ws.servers.get(i)).map(|s| s.id.clone())) {
                Some(id) => this.change_port(&id, w, cx),
                None => this.notify("Select a server first.".into(), cx),
            },
            QuitAnyway => |this, _w, cx| this.quit_now(cx),
            CancelQuit => |this, _w, cx| this.cancel_quit(cx),
            Quit => |this, _w, cx| this.request_quit(cx),
            ShowAbout => |this, _w, cx| this.show_about(cx),
            HideApp => |_this, _w, cx| cx.hide(),
            HideOthers => |_this, _w, cx| cx.hide_other_apps(),
            ShowAllApps => |_this, _w, cx| cx.unhide_other_apps(),
            MinimizeWindow => |_this, w, _cx| w.minimize_window(),
            ZoomWindow => |_this, w, _cx| w.zoom_window(),
            ToggleFullscreen => |_this, w, _cx| w.toggle_fullscreen(),
            RevealWorkspace => |this, _w, cx| this.reveal_workspace(cx),
            OpenDocumentation => |this, _w, cx| this.open_documentation(cx),
            OpenReleaseNotes => |this, _w, cx| this.open_release_notes(cx),
            ReportIssue => |this, _w, cx| this.report_issue(cx),
            EditCut => |this, _w, cx| this.edit_cut(cx),
            EditCopy => |this, _w, cx| this.edit_copy(cx),
            EditPaste => |this, _w, cx| this.edit_paste(cx),
            EditSelectAll => |this, _w, cx| this.edit_select_all(cx),
        });
        let root = root.on_action(cx.listener(
            |this: &mut Shell,
             a: &actions::OpenRecent,
             _w: &mut Window,
             cx: &mut Context<Shell>| {
                this.open_recent(std::path::Path::new(&a.path), cx)
            },
        ));

        // Registered only while a workspace is open, so the palette and `ui.list_actions`
        // report them as disabled in Welcome.
        let root = root.when(self.store.is_some(), |d| {
            d.on_action(cx.listener(
                |this: &mut Shell,
                 _: &actions::CloseWorkspace,
                 _w: &mut Window,
                 cx: &mut Context<Shell>| {
                    this.guard(crate::lifecycle::Intent::Close, cx)
                },
            ))
            .on_action(cx.listener(
                |this: &mut Shell,
                 _: &actions::ShowSidebarServers,
                 _w: &mut Window,
                 cx: &mut Context<Shell>| {
                    this.set_sidebar_tab(tree::SidebarTab::Servers, cx)
                },
            ))
            .on_action(cx.listener(
                |this: &mut Shell,
                 _: &actions::ShowSidebarEnvironments,
                 _w: &mut Window,
                 cx: &mut Context<Shell>| {
                    this.set_sidebar_tab(tree::SidebarTab::Environments, cx)
                },
            ))
            .on_action(cx.listener(
                |this: &mut Shell,
                 _: &actions::ShowSidebarDatasets,
                 _w: &mut Window,
                 cx: &mut Context<Shell>| {
                    this.set_sidebar_tab(tree::SidebarTab::Datasets, cx)
                },
            ))
            .on_action(cx.listener(
                |this: &mut Shell,
                 _: &actions::RenameWorkspace,
                 w: &mut Window,
                 cx: &mut Context<Shell>| { this.rename_workspace(w, cx) },
            ))
        });

        root.child(crate::dev_id!(
            self.render_title_bar(&t, cx),
            "title-bar",
            Toolbar,
            "Title bar",
            None
        ))
        .child(if self.store.is_none() {
            self.render_welcome(&t, cx).into_any_element()
        } else {
            body_h_with_bottom(self, body_h, &t, cx).into_any_element()
        })
        .child(crate::dev_id!(
            self.render_status_bar(&t, cx),
            "status-bar",
            Toolbar,
            "Status bar",
            None
        ))
        .when(self.picker.is_some(), |d| {
            d.child(crate::dev_id_z!(
                self.render_picker(&t, cx),
                1,
                self.picker_ids().0,
                Modal,
                self.picker_ids().1,
                None
            ))
        })
        .when(self.quit_confirm, |d| {
            d.child(self.render_quit_confirm(&t, cx))
        })
        .when(self.any_dialog_open(), |d| {
            d.child(self.render_dialogs(&t, cx))
        })
        .when(self.about_open, |d| d.child(self.render_about(&t, cx)))
        .when(self.bar_visible() && self.bar.open.is_some(), |d| {
            d.child(self.render_bar_backdrop(cx))
        })
        .when(self.lc.method_menu.is_some(), |d| {
            d.child(crate::dev_id_z!(
                self.render_method_menu(&t, cx),
                1,
                "route-method-menu",
                Menu,
                "Route method menu",
                None
            ))
        })
        .when(self.menu.is_some(), |d| {
            d.child(crate::dev_id_z!(
                self.render_row_menu(&t, cx),
                1,
                "row-menu",
                Menu,
                "Row context menu",
                None
            ))
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
            d.child(shell.resize_handle_h(t, cx)).child(crate::dev_id!(
                shell.render_bottom_dock(t, cx),
                "bottom-dock",
                Dock,
                "Bottom dock",
                None
            ))
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
