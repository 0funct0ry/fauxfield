---
title: Themes and fonts
description: Bundled themes, your own Zed theme files, following the OS appearance, and the IBM Plex fonts.
---

Fauxfield reads Zed theme JSON. Pick a theme with **Cmd+K Cmd+T** (or click the theme name in the status bar): the list is grouped Dark then Light, each row shows a swatch strip, moving the highlight previews it, **Esc** reverts, **Enter** applies, and typing filters. The footer switch **Follow system appearance** makes the app switch between your light and dark theme with the OS.

## settings.toml

```toml
theme = { mode = "system", light = "One Light", dark = "One Dark" }
```

`mode` is `system`, `light` or `dark`. The GUI, `fauxfield theme set <name>` and the MCP server all read this file, and the app reloads it when it changes.

## Your own themes

Drop Zed theme JSON files into `~/.config/fauxfield/themes/`. They reload within a quarter of a second, win name clashes with bundled themes and are tagged "user". A broken file is skipped and listed in Problems and in `fauxfield doctor` as `themes/foo.json: line 12: message`. Missing keys fall back to One Dark or One Light; `doctor` counts them.

## Fonts

The UI uses IBM Plex Sans and code uses IBM Plex Mono (SIL OFL). They ship with the desktop app only; the headless build has none.
