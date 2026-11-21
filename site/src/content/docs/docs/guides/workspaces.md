---
title: Workspaces and files
description: The workspace folder is the source of truth. Layout, autosave, hot reload and validation.
---

A **Workspace** is a folder of TOML files. The app, the CLI and MCP tools all read and write the same
files; nothing lives only in the app.

```
my-workspace/
  fauxfield.toml        # manifest
  servers/*.toml        # one file per Server (the file name follows the Server name)
  environments/*.toml   # one file per Environment
  .fauxfield/           # runtime only, git-ignored (ui.json, run state)
```

## Create and open

```bash
fauxfield init my-workspace      # creates the layout with one example Server
fauxfield my-workspace           # opens the app on it
fauxfield                        # nearest ancestor with fauxfield.toml, else the Welcome view
```

The Welcome view offers Open folder, Create workspace and Recent workspaces.

## Editing

- Create, rename, duplicate and delete Servers, Folders, Routes and Responses from the tree context
  menu, the Command Palette, or `Cmd/Ctrl+N` (Route), `Cmd/Ctrl+Shift+N` (Server), `Cmd/Ctrl+D`.
- Click a Route's method or path, or a Response's status, delay, content type or headers to edit
  them. `Edit: Undo` and `Edit: Redo` act on the file of the current Server.
- Edits go through `toml_edit`, so comments, key order and fields Fauxfield does not know yet are kept.
- Changes are written after 300 ms of idle with an atomic rename. Edits made in another editor reload
  silently; if they collide with an unsaved edit, a merge banner offers **Keep mine** or **Load from disk**.

## Validation

`fauxfield validate [-w PATH] [--json]` prints diagnostics (`file`, `path`, `severity`, `code`,
`message`) and exits with code 3 when there are errors. The Problems panel shows the same list; click
a problem to open its Route or Server. The server file JSON Schema is published at
`/schema/v1/server.json` (`cargo xtask schema`).

| Code | Meaning |
| ---- | ------- |
| E001 | File does not parse |
| E010–E013 | Duplicate Server, Route, Folder or Response ID |
| E020 | Port already used by another enabled Server |
| E030 | Route names an unknown Folder |
| E031, E032 | Bad Route path or method |
| E033, E034, E035 | Bad param pattern, unknown `match` value, bad regex |
| W051 | Route is never reached: shadowed by an earlier-winning Route |
| W052 | `match = "exact"` on a path that looks like a param pattern |
| E040, E041, E042 | Bad status, both `body` and `body_file`, `body_file` outside the Workspace |

## Listing

```bash
fauxfield server list --json
fauxfield route list --server example-api --json
```

## Agents

`fauxfield mcp [PATH]` exposes `workspace_info`, `list_servers`, `get_server`, `create_server`,
`update_server`, `delete_server`, `list_routes`, `get_route`, `upsert_route`, `delete_route` and
`validate`. Writes use the same code path as the app. Variables that look like secrets are redacted.

## Open, close, rename and Recent (app)

- **Open folder** and **Create workspace** (Welcome, the workspace menu in the sidebar, the Command
  Palette) open the system folder dialog. If no dialog is available you type the path instead; the
  palette also has **Open Folder by Path** and **Create by Path**.
- Opening a folder without `fauxfield.toml` says so and offers **Create workspace here**. Creating in
  a folder that is already a workspace offers **Open it**. Creating in a non-empty folder asks first;
  existing files are never overwritten.
- **Close workspace** (palette, workspace menu) stops the servers after asking, releases the host and
  returns to Welcome. Switching workspaces uses the same confirmation. Unresolved merge conflicts
  block it ("Resolve the merge banner first.").
- **Recent** lists up to 10 workspaces with their name, folder and when you opened them. A folder that
  moved shows "Not found". **Remove** edits only the list; **Clear recent** empties it.
- **Rename workspace** changes `name` in `fauxfield.toml` (comments and key order are kept, and it can
  be undone). The folder on disk is not renamed. From the terminal:

```bash
fauxfield workspace info --json
fauxfield workspace rename "My Mocks"
```

An invalid name (empty, over 80 characters, control characters) exits with code 3.

## The sidebar

The left dock has a header with the workspace name, an icon-only tab strip and one list. The tabs are
Servers, Environments and Datasets; hover one for its name and shortcut (Cmd/Ctrl+Alt+1, 2, 3, also in
View > Sidebar and the Command Palette). The selected tab is remembered between launches. Opening an
item from Quick Search or Problems switches to the tab that owns it.

## Environments and variables

An Environment is a file in `environments/` with a `[variables]` table. Exactly one is active per run: pick it in the title bar, press Cmd/Ctrl+E, run `fauxfield run --env staging`, or `fauxfield env use staging` (a running host switches live; templated headers and the Server `prefix` change on the next request, no restart).

```toml
name = "staging"
dotenv = ".env.staging"          # optional; its values are never written back
[variables]
base_url = "https://staging.acme.dev"
auth_token = "${env:ACME_TOKEN}"  # read from dotenv, then the process environment
```

`{{ vars.name }}` resolves, most specific first: runtime, route, server, environment, global. `{{ env }}` is the active Environment name. The Inspector lists every variable in scope with where it came from. Keys containing `token`, `secret`, `password` or `key` are masked until you choose Show secrets.

```bash
fauxfield env list
fauxfield var set base_url https://x.dev --scope environment --env staging
fauxfield var get base_url --server users-api
```

## Start from an example

`fauxfield init <path> --example <name>` creates one of ten embedded multi-Server Workspaces instead of the one-Server layout (`--list-examples` shows them). Existing files are never changed: a folder that already holds any of the example's files is refused, and `--force-example` writes the rest and skips those. See [Example Workspaces](/docs/guides/examples/).
