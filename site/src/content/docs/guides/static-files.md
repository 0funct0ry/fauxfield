---
title: Static files and file templates
description: Serve a directory with MIME detection, ETag, ranges and Tera-rendered template files.
---

A `static` **Route** serves a directory of your workspace.

```toml
[[routes]]
id = "rte_assets"
kind = "static"
path = "/assets/*"             # /assets and /assets/<file>
dir = "files/public"           # workspace-relative, never outside it
index = "index.html"           # served for a directory request
template_extensions = [".tera"]
```

- **MIME** comes from the file extension; `ETag`, `Last-Modified` and `If-None-Match` / `If-Modified-Since` (304) are set.
- **Ranges.** A single `Range: bytes=a-b` gets `206` with `Content-Range`; an unsatisfiable range gets `416`. Template files are not range-able.
- **Methods.** `GET` and `HEAD`.
- **Traversal** (`..`, encoded `..`, absolute paths, symlinks leaving the directory) is rejected with `403` or `404`.
- **Templates.** A file whose name ends in a template extension is rendered through Tera with the same context as a Response (`request`, `vars`, `env`, `server`, `data`, `state`). It is served with the MIME type of the inner extension: `page.html.tera` is `text/html`, and `/page.html` finds it too. A template error is a `500` problem+json that Traffic records.

```html
<!-- files/public/index.html.tera -->
<h1>Environment: {{ env }}</h1>
```

In the app, **Route: New Static Files…** (Command Palette) picks a folder; the Route tab has the directory picker, the index and template-extension fields, and a file browser with a preview. The MCP tool `list_static_files` returns the same list.
