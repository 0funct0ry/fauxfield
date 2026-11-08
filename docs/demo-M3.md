# Demo: M3 Run a mock server (GUI and headless)

Every step below was exercised by the `run` and `mcp` integration tests, the GUI tests, or the
`m03-run` scenario. The example server in a fresh workspace listens on 127.0.0.1:3001.

## In the app

```bash
fauxfield init demo && make run -- demo        # or: fauxfield demo
```

1. Click **Example API** in the tree and press `Cmd/Ctrl+R`: the dot turns green and the status bar
   reads `1/1 servers running`.
2. In a terminal: `curl -i localhost:3001/health` returns `200` and `{"status": "ok"}`.
3. The Traffic tab in the bottom dock (`Cmd/Ctrl+\`` toggles the dock) shows the request. Select it: the Inspector shows the request,
   response and timing. Click **Copy as cURL** and paste it into a terminal.
4. Open the Health route and press `Cmd/Ctrl+Enter` (Send test): its request fires and its row is
   selected. `curl localhost:3001/missing` returns a `404 application/problem+json` with `closest`.
5. In another terminal, with the app still open:

   ```bash
   fauxfield status                        # host pid and the running server
   fauxfield logs --follow                 # the same requests as Traffic
   fauxfield logs --status 4xx --json      # only the 404s, one JSON object per line
   ```

6. Port in use: `python3 -m http.server 3001` in another terminal, stop the server in the app and
   start it again. A toast says `Port 3001 is already in use` with **Change port**.
7. Press `Cmd/Ctrl+Q` with a server running: a dialog asks before quitting.
8. Edit `servers/example-api.toml` in an editor (change the body) and save: the next `curl` returns
   the new body without restarting.

## Headless

```bash
fauxfield run demo                         # serves identical responses; Ctrl+C drains and exits
fauxfield run demo --port-offset 100 --log-format json
fauxfield status -w demo --json            # from another terminal
fauxfield stop -w demo                     # stops servers; the headless host exits
```

Exit codes: `4` for `status`/`stop`/`logs` with no host, `5` when a port is taken, `3` when the
workspace has validation errors.

## Agents (MCP)

```bash
fauxfield mcp demo
```

Call `start_server {"server": "example-api"}`, `send_test_request {"server": "example-api", "path":
"/health"}`, `get_traffic {"status": "2xx"}`, `stop_server {}`. With the app open, the same tools act on
the app's engine when it is a dev-mcp build (`make dev-mcp`).

## UI scenarios

```bash
make smoke-ui        # runs every scenario, including m03-run, against a dev-mcp app
```
