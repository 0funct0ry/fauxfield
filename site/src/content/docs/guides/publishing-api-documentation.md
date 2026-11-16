---
title: Publishing API documentation
description: Serve Swagger UI for a Server from a generated OpenAPI 3.1 document, with Try it out against the running Server.
---

A Server can publish interactive API documentation for itself. Fauxfield generates an OpenAPI 3.1 document from the Server's files and serves Swagger UI on the Server's own port, so *Try it out* needs no CORS and its requests appear in Traffic like any other.

```toml
[docs_site]
enabled = true          # default false
path = "/__docs"        # outside `prefix`; starts with /, no trailing slash, no : * {
title = "Pet Store"     # default: the Server name
version = "1.0.0"
description = "Markdown; defaults to the Server docs"
```

With the Server running, open `http://127.0.0.1:<port>/__docs/`. The page works offline: Swagger UI is embedded in the binary.

| Request | Answer |
|---|---|
| `GET {path}` | 308 to `{path}/` |
| `GET {path}/` | the Swagger UI page |
| `GET {path}/openapi.json` | the document (`ETag`, 304 on `If-None-Match`) |
| `GET {path}/assets/<file>` | embedded assets (gzip, immutable caching) |
| other methods, unknown files | 405 / 404 |

The docs path is answered before CORS, rate limits and Route matching, is never recorded in Traffic, and wins over any Route at or under it (W096).

## What is documented

Every enabled Route is listed, grouped by Folder: http Routes (`ANY` expands to GET, POST, PUT, PATCH and DELETE), `crud` Routes (one operation per entry of `crud_ops`, with generated parameters, headers, status codes and the Dataset schema), `static` Routes and WebSocket Routes. Query and header Rules become optional parameters, and each Response becomes a named example for its status code. Schemas come from Dataset columns first, then from up to 100 seed rows; Tera bodies are rendered once in preview mode with no side effects, `fake_seed` 0 and secret Variables shown as `***`.

Not documented: regex Routes (unless `openapi.path` is set; W097), disabled or shadowed Routes, Routes in disabled Folders, callbacks, cookie and body Rules (listed under `x-fauxfield-rules`), WebSocket messages and any authentication scheme.

## Where to write it in the app

- **Description (Markdown):** the Route's **Docs** sub-tab, the **Docs** section of a Folder tab (tag description) and the **Docs** section of the Server tab (page intro). Text is saved when you leave the editor.
- **Summary, tags, operation ID, hidden, documented path, request schema:** the **API documentation** group of the Route's Docs sub-tab.
- **Response schema file:** the *Schema file* field of the Response editor, next to Description.
- **Title, version, intro:** the Server tab's API documentation section.
- Agents can set the same keys with MCP `upsert_route` (`docs`, `openapi`, Response `schema`) and `update_server` (`docs`).

## Overrides

```toml
[routes.openapi]
hidden = false
summary = "List pets"
description = "Markdown"
tags = ["Pets"]
operation_id = "list_pets"
path = "/pets/{id}"            # needed for regex Routes
request_schema = "schemas/pet.json"   # JSON Schema file inside the workspace
```

A Response may carry `schema = "schemas/pet-list.json"`. Schema files stay inside the workspace (E042) and must be valid JSON (E115).

## From the GUI, CLI and MCP

The Server editor has an **API documentation** section (switch, path, title, version, description, *Open in browser*, *Copy URL*, *Copy OpenAPI JSON*, *Save OpenAPI as…*). The palette has *Server: Toggle API Documentation*, *Open API Documentation* (`Cmd/Ctrl+Alt+O`), *Copy API Documentation URL* and *Export OpenAPI Document…*. A running Server with docs shows a book badge in the tree.

```bash
fauxfield docs spec example-api -o openapi.json   # no host needed
fauxfield docs url                                # running Servers that publish docs
fauxfield docs open example-api
```

MCP: `get_openapi` returns the document (capped at 64 KB, `truncated: true` beyond) and `update_server` accepts `docs_site`.

## Security

The documentation has no authentication of its own: anyone who can reach the Server's port can read it. Enabling it on a non-loopback `host` raises W095.
