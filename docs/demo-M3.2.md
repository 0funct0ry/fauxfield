# Demo: M3.2 Application menu and About box

Run from the repository root. `make run ARGS=fixtures/example-workspace` (use a copy; the app edits files).

## In the app

1. macOS: the menu bar reads Fauxfield, File, Edit, View, Server, Route, Window, Help. Windows and
   Linux: File to Help sit at the left of the title bar.
2. Close the workspace (File > Close Workspace, `Cmd/Ctrl+Shift+W`): Welcome appears and workspace
   items are greyed. File > Open Recent > the workspace reopens it; a missing folder is greyed.
3. View > Hide Left Dock flips to Show Left Dock (`Cmd/Ctrl+B`). Server > Run All Servers flips to
   Stop All Servers. Edit > Undo is greyed until something is edited.
4. Open the Command Palette (`Cmd/Ctrl+Shift+P`): menu items are greyed while it is open.
5. About Fauxfield (app menu on macOS, Help elsewhere): version, commit, target, gpui. **Copy Version
   Info** toasts "Version info copied". `Esc` or `Enter` closes it.
6. Windows/Linux (or macOS via the dev override): `Alt+F` opens File, arrows move, `Right` opens
   Open Recent, `Esc` steps back. Narrow the window below 1000 px for the hamburger.

## Terminal

```bash
fauxfield version
fauxfield version --json
make smoke-ui          # runs m032-menu with the other scenarios
```

## Native macOS menu checklist (by eye; screenshots cannot capture it)

Open every menu and submenu; confirm labels, bindings and greyed items on Welcome and with a
workspace open; Open Recent with a missing folder; About; closing the last window quits the app.
Record the result here when done: NOT YET CHECKED.

## Deferred

Settings… (M17), Check for Updates… (M19), published docs URLs (M18/M19), selectable About text.
