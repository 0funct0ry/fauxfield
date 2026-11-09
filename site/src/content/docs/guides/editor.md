---
title: The code editor
description: Editing bodies in Fauxfield's built-in code editor.
---

Every multi-line text in Fauxfield (today the Response body, the body file preview and the request
and response bodies in Traffic) uses one editor.

## Editing

- Click a Response body to edit it. **Cmd/Ctrl+Enter** or clicking outside saves; **Esc** discards.
- Brackets and quotes close themselves, typing a closer steps over it, and typing a bracket over a
  selection wraps it. **Enter** keeps the indent and indents after `{ [ (` (and after `:` in YAML).
- **Tab** indents (a multi-line selection indents as a block); **Shift+Tab** outdents.
- **Cmd/Ctrl+/** toggles comments (YAML, TOML, XML, HTML). **Alt+Up/Down** moves lines,
  **Alt+Shift+Down** duplicates, **Cmd/Ctrl+Shift+K** deletes a line.
- **Ctrl+G** goes to a line, **Cmd/Ctrl+Shift+\\** jumps to the matching bracket, **Alt+Z** toggles
  soft wrap, **Cmd/Ctrl+Shift+I** formats JSON, XML and HTML (one undo step). Errors show the line
  and column.
- **Cmd/Ctrl+Z** undoes typing in groups while the editor has focus.

## Settings

`settings.toml` in the config folder:

```toml
[editor]
tab_size = 2
soft_wrap = false
show_whitespace = "none"   # none | selection | all
indent_guides = true
line_numbers = true
auto_close = true
cursor_blink = true
format_json_on_save = false

[editor.languages.markdown]
soft_wrap = true
```

Bad values fall back to the default and show a notice. Settings are read at startup.
