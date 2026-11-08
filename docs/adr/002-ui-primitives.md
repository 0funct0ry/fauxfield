# ADR-002: UI primitives (gpui-component decision)

Status: accepted (M1). Answers SPEC §20 question 1.

## Decision
Do not use `gpui-component`. Fauxfield owns a small widget set in `fauxfield-gui::ui`
(icon, method badge, key cap, status dot, toolbar button, hairlines) and builds lists with
GPUI's `uniform_list`. Single-line text input in the picker is handled by the picker itself.

## Why
- The `gpui-component` releases on crates.io target a different GPUI package (`gpui-pre`), so
  their types cannot be mixed with the pinned `gpui 0.2.2` (ADR-001).
- M1 needs virtual lists and a table, which `uniform_list` covers. Text input arrives with the
  response editor in M6.
- Keeping primitives behind `ui` means a later swap touches one module.

## Consequences
- Fonts: SPEC §9.1 names IBM Plex Mono for code. It is not bundled yet, so JetBrains Mono (OFL)
  stands in via `assets::BUFFER_FONT`. Swap the constant and font list when Plex Mono is added.
