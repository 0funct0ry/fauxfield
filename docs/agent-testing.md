# Agent testing guide

Fauxfield can be run, driven, inspected, screenshotted and audited by an agent through MCP while
you build it (SPEC §11.4). All of this is **dev-only**: the `ui` tool class exists only in builds
with the `dev-mcp` cargo feature, which never ships in release builds.

## Setup

Start the app with the MCP host on a unix socket (needs a display):

```bash
make dev-mcp MCP_SOCKET=/tmp/ff.sock
```

`.mcp.json` for Claude Code, socket (the app is already running):

```json
{ "mcpServers": { "fauxfield-ui": { "command": "nc", "args": ["-U", "/tmp/ff.sock"] } } }
```

or let the client own the process over stdio (stdout is protocol-only):

```json
{
  "mcpServers": {
    "fauxfield-ui": {
      "command": "cargo",
      "args": ["run", "-q", "-p", "fauxfield", "--features", "dev-mcp", "--", "--mcp-stdio"]
    }
  }
}
```

Call `server_info`: `features` must list `dev-mcp`. The headless `fauxfield mcp` has the product
tools only, no `ui.*`.

## Tool catalog (class `ui`)

| Group | Tools |
|---|---|
| State | `ui.state` (includes `menus`), `ui.open_workspace`, `ui.create_workspace`, `ui.open_tab`, `ui.menu` (`list`, `invoke`, `open`, `close` by label path, e.g. `[File, Open Recent, demo]`) |
| Workspace | `update_workspace`, `update_server` (incl. `unmatched_hint`), `ui.command` `close_workspace`, `add_recent`, `remove_recent`, `hold_port` (binds a port until `ui.reset_view`), `set_menu_style` (`native`|`in_window`), `open_menu` (`File` or `File/Open Recent`, for state fixtures), `request_quit` (refused when nothing runs) |
| Run | `start_server`, `stop_server`, `restart_server`, `send_test_request`, `get_traffic` (product tools, not `ui.*`; they drive the engine the window hosts) |
| Input | `ui.list_actions`, `ui.invoke_action`, `ui.command`, `ui.key`, `ui.type_text`, `ui.wait_for_idle`, `ui.wait_for` (`ui.click` and `ui.scroll` return an error: GPUI 0.2.2 cannot inject pointer events) |
| Tree | `ui.tree`, `ui.find`, `ui.click_element` |
| Screenshots | `ui.screenshot`, `ui.screenshot_all`, `ui.set_theme`, `ui.resize` |
| Audit | `ui.audit`, `ui.walkthrough` |
| Scenarios | `ui.run_scenario`, `ui.record` |
| Events | `ui.events` (kinds include `menu` opened/closed/invoked and `about` opened/closed) |

The registry is the source of truth: `tools/list` shows names, descriptions and schemas.

### Scenarios

YAML in `fixtures/scenarios/<name>.yaml`; the name equals the file stem. Steps are one-key maps:

```yaml
name: demo
size: default          # optional: default | compact | min | WxH
steps:
  - key: secondary-shift-p
  - type_text: theme
  - assert: {pointer: /command_palette/query, equals: theme}   # equals | contains | matches | not
  - find: {label: Traffic, role: table}
  - assert_find: {pointer: /matches/0/total, equals: 10000}    # or ids: [quick-search]
  - screenshot: {name: palette}
  - audit: {themes: [one-light], states: [route-tab]}          # fails above max_errors (default 0)
  - wait_event: {kind: generation, timeout_ms: 2000}
```

Other steps: `invoke`, `command`, `click_element`, `open`, `set_theme`, `resize`, `wait_for_idle: {}`,
`wait_for`, `state` (a named state from `fixtures/ui-states` or a list of steps). A scenario starts
from the baseline view; `reset: false` opts out. The first failing step stops the run and reports
expected, actual, `ui.state` and a screenshot.

Run one with `ui.run_scenario {"name": "m01-shell"}` or from a shell (exit 0 pass, 1 failed step,
2 app unreachable):

```bash
fauxfield mcp scenario fixtures/scenarios/m01-shell.yaml --socket /tmp/ff.sock --out target/shots [--no-capture]
```

