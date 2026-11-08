//! Input tools of M1.3 (SPEC §11.4): actions, typed commands, keystrokes, text, pointer events.
//! Runs on the GPUI thread. Everything goes through the same `Shell` methods and GPUI dispatch
//! paths the user triggers, never a second code path.

use fauxfield_mcp::UiError;
use gpui::{App, Context, Keystroke, Modifiers, Window};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::actions::{self, ACTIONS};
use crate::shell::{PickerKind, Shell};
use crate::tabs::TabKind;

use super::handler::TabKindArg;

#[derive(Deserialize, JsonSchema)]
pub struct InvokeActionInput {
    /// Action name from `ui.list_actions` (e.g. `command_palette`).
    pub name: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct KeyInput {
    /// One keystroke (`cmd-shift-p`, `escape`, `enter`) or a chord of space-separated
    /// keystrokes (`cmd-k cmd-t`). `secondary` is Cmd on macOS and Ctrl elsewhere.
    pub keys: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct TypeTextInput {
    /// Text to type, one keystroke per character, into the focused input.
    pub text: String,
}

#[derive(Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ButtonArg {
    #[default]
    Left,
    Right,
    Middle,
}

#[derive(Deserialize, JsonSchema)]
pub struct ClickInput {
    /// X in window coordinates (points).
    pub x: f32,
    /// Y in window coordinates (points).
    pub y: f32,
    #[serde(default)]
    pub button: ButtonArg,
    /// Click count (2 = double click). Default 1.
    #[serde(default)]
    pub clicks: Option<usize>,
    /// Held modifiers, e.g. `shift` or `secondary-alt`.
    #[serde(default)]
    pub modifiers: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ScrollInput {
    pub x: f32,
    pub y: f32,
    /// Horizontal scroll in points (positive moves content right).
    #[serde(default)]
    pub dx: f32,
    /// Vertical scroll in points (negative scrolls down, like a wheel).
    #[serde(default)]
    pub dy: f32,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CommandName {
    OpenTab,
    SelectTab,
    ToggleDock,
    SetEnvironment,
    Search,
    /// Close the open workspace (through the shared guard).
    CloseWorkspace,
    /// Bind `port` on loopback and keep it until `ui.reset_view`.
    HoldPort,
    /// Open the quit confirmation; refused when nothing is running.
    RequestQuit,
    /// Remove a Recent entry by `path`.
    RemoveRecent,
    /// Add `path` (which need not exist) to Recent, newest first.
    AddRecent,
    /// Override where the menu is drawn: `style` = `native` or `in_window` (dev builds only).
    SetMenuStyle,
    /// Open an in-window menu: `id` = `File` or `File/Open Recent` (for state fixtures).
    OpenMenu,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DockArg {
    Left,
    Right,
    Bottom,
}

/// One flat object: `command` picks the operation, the other fields are its arguments.
#[derive(Deserialize, JsonSchema)]
pub struct CommandInput {
    /// `open_tab` (kind, id), `select_tab` (index or id), `toggle_dock` (dock),
    /// `set_environment` (name), `search` (query), `close_workspace`, `hold_port` (port),
    /// `request_quit`, `remove_recent` (path), `add_recent` (path).
    pub command: CommandName,
    /// open_tab: `route`, `server` or `environment`.
    #[serde(default)]
    pub kind: Option<TabKindArg>,
    /// open_tab: route id, server id or environment name. select_tab: a tab id.
    #[serde(default)]
    pub id: Option<String>,
    /// select_tab: 1-based position in the tab bar (like Cmd+1..9).
    #[serde(default)]
    pub index: Option<usize>,
    /// toggle_dock: `left`, `right` or `bottom`.
    #[serde(default)]
    pub dock: Option<DockArg>,
    /// set_environment: environment name.
    #[serde(default)]
    pub name: Option<String>,
    /// search: text to put in Quick Search (prefixes `@ / # $ >` work).
    #[serde(default)]
    pub query: Option<String>,
    /// hold_port: TCP port to hold.
    #[serde(default)]
    pub port: Option<u16>,
    /// remove_recent: workspace folder path.
    #[serde(default)]
    pub path: Option<String>,
    /// set_menu_style: `native` or `in_window`.
    #[serde(default)]
    pub style: Option<String>,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MenuAction {
    List,
    Invoke,
    Open,
    Close,
}

/// `ui.menu`: inspect and drive the application menu.
#[derive(Deserialize, JsonSchema)]
pub struct MenuInput {
    /// `list`, `invoke`, `open` (in-window bar) or `close`.
    pub action: MenuAction,
    /// invoke / open: label path such as `["File", "Open Recent", "demo — ~/demo"]`.
    #[serde(default)]
    pub path: Option<Vec<String>>,
}

pub fn menu(
    input: MenuInput,
    shell: &mut Shell,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Result<Value, UiError> {
    match input.action {
        MenuAction::List => {
            Ok(serde_json::to_value(super::state::menus_state(shell)).expect("menus serialize"))
        }
        MenuAction::Invoke => {
            let path = need(input.path, "path", "invoke")?;
            shell
                .run_menu_path(&path, window, cx)
                .map_err(UiError::Failed)?;
            Ok(json!({ "ok": true, "path": path }))
        }
        MenuAction::Open => {
            if shell.menu_style != crate::menu::MenuStyle::InWindow {
                return fail("the menu is native; use ui.command set_menu_style in_window first");
            }
            let path = need(input.path, "path", "open")?;
            let first = path.first().cloned().unwrap_or_default();
            let idx = if shell.bar_collapsed() {
                Some(0)
            } else {
                shell.bar_menus().iter().position(|m| m.title == first)
            };
            let Some(i) = idx else {
                return fail(format!("no menu {first:?}"));
            };
            shell.bar_open_menu(i, cx);
            if let Some(sub) = path.get(1) {
                let entries = shell.bar_entries(i);
                let at = entries.iter().position(
                    |e| matches!(e, crate::menu::MenuEntry::Submenu { label, .. } if label == sub),
                );
                match at {
                    Some(s) => shell.bar_open_sub(s, cx),
                    None => return fail(format!("no submenu {}", path.join("/"))),
                }
            }
            Ok(json!({ "ok": true, "open": path }))
        }
        MenuAction::Close => {
            shell.bar_close(cx);
            Ok(json!({ "ok": true }))
        }
    }
}

static HELD_PORTS: std::sync::Mutex<Vec<std::net::TcpListener>> = std::sync::Mutex::new(Vec::new());

/// Drop every listener `hold_port` created (called by `ui.reset_view`).
pub fn release_held_ports() {
    HELD_PORTS.lock().unwrap_or_else(|p| p.into_inner()).clear();
}

fn fail<T>(msg: impl Into<String>) -> Result<T, UiError> {
    Err(UiError::Failed(msg.into()))
}

fn need<T>(v: Option<T>, what: &str, cmd: &str) -> Result<T, UiError> {
    v.ok_or_else(|| UiError::Failed(format!("{cmd} needs `{what}`")))
}

/// Opens a tab as a preview, like a single click on its tree row.
pub fn open_tab(
    shell: &mut Shell,
    kind: TabKindArg,
    id: &str,
    cx: &mut Context<Shell>,
) -> Result<Value, UiError> {
    let (kind, row_id) = match kind {
        TabKindArg::Route => (TabKind::Route, id.to_string()),
        TabKindArg::Server => (TabKind::Server, id.to_string()),
        TabKindArg::Environment => (TabKind::Environment, format!("env:{id}")),
    };
    match shell.tab_for_row(&row_id) {
        Some((k, _)) if k == kind => {}
        _ => {
            return fail(format!(
                "no {} with id {id:?}",
                serde_json::to_value(kind)
                    .unwrap_or_default()
                    .as_str()
                    .unwrap_or("tab"),
            ));
        }
    }
    shell.click_row(&row_id, 1, cx);
    Ok(json!({ "tab": shell.tabs.active().cloned() }))
}

pub fn list_actions(shell: &Shell, window: &mut Window, cx: &mut Context<Shell>) -> Value {
    let list: Vec<Value> = ACTIONS
        .iter()
        .map(|a| {
            let binding = shell.keymap.binding_for(a.id);
            let context = shell
                .keymap
                .bindings
                .iter()
                .find(|b| b.action == a.id)
                .and_then(|b| b.context.clone());
            let enabled = window.is_action_available((a.build)().as_ref(), cx);
            json!({
                "name": a.id,
                "label": a.label,
                "binding": binding,
                "context": context.unwrap_or_else(|| "Shell".into()),
                "enabled": enabled,
            })
        })
        .collect();
    json!({ "actions": list })
}

pub fn invoke_action(
    name: &str,
    shell: &mut Shell,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Result<Value, UiError> {
    let Some(info) = actions::find(name) else {
        return fail(format!("unknown action {name:?}; see ui.list_actions"));
    };
    if info.id == "quit" {
        return fail("action \"quit\" is refused over MCP: it would end the session");
    }
    let action = (info.build)();
    // The handlers of the last painted frame decide availability, but a tool call can arrive
    // before the frame that follows a state change. The fresh menu rules decide whether the
    // action is enabled; when only the painted frame is behind, dispatch after the next frames.
    if !shell.enabled_now(info.id, window) {
        return fail(format!(
            "action {name:?} is disabled in the current context"
        ));
    }
    if window.is_action_available(action.as_ref(), cx) {
        // GPUI defers the dispatch; its effect lands before the next bridge request is served.
        window.dispatch_action(action, cx);
    } else {
        shell.dispatch_next_frames(info.id, window, cx);
    }
    Ok(json!({ "ok": true, "action": name }))
}

pub fn command(
    input: CommandInput,
    shell: &mut Shell,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Result<Value, UiError> {
    match input.command {
        CommandName::OpenTab => {
            let kind = need(input.kind, "kind", "open_tab")?;
            let id = need(input.id, "id", "open_tab")?;
            open_tab(shell, kind, &id, cx)
        }
        CommandName::SelectTab => {
            let count = shell.tabs.tabs().len();
            let index = match (input.index, input.id) {
                (Some(i), _) if (1..=count).contains(&i) => i - 1,
                (Some(i), _) => return fail(format!("tab index {i} out of range (1..={count})")),
                (None, Some(id)) => match shell.tabs.tabs().iter().position(|t| t.id == id) {
                    Some(i) => i,
                    None => return fail(format!("no open tab with id {id:?}")),
                },
                (None, None) => return fail("select_tab needs `index` or `id`"),
            };
            shell.activate_tab(index, cx);
            Ok(json!({ "ok": true, "active_tab": shell.tabs.active().map(|t| t.id.clone()) }))
        }
        CommandName::ToggleDock => {
            match need(input.dock, "dock", "toggle_dock")? {
                DockArg::Left => shell.toggle_left(cx),
                DockArg::Right => shell.toggle_right(cx),
                DockArg::Bottom => shell.toggle_bottom(cx),
            }
            Ok(json!({ "ok": true }))
        }
        CommandName::SetEnvironment => {
            let name = need(input.name, "name", "set_environment")?;
            if shell.ws.find_environment(&name).is_none() {
                return fail(format!("no environment named {name:?}"));
            }
            shell.set_environment(&name, cx);
            Ok(json!({ "ok": true, "active_environment": shell.active_env }))
        }
        CommandName::SetMenuStyle => {
            let name = need(input.style, "style", "set_menu_style")?;
            let style = match name.as_str() {
                "native" => crate::menu::MenuStyle::Native,
                "in_window" => crate::menu::MenuStyle::InWindow,
                other => return fail(format!("unknown style {other:?}; use native or in_window")),
            };
            shell.menu_style = style;
            shell.menu_last = None;
            shell.bar_close(cx);
            cx.notify();
            Ok(json!({ "ok": true, "style": name }))
        }
        CommandName::OpenMenu => {
            let id = need(input.id, "id", "open_menu")?;
            let path = id.split('/').map(str::to_string).collect();
            menu(
                MenuInput {
                    action: MenuAction::Open,
                    path: Some(path),
                },
                shell,
                window,
                cx,
            )
        }
        CommandName::CloseWorkspace => {
            if shell.store.is_none() {
                return fail("no workspace is open");
            }
            shell.guard(crate::lifecycle::Intent::Close, cx);
            Ok(json!({
                "ok": true,
                "view": if shell.store.is_some() { "workspace" } else { "welcome" },
                "modal": shell.dialog_name(),
            }))
        }
        CommandName::HoldPort => {
            let port = need(input.port, "port", "hold_port")?;
            let l = std::net::TcpListener::bind(("127.0.0.1", port))
                .map_err(|e| UiError::Failed(format!("cannot hold port {port}: {e}")))?;
            HELD_PORTS.lock().unwrap_or_else(|p| p.into_inner()).push(l);
            Ok(json!({ "ok": true, "port": port }))
        }
        CommandName::RequestQuit => {
            if shell.servers_running() == 0 {
                return fail("refused: no servers are running, this would quit the app");
            }
            shell.request_quit(cx);
            Ok(json!({ "ok": true, "modal": "quit_confirm" }))
        }
        CommandName::AddRecent => {
            let path = need(input.path, "path", "add_recent")?;
            shell.add_recent(std::path::PathBuf::from(path), cx);
            Ok(json!({ "ok": true }))
        }
        CommandName::RemoveRecent => {
            let path = need(input.path, "path", "remove_recent")?;
            shell.remove_recent(std::path::Path::new(&path), cx);
            Ok(json!({ "ok": true }))
        }
        CommandName::Search => {
            let query = need(input.query, "query", "search")?;
            shell.open_picker(PickerKind::Search, window, cx);
            shell.picker_type(&query, cx);
            Ok(json!({ "ok": true }))
        }
    }
}

/// Key dispatch runs shell listeners, so it must not run inside an update of the `Shell`.
pub fn key(keys: &str, window: &mut Window, cx: &mut App) -> Result<Value, UiError> {
    super::tree::Registry::stamp_key(cx);
    let strokes: Vec<Keystroke> = keys
        .split_whitespace()
        .map(|s| {
            Keystroke::parse(s).map_err(|e| UiError::Failed(format!("bad keystroke {s:?}: {e}")))
        })
        .collect::<Result<_, _>>()?;
    if strokes.is_empty() {
        return fail("`keys` is empty");
    }
    let mut handled = false;
    for k in strokes {
        handled = window.dispatch_keystroke(k, cx);
    }
    Ok(json!({ "ok": true, "handled": handled }))
}

pub fn type_text(text: &str, window: &mut Window, cx: &mut App) -> Result<Value, UiError> {
    let mut n = 0;
    for ch in text.chars() {
        let stroke = match ch {
            '\n' => Keystroke {
                modifiers: Modifiers::default(),
                key: "enter".into(),
                key_char: None,
            },
            ' ' => Keystroke {
                modifiers: Modifiers::default(),
                key: "space".into(),
                key_char: Some(" ".into()),
            },
            c => Keystroke {
                modifiers: Modifiers {
                    shift: c.is_uppercase(),
                    ..Modifiers::default()
                },
                key: c.to_lowercase().to_string(),
                key_char: Some(c.to_string()),
            },
        };
        window.dispatch_keystroke(stroke, cx);
        n += 1;
    }
    Ok(json!({ "ok": true, "typed": n }))
}

const NO_POINTER: &str = "pointer injection is unavailable: gpui 0.2.2 keeps \
    Window::dispatch_event's result type crate-private, so synthetic mouse events cannot be sent \
    from outside GPUI. Use ui.command / ui.invoke_action / ui.key now, and ui.click_element (M1.4).";

/// Synthetic pointer events are not possible with the pinned GPUI (SPEC §11.4); a clear error
/// tells the agent what to use instead.
pub fn click(_input: ClickInput) -> Result<Value, UiError> {
    fail(NO_POINTER)
}

/// See [`click`].
pub fn scroll(_input: ScrollInput) -> Result<Value, UiError> {
    fail(NO_POINTER)
}
