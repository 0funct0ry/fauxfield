---
title: Fail every third request
description: Return a 503 on the third call to test retry logic in your client.
---

Add two Responses to a Route. The Rule on the second matches on the call count.

```toml
[[routes]]
id = "rte_flaky"
method = "GET"
path = "/status"

[[routes.responses]]
id = "rsp_ok"
name = "OK"
default = true
status = 200
body = '{"ok": true}'

[[routes.responses]]
id = "rsp_down"
name = "Flaky upstream"
status = 503
headers = { "Retry-After" = "1" }
```

Add a `count` Rule to *Flaky upstream* in the Rules tab (call count equals 3). See
[Routes and matching](/docs/guides/routes/) for the Rule operators.

```bash
for i in 1 2 3; do curl -s -o /dev/null -w "%{http_code}\n" localhost:3001/status; done
```
