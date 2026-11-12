# Changelog

## Unreleased

### Added
- Datasets in Responses and Rules (M10.06): any Route can read and change a Dataset with no CRUD Route. **Reads**: the Response editor's **Dataset** body source (Dataset, mode all / one by key / filtered / count / newest, filter, sort, limit) generates an editable `data.<name>` template using the new `dataset_rows(data=, filter=, sort=, order=, page=, limit=)` and `dataset_count(data=, filter=)` template functions (CRUD operators `_gte`, `_lte`, `_ne`, `_like`). **Actions**: `[[routes.responses.actions]]` (`insert`, `update` merge or replace, `delete`, `delete_where`, `set`, `increment`, `reset`) run in order after the Response is chosen and before the body renders, with Tera inputs over the request, results in `action.<n>`, an optional per-step `fail_if`, and all-or-nothing rollback answering `action_error_status` (default 409); Rules and templates still never mutate. **Rules**: the `dataset` target (`name.$.path`) now works and a new `dataset_count` target counts rows matching a filter (`orders?status=open`); keys may use `{{ request... }}`. Singleton (object) Datasets accept PUT and PATCH on their CRUD collection path and the same `update`/`set` actions. References: `dataset_references` (CLI `data refs`, MCP, GUI Used by and delete dialog) now include actions and rules, and rename rewrites them. Diagnostics E086-E088, W072 (action on a persisted Dataset writes to disk). GUI: Actions sub-tab, palette *Response: Add Dataset Action*, Traffic marker and **Dataset changes** in the inspector. `send_test_request` returns `dataset_changes`; `render_template` accepts `datasets` sample state; `ui.state` responses report `actions`, `invalid_actions`, `dataset_source`.
- Import from JSON, CSV and Parquet (M10.05): `fauxfield_engine::import` reads a JSON array or object, a CSV (header row, delimiter and quote detection, per-column type inference with overrides, ragged-row warnings) or a Parquet file (structs and lists become objects and arrays, timestamps ISO 8601, decimals numbers or strings) into `data/<name>.json`; the file is not referenced afterwards. Key column (`id` or `--key`) with duplicate and missing-key warnings, a soft row limit that warns and a hard limit that refuses. GUI: **Dataset: Import File…** (new Dataset), the Dataset tab **Import…** (replace or append), drag a file onto the sidebar, a preview notice and an options prompt; **Export…** writes `.json`, `.csv` or `.parquet` by extension. CLI: `data import [--format] [--append] [--key] [--delimiter] [--column-type] [--decimal-as] [--soft-limit] [--hard-limit]` and `data export --format`; MCP `import_dataset`; `ui.state` `import_preview`; scenario `m10-05-dataset-import`. The `parquet` crate (no Arrow, Snappy only) adds about 1.6 MB to the headless binary.
- Dataset building in the Data Table (M10.04): **New From Table…** (name, then `name:type` columns), optional `columns` on a Dataset (E078, E079), double-click a row to edit a cell with a typed `TextInput`, **Add row**, **Add column…** (`name:type=generator`), rename and remove column, paste tab-separated rows, **Generate rows…** (`N bake` appends to `data/<name>.json`, `N keep` writes a generator block), a Seed view with a JSON editor (**Save seed**, **Reload from seed**), **Save Rows to Seed**, source and columns shown in the tab, per-row engine and control APIs (`data.update_cell`, `data.insert`, `data.delete`), MCP `bake_dataset` and `columns`/`generator` on `upsert_dataset`, `fauxfield data create --column`, `ui.state` `declared_columns`, `source`, `dirty_seed`, `editing`, and scenario `m10-04-dataset-editing`.
- DataTable component (M10.03): a first-party virtualised data grid (`fauxfield-gui::data_table`, re-exported through `ui`) with row, column and cell selection, the full keyboard model (key context `DataTable`: arrows, Home, End, Page Up/Down, Esc), sort cycling, column resize (grab room, clamps, double-click fit), drag-to-reorder with insertion gap and region rule, left and right fixed columns, multi-level group headers, row and column virtualisation with visible-range callbacks, infinite loading, loading and empty views, stripe, border, size and scrollbar options, a keyboard-only focus ring and `headers`/`dump`/`dump_range` export. The Dataset tab's table now uses it, read-only: sort, resize, reorder, select by mouse and keyboard, right-click row menu (Copy cell, Copy row as JSON, Delete row; also palette actions), double-click opens the row in the JSON view. Column widths and order persist per Dataset in the layout file. `ui.state` gains `/dataset/table`; dev-mcp gains `ui.command table` (resize, fit, move_column) because pointer drags cannot be injected.
- Seed generators (M10.02): a Dataset may declare `[datasets.generator]` (row count + fields with `int`, `float`, `bool`, `uuid`, `ulid`, `sequence`, `regex`, `one_of`, `string`, `lorem`, `fake`, `datetime`, `date`, `const`, `ref`, `null_or`, `template`, nested `object`/`array`) as a third seed source; deterministic with `fake_seed`, `ref` load order with cycle diagnostic (E077), field-path diagnostics (E076). Editing a seed file or generator block now reloads only that Dataset. Dataset tab shows a generator preview. CLI `fauxfield data generate [--rows] [--seed] [--bake]`; MCP `preview_generator`; reference page generated by `cargo xtask generator-docs`.
- Datasets are standalone (M10.01): **New Dataset…** (Datasets tab `+`, palette *Dataset: New…*) creates an empty Dataset with no Route; a `crud` Route only references one (`dataset = "<name>"`), and deleting the Route keeps the Dataset, its seed and its persisted state. Row menu: Open, Duplicate, Rename, Reset, Export, Delete; an *unused* badge marks Datasets no Route uses; the Dataset tab has a *Used by* list; the crud Route editor repoints its Dataset with a combo box. Rename rewrites every crud Route that references it and warns about `data.<name>` in templates; Delete is blocked behind a dialog listing references, with separate options to remove the seed file and persisted state. CLI: `fauxfield data create|delete|rename|refs` (all with `--json`). MCP: `delete_dataset`, `dataset_references`; `upsert_dataset` without `crud_path` writes no Route.
- One reusable `combo_box::ComboBox` now backs every drop-down choice (rule target and operator, Response mode and fault, the Route verb with its "Custom…" entry). It opens a window-aware popup under the trigger (flips and stays on screen, scrolls), supports Enter/Space/Up/Down to open, Up/Down/Home/End/PageUp/PageDown, type-ahead, Enter/Tab to pick, Esc to close with focus returned, hover highlight, outside-click close, optional search box and clear button, disabled entries, separators, action entries, a fallback label for values not in the list, and `Change`/`Action`/`Clear`/`Open`/`Close` events. The row-menu kinds, `combo_trigger` and the bespoke verb menu are gone.
- New Server dialog (`+`, `Cmd/Ctrl+Shift+N`): Name, Host (default `127.0.0.1`) and Port (next free) with inline validation (port range, host format, port already used on the host).
- One reusable single-line input, `text_input::TextInput`, now backs every input box (New Server dialog, Quick Search, Command Palette and prompts, inline Response, header and Server fields). Compared with gpui-kit's `Input` it adds: IME composition and dead keys through GPUI's input handler, grapheme-aware caret and deletion, drag selection that continues outside the box, double-click word and triple-click select-all, emacs and line/word shortcuts, forward delete variants, undo/redo with coalescing, a right-click menu, a clear button, masked mode with a reveal toggle, read-only and disabled states, sizes, filters, `max_len`, `validate`, `on_paste`, and Change/Enter/Escape/Tab/Focus/Blur events.
- Rules tab: the negate button shows `!`; a per-rule *Copy cURL* button copies a request that triggers the rule (all rules of the Response when they are combined with AND); comboboxes take keyboard focus (Tab / Shift+Tab between them, Enter, Space or Up/Down to open).
- Response properties: Fault is a combobox, Template is a checkbox, Description fills the row. Every picker menu (Fault, rule Target and Op, context menus) is keyboard navigable: Up/Down/Home/End, a letter jumps to the next match, Enter or Tab picks, Esc closes.
- `Cmd/Ctrl+S` saves the Response body or field being edited (File > Save, palette *File: Save*). `Cmd/Ctrl+Enter` no longer saves; it only sends the test request (or opens pinned in Quick Search).
- Template shorthand: `{{ fake.name }}`, `{{ fake.email }}`, `{{ fake.address.city }}` for `fake(kind=…)` (a kind may be shortened to its last segment when unambiguous, also in `fake(kind='email')`), and `{{ uuid }}`, `{{ ulid }}`, `{{ now }}` without parentheses; editor completion offers both forms.
- M10 Static files, file templating, CORS and HTTPS: `kind = "static"` Routes (`dir`, `index`, `template_extensions`; MIME detection, `ETag`/`Last-Modified` with 304, single ranges (206/416), HEAD, traversal protection; `*.html.tera` is rendered through Tera and served as `text/html`); Server `[cors]` (automatic preflight, headers on every response, origin globs, `*` with credentials reflects the origin); Server `[tls]` (`dev-ca` with an rcgen local CA and per-Server leaf certificates, `files` mode with a 14-day expiry warning, `also_http_port`). CLI `fauxfield certs status|trust|untrust` (prints the platform trust command, runs it only with `--yes`) and `fauxfield doctor`. GUI: static Route tab with directory picker and file browser preview, CORS section with presets, TLS section with certificate status, CA trust state and a "Trust dev CA" confirmation that shows the exact command, lock icon on HTTPS Servers, palette actions *Route: New Static Files…* and *HTTPS: Trust Dev CA…*. MCP `list_static_files`, `certs_status`, `certs_trust_command`; diagnostics E090 to E094, W090.
- M9 Network and error simulation: Server `[network]` (`latency_ms`, `jitter_ms`, `bandwidth_kbps`, a throttled body stream) and `[rate_limit]` (429 problem+json with `Retry-After` and `X-RateLimit-*`, keyed by `ip`, `header:<name>` or `query:<name>`); Response `delay_ms` accepts a range (`"200..800"`), `fault` (`timeout`, `reset` = TCP RST, `close`, `empty`, `malformed`, `slow_body`) and `fault_rate`; per-Server chaos switch and a workspace network profile (None, Fast 3G, Slow 3G, Flaky Wi-Fi, Offline; Offline refuses connections), both runtime only. Traffic shows a match, render, delay, transfer breakdown and marks injected faults. GUI: Network and Rate limit sections in the Server tab, Delay range, Fault and Fault rate fields, a fault icon in the response list, status-bar profile switcher, chaos badge in the tree, fault variants in *Add error preset* ("502 after 10 s", "Connection reset"…). CLI `fauxfield run --network <profile> --chaos <rate>`; MCP `set_network_profile`, `set_chaos`; diagnostics E080 to E085, W060.
- M8 Datasets and stateful CRUD: typed `[[datasets]]` (`seed` or `seed_template`, `key`, `id_strategy` uuid/increment/ulid, `persist` to `.fauxfield/state/`) and `kind = "crud"` Routes that generate list, get, create, replace, merge-patch and delete with `_page`, `_limit`, `_sort`, `_order`, `q`, `field`, `_gte`, `_lte`, `_ne`, `_like`, `X-Total-Count`, `Link`, `Location`, optional JSON Schema `validate` (422 problem+json) and overrides by higher-priority `http` Routes. `data.<name>` in templates; *Reset State* restores seeds (GUI, `fauxfield data reset`, MCP `reset_state`). GUI: Datasets sidebar tab with live row counts, Dataset tab (virtualised table and JSON views, inline edit, add and delete row, Reset, Export), New CRUD resource wizard, expandable CRUD Route nodes with a CRUD badge, Server tab Datasets section. CLI `fauxfield data list|show|reset|export|import`; MCP `upsert_dataset`, `reset_state`; diagnostics E070 to E075, W070, W071.
- M7 Environments and variables: variable resolver (global < environment < server < route, runtime on top) with `dotenv` and `${env:NAME}` (never written back); live Environment switch (title bar, Cmd/Ctrl+E, `fauxfield run --env`, `fauxfield env list|use|add`) that re-renders templated prefixes and headers without a restart; Route `variables`; editable Environment, Server and Route Variables tables with source column and secret masking; Inspector "Variables in scope"; `fauxfield var get|set|unset --scope`; MCP `set_variable`; "New environment" in the sidebar, File menu and palette.
- M6 Tera templating and fake data: Response bodies, `body_file` (path and contents), header values,
  Route headers and string rule values render with Tera (`template = false` sends verbatim); context
  `request`, `vars`, `env`, `server`, `data`, `state`; functions `fake`, `uuid`, `ulid`, `now`,
  `random_int`, `random_float`, `pick`, `weighted`, `seq`, `repeat`, `jsonpath`, `hmac_sha256`, `jwt`,
  `lorem`, `file` and filters `json`, `pretty_json`, `base64_encode`/`decode`, `url_encode`, `slugify`,
  `sha256`, `pad`, `money`; 23 dotted `fake` kinds on the `fake` crate. Server `fake_seed` makes output
  reproducible per (seed, route, call). Compiled-template cache, 50 ms budget, 10 MiB cap, workspace-confined
  `file()`. Template errors return 500 problem+json and are recorded in Traffic (`error`). New Route
  `headers` (Response header of the same name wins). Editor: Tera highlighting, inline syntax
  diagnostics, context/function/filter/fake-kind autocomplete. `fauxfield render` CLI, MCP
  `render_template`, `fake_seed`/`template`/route `headers` on the MCP write tools, and a generated
  template reference page (`cargo xtask template-docs`).
