---
title: Using Fauxfield in CI
description: Run a Workspace headless in pipelines, with deterministic data and machine-readable output.
---

The headless runner serves the same folder the app edits, with no GPU and no display.

```bash
fauxfield validate --json            # fail the job on a broken Workspace (exit 3)
fauxfield run --seed 7 --env ci &    # deterministic fake data, the ci Environment
# ... run your tests against localhost ...
fauxfield stop
```

## Tips

- **Pin the data.** `--seed N` makes `fake` and generated data repeat exactly.
- **Free the ports.** `--port-offset 1000` shifts every Server; `--server users-api` runs only some.
- **Read the traffic.** `fauxfield logs --json --status 5xx` prints one object per request;
  `logs --har out.har` exports HAR.
- **Exit codes** are stable: 3 validation failed, 4 host not running, 5 port in use. See the
  [CLI reference](/docs/reference/cli/).
- **Docker**: `docker run -p 3001:3001 -v "$PWD:/workspace" ghcr.io/0funct0ry/fauxfield`.

## GitHub Actions

```yaml
- run: curl -fsSL https://0funct0ry.github.io/fauxfield/install.sh | sh
- run: fauxfield validate
- run: fauxfield run --seed 7 --env ci &
- run: npm test
```