`ui.record {"action": "start"}` … drive the app … `ui.record {"action": "stop", "name": "x"}` writes
the state-changing calls as a scenario. `broken-assertion` fails on purpose, to prove reports.

### Events

`ui.events {since, kinds, wait_ms}` reads a 1024-entry buffer of `generation` (the `ui.state`
snapshot changed) and `toast` events, each with a monotonic `seq`; `gap` is true if events before
`since` were dropped. Wait with `wait_ms` instead of polling `ui.state`.

## Safety model

- Tool classes: `read`, `write`, `execute`, `ui`. `ui` can never be granted by `--allow`; only the
  in-app host adds it.
- The socket is mode 0600, there is no TCP listener, and a stale socket file is replaced but a
  live one is refused.
- Tools never touch GPUI off the main thread; an unanswered `ui.*` call returns "ui thread busy"
  after 5 s. `ui.invoke_action` refuses `quit`.
- `dev-mcp` is absent from release builds: no MCP-in-GUI code and no `--mcp-*` flags.

## macOS Screen Recording

On macOS screenshots use `screencapture`, which needs Screen Recording permission for the process
that launched the app (your terminal or editor): System Settings → Privacy & Security → Screen
Recording. Without it `ui.screenshot` returns an error naming the permission instead of an image;
use `capture: false` (or `--no-capture`) to keep going. On Linux install `grim` or ImageMagick
(`import`); CI uses xvfb.

## Bug-hunting playbook

1. `make smoke-ui`, or run every scenario: they are the regression baseline.
2. `ui.audit {states: ["all"], themes: ["one-dark","one-light"], sizes: ["default","min"]}`: errors
   (UX102/103/110/120/130) must be zero; read the warnings. Add `perf: true` for UX150 (noisy in
   debug builds).
3. `ui.screenshot_all` and look at the images: clipping, overlap, missing empty or error states,
   low contrast.
4. Explore by keyboard and by element id: `ui.find` → `ui.click_element`; `ui.key` for every
   binding in SPEC §9.8, including the Cmd+K chord and its 400 ms timeout.
5. Try the edges: tiny window (`ui.resize min`), every theme, a picker open while switching tabs,
   Escape from every modal.
6. Record a failing session with `ui.record`, keep it as a scenario, and attach the screenshot and
   element ids to the report. Separate app failures from flakiness (timing, ports).
7. Product tools (`start_server`, `send_test_request`, `get_traffic`, `validate`) arrive with the
   milestones that add them; use them to check the GUI shows what the engine did.

## Menu notes (M3.2)

- A native macOS menu cannot be seen or screenshotted: assert on `ui.state.menus` / `ui.menu list`, which reflect the built model exactly. To screenshot the bar, `ui.command set_menu_style in_window`.
- A greyed item is an item whose handler is not registered, so `ui.invoke_action` on it fails. When state changed in the same call sequence, an action is dispatched after the next frames; input tools wait for that, so no extra `ui.wait_for_idle` is needed for it.
- `ui.menu invoke` on a Recent entry accepts the bare workspace name as the last path segment.

## Sidebar (M3.3)

- `ui.state`: `sidebar {tab, tabs[{id,label,count}]}`, `tree.visible_rows` (rows of the active tab).
- `ui.command {command: set_sidebar_tab, tab: servers|environments|datasets}`; `ui.events` kind `sidebar` `{tab}`.
- Dev ids: `sidebar-tabs`, `sidebar-tab-<tab>`, `sidebar-title`, `sidebar-new-server`, `sidebar-empty-<tab>`, `sidebar-empty-new-server`.
- Datasets (M10.01): `sidebar-new-dataset`, `sidebar-empty-new-dataset`, `tag-unused-<row id>`, `ds-used-by-<route id>`, `server-new-dataset-<server id>`, `route-dataset-<route id>` (combo), modal `delete_dataset_confirm` (`delete-dataset-confirm-yes`, `delete-dataset-seed`, `delete-dataset-state`); `ui.state.datasets[]` has `unused` and `used_by`; scenario `m10-01-dataset-lifecycle`.
- Rows of a hidden tab are not in `ui.tree`; click the tab first. Fixtures `sidebar-*`; scenario `m033-sidebar`.
