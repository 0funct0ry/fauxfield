# Demo: M3.3 Tabbed sidebar

1. `make run` and open `fixtures/example-workspace`. The left dock shows the workspace header,
   three icon tabs and the SERVERS list ("SERVERS 2", with a "+" button).
2. Hover a tab: tooltip "Environments (⌘⌥2)" (Ctrl+Alt+2 elsewhere).
3. Click the layers tab, or press ⌘⌥2: only environments are listed. Double-click `staging` to pin it.
4. Press ⌘⌥3: Datasets (table icons). View > Sidebar shows the active tab checked.
5. Hide the dock with ⌘B, press ⌘⌥1: the dock reopens on Servers.
6. On Datasets, press ⌘K, type `@Payments`, Enter: the sidebar jumps to Servers.
7. Quit and relaunch: the last tab is restored.
8. Open `fixtures/empty-workspace` on each tab to see the empty states.
9. Open `fixtures/many-servers-workspace`: the strip and title stay put while 40 servers scroll.

Deferred: New environment / New dataset buttons (M7, M8), dataset row counts (M8), native check marks
in macOS menus (gpui 0.2.2 has none; a "✓" prefix is used).
