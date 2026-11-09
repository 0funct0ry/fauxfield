---
title: CORS
description: Automatic preflight answers and CORS headers on every response.
---

```toml
[cors]
enabled = true
origins = ["http://localhost:*", "https://app.acme.test"]   # "*" allows any origin
methods = ["GET", "POST", "PUT", "PATCH", "DELETE"]
headers = ["*"]                # "*" reflects Access-Control-Request-Headers
credentials = true
max_age = 600
```

- A request with `Origin` and `Access-Control-Request-Method` is answered with `204` and the `Access-Control-Allow-*` headers, unless a Route explicitly handles `OPTIONS`.
- Every other response (mock, static, 404, 429) gets `Access-Control-Allow-Origin` and `Vary: Origin` for an allowed origin. An origin that is not allowed gets no CORS headers.
- `*` in an origin matches any run of characters (`http://localhost:*`). `origins = ["*"]` with `credentials = true` reflects the request origin, as browsers require.

The Server tab's CORS section has two presets: **Allow localhost dev servers** and **Allow all**.
