# Fauxfield

Local-first mock API tool in Rust: a GPUI desktop app, a headless runner, a CLI and an MCP
server, all driven by a workspace folder of TOML files.

> **Status: pre-release (M1, UI shell).** The desktop shell runs on mock data. Real workspaces
> arrive in M2 and running mock servers in M3.

## Run it

```bash
make run          # desktop app (debug)
make help         # every build, test, lint and clean target
make doctor       # toolchain and platform detection
```

On macOS without Xcode's Metal toolchain, the Makefile enables GPUI's `runtime-shaders` feature
automatically. For direct cargo use: `cargo run -p fauxfield --features runtime-shaders`.

Headless build (no GPU dependencies): `make build-headless`.

## Agents

Let a coding agent mock your APIs over MCP:

```bash
fauxfield agents init --claude    # writes .mcp.json and an AGENTS.md section after showing a diff
fauxfield .                       # open the app to watch the agent work
```

Restart Claude Code and run `/mcp`, then ask it to "mock the endpoints this React page needs and
start them". Use `fauxfield mcp . --read-only` for a read-only agent. See
`site/src/content/docs/guides/agents.md`; the tool and CLI references are generated
(`make docs-gen`, checked by `make docs-check`).

## Keys

| Action                            | macOS                  | Linux / Windows                       |
|-----------------------------------|------------------------|---------------------------------------|
| Quick Search                      | `⌘K`                   | `Ctrl+K`                              |
| Command Palette                   | `⌘⇧P`                  | `Ctrl+Shift+P`                        |
| Theme selector                    | `⌘K ⌘T`                | `Ctrl+K Ctrl+T`                       |
| Toggle left / bottom / right dock | `⌘B` / ``⌘` `` / `⌘⌥B` | `Ctrl+B` / ``Ctrl+` `` / `Ctrl+Alt+B` |
| Go to tab 1..9                    | `⌘1`..`⌘9`             | `Ctrl+1`..`Ctrl+9`                    |

Override bindings in `~/.config/fauxfield/keymap.toml` (same format as
`crates/fauxfield-gui/assets/keymap/default.toml`).

## Layout

See `AGENTS.md` for vocabulary and conventions. Crates live in `crates/`; only `fauxfield-gui`
and `fauxfield` may depend on GPUI.

MIT licensed.