- M5 Multiple and conditional responses: `response_mode` (`rules`, `sequential`, `random`,
  `disabled`), per-Response `rules` (targets header, query, cookie, param, body, method, path, ip,
  count; ops eq, ne, contains, starts_with, ends_with, regex, exists, absent, gt, gte, lt, lte, in,
  json_schema; `negate`; `rules_logic` and/or), `description` and `disabled`; body parsing for JSON
  (JSONPath), urlencoded forms, multipart fields and text; per-route call counters (reset by
  *Dataset: Reset State*); a rule trace on every recording (`trace`, `response_id`) shown in the
  Inspector; diagnostics E043 to E049 and W044. GUI: response list with drag reorder, default star,
  rule-count and off badges, description line, Disable, Move up/down and *Add error preset* (RFC 9457
  problem+json); Rules sub-tab (one table grouped by Response, and/or switch, row editor with
  inline validation); Response mode picker in Settings; Send test uses a per-Route sample request
  (method, query, headers, cookies, body) edited in a dialog and saved in `.fauxfield/ui.json`.
  CLI: `fauxfield response add` and `response set-default`. MCP: `upsert_route` takes
  `response_mode` and Response `rules`, `rules_logic`, `description`, `disabled`; `get_traffic`
  returns the trace. dev-mcp: `ui.state.route`, scenario `m05-responses`.
