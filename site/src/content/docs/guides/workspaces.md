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
