---
title: Routes and matching
description: Exact, param and regex Routes, precedence, Folders, strict trailing slash and shadowing.
---

A Route matches by `method` and `path`. `match` is `exact`, `param` or `regex`; leave it out and
Fauxfield infers `param` when a segment is `:name`, `{name}` or starts with `*`, else `exact`.

| Pattern | Matches |
|---|---|
| `/users/:id` or `/users/{id}` | one non-empty segment, captured as `id` |
| `/files/*rest` | one or more trailing segments, captured as `rest` (bare `*` captures `*`) |
| `^/v[0-9]+/orders/(?P<id>\d+)$` with `match = "regex"` | the path (Server prefix removed); named groups become params |

Params are percent-decoded strings and show in Traffic under **Matched route**.

## Precedence

Among Routes whose method and pattern match, the winner is the highest `priority` (default 0), then
exact before param before regex before wildcard, then **effective file order**: Folders in order,
each with its Routes, then Routes without a Folder (the order the tree shows). `ANY` matches every
method and has no specificity bonus.

`strict_slash = true` on a Server makes `/users` and `/users/` different paths.

## Folders

Folders group Routes in the tree. Drag Routes between Folders (or onto a Server to move them to the
root), or use `Alt+Up` / `Alt+Down` and the row menu. Folder menu: disable or enable all Routes
(`enabled = false`), start collapsed, delete with or without its Routes. Double-click a Folder to
open its tab.

## Shadowing

A Route that an earlier-winning Route provably matches for every request is reported as **W051** in
Problems and with a warning icon in the tree. The check is conservative.

## CLI

```bash
fauxfield route add users-api GET '/users/:id' --folder Users --priority 5 --json
```
