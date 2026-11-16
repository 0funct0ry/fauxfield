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

## Check it works

```bash
fauxfield version
fauxfield doctor        # GPU backend, ports, certificate trust
```

Next: [make your first mock](/docs/getting-started/first-mock/).