- M4.1 Reusable code editor: one `CodeEditor` component (`fauxfield-gui/src/code_editor/`) replaces
  the hand-rolled body editor. Grapheme-aware buffer with undo (200 steps / 1 MiB, typing grouped
  within 1 s), soft wrap, tabs, IME, mouse (double/triple click, drag auto-scroll, wheel), auto-close,
  overtype, surround, smart Enter, block indent, comment toggle, line move/duplicate/delete, go to
  line, bracket match, synchronous completion providers, tree-sitter highlighting (JSON, YAML, XML,
  Markdown; incremental), `{{var}}` colouring, JSON/XML/HTML format and validate with line and
  column, `[editor]` settings, new editor theme tokens. The Response body, the file preview and the
  Traffic detail bodies use it. dev-mcp: `ui.state.editors[]`, scenario `m041-editor`.
- Response pane in the Route tab: Status Code (editable), Status (reason phrase, read-only),
  Delay, then Body and Headers sub-tabs. Body: Inline editor with mouse cursor placement and
  selection, JSON/YAML/XML/HTML highlighting, JSON validate and Format, `{{ }}` spans marked;
  File source (`body_file`, workspace-relative, live preview); Dataset shown disabled until M8.
  Headers: key/value table with header-name autocomplete; Content-Type edits the Response's
  `content_type`. Response rows have a menu (Set as default, Rename, Duplicate, Delete).
