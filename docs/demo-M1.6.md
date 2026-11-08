# Demo: M1.6 UI audit

1. `make dev-mcp ARGS=fixtures/example-workspace MCP_SOCKET=/tmp/ff.sock` (macOS, needs a display).
2. Connect a client (`.mcp.json` with the `fauxfield-ui` socket server) and call:
   - `ui.audit {states: ["all"], themes: ["one-dark","one-light"], sizes: ["min"]}`: 24 cells in
     about 4 s, 0 errors, repeated calls return identical findings. Warnings left: UX101 (2
     elements), UX131 (tree rows, chevrons, sub-tabs, section nav, Traffic rows), UX140 (Traffic,
     Logs, tree, Problems fixture).
   - `ui.audit {rules: ["UX150"]}`: frame and key-to-paint timing (debug builds are slow).
   - `ui.audit {subtree: "<id from ui.tree>", theme: "one-light"}`: one subtree under one theme.
   - `ui.walkthrough {steps: [{invoke: command_palette}, {type: theme}, {key: escape}]}`: steps,
     keystrokes, clicks, wall-clock ms, toasts.
3. Tests: `make test-gui` (seeded defects for UX101/102/103, data-level seeds in
   `fauxfield-mcp::audit`).
