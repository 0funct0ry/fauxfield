# Fauxfield

[![CI](https://github.com/0funct0ry/fauxfield/actions/workflows/ci.yml/badge.svg)](https://github.com/0funct0ry/fauxfield/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Rust 1.89+](https://img.shields.io/badge/rust-1.89%2B-orange.svg)

Fauxfield is a fast desktop app and headless server for designing and running mock APIs.
Your mocks are plain TOML files that live in your repository.
Frontend developers, QA engineers, CI jobs and coding agents all use the same files.

> **Status: pre-release.** No version is published yet. Build Fauxfield from source for now.

## Key features

- **One folder, many mocks.** A Workspace is a folder of TOML files. Review it in pull requests.
- **Live editing.** Change a file or use the app. Both stay in sync and reload at once.
- **Smart Responses.** Use Rules to pick a Response. Use templates and fake data to fill it.
- **Datasets and CRUD.** Serve a table of rows with list, get, create, update and delete Routes.
- **Network and error simulation.** Add delay, faults and rate limits.
- **Proxy and partial mocking.** Mock some Routes and send the rest to a real upstream.
- **WebSockets and webhooks.** Mock WebSocket Routes. Send Callbacks (webhooks) after a Response.
- **HTTPS.** Use a local development CA or your own certificate.
- **API documentation.** Publish Swagger UI and an OpenAPI document for any Server.
- **Environments and Variables.** Switch between sets of values, with secret values hidden.
- **Agent access.** A built-in MCP server (Model Context Protocol) lets a coding agent edit mocks.
- **CLI.** Every command supports `--json` for scripts.
- **Themes.** Choose from bundled themes or add your own.

### Roadmap

- Record traffic from an upstream into a Server (`fauxfield record`).
- Import and export OpenAPI documents (`fauxfield import`, `fauxfield export`).
- Templates gallery (`fauxfield template`).

These commands exist but print "not available" for now.

## Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust | 1.89 or newer | `rustup` reads `rust-toolchain.toml` and installs the stable toolchain |
| make | any | Wraps the build, test and lint commands |
| Xcode command line tools | recent | macOS. Without the Metal toolchain, the Makefile enables `runtime-shaders` |

On Linux, the desktop app needs these packages (the headless build does not):

```bash
sudo apt-get install -y libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
  libxcb1-dev libvulkan-dev libasound2-dev libfontconfig-dev libssl-dev
```

The app draws with Metal on macOS, Vulkan on Linux and DirectX 11 on Windows.
Run `fauxfield doctor` to check your platform.

## Installation

Build from source:

```bash
git clone https://github.com/0funct0ry/fauxfield.git
cd fauxfield
make build   # build the whole workspace (debug)
make run     # build and open the desktop app
```

Build the headless binary. It has no GPU dependencies:

```bash
make build-headless-release
```

Run `make help` to list every target.

## Usage

### Create a Workspace

```bash
fauxfield init my-mocks     # create a Workspace with one example Server
cd my-mocks
fauxfield validate          # check the files
fauxfield run               # serve the Servers without the app
```

Open the app on a Workspace with `fauxfield .`.

### Workspace layout

```text
my-mocks/
├── fauxfield.toml        # Workspace settings
├── environments/
│   └── local.toml        # Environments and Variables
└── servers/
    └── users-service.toml   # one file per Server
```

### A Server file

```toml
schema = 1
id = "srv_5fawrsg5"
name = "orders-service"
enabled = true
host = "127.0.0.1"
port = 3003

[[routes]]
id = "rte_m9ba5gzd"
method = "GET"
path = "/orders/:id"

[[routes.responses]]
id = "rsp_x7tygaq9"
name = "OK"
default = true
status = 200
content_type = "application/json"
body = """
{
  "id": "{{request.params.id}}",
  "name": "{{fake.name}}"
}"""
```

Start it, then call it:

```bash
fauxfield run
curl http://127.0.0.1:3003/orders/42   # in a second terminal
```

### Useful commands

```bash
fauxfield status            # show the running host (exit code 4 if none)
fauxfield logs              # print request logs from the running host
fauxfield stop              # stop the Servers
fauxfield doctor            # check ports, certificates, GPU and display
fauxfield version --check   # look up the latest release (never installs)
fauxfield --help            # every command
```

### Let a coding agent mock your APIs

```bash
fauxfield agents init --claude   # show a diff, then write .mcp.json and an AGENTS.md section
fauxfield .                      # open the app to watch the agent work
```

Use `fauxfield mcp . --read-only` for an agent that can only read.
See the guide in `site/src/content/docs/docs/guides/agents.md`.

### Keys

| Action | macOS | Linux and Windows |
|---|---|---|
| Quick Search | `⌘K` | `Ctrl+K` |
| Command Palette | `⌘⇧P` | `Ctrl+Shift+P` |
| Theme selector | `⌘K ⌘T` | `Ctrl+K Ctrl+T` |
| Settings | `⌘,` | `Ctrl+,` |

Change any key in the Settings tab or in `keymap.toml`.

## Testing and quality

```bash
make test                          # run all tests
make lint                          # rustfmt and clippy with warnings as errors
make ci                            # run what CI runs
cargo test -p fauxfield-engine     # test one crate
cargo test -p fauxfield-engine crud_list_filters   # run tests that match a name
make bench                         # run the performance benchmarks
```

## Deployment

Release automation is configured but has not published a release yet.
It uses [cargo-dist](https://github.com/axodotdev/cargo-dist) and GitHub Actions.
A release will contain:

- Desktop archives and installers for macOS, Linux and Windows.
- A headless binary and a Docker image (`ghcr.io/0funct0ry/fauxfield`) for CI.
- Checksums, build attestations and an SBOM.

Run the headless image like this after the first release:

```bash
docker run -p 3001:3001 -v "$PWD:/workspace" ghcr.io/0funct0ry/fauxfield
```

Fauxfield checks for updates only if you turn on `check_for_updates` in the settings.
It never installs updates. See `RELEASING.md` for the release steps.

## Contributing

1. Create a branch.
2. Make your change. Use the project words: Workspace, Server, Folder, Route, Response, Rule,
   Callback, Dataset, Environment, Variable, Recording, Template. See `AGENTS.md`.
3. Run `make lint test`.
4. Add a line to `CHANGELOG.md` under Unreleased.
5. Open a pull request.

Only `fauxfield-gui` and the `fauxfield` binary may depend on GPUI.
Documentation is in `site/src/content/docs/`. Reference pages are generated:
run `make docs-gen`, and `make docs-check` verifies them.

## License

Fauxfield uses the MIT license. See [LICENSE](LICENSE).
