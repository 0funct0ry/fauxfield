---
title: Settings
description: The Settings tab, settings.toml, and how changes apply.
---

Open Settings with **Cmd+,** (**Ctrl+,** on Windows and Linux), the app menu, the status-bar gear or the Command Palette (**Open Settings**). There is one Settings tab per window. It edits `settings.toml`; the file stays the source of truth.

## Using the tab

Pick a category on the left: Appearance, Editor, Servers & Engine, MCP & Agents, Traffic, Keymap, Accessibility or Advanced. Each row has a label, a description, its control, a dot when the value differs from the default, and a reset button that removes the key.

**Search** (the box at the top, or **Settings: Search** in the palette) filters across all categories by label, description and TOML key, highlights the match and shows an empty state when nothing matches.

Number and text rows commit on **Enter** or when they lose focus. An out-of-range value shows an inline error and is not written. **Reset section** removes every key of the category; **Open settings.toml** opens the file.

## The file

Writes go through `toml_edit`, so your comments, key order and unknown keys survive. Edit the file by hand and the tab follows within a moment. A value that fails validation becomes a Problem (`settings.invalid`) and the default stays in effect.

Theme, fonts, scale and motion apply instantly. `[mcp]` and `[traffic]` rows are read when a session starts and say so. `ui_scale`, `reduce_motion` and `high_contrast` are stored here; their effect arrives with the accessibility work. Telemetry is shown disabled: there is none.

Every key is listed in the [settings reference](/reference/settings/), and a [JSON Schema](/schema/v1/settings.json) is published for editor completion.
