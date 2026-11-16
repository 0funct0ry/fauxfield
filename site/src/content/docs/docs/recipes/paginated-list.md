---
title: Paginated list with filters
description: Serve a Dataset as a list with pagination, filters and sorting.
---

Declare a Dataset with a generator, and point a `crud` Route at it. The list operation paginates,
filters and sorts without further setup.

```toml
[[datasets]]
name = "invoices"

[datasets.generator]
rows = 24

[[datasets.generator.fields]]
name = "id"
kind = "sequence"

[[datasets.generator.fields]]
name = "status"
kind = "one_of"
values = ["paid", "overdue", "void"]

[[routes]]
id = "rte_invoices"
kind = "crud"
path = "/invoices"
dataset = "invoices"
```

```bash
fauxfield run --seed 7
curl 'localhost:3001/invoices?status=overdue&_sort=id&_page=1&_limit=10'
```

The response carries `X-Total-Count` and a `Link` header. Operators and the generator kinds are in
[Datasets and CRUD](/docs/guides/datasets-and-crud/) and the
[seed generators](/docs/reference/seed-generators/); check the TOML against the
[server file reference](/docs/reference/server-file/) (`fauxfield validate` reports mistakes).
