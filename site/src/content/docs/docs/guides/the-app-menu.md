---
title: The application menu
description: What each menu holds, how it differs per platform, and the About box.
---

Every action that works today is in the menu, with its live keybinding. Items that cannot act are
greyed, never hidden, and a greyed item does nothing from its key either.

| Menu | Holds |
|---|---|
| **Fauxfield** (macOS) | About, Services, Hide, Hide Others, Show All, Quit |
| **File** | New Server, Route, Folder; Create Workspace, Open Folder, Open Recent; Close Tab, Pin Tab, Close and Rename Workspace, Reveal in Finder/File Manager/Explorer; Quit (Windows, Linux) |
| **Edit** | Undo, Redo, Cut, Copy, Paste, Select All, Duplicate, Rename, Delete |
| **View** | Quick Search, Command Palette, the three docks, Traffic/Problems/Logs, Theme, Full Screen |
| **Server** | Run/Stop All, Start/Stop, Restart, Change Port, Switch Environment, Traffic |
| **Route** | Copy as cURL, Edit Method and Path, New and default Response, Response fields |
| **Window** | Minimize, Zoom or Maximize, Go to Tab |
| **Help** | Documentation, Release Notes, Report an Issue, About (Windows, Linux) |

Labels follow state ("Hide Left Dock" / "Show Left Dock", "Run All Servers" / "Stop All Servers").
**Open Recent** lists up to ten workspaces; one whose folder is gone is greyed and marked
"(not found)". While a dialog, prompt, Quick Search or the Command Palette is open, most items are
disabled; Quit, Hide, Minimize, Full Screen, Help and About stay available.

## Platforms

macOS uses the system menu bar. Windows and Linux draw the same menus in the title bar: click a
title or press `Alt` plus its underlined letter, move with the arrow keys, type a letter to jump,
`Enter` runs, `Esc` closes one level. Below 1000 px wide the titles collapse into one menu button.

## About and bug reports

**About Fauxfield** shows the version, commit, target and gpui version. **Copy Version Info**
puts a block like this on the clipboard; paste it into a bug report:

```
Fauxfield 0.1.0 (a7f3116, 2026-11-08)
Build: debug, aarch64-apple-darwin
OS: macos aarch64
GPUI: 0.2.2
```

`fauxfield version` prints the same facts (`--json` for scripts).
