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
rows. See the [generator reference](/docs/reference/seed-generators/).

```bash
fauxfield data generate example-api users --rows 10 --seed 42       # print
fauxfield data generate example-api users --bake                    # write data/users.json
```

Over MCP: `preview_generator` returns sample values for a definition without touching a Dataset.

## The Dataset table

The Dataset tab shows rows in a virtualised table that stays smooth with 100,000 rows.

- Click a column header to sort: first descending, then ascending, then back to the stored order. Only one column is sorted at a time. Sorting is a view; it never rewrites the Dataset.
- Drag the edge of a header to resize a column (double-click the edge to fit it to its content). Drag a header to move the column. Widths and order are remembered per Dataset.
- Click a row to select it. With the table focused, Up and Down move the selection (wrapping), Home and End jump to the first and last row, Page Up and Page Down move a page, and Esc clears the selection.
- Right-click a row for *Copy cell*, *Copy row as JSON* and *Delete row*. Double-click a row to open it in the JSON view.

The table is read-only in this version; use *Add row*, the JSON view or the seed file to change data.

## Building a Dataset in the table

Run **Dataset: New From Table…** from the Command Palette, name the Dataset and list its columns as `name:type, ...` (types `string`, `int`, `float`, `bool`, `datetime`, `json`). No Route is created.

- Double-click a row to edit a cell; Enter commits, Esc cancels. Values are checked against the column type.
- **Add row**, **Paste Rows** (tab-separated text) and Delete Row write through to `data/<name>.json` for a Dataset built this way. Other edits are live-only until **Save Rows to Seed**; Reset restores the seed.
- **Add column…** takes `name:type` or `name:type=generator` (for example `email:string=fake:internet.email`).
- **Generate rows…** takes `200 bake` (append to the static seed) or `200 keep` (write a `[datasets.generator]` block, regenerated at each start and deterministic with `fake_seed`).
- The **Seed** view edits the seed file as JSON; **Save seed** writes it, **Reload from seed** discards edits.

```toml
[[datasets]]
name = "products"
seed = "data/products.json"

[[datasets.columns]]
name = "id"
type = "int"
```

## Importing and exporting files

Fill a Dataset from a JSON, CSV or Parquet file. Import is a one-time conversion: the rows are written to `data/<name>.json` and the original file is not referenced again.

- **GUI:** *Dataset: Import File…* creates a new Dataset (also: drop a file on the sidebar); **Import…** in a Dataset tab replaces or appends. A notice previews the format, row count, inferred column types, key and warnings; the options prompt takes `replace|append`, `key=COL`, `delim=X` (or `tab`), `decimal=string` and `COL:TYPE` overrides (`int`, `float`, `bool`, `string`). **Export…** picks the format from the extension (`.json`, `.csv`, `.parquet`).
- **CLI:** `fauxfield data import <server> <dataset> <file> [--format json|csv|parquet] [--append] [--key col] [--delimiter ,] [--column-type zip:string]` and `fauxfield data export <server> <dataset> --format csv --out rows.csv`. `--json` reports the row count and warnings.
- **MCP:** `import_dataset` (with `dry_run` to preview).

CSV cells are typed by inference (int, float, bool, null, string; `007` stays a string). Warnings cover ragged rows, duplicate keys and cells that fail an overridden type. Above 100 000 rows you get a warning; above 1 000 000 the import is refused (`--soft-limit`, `--hard-limit`). Parquet and CSV exports are flat: nested values are written as JSON text.

From the command line: `fauxfield data create <server> products --column id:int --column name:string`. MCP: `upsert_dataset` accepts `columns` and `generator`, and `bake_dataset` writes generated rows to the seed.

## Using a Dataset from any Route

A Route does not need `kind = "crud"` to use a Dataset.

### Reading

In the Response editor choose **Body > Dataset**, pick the Dataset and a mode, then **Generate template**. The generated body stays a normal, editable template:

```toml
body = '{{ dataset_rows(data=data.orders, sort="id", order="desc", limit=1) | first | json_encode() | safe }}'
```

`dataset_rows` takes the CRUD filter operators (`status=open&total_gte=5`, `_lte`, `_ne`, `_like`), `sort`, `order`, `page` and `limit`; `dataset_count` returns the number of matching rows. For a "no match" status add a Response with a rule `dataset_count` / key `orders?id={{ request.params.id }}` / `eq` / `0` and status 404.

### Changing

Add Dataset **actions** to a Response. They run after the Response is chosen and before the body renders, and apply together or not at all:

