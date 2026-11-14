---
title: Callbacks
description: Simulate webhooks with outgoing requests fired after a Response is sent, with signing, retries and re-fire.
---

A **Callback** is an outgoing HTTP request fired after a **Response** has been sent. Use it to mock an asynchronous flow: a client posts a charge, gets `202`, and a webhook arrives later.

```toml
[[routes.responses.callbacks]]
id = "cb_payment_settled"
method = "POST"                       # default
url = "{{ request.json.callback_url }}"
delay_ms = 2000
retries = 2                           # extra attempts, default 0, at most 10
backoff_ms = 500                      # first wait, doubles each retry
timeout_ms = 10000                    # per attempt
body = '{"event":"payment.settled","id":"{{ request.json.id }}"}'

  [routes.responses.callbacks.headers]
  content-type = "application/json"
  x-signature = "{{ hmac_sha256(key=vars.webhook_secret, data=callback.body) }}"
```

## Rendering

`url`, `headers` and `body` are Tera templates. The `body` renders first; then `callback.body` (the rendered body) joins the context so a header can sign exactly what is sent. `response.status`, `response.headers`, `response.body` and `response.json` describe the Response that was sent. A template error is recorded as a failed entry and not retried.

## Delivery

- Callbacks fire once the response body has been sent; `delay_ms` counts from then. A Response that injects a connection-dropping fault fires none.
- A connection error, a timeout or a `5xx` answer is retried after `backoff_ms`, doubling each time. A `4xx` answer is recorded as an error and not retried.
- Redirects are not followed. Private and loopback addresses are allowed (that is the point) and logged.

## Traffic

Every attempt is an entry with source **callback**, shown directly under the request that triggered it. Select it to see the outgoing request and the receiver's answer, and the *Triggered by* link back to the request. A failed entry raises a toast with the error.

**Re-fire callback** (Traffic menu, command palette *Traffic: Re-fire Callback*, or the Inspector button) sends the Callbacks again now, without the delay. On a request it fires all of its Callbacks; on a callback entry only that one.

## GUI, CLI and MCP

- Response editor → **Callbacks** sub-tab: a table of every Callback (name, verb, URL, delay, retries, timeout, state). The `+` button opens a full-width modal: VERB, URL, delay, retries, backoff, timeout, id, name and description fields, a key/value Headers editor with the Headers tab's completions, a Body editor (Tera completions, highlighting, format on paste) and a live preview of what the receiver gets beside it (rendered against the latest recorded request of the Route). Each row has View (read-only modal), Edit and Delete buttons and a `...` menu with Duplicate, Move up/down and Disable. With a row selected the table also takes Enter (edit), Space (view), Delete, Cmd/Ctrl+D (duplicate), Alt+Up/Down (move) and Alt+D (disable). The modal saves only on **Save** (Cmd/Ctrl+S).
- `fauxfield callback refire <server> <recording> [--callback <id>] [--json]` (needs a running host; ids come from `fauxfield logs`).
- MCP: `upsert_route` takes `callbacks` per Response; `refire_callback` re-fires.

The **Payments provider** built-in template wires `POST /charges` → `202` → a signed `payment.settled` webhook two seconds later.
