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

## Multiple and conditional responses

A Route can hold several Responses. `response_mode` picks one per request:

| Mode | Behaviour |
|---|---|
| `rules` (default) | The first Response, in order, whose rules pass. Falls back to the Response marked `default`, else the first one. A Response without rules is only reached by that fallback. |
| `sequential` | Cycles through the Responses in order; *Dataset: Reset State* restarts it. |
| `random` | A uniform random pick. |
| `disabled` | The Route is skipped as if it did not exist. |

A Response with `disabled = true` is skipped in every mode. Each rule has a `target`
(`header`, `query`, `cookie`, `param`, `body`, `method`, `path`, `ip`, `count`; `variable` and
`dataset` arrive later), a `key`, an `op` (`eq`, `ne`, `contains`, `starts_with`, `ends_with`,
`regex`, `exists`, `absent`, `gt`, `gte`, `lt`, `lte`, `in`, `json_schema`), a `value` and an optional
`negate`. `rules_logic` is `and` (default) or `or`. A body key starting with `$` is a JSONPath
(`$.user.role`); otherwise it names a form field. `count` is the Route's call number.

```toml
[[routes.responses]]
id = "rsp_admin"
name = "Admin"
  [[routes.responses.rules]]
  target = "query"
  key = "role"
  op = "eq"
  value = "admin"

[[routes.responses]]
id = "rsp_401"
name = "No token"
status = 401
  [[routes.responses.rules]]
  target = "header"
  key = "Authorization"
  op = "absent"
```

Every request in Traffic records which Responses were evaluated, which rules passed and the value
compared (Inspector, *Response selection*). The Rules sub-tab edits all rules of a Route in one table,
*Add error preset* creates RFC 9457 `application/problem+json` Responses, and *Sample…* sets the
request Send test fires. `fauxfield response add <route>` and `fauxfield response set-default
<response>` do the same from the CLI.

## CLI

```bash
fauxfield route add users-api GET '/users/:id' --folder Users --priority 5 --json
fauxfield response add <route-id> --name Teapot --status 418 --json
fauxfield response add <route-id> --preset 401
fauxfield response set-default <response-id>
```

## Templating

Response bodies, `body_file` contents, header values, Route `headers` and string rule values are
[Tera](https://keats.github.io/tera/) templates unless the Response sets `template = false`.

```toml
[routes.headers]
x-request-id = "{{ uuid() }}"

[[routes.responses]]
id = "rsp_list"
content_type = "application/json"
body = """
[{% for _ in repeat(n=20) %}{"name": "{{ fake(kind='name') }}", "email": "{{ fake(kind='internet.email') }}"}{% if not loop.last %},{% endif %}{% endfor %}]
"""
```

Shorthand: `{{ fake.name }}`, `{{ fake.email }}` and `{{ fake.address.city }}` mean the same as
`{{ fake(kind='name') }}` and so on (a kind may be shortened to its last segment when unambiguous),
and `{{ uuid }}`, `{{ ulid }}` and `{{ now }}` work without parentheses. Use the call form to pass
arguments, for example `now(format=...)`.

Set `fake_seed = 42` on the Server for identical output across runs (the RNG derives from the seed,
the Route ID and the call number); without it the output is random and the tree shows a dice icon.
A Route header is added to every Response; a Response header of the same name wins. Syntax errors are
underlined in the editor, a preview renders against the sample request, and a failing template returns
500 with the error in Traffic. See [Template functions and fake kinds](/reference/template-functions/).

```bash
fauxfield render users-api <route-id> --seed 42 --json
```
