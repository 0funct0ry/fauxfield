# M1.5 demo: screenshots at a given state

Needs Screen Recording permission for the terminal (macOS) or `grim`/ImageMagick `import` (Linux).

```bash
make dev-mcp MCP_SOCKET=/tmp/ff.sock
```

Then, from Claude Code or any MCP client on that socket:

1. `ui.screenshot {state: "palette-open"}` returns the PNG, `ui.state`, size, theme, scale, tier.
2. `ui.screenshot {element: "traffic", state: "traffic-10k"}` crops to the Traffic table.
3. `ui.screenshot_all {out_dir: "target/shots"}` captures all nine states in One Dark and One Light
   (18 files) and restores theme and size.
4. `ui.set_theme {name: "one-light"}`, `ui.resize {size: "compact"}` work on their own.
5. Without permission the call fails with the exact setting to change, never a blank image.
6. `ui.screenshot {capture: false}` returns only `ui.state`.
