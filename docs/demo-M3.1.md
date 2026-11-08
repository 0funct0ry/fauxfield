# Demo: M3.1 Workspace lifecycle and M3 completion

Run from the repository root. `make run ARGS=fixtures/example-workspace` (use a copy; the app edits files).

## In the app

1. Close the workspace: hamburger next to the workspace name in the sidebar, **Close workspace**.
   Welcome appears; the workspace is first in Recent. Click it to reopen.
2. Hamburger, **Rename workspace...**, clear the name, press Enter (inline error), type `My Mocks`, Enter.
   Window title and sidebar show it; `Cmd+Z` undoes it.
3. Hamburger, **New server...**, name it `Scratch`. Click it, then the Host value: `0.0.0.0` shows
   "Reachable from your network.". Change Port and Prefix (`/v2`).
4. Toggle **Enabled** off: the row dims with a `disabled` tag. **Run all** skips it. Click **Start** on
   its tab: toast "Scratch is disabled. Enable it first." with **Enable and start**.
5. Start a server, hamburger, **Close workspace**: "Stop 1 server and close...?" Cancel, then Close.
6. A server row hamburger, **Delete**: the confirmation names the file and routes.

## Terminal

```bash
fauxfield workspace info --json
fauxfield workspace rename "My Mocks"
fauxfield run --server Scratch        # exit 1 while it is disabled
fauxfield logs --har out.har          # exit 2: --har arrives in M12
fauxfield mcp --transport pipe        # Windows only; on Unix: exit 2 naming unix
```

## UI scenarios

```bash
make smoke-ui SMOKE_ARGS=--no-capture   # includes m031-workspace
```
