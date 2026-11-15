---
title: Agents and MCP
description: Let Claude Code, Codex or Cursor create Routes, start Servers and check Traffic through the Fauxfield MCP server, while you watch it happen in the app.
---

Fauxfield ships an **MCP server**. A coding agent that speaks MCP can read your **Workspace**, create **Routes** and **Responses**, start **Servers**, send test requests and read **Traffic**. When the app is open it is the host, so every change an agent makes appears live.

## Set it up (Claude Code)

1. Install Fauxfield and make sure `fauxfield` is on your `PATH` (`fauxfield version` works).
2. In your project folder, create or open a Workspace and configure the agent:

   ```bash
   fauxfield init            # skip if the folder already has fauxfield.toml
   fauxfield agents init --claude
   ```

   `agents init` shows a diff of every file it would write, then asks `Write N files? [y/N]`. It writes:

   - `.mcp.json` with a `fauxfield` entry (`fauxfield mcp . --allow read,write,execute`). Existing servers and keys are kept.
   - an `AGENTS.md` section between `<!-- fauxfield:begin -->` and `<!-- fauxfield:end -->` that tells the agent the layout, the vocabulary and the tool loop. Re-running updates it in place.
   - the `.fauxfield/` line in `.gitignore`.

   Use `--dry-run` to preview, `--yes` to skip the question, `--codex` or `--cursor` for those clients, and `--json` for a machine-readable plan. Without a client flag on a terminal it asks which clients to set up. In the app, run **Agents: Set Up MCP…** from the Command Palette or the File menu: the same plan, with a diff per file and a Write files button. Cancel writes nothing.
3. Restart Claude Code in that folder and run `/mcp`. `fauxfield` should be listed as connected.
4. Open the Workspace in the app (`fauxfield .`) so you can watch.
5. Ask for something real: *"mock the endpoints this React page needs and start them"*.

The agent reads the page, calls `upsert_route` once per endpoint, `start_server`, `send_test_request` for each Route, and `get_traffic` to confirm.

## What you see in the app

- **Status bar:** `● Claude Code attached` (or `2 agents attached`, with a tooltip) while an agent is connected. Click it to open **Logs** filtered to **Agent activity**.
- **Tree rows flash** for 1.2 seconds with an `● agent` tag when an agent creates or updates a Route, Server, Environment or Dataset. A row hidden inside a collapsed Server or Folder flashes its nearest visible parent. With reduced motion the tag stays for 3 seconds without the highlight.
- **Logs → Agent activity:** one line per tool call with time, client, tool, target and outcome. Calls the agent was not allowed to make show in the warning colour. **Copy as Markdown** copies the visible lines. The log never holds secret values.

## Read-only mode

```bash
fauxfield mcp . --read-only        # same as --allow read
```

Write and execute tools are then not listed, and calling one is refused with a protocol error, so an agent cannot mistake a refusal for a failed operation. Choose **Read only** in the setup dialog, or pass `--allow read` to `agents init`.

## Transports

| Transport | Flag | Notes |
|---|---|---|
| stdio | default | What `agents init` configures. Stdout carries only the protocol. |
| unix socket / named pipe | `--transport unix` | `--socket <path>`; mode 0600. |
| streamable HTTP | `--transport http [--port N]` | Binds `127.0.0.1` only (`--host` must be loopback), endpoint `/mcp`. Each run has a bearer token, printed on start and saved to `.fauxfield/run/mcp-http.json` (mode 0600). Requests with a non-loopback `Origin` are refused (403) and requests without the token get 401. |

Settings can also live under `[mcp]` in `settings.toml` (`transport`, `allow`, `socket`, `port`, `redact`); flags win.

## Safety

- File arguments (`body_file`, `dir`, import paths) must resolve inside the Workspace; `../`, absolute paths outside it and symlinks that leave it are refused.
- Variables whose names contain `token`, `secret`, `password` or `key` (and any glob in `[mcp] redact`, for example `"*session*"`) are masked as `***` in every result, error and resource.
- `get_traffic` returns at most 500 Recordings and bodies are cut at 64 KB with `truncated: true`.
- Servers started over MCP bind loopback unless the Server file says otherwise.

## Troubleshooting

- **`/mcp` does not list `fauxfield`:** run `fauxfield version` in a terminal; if it is not found, run `fauxfield agents init --claude --command /full/path/to/fauxfield`.
- **`no host is running`:** read-only tools work without a host. `start_server` makes the MCP process the host when no app or `fauxfield run` is running for the Workspace. Open the app first to watch live.
- **Port in use:** the agent gets the error from `start_server`; change the Server port in the app or with `upsert`/`update_server`.
- **A value shows as `***`:** the variable name looks secret. Read it in the app or the file; agents never see it.
- **Tools and arguments:** see the generated [MCP tools](/reference/mcp-tools/) reference and the [CLI reference](/reference/cli/).