- M4 Route matching and folders: param (`:id`, `{id}`, `*rest`) and regex Routes with named-group
  params, `ANY`, `priority`, `strict_slash`; a compiled matcher (hash map, segment trie, regex set;
  p99 under 1 µs at 1,000 Routes, bench `matcher`); shadowing analysis (W051) and diagnostics
  E033/E034/E035/W052; Folder `enabled`/`collapsed`; store ops `move_route`, `move_folder`,
  `delete_folder(with_routes)` and single-key setters; tree: warning icons, route counts, disabled
  Folders, drag and drop, Alt+Up/Down, Folder menu and Folder tab; Route header Match chip and
  Settings sub-tab (match mode, priority, test a path); Traffic detail shows matched route and
  params; `fauxfield route add`, `route list --json` gains `match`, `priority`, `shadowed_by`; MCP
  `match_path`; fixture `fixtures/routing-workspace`.
- M3.3 Tabbed sidebar: the left dock has an icon-only tab strip (Servers, Environments, Datasets)
  under the workspace header, one list at a time, with the count in the panel title. Actions
  `Sidebar: Show Servers/Environments/Datasets` (Cmd/Ctrl+Alt+1..3), View > Sidebar submenu with
  the active tab checked, tab persisted in layout.json, reveal switches tab, hidden rows cannot be
  renamed/duplicated/deleted by shortcut. dev-mcp: `ui.state` `sidebar` and `tree.visible_rows`,
  `ui.command set_sidebar_tab`, `sidebar` events, seven `sidebar-*` fixtures, `m033-sidebar`.
