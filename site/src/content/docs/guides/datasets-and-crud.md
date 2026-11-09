---
title: Datasets and CRUD
description: Fake JSON databases and json-server-style CRUD Routes, with reset and persistence.
---

A **Dataset** is a named JSON collection. A `crud` **Route** serves it with six operations.

```toml
[[datasets]]
name = "products"
seed = "data/products.json"      # or seed_template = "files/products.json.tera"
key = "id"
id_strategy = "increment"        # uuid | increment | ulid
persist = false                  # true: mutations survive a restart (.fauxfield/state/)

[[routes]]
id = "rte_products"
kind = "crud"
path = "/products"
dataset = "products"
validate = "schemas/product.json"   # optional JSON Schema, 422 problem+json on failure
```

| Method | Path | Behaviour |
|---|---|---|
| GET | `/products` | List with `_page`, `_limit`, `_sort`, `_order`, `q`, `field=v`, `field_gte`, `field_lte`, `field_ne`, `field_like`; sets `X-Total-Count` and `Link` |
| GET | `/products/:id` | One item or 404 |
| POST | `/products` | Create (201, `Location`) |
| PUT | `/products/:id` | Replace |
| PATCH | `/products/:id` | JSON merge patch (RFC 7386) |
| DELETE | `/products/:id` | 204 |

A normal `http` Route with the same method and path and a higher priority overrides one operation.
Templates read the live rows as `data.products`.

## In the app

The **Datasets** sidebar tab lists each Dataset with its row count. Open one for a table or JSON view
of the live rows; click a cell to edit, **Add row**, **×** to delete, **Export…**, and **Reset**
(Cmd/Ctrl+Shift+Backspace) to restore the seed. **New CRUD resource…** asks for a name, a path and
fields (`name:name, email:internet.email, price:float`) and writes the seed template and the Route.

## Command line

```bash
fauxfield data list
fauxfield data show example-api users
fauxfield data export example-api users --out users.json
fauxfield data import example-api users users.json
fauxfield data reset --server example-api
```

With a running host these act on live data; otherwise `import` rewrites the seed file and `reset`
clears persisted state.
