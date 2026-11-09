---
title: Network and error simulation
description: Latency, bandwidth, faults, rate limits, chaos and network profiles for resilience testing.
---

Fauxfield can make a mock behave like a bad network. Everything below is a file setting except the
**network profile** and the **chaos switch**, which are runtime-only.

## Latency, jitter and bandwidth

```toml
[network]
latency_ms = 40
jitter_ms = 20          # each request waits 40 ± 20 ms
bandwidth_kbps = 400    # 0 = unlimited; the body is streamed at this speed
```

A Response can add its own delay, a fixed number or a range:

```toml
[[routes.responses]]
delay_ms = "200..800"
```

## Faults

```toml
[[routes.responses]]
status = 200
fault = "reset"         # none, timeout, reset, close, empty, malformed, slow_body
fault_rate = 0.3        # 30 % of requests; default 1.0
```

| Fault | What the client sees |
|---|---|
| `timeout` | Nothing, until it gives up |
| `reset` | A TCP reset (`ECONNRESET`) |
| `close` | The headers, then the connection closes |
| `empty` | The status and headers with no body |
| `malformed` | A body shorter than its `content-length` |
| `slow_body` | The body at the network bandwidth (56 kbps if none is set) |

*Add error preset* on a Route also offers "502 after 10 s", "504 after 30 s" and one preset per fault.
Traffic marks injected faults with a bolt, and the Inspector shows the timing breakdown (match,
render, delay, transfer).

## Rate limiting

```toml
[rate_limit]
enabled = true
requests = 100
window = "1m"           # 30s, 1m, 2h
key = "ip"              # or header:x-api-key, query:user
```

Past the limit a request gets `429` with `Retry-After` and `X-RateLimit-Limit`, `-Remaining`, `-Reset`.
*Reset State* clears the counters.

## Network profiles and chaos

The status bar switches the whole workspace between **None**, **Fast 3G**, **Slow 3G**, **Flaky Wi-Fi**
(5 % resets) and **Offline** (connections are refused). A profile replaces every Server's `[network]`
until it is set back to None. **Chaos** (Server menu, tree menu, palette) makes 20 % of a Server's
requests fail with a random fault; a badge shows it in the tree.

```bash
fauxfield run --network slow-3g --chaos 0.3
```

Agents can use the MCP tools `set_network_profile` and `set_chaos`.