```toml
[[routes.responses]]
id = "rsp_ok"
status = 201
action_error_status = 422
body = '{{ action.0 | json_encode() | safe }}'

[[routes.responses.actions]]
dataset = "orders"
op = "insert"
[routes.responses.actions.fields]
sku = "{{ request.json.sku }}"

[[routes.responses.actions]]
dataset = "stock"
op = "increment"
key = "{{ request.json.sku }}"
field = "qty"
by = "-{{ request.json.qty }}"
fail_if = "{{ result.qty < 0 }}"
```

Ops: `insert`, `update` (`mode = "merge"` or `"replace"`), `delete`, `delete_where` (`filter`), `set`, `increment`, `reset`. Each step's result (new row, changed row, or `{count}`) is `action.<n>`. An action on a persisted Dataset writes to disk (warning W072). Traffic marks requests that changed a Dataset.

### Rules and singletons

Rule targets `dataset` (`name.$.path`) and `dataset_count` (`name?field=value`) branch on Dataset data. A Dataset seeded with an object accepts `PUT` (replace) and `PATCH` (merge) on its CRUD path, and `update`/`set` actions.

## Operation Responses and Rules on CRUD Routes

A CRUD Route can answer a special case itself and leave everything else to the built-in handler. Select an operation row under the Route in the tree (only operations in `crud_ops` appear; the badge counts its Responses), choose **Add Response**, and use the Responses and Rules sub-tabs as on any Route. The editor says *Falls through to built-in create* when no Response matches.

```toml
[[routes]]
id = "rte_users"
kind = "crud"
path = "/users"
dataset = "users"
  [[routes.operations.create.responses]]
  id = "rsp_dup"
  name = "Duplicate email"
  status = 409
  content_type = "application/problem+json"
  body = '{"title":"Duplicate email"}'
    [[routes.operations.create.responses.rules]]
    target = "body"
    key = "$.email"
    op = "eq"
    value = "taken@example.com"
```

- The first Response, in order, whose Rules pass is sent; if none passes, the generated handler runs. A Response without Rules never matches and there is no `default`.
- Rules use every target in the Rules reference (`body`, `header`, `count`, `dataset`, `dataset_count`, …). `count` is per operation and restarts with Reset state.
- A matching Response can carry Dataset actions, delay, fault, headers and Callbacks. Actions run before the body renders, so a Response can change rows and still send a custom body.
- `validate` runs first: a create or update body the schema rejects gets the built-in 422. A higher-priority `http` Route still overrides an operation outright.
- Traffic shows the Response name, or `crud:<op>` when the built-in handler answered. Palette: *Route: Add Operation Response*, *Route: Remove Operation Response*.
- Not available per operation: a separate path, Dataset or `validate`, a default fallback Response, or sequential and random modes. Warnings: W073 (no Rules), W098 (`default` ignored); errors E116 (unknown operation) and E117 (operation not in `crud_ops`).

## Schema designer and table workflow

**New Dataset…** (the Datasets tab `+` or the palette) opens the schema designer. Give the Dataset a name, pick the Server and a source (static seed file, seed template or generator), then design the columns: type, primary key, unique, required, default, generator and description. Starter templates (*id + name*, *Users*, *Products*) fill the grid. Problems show inline and the **Create dataset** button stays disabled until there are none. Creating a Dataset never adds a Route; tick *Also create CRUD Routes* or use **Create CRUD Routes…** later.

Column constraints are enforced on cell edits, inserted rows and HTTP writes (a violation answers 422 with an `errors` list). `default` fills a missing value in a new row.

The Dataset tab keeps one status line (`N rows · M columns · source · used by K Routes`; click it for details). The hamburger menu holds the view switch, **Export…**, **Seed data…**, **Import…**, **Add column…**, **Edit schema…**, **Create CRUD Routes…**, **Reset** and **Clear data…** (empties the live rows; Reset restores them from the seed).

Select rows with the checkboxes, Shift-click for a range and Cmd/Ctrl-click to toggle. The selection bar offers **Copy as JSON**, **Duplicate** and **Delete** (several rows ask first; the toast has **Undo**). Type, press Enter or double-click a cell to edit it; Tab and Shift-Tab move between cells, Enter commits and moves down, Esc cancels, and Cmd/Ctrl-Z undoes the last cell edit.

**Add column…** opens a dialog with the name, type, constraints (primary key, unique, required), default, the generator that fills existing rows, and a description. Table columns grow evenly to fill the width.
