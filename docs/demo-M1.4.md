# M1.4 demo script: semantic UI tree

```bash
make dev-mcp MCP_SOCKET=/tmp/fauxfield.sock
```

From Claude Code (see `docs/demo-M1.2.md` for `.mcp.json`):

1. `ui.find {"label": "Traffic", "role": "table"}` returns the table with `total: 10000`;
   `ui.find {"text": "traffic-row-"}` returns only the visible rows.
2. `ui.click_element {"id": "row-rt_health"}` opens the route as a preview tab (`ui.state` tabs[0]).
3. `ui.click_element {"id": "title-search"}`, then `ui.find {"role": "modal"}`: `quick-search`, z 1,
   and the input is `focused`.
4. `ui.tree {"root": "status-bar", "visible_only": true}` lists the status-bar controls with bounds.
5. `ui.click_element {"id": "tab-close-rt_health"}` closes the tab.
