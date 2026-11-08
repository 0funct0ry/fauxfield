# M1 demo script: UI shell

Paste into the PR description. Keys are macOS; on Linux and Windows use `Ctrl` for `Cmd`.

## Setup

```bash
make doctor     # shows whether runtime-shaders is on (macOS without Xcode's Metal toolchain)
make run        # opens the desktop app on mock data
```

## In the app

1. **Tree and tabs.** Click `GET /users/:id` in the left dock: a preview tab opens (italic title).
   Click `POST /users`: it replaces the preview. Double-click `GET /users/:id`: it is pinned.
   Click `Payments` (server tab) and `local` (environment tab); press `Cmd+1`..`Cmd+3` to jump.
   Drag a tab onto another to reorder; `Cmd+W` closes the active tab.
2. **Views.** Route tab: sub-tabs Responses / Rules / Headers / Callbacks / Docs / Settings.
   Server tab: section nav and Start/Stop. Environment tab: variable table; secrets are masked
   until "Show secrets".
3. **Docks.** `Cmd+B` left, ``Cmd+` `` bottom, `Cmd+Alt+B` right. Drag the 1 px dividers to
   resize. Quit and relaunch: sizes, open tabs and visibility are restored.
4. **Traffic.** Open the bottom dock and scroll the 10,000-row table; click a row to see it in
   the Inspector (right dock). Switch to Problems and Logs; click the problem to open its route.
5. **Quick Search.** `Cmd+K`, type `charges`. Try the prefixes `>` commands, `@` servers,
   `/` routes by path, `#` folders, `$` variables. `Enter` opens a preview tab,
   `Cmd+Enter` a pinned tab, `Alt+Enter` reveals in the tree.
6. **Command Palette.** `Cmd+Shift+P`: every action with its binding. Run "Toggle Left Dock";
   reopen the palette and see it float to the top.
7. **Chord and themes.** Press `Cmd+K` and wait 400 ms: Quick Search opens. Press `Cmd+K` then
   `Cmd+T`: the theme selector opens. Move with the arrow keys to preview One Light live;
   `Esc` reverts, `Enter` keeps it (and persists it).
8. **Environment and servers.** `Cmd+E` switches environment (type `stag`, `Enter`);
   `Cmd+Shift+R` runs/stops all servers (mock); the title-bar button does the same.
9. **Context menu.** Hover a tree row and click `⋯`, or right-click it. "Open", "Start/Stop"
   and "Set active" work; Rename / Duplicate / Delete / New route name the milestone that
   delivers them (M2).
10. **Title bar.** Double-click empty title-bar space to zoom the window.
11. **Stubs.** `Cmd+N` shows "Creating routes arrives in M2."

## Commands

```bash
make help                          # detailed target list
make lint && make test             # fmt + clippy -D warnings, then all tests
make ci                            # what CI runs
make build-headless && make verify-headless   # no GPUI in the headless build
./target/debug/fauxfield version --json
./target/debug/fauxfield completions zsh | head
make clean-all -n                  # preview what clean-all removes
```
