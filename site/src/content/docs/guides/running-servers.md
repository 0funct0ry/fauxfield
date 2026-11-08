---
title: Running servers
description: Start Servers from the app or headless, watch Traffic, and attach from the CLI or an agent.
---

A **Server** serves the **Routes** in its file on its own port. The app, `fauxfield run` and MCP tools
all drive the same engine, and the workspace files stay the source of truth: edit a file and a running
Server picks up the change without dropping connections.

## In the app

- Start or stop the current Server with `Cmd/Ctrl+R`, everything with `Cmd/Ctrl+Shift+R`. Restart is in
  the Command Palette (**Server: Restart Current**).
- The tree shows a status dot and the port; the status bar counts running Servers.
- `Cmd/Ctrl+Enter` on an open Route (**Route: Send Test Request**) sends its request to the running
  Server and selects the result in Traffic.
- Traffic lists every request. Select one to see its request, response and timing in the Inspector, and
  use **Copy as cURL** to reproduce it. **Traffic: Clear** empties the panel.
- If a port is taken, a toast says so and offers **Change port**.
- Quitting while Servers run asks first.

## Headless

```bash
fauxfield run [PATH]                       # serve every enabled Server until Ctrl+C
fauxfield run --server users-api --port-offset 100 --host 0.0.0.0
fauxfield run --log-format json            # one JSON line per request on stdout
```

Banners go to stderr, request lines to stdout. `fauxfield run` drains in-flight requests for up to five
seconds on `SIGINT` or `SIGTERM`. Exit code 3 means the workspace has validation errors, 5 means a port
is in use.

## Attach to the running host

One process per workspace hosts the Servers (the app or `fauxfield run`). Other commands attach over a
local control socket (never TCP) described by `.fauxfield/run/host.json`:

```bash
fauxfield status [--json]
fauxfield logs [--follow] [--server users-api] [--status 5xx] [--since 10m] [--json]
fauxfield stop [--server users-api]        # without --server, a headless host also exits
```

They exit with code 4 when no host is running. With `--json`, `logs` prints one object per request:
`{ts, server, method, path, status, route, duration_ms}`.

## Unmatched requests

A request that matches no Route gets a `404` `application/problem+json` body that lists the closest
Routes by edit distance:

```json
{
  "type": "about:blank", "title": "No route matched", "status": 404,
  "detail": "No route matches GET /api/user.", "instance": "/api/user",
  "closest": [{ "method": "GET", "path": "/api/users", "distance": 1 }]
}
```

## Agents

`start_server`, `stop_server`, `restart_server`, `send_test_request` and `get_traffic` are MCP tools.
`fauxfield mcp` attaches to a live host, or becomes the host the first time a tool needs one, so an
agent can read the Traffic of Servers you started in the app.

## Enabled and disabled servers

A server has `enabled = true` by default. Run all, `fauxfield run` and `start_server` without a
`server` start enabled servers only. Starting a disabled server explicitly fails with "<name> is
disabled. Enable it first." (exit code 1; the app offers **Enable and start**).

## Server settings in the app

The Server tab edits name, host, port, prefix, enabled and **Suggest closest routes on 404**
(`unmatched_hint = false` omits `closest` from the 404 body). Changing the host or port of a running
server restarts only its listener; changing the prefix swaps routes without a restart. A non-loopback
host such as `0.0.0.0` makes the server reachable from your network.

## Windows control socket

On Windows the control socket is a named pipe `\\.\pipe\fauxfield-<hash>-<random>` created with
`first_pipe_instance` and `reject_remote_clients`. The random part is the access token: it lives only
in `.fauxfield/run/host.json`. Any local process that can read your workspace folder can connect.
`fauxfield run` stops on Ctrl+C (there is no SIGTERM on Windows); `fauxfield stop` ends a headless host.
`fauxfield mcp --transport pipe` serves MCP over a named pipe (`both` means stdio plus the pipe).
