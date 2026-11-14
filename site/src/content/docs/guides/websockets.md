---
title: WebSockets
description: Mock WebSocket endpoints with echo, scripted and broadcast modes, templated messages and a live connection monitor.
---

A `websocket` **Route** upgrades on the same port as the rest of the **Server**.

```toml
[[routes]]
id = "rte_events"
kind = "websocket"
path = "/ws/events"
mode = "script"            # echo | script | broadcast

  [[routes.messages]]
  trigger = "connect"
  body = '{"type":"hello","at":"{{ now() }}"}'

  [[routes.messages]]
  trigger = "interval"
  every = "5s"             # 500ms, 5s, 1m, 2h (at least 10ms)
  body = '{"type":"tick","value":{{ random_int(min=0, max=100) }}}'

  [[routes.messages]]
  trigger = "message"
  when = { target = "body", key = "$.type", op = "eq", value = "ping" }
  body = '{"type":"pong"}'
```

## Modes

| Mode | Behaviour |
|---|---|
| `echo` (default) | Sends every incoming message back. |
| `script` | Sends `messages` on `connect`, on an `interval`, or in reply to an incoming `message` whose `when` matches (first match wins; no `when` matches everything). |
| `broadcast` | Like `script`, but interval messages go to every connected client, and an incoming message that no script matches is sent to the other clients. |

## Messages

- `body` is a Tera template. Besides the usual context it sees `connection.id` and, for replies, `message.text`, `message.json` (parsed JSON or null) and `message.binary`.
- `format` controls how a body that renders to valid JSON is sent: `compact` (default, one line), `pretty` (2 spaces) or `raw` (exactly as written). Text that is not JSON is always sent as it is. Binary messages ignore it.
- `binary = true` treats `body` as base64 and sends a binary frame.
- `close_code = 4000` closes the connection after the message is sent.
- Protocol pings are answered with pongs automatically. When the Server stops, clients get close code 1001.
- A plain `GET` without `Upgrade: websocket` answers `426`.

## In the app

**New WebSocket route** (Server menu) or **Route: New WebSocket** (Command Palette) adds a Route. The Route tab has:

- a **Mode** switch: Echo, Script or Broadcast, with a one-line explanation;
- for Script and Broadcast, a **Messages** table (When, Condition, Body, Close). Add rows with *On connect*, *Every…* or *On message*. Click a row to edit it: trigger, interval, JSONPath condition, close code, binary, and the body in a multi-line editor with a rendered preview (**Apply body** saves it);
- **Format JSON** in the body editor and in the Send box re-indents JSON (templates such as `{{ uuid() }}` are kept), and **Send as** picks Compact, Pretty or As written. In the body editor and the Send box the Preview sits beside the editor and shows exactly what a client receives; **Render again** draws new random values;
- the live **Connections** (select one to filter the timeline; **Disconnect** closes it);
- **Send**: a multi-line template box with its preview beside it, a Text or Binary (base64) format, **Send as** (Compact, Pretty, As written), a target (all connections or one) and **Send** (Cmd/Ctrl+Enter). The text is a Tera template rendered once per recipient, so `{{ uuid() }}` and `{{ connection.id }}` differ per client; a template error is shown and nothing is sent. The `fauxfield ws send` command and the MCP tool send the text as given;
- the **Timeline** table (time, IN/OUT, type, size, payload) with All/In/Out filters, a payload filter and **Clear**. Click a row to see the full frame, formatted when it is JSON, with **Copy**.

The upgrade itself is a `101` row in Traffic.

## CLI and MCP

```bash
fauxfield ws list my-server
fauxfield ws log my-server --connection wsc_abc12345
fauxfield ws send my-server '{"type":"announce"}' --route rte_events
fauxfield ws disconnect my-server wsc_abc12345 --code 1000
```

Every command takes `--json`. The MCP tools are `list_ws_connections`, `send_ws_message` and `disconnect_ws_client`.
