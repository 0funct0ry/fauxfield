---
title: Proxy and partial mocking
description: Mock some endpoints and forward the rest to a real API.
---

```toml
[proxy]
enabled = true
upstream = "{{ vars.upstream }}"   # templated; rendered per request
mode = "fallback"                  # fallback | passthrough
preserve_host = false
timeout_ms = 30000
insecure = false                   # accept invalid upstream certificates (red in the app)

[proxy.request_headers]
"x-forwarded-by" = "fauxfield"

[proxy.response_headers]
"x-served-by" = "fauxfield"

[[routes]]
id = "rte_catalog"
path = "/catalog"
proxy = true     # forward this Route even when others are mocked

[[routes]]
id = "rte_latest"
path = "/orders/latest"
pinned = true    # stay mocked even in passthrough mode
```

## Modes

| Request | `fallback` | `passthrough` |
|---|---|---|
| No Route matches | forwarded | forwarded |
| Route matches | mocked (forwarded if `proxy = true`) | forwarded, unless `pinned = true` |

Disabled Routes and Folders count as "no Route matches", so disabling a mock hands the path back to the upstream.

## What is forwarded

- The Server `prefix` is removed, then the path and query are appended to the upstream base path.
- Bodies stream in both directions; Traffic keeps the first 1 MiB.
- `Host` is the upstream host unless `preserve_host` is on. `x-forwarded-for` and `x-forwarded-host` are added.
- Redirects are returned to the client, never followed.
- Server `[network]` latency, rate limits and CORS apply to proxied responses. Per-Response delays, faults and chaos do not.

## Errors

An unreachable upstream answers `502` and a timeout `504`, both `application/problem+json` naming the upstream. In Traffic the row is tinted red; select it and choose **Mock this route** to create a Route from it.

## In the app

- Server tab, **Proxy**: enable, mode, upstream (with the rendered value underneath), timeout, headers and the insecure toggle.
- Route **Settings**, **Proxy**: *Forward to upstream* and *Pin mock*. The tree shows a globe for forwarded Routes and a pin for pinned ones.
- Traffic has a **SOURCE** column (`mock` or `proxy`); filter with `route: proxy`.
- MCP: `update_server` takes a `proxy` object, `upsert_route` takes `proxy` and `pinned`.
