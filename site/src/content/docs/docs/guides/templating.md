---
title: Templating
description: Tera templates in Response bodies, headers and URLs, with request data, Variables and fake data.
---

Response bodies, headers and Callback URLs are [Tera](https://keats.github.io/tera/) templates.

```toml
body = '''
{
  "id": "{{ request.params.id }}",
  "name": "{{ fake(kind="name") }}",
  "plan": "{{ pick(from=["free","team","pro"]) }}",
  "seats": {{ random_int(min=1, max=40) }}
}
'''
```

## What templates can read

- `request`: `method`, `path`, `params`, `query`, `headers`, `cookies`, `body` and `json` of the request.
- `vars`: Variables of the active Environment and the Server.
- `data`: Datasets (see [Datasets and CRUD](/docs/guides/datasets-and-crud/)).

## Fake and random data

`fake(kind=...)`, `uuid()`, `pick(from=[...])`, `random_int`, `seq(name=)` and the rest are listed in
the [template reference](/docs/reference/template-functions/). Set `fake_seed` on a Server, or run
`fauxfield run --seed N`, to make output repeat exactly, which keeps CI deterministic.

## Previewing

The Response editor renders the template live, with a sample request you can change. From the
shell, `fauxfield render <SERVER> <ROUTE>` renders a Response against a sample request without
running the Server (`--json` for machine output).

## Limits

Templates read state but never change it; use Dataset actions to write. A template error answers
500 with the error in Traffic and Problems, not a blank page.
