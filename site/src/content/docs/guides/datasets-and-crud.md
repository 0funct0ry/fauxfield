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
fields (`name:name, email:internet.email, price:float`) and writes the seed template and the Route;
give it the name of an existing Dataset and it only adds the Route.

A Dataset exists on its own: **New Dataset…** (the `+` on the Datasets tab) creates an empty one with
no Route. A `crud` Route just references a Dataset by name, so deleting the Route leaves the Dataset,
its seed and its persisted state alone. Row menu: Open, Duplicate, Rename, Reset, Export, Delete.
Datasets that no Route uses carry an *unused* badge, and the Dataset tab lists what uses it under
**Used by**. Rename rewrites the Routes that reference the Dataset (and moves its default seed file
and persisted state) and warns about templates that use `data.<old name>`. Delete is blocked while
anything references the Dataset; the dialog lists the references and separately offers to delete the
seed file and persisted state.

## Command line

```bash
fauxfield data list
fauxfield data show example-api users
fauxfield data export example-api users --out users.json
fauxfield data import example-api users users.json
fauxfield data reset --server example-api
fauxfield data create example-api orders        # empty Dataset, no Route
fauxfield data refs example-api orders          # Routes and templates that use it
fauxfield data rename example-api orders purchases
fauxfield data delete example-api purchases     # --force --remove-seed --remove-state
```

Over MCP: `upsert_dataset` (omit `crud_path` for a Dataset without a Route), `dataset_references`
and `delete_dataset`.

With a running host these act on live data; otherwise `import` rewrites the seed file and `reset`
clears persisted state.

## Seed generators

Instead of a `seed` file or a `seed_template`, a Dataset can declare a generator: a row count and a
field list. At most one seed source is allowed per Dataset (diagnostic E075).

```toml
[[datasets]]
name = "users"
[datasets.generator]
rows = 50
[[datasets.generator.fields]]
name = "id"
kind = "uuid"
[[datasets.generator.fields]]
name = "email"
kind = "fake"
fake = "internet.email"
[[datasets.generator.fields]]
name = "team"
kind = "ref"
dataset = "teams"      # key of a row of another Dataset in the same Server
```

Set `fake_seed` on the Server to get identical rows on every start. A `ref` makes the referenced
Dataset load first; a cycle is diagnostic E077, and a bad option is E076 with the field path.
Editing a generator block or a seed file reloads only that Dataset. The Dataset tab previews the first
rows. See the [generator reference](/reference/seed-generators/).

```bash
fauxfield data generate example-api users --rows 10 --seed 42       # print
fauxfield data generate example-api users --bake                    # write data/users.json
```

Over MCP: `preview_generator` returns sample values for a definition without touching a Dataset.
