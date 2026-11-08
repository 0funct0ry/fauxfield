# Changelog

## Unreleased

### Added
- M1.4 Semantic UI tree (dev builds): `dev_id!` tags elements (compiled out without `dev-mcp`),
  `ui.tree`, `ui.find` and `ui.click_element` (role, label, bounds, visible, clipped, focused, z;
  virtualised lists report `total`). Clicks run the same `Shell` methods as the pointer handlers.
- M1.3 Actions and input over MCP (dev builds): `ui.list_actions`, `ui.invoke_action`
  (disabled action is an error, `quit` is refused), `ui.command` (open_tab, select_tab,
  toggle_dock, set_environment, search), `ui.key` (keystrokes and chords, same 400 ms Cmd+K
  timeout), `ui.type_text`, `ui.wait_for_idle`, `ui.wait_for` (JSON-pointer conditions), and
  `ui.click` / `ui.scroll` (return a clear error: the pinned GPUI cannot inject pointer events).
  Input tools return the new `ui.state` generation and toasts.
- M1.2 In-app MCP host (dev builds only): `dev-mcp` feature on `fauxfield-gui` and `fauxfield`,
  `fauxfield --mcp-socket [PATH]` and `--mcp-stdio`, the `GuiBridge` into the GPUI thread (5 s
  "ui thread busy" timeout), and the `ui.state`, `ui.open_workspace`, `ui.open_tab` and
  `ui.capture_probe` tools. `make dev-mcp` / `build-dev-mcp`. Release builds contain none of it.
- M1.1 MCP core: `fauxfield-mcp` (rmcp, no GPUI) with a class-filtered tool registry
  (read/write/execute/ui), `ping` and `server_info` tools, the `UiBridge` trait, stdio and unix
  socket transports (0600, stale-socket replacement, removed on exit), and `fauxfield mcp
  [PATH] [--transport] [--socket] [--allow] [--read-only]`. `[mcp]` keys in settings.toml are
  read; flags win.
- M1 UI shell: Cargo workspace, GPUI window with title bar, left/right/bottom docks, tab bar
  with preview and pinned tabs, status bar, Route / Server / Environment tabs and a virtualised
  Traffic panel (10,000 mock rows), all on mock data.
- Quick Search (`Cmd/Ctrl+K`, prefixes `>` `@` `/` `#` `$`), Command Palette
  (`Cmd/Ctrl+Shift+P`) and theme selector (`Cmd/Ctrl+K Cmd/Ctrl+T`, live preview, Esc reverts)
  with the 400 ms chord timeout.
- Zed theme-family JSON loader with One Dark and One Light bundled.
- Keymap from a bundled default TOML with an optional user override.
- Layout (docks, tabs, theme) persisted as JSON in the platform data dir.
- CLI skeleton: `fauxfield [PATH]`, `open`, `version [--json]`, `completions`.
- Makefile with a detailed `make help`, and a CI matrix (macOS, Linux, Windows).
