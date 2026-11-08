# Demo: M1.7 scenarios, events, playbook

1. `make dev-mcp MCP_SOCKET=/tmp/ff.sock` (needs a display; macOS screenshots need Screen Recording).
2. From a shell, run a scenario against the app:
   ```bash
   cargo run -q -p fauxfield --features dev-mcp -- mcp scenario fixtures/scenarios/m01-shell.yaml --socket /tmp/ff.sock --out target/shots
   ```
   49 steps pass; three screenshots land in `target/shots`. Exit code 0.
3. The same with the deliberately broken one:
   `... mcp scenario fixtures/scenarios/broken-assertion.yaml --socket /tmp/ff.sock --out target/shots`
   prints expected, actual and the generation, writes `broken-assertion-failure.png`, exits 1.
   Pointing `--socket` at a path with no app exits 2.
4. From Claude (`.mcp.json` in `docs/agent-testing.md`): `ui.run_scenario {"name": "m01-shell"}`;
   `ui.events {"wait_ms": 2000}` after pressing a key; `ui.record {"action": "start"}`, drive the
   app, `ui.record {"action": "stop", "name": "my-flow"}` writes `fixtures/scenarios/my-flow.yaml`.
5. `make smoke` (headless MCP over stdio and socket) and `make smoke-ui` (all scenarios, the audit
   gate, and the broken one expected to fail).
6. Tests: `make test` (runner, events, recorder, CLI exit codes).
