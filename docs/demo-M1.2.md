# M1.2 demo script: in-app MCP host and `ui.state`

Paste into the PR description. macOS shown; the capture probe needs Screen Recording permission
for the app that launches Fauxfield (your terminal).

## Setup

```bash
make dev-mcp MCP_SOCKET=/tmp/fauxfield.sock   # opens the app and serves MCP on the socket
```

`.mcp.json` for Claude Code (socket via a stdio bridge such as `socat`, or let Claude launch the app):

```json
{
  "mcpServers": {
    "fauxfield-ui": {
      "command": "cargo",
      "args": ["run", "-p", "fauxfield", "--features", "dev-mcp,runtime-shaders", "--", "--mcp-stdio"]
    },
    "fauxfield-ui-socket": {
      "command": "socat",
      "args": ["STDIO", "UNIX-CONNECT:/tmp/fauxfield.sock"]
    }
  }
}
```

## Steps

1. `ui.state`: tabs empty, left dock open, `generation` 1, `window` size and scale.
2. `ui.open_tab {"kind":"route","id":"rt_health"}`: an italic preview tab `GET /health` appears.
3. `ui.state` again: the tab is in `tabs` (`preview: true`), `generation` increased; read it twice
   with no change in between and `generation` stays the same.
4. In the app press `Cmd+Shift+P`, type `theme`: `ui.state` shows `command_palette.open`,
   the query and the result count. `Esc` closes it.
5. `ui.open_tab {"kind":"route","id":"nope"}` returns a tool error, not a crash.
6. `ui.capture_probe`: bounds in screen points, scale and the captured PNG size
   (2560x1600 for the default window on Retina). Revoke Screen Recording and it names the
   permission to grant.
7. `Ctrl+C` in the terminal: the app quits and the socket file is removed.

A release build (`make build-release`) has no `--mcp-*` flags: `fauxfield --help` does not list them.
