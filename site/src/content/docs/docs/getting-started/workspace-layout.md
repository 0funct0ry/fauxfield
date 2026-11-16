---
title: Workspace layout
description: What each file and folder of a Workspace is for, and what to commit.
sidebar:
  order: 3
---

A Workspace is a folder you can review in a pull request.

```
acme-frontend/
  fauxfield.toml        global Variables and defaults
  servers/              one TOML file per Server: Routes, Responses, Rules, Datasets
  environments/         one TOML file per Environment (local, staging, ...)
  data/                 seed files for Datasets (JSON)
  files/                static and templated bodies
  .fauxfield/           runtime state; git-ignored
```

- **Commit** everything except `.fauxfield/`. `fauxfield init` writes the ignore rule for you.
- **Edit anywhere.** The app, your editor, the CLI and agents all change the same files. Saving from
  another editor hot-reloads the running Server in milliseconds.
- **Comments and key order survive.** The app edits through `toml_edit`.
- **Schemas** for editor completion: [server file](/schema/v1/server.json),
  [settings](/schema/v1/settings.json).

See [Workspaces and files](/docs/guides/workspaces/) for autosave and validation, and the
[Server file reference](/docs/reference/server-file/) for every key.
