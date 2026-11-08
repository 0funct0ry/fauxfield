# Changelog

## Unreleased

### Added
- M2 Workspace files and persistence: `fauxfield-model` (file types, prefixed IDs, validation
  diagnostics, server JSON Schema via `cargo xtask schema`), `fauxfield-store` (toml_edit
  round-tripping that keeps comments and order, atomic debounced autosave, file watcher with
  merge-conflict detection, per-file undo/redo, recent workspaces, `.fauxfield/ui.json`).
  `fauxfield init`, `validate` (exit 3 on errors), `server list`, `route list` (all with `--json`).
  The app opens real workspaces: Welcome view, create/rename/duplicate/delete of Servers, Folders,
  Routes and Responses, route and response field editing, merge banner, Problems from
  diagnostics. MCP tools `workspace_info`, `list_servers`, `get_server`, `create_server`,
  `update_server`, `delete_server`, `list_routes`, `get_route`, `upsert_route`, `delete_route`,
  `validate`; `ui.open_workspace` opens a real workspace; `ui.state` gains `view`, `problem_list`,
  `merge_conflicts`, `recent_workspaces`, `prompt`. `fixtures/example-workspace`, scenario
  `m02-workspace`, state fixtures `empty-workspace`, `problems-empty`, `problems-open`.
  Known gaps: multi-line body editing and the Raw file tab need an editor widget (M6); fields
  are edited through one-line prompts.
- M1.7 Scenarios and the agent playbook (dev builds): YAML scenarios in `fixtures/scenarios/` with
  `ui.run_scenario` (by name or inline; first failure reports expected, actual, `ui.state` and a
  screenshot), `ui.record`, and `ui.events` (1024-entry replay buffer, `seq`, `gap`, `wait_ms`).
  `fauxfield mcp scenario FILE --socket PATH [--out DIR] [--no-capture]` runs one from a shell
  (exit 0 pass, 1 failed step, 2 unreachable). New scenarios `m01-shell`, `audit-all` and the
  deliberately failing `broken-assertion`. `docs/agent-testing.md`, `make smoke`, `make smoke-ui`
  and CI jobs `smoke-headless` and `smoke-ui`.
- M1.6 UI audit (dev builds): `ui.audit` runs UX101, UX102, UX103, UX110, UX120, UX130, UX131,
  UX140 and (with `perf` or in `rules`) UX150 over `ui.tree`, theme token pairs, the actions table,
  the view table and the state fixtures; `themes`, `sizes` and `states` sweep a matrix and restore
  theme and size. `ui.walkthrough` reports steps, keystrokes, clicks, wall-clock time and toasts.
  The pure rules live in `fauxfield-mcp::audit` (no GPUI). Tree nodes carry `meta` (interactive,
  action, tooltip, truncates, keyboard); `ui.perf_probe` exposes frame and key-to-paint samples.
  New state fixtures `no-tabs`, `quick-search-empty`, `palette-empty`.
- Changed: tab close buttons, tree chevrons and tree row menu buttons are now 20 pt targets; four
  One Light tokens (`text.accent`, `created`, `modified`, syntax `string`) are slightly darker so
  every theme pair meets its WCAG ratio.
- M1.5 Screenshots (dev builds): `ui.screenshot` (window or element crop, optional named or inline
  `state`), `ui.screenshot_all` (states x themes x sizes, restores theme and size even when a cell
  fails), `ui.set_theme` (not persisted) and `ui.resize`. A `Capture` trait picks macOS
  `screencapture` or Linux `grim`/`import`, verifies size, flatness and permission, and reports the
  tier. Nine state fixtures in `fixtures/ui-states/`; Traffic time and duration are masked while
  capturing. The MCP server returns PNGs as image content blocks.
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