- Toggle switches now match the mockup (26x14 track, 10 pt knob).
- M3.2 Application menu and About box: one data-driven menu model (`fauxfield-gui::menu`) drawn
  natively on macOS and as an in-window bar on Windows and Linux (collapses to a hamburger below
  1000 px; Alt mnemonics, arrow keys, type-ahead), with state-driven enabling and labels, Open
  Recent (missing folders greyed) and a modal-aware rule so greyed items and their keys do nothing.
  New actions: About, Hide/Hide Others/Show All, Minimize, Zoom, Full Screen, Reveal Workspace,
  Documentation, Release Notes, Report an Issue, Cut/Copy/Paste/Select All in text inputs. About
  box with `Copy Version Info`; `BuildInfo` (commit, date, target, gpui) in `fauxfield-control`
  and `fauxfield version [--json]`. Keymap `platform` field and new defaults (Open Folder, Close
  Workspace, Undo/Redo, Full Screen, Minimize). Dev-mcp: `ui.menu`, `ui.state.menus`,
  `menu`/`about` events, `set_menu_style`, scenario `m032-menu`, six state fixtures.
- M3.1 Workspace lifecycle and M3 completion: native folder picker (typed-path fallback), Close
  workspace with a shared guard and confirmation, a Recent list (remove, clear, missing folders),
  rename workspace (`Store::set_workspace_name`, `fauxfield workspace info|rename`, MCP
  `update_workspace`), a sidebar workspace node and per-server hamburger menus, an editable Server
  tab (host, port, prefix, enabled, 404 closest-routes hint), disabled servers (skipped by Run all
  and `fauxfield run`, refused on explicit start, `Enable and start` toast), delete confirmation,
  safe duplicate. The control socket and MCP transport work on Windows (named pipes), read-only MCP
  no longer creates a host, an invalid file edit keeps the previous routes serving, `unmatched_hint`
  setting, `logs --har` reports "--har arrives in M12", `mock.rs` removed. Scenario
  `m031-workspace`, ten new state fixtures, `ui.create_workspace` and new `ui.command`s.
  Known gaps: Windows CI jobs not run locally; save-to-served latency is about 69 ms against the
  50 ms budget (bench in M4).
- M3 Run a mock server (GUI and headless): `fauxfield-engine` (a hyper listener per Server, exact
  Route matching with the Server prefix, static Responses with headers, `body_file` and delay, a
  404 `application/problem+json` with the closest Routes, atomic route-table hot reload, port-in-use
  errors, a 2000-entry recording ring buffer with header redaction and Copy as cURL) and
  `fauxfield-control` (`ControlApi`, the host lock `.fauxfield/run/host.json`, a JSON-RPC 2.0
  control socket with owner-only permissions, shared stale-socket handling). CLI: `fauxfield run`
  (`--server`, `--port-offset`, `--host`, `--watch`, `--log-format`), `status`, `stop`, `logs
  [--follow] [--status] [--since]`, all with `--json`; exit code 4 without a host and 5 for a port
  in use. The app starts, stops and restarts Servers (`Cmd/Ctrl+R`, `Cmd/Ctrl+Shift+R`), shows live
  Traffic with a request, response and timing Inspector, Copy as cURL, Send test
  (`Cmd/Ctrl+Enter`), a port-in-use toast with Change port, Logs, and a quit confirmation. MCP
  tools `start_server`, `stop_server`, `restart_server`, `get_traffic`, `send_test_request`;
  `fauxfield mcp` attaches to a live host or becomes one; `ui.events` gains `server` and `traffic`;
  `ui.state` gains server status, address and error, the latest request, and `toast_action`.
  Scenario `m03-run`, state fixtures `server-running`, `traffic-live`, `traffic-empty`, CI job
  `headless-run`. Known gaps: no Windows control socket or MCP named pipe yet; the mock Traffic
  generator remains only for tests and dev-mcp builds.
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
