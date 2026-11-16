---
title: Install
description: Install Fauxfield on macOS, Linux, Windows or Docker.
sidebar:
  order: 1
---

Fauxfield is one binary: the desktop app, the `fauxfield` command, the headless runner and the MCP
server. Pick the package that fits; all of them give you the same `fauxfield` command.

## macOS

```bash
brew install --cask fauxfield                    # app + CLI
brew install 0funct0ry/tap/fauxfield-cli         # headless CLI only
```

## Linux

```bash
curl -fsSL https://0funct0ry.github.io/fauxfield/install.sh | sh
```

`.deb`, `.rpm`, AppImage and a static headless build are on the
[releases page](https://github.com/0funct0ry/fauxfield/releases). The app needs Vulkan; X11 and
Wayland are supported. The headless build needs neither.

## Windows

```powershell
winget install 0funct0ry.Fauxfield
irm https://0funct0ry.github.io/fauxfield/install.ps1 | iex
```

## Docker

```bash
docker run -p 3001:3001 -v "$PWD:/workspace" ghcr.io/0funct0ry/fauxfield
```

The image runs `fauxfield run /workspace` with the headless build.

## Updates

Fauxfield never updates itself. If you opt in (**Settings → Advanced → Check for updates**, or
`check_for_updates = true` in `settings.toml`; off by default) it asks GitHub for the latest release
once a day and shows an "Update available" notice in the status bar that opens the release page.
**Help → Check for Updates…** and `fauxfield version --check` do the same on demand. Nothing is
downloaded or installed, and no other data is sent.

## Verify a download

Every release file has a `.sha256` file and a GitHub artifact attestation
(`gh attestation verify <file> --repo 0funct0ry/fauxfield`); an SBOM (`*.cdx.json`) is attached to
each release.

## Check it works

```bash
fauxfield version
fauxfield doctor        # GPU backend, ports, certificate trust
```

Next: [make your first mock](/docs/getting-started/first-mock/).
