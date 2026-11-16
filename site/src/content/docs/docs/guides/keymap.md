---
title: Keymap
description: Rebind actions, resolve conflicts, and the vim list-navigation preset.
---

The **Keymap** category of the [Settings tab](/docs/guides/settings/) lists every action with its binding and where that binding comes from: `default`, `vim preset` or `user`. Search filters by action name, id or keys.

## Rebinding

**Record** opens a modal: press the combination, or two within 400 ms for a chord such as `Cmd+K Cmd+T`. If another action already uses it, the modal names the conflict before you can save and asks you to choose:

- **Replace**: the other action loses that binding.
- **Allow both, by context**: the new binding gets a context (for example `Tree`), so each works where it applies.

**Clear** leaves the action unbound, **Reset** drops your override so the default returns, and **Copy** puts the `[[binding]]` snippet on the clipboard.

## keymap.toml

Only your overrides are written, beside `settings.toml`:

```toml
[[binding]]
action = "run_all"
keys = "secondary-shift-r"
context = "Tree"          # optional
platform = "macos"        # optional: macos, windows, linux (or a list)
```

Layers are defaults, then the vim preset (when enabled), then your file. The first entry for an action replaces its lower-layer bindings; an empty `keys` unbinds it. Unknown actions and malformed keys are listed in Problems (`keymap.invalid`) and skipped. The file is watched, so edits apply at once, and the palette and menus show the current bindings.

## Vim list navigation

Turn on **Vim list navigation** (`vim_list_navigation = true`) to add `j`/`k`, `g g`/`G`, `h`/`l` (collapse and expand), `/` (filter), `Enter` (open) and `Esc` (leave) while the sidebar tree has focus. Click a row to focus the tree. Text inputs and editors are never affected. Every default is in the [keybindings reference](/docs/reference/keybindings/).
