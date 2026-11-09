# ADR-001: GPUI version

Status: accepted (M1)

## Decision
Pin the crates.io release `gpui = "=0.2.2"` with an exact requirement in the workspace
`Cargo.toml`. Only `fauxfield-gui` and `fauxfield` may depend on it.

## Why
- A published crate gives reproducible, cacheable builds on all three CI runners; a Zed git rev
  pulls a far larger tree.
- Other GPUI projects ship on the same pin, so patterns carry over.
- GPUI's API churns; upgrades happen deliberately in their own change.

## Consequences
- GPUI compiles Metal shaders at build time, which needs Xcode's Metal toolchain.
  `fauxfield-gui` exposes `runtime-shaders` (`gpui/runtime_shaders`); the Makefile enables it
  automatically when `xcrun --find metal` fails. Direct cargo users pass
  `--features fauxfield/runtime-shaders`.
- Linux CI installs the Wayland/X11/xkbcommon/Vulkan development packages.
- `gpui` is a dev-dependency with `test-support` for `TestAppContext` tests.
