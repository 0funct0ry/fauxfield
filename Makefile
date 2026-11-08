# =============================================================================
# Fauxfield Makefile
# =============================================================================
#
# A thin, documented wrapper around cargo. Every target is a single, readable
# cargo invocation (or a short shell snippet) so the Makefile never hides what
# is actually being run. Nothing here is required: `cargo build` etc. still
# work. The Makefile exists to (a) remember the right flags, (b) mirror CI
# locally, and (c) paper over platform quirks (see "Platform detection").
#
# Run `make` or `make help` for the full, generated target list.
#
# How this file is organised
#   1. Settings        shell, make behaviour, defaults
#   2. Variables       everything you can override on the command line
#   3. Platform        macOS Metal / runtime-shaders detection
#   4. Helpers         small macros reused by the targets
#   5. Targets         grouped by purpose; each has a `## description`
#
# Conventions
#   * `## text` after a target name is its one-line help; `##@ Title` starts a
#     help section. `make help` is generated from these, so keep them current.
#   * Debug is the default profile. Release variants carry a `-release` suffix.
#   * Targets that need an optional tool check for it and print an install hint.
#   * Targets for later milestones fail fast with "not available until Mn", so
#     nothing silently succeeds before the code exists (SPEC §16).
#   * Unix only (macOS, Linux). On Windows use cargo directly or WSL.
# =============================================================================


# -----------------------------------------------------------------------------
# 1. Settings
# -----------------------------------------------------------------------------

# bash for `pipefail` and `$'..'`; -e/-u make recipes fail fast on errors and typos.
SHELL       := /usr/bin/env bash
.SHELLFLAGS := -eu -o pipefail -c

# Running bare `make` shows help instead of silently starting a long build.
.DEFAULT_GOAL := help

# Delete a target's output if its recipe fails; skip make's built-in C rules.
.DELETE_ON_ERROR:
MAKEFLAGS += --no-builtin-rules


# -----------------------------------------------------------------------------
# 2. Variables (override on the command line, e.g. `make test-crate CRATE=fauxfield-gui`)
# -----------------------------------------------------------------------------

CARGO  ?= cargo

# Package that `test-crate`, `clippy-crate`, `doc-crate` and `bench-crate` operate on.
CRATE  ?=

# Free-form arguments. For `run*` targets they go to the program (after `--`);
# for `test*` targets they go to the test binary (a name filter, `--nocapture`, ...).
ARGS   ?=

# Cross-compilation target triple, e.g. aarch64-apple-darwin. Empty means the host.
TARGET ?=

# Extra cargo features appended to every workspace build (comma separated).
FEATURES_EXTRA ?=

# Set to 1 to pass --locked (fail if Cargo.lock would change; what CI should use).
LOCKED ?=

# Parallel jobs for cargo (empty = cargo's default, one per CPU).
JOBS   ?=

# Anything else you want on every cargo command line, e.g. CARGO_FLAGS=--offline.
CARGO_FLAGS ?=

# Force (1) or disable (0) GPUI runtime Metal shader compilation. Empty = auto-detect.
RUNTIME_SHADERS ?=

# Extra arguments for the UI scenario runner (`smoke-ui`, from M1.7).
SMOKE_ARGS ?=

# Where `make dist` writes archives.
DIST_DIR ?= dist

# Binaries that `make dist`, `make size` and `make install` care about.
BINARIES := fauxfield

# Rust package that produces the main binary.
BIN_PACKAGE := fauxfield

# Workspace version, read from [workspace.package] in Cargo.toml (first `version =` line).
VERSION := $(shell sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -n 1)

# Host triple, evaluated lazily because it spawns rustc and most targets do not need it.
HOST_TRIPLE = $(shell rustc -vV | sed -n 's/^host: //p')

# Flags shared by every cargo invocation (assembled from the variables above).
CARGO_OPTS = $(if $(LOCKED),--locked) $(if $(JOBS),-j $(JOBS)) $(if $(TARGET),--target $(TARGET)) $(CARGO_FLAGS)

# Profile selectors. Written out as variables so recipes read naturally.
RELEASE := --release

# Directory holding built binaries for a profile: target/<profile> or target/<triple>/<profile>.
out_dir = target/$(if $(TARGET),$(TARGET)/)$(1)


# -----------------------------------------------------------------------------
# 3. Platform detection
# -----------------------------------------------------------------------------
# GPUI compiles Metal shaders at build time, which needs the Xcode Metal
# toolchain. With only the Command Line Tools installed, fall back to GPUI's
# runtime shader compilation (`runtime-shaders` feature). Override with
# RUNTIME_SHADERS=1 or RUNTIME_SHADERS=0.

ifeq ($(RUNTIME_SHADERS),)
ifeq ($(shell uname -s),Darwin)
ifeq ($(shell xcrun --find metal >/dev/null 2>&1 && echo yes),)
RUNTIME_SHADERS := 1
endif
endif
endif
ifeq ($(RUNTIME_SHADERS),0)
RUNTIME_SHADERS :=
endif

comma := ,
# Make has no predefined space; build one for the substitution below.
empty :=
space := $(empty) $(empty)

# Comma-joined feature list for the workspace build: `fauxfield/runtime-shaders` plus FEATURES_EXTRA.
_ws_features := $(strip $(if $(RUNTIME_SHADERS),fauxfield/runtime-shaders) $(FEATURES_EXTRA))
_ws_features := $(subst $(space),$(comma),$(_ws_features))
# Flag for commands that use `--workspace`. Empty when no features are needed.
FEATURES := $(if $(_ws_features),--features $(_ws_features))

# `-p <crate>` commands cannot use the `fauxfield/...` path unless that crate is the package,
# so the shader feature is added per package instead (only the two GUI-facing crates have it).
# Usage: $(call pkg_features,<package>)
pkg_features = $(if $(RUNTIME_SHADERS),$(if $(filter fauxfield fauxfield-gui,$(1)),--features runtime-shaders))

# Feature list for the in-process MCP dev host (M1.2).
DEV_MCP_FEATURES := dev-mcp$(if $(RUNTIME_SHADERS),$(comma)runtime-shaders)


# -----------------------------------------------------------------------------
# 4. Helpers
# -----------------------------------------------------------------------------

# Fail early with a clear message when a required variable is empty.
# Usage: $(call require_var,CRATE)
require_var = @if [ -z "$($(1))" ]; then echo "error: $(1) is required, e.g. make $@ $(1)=<value>" >&2; exit 2; fi

# Fail early with an install hint when a cargo subcommand / tool is missing.
# Usage: $(call require_tool,<binary>,<install command>)
require_tool = @command -v $(1) >/dev/null 2>&1 || { echo "error: '$(1)' not found. Install it with: $(2)" >&2; exit 127; }

# Fail with a pointer to the milestone that delivers the target.
# Usage: $(call not_yet,M18,site)
not_yet = @echo "error: 'make $(2)' is not available until $(1) (see internal-docs/PROMPTS.md)" >&2; exit 1


# =============================================================================
# 5. Targets
# =============================================================================

##@ Help

.PHONY: help
help: ## Show this help (targets, variables, examples)
	@printf '%s\n' "$$HELP_HEADER"
	@if [ -t 1 ]; then b=$$'\033[1m'; c=$$'\033[36m'; r=$$'\033[0m'; else b=; c=; r=; fi; \
	awk -v b="$$b" -v c="$$c" -v r="$$r" 'BEGIN { FS = ":.*## " } \
		/^##@ / { printf "\n%s%s%s\n", b, substr($$0, 5), r; next } \
		/^[a-zA-Z0-9_.-]+:.*## / { printf "  %s%-26s%s %s\n", c, $$1, r, $$2 }' $(MAKEFILE_LIST)
	@printf '%s\n' "$$HELP_FOOTER"

define HELP_HEADER
Fauxfield build tasks.  Usage: make <target> [VARIABLE=value ...]

Fauxfield is a local-first mock API tool: GPUI desktop app + headless runner + CLI + MCP server.
This Makefile wraps cargo; `make -n <target>` shows exactly what a target runs.

Profiles: targets build the debug profile unless named *-release.
Debug output lives in target/debug, release output in target/release
(or target/<triple>/<profile> when TARGET is set).

macOS note: without Xcode's Metal toolchain the GPUI `runtime-shaders` feature is enabled
automatically (see `make doctor`). Targets marked "(Mn)" arrive in that milestone and fail fast
until then.
endef
export HELP_HEADER

define HELP_FOOTER

Variables (override as `make <target> VAR=value`)
  CRATE=<name>             Package for test-crate, clippy-crate, doc-crate, bench-crate.
  ARGS="..."               Program args for run* targets; test filter / flags for test* targets.
  TARGET=<triple>          Cross-compile, e.g. x86_64-unknown-linux-musl.
  FEATURES_EXTRA=a,b       Extra cargo features added to workspace builds.
  RUNTIME_SHADERS=1|0      Force/disable GPUI runtime Metal shaders (default: auto-detect on macOS).
  LOCKED=1                 Pass --locked to cargo (reproducible; recommended in CI).
  JOBS=<n>                 Limit cargo parallelism.
  CARGO_FLAGS="..."        Any other flags for every cargo call (e.g. --offline).
  SMOKE_ARGS="..."         Extra flags for the UI scenario runner (smoke-ui, from M1.7).
  DIST_DIR=<dir>           Output directory for `make dist` (default: dist).

Examples
  make build                       Debug build of the whole workspace.
  make run                         Start the desktop app (debug).
  make run ARGS="version --json"   Pass arguments to the fauxfield binary.
  make test-crate CRATE=fauxfield-gui ARGS=tabs   Tests of one crate, filtered by name.
  make ci                          Everything CI runs, locally.
  make clean-intermediates         Free disk space but keep the built binaries.
  make clean-all                   Remove every artifact the Makefile can create.

Tip: `make -n <target>` prints the commands without running them.
endef
export HELP_FOOTER


##@ Build

.PHONY: build build-release headless build-headless build-headless-release build-dev-mcp check check-release

build: ## Build the whole workspace (debug)
	$(CARGO) build --workspace $(FEATURES) $(CARGO_OPTS)

build-release: ## Build the whole workspace (release: thin LTO, stripped)
	$(CARGO) build --workspace $(RELEASE) $(FEATURES) $(CARGO_OPTS)

headless: build-headless ## Alias of build-headless (SPEC §16)

build-headless: ## Build the binary without the GUI (no GPUI), debug
	$(CARGO) build -p $(BIN_PACKAGE) --no-default-features --features headless $(CARGO_OPTS)

build-headless-release: ## Build the binary without the GUI (no GPUI), release
	$(CARGO) build -p $(BIN_PACKAGE) --no-default-features --features headless $(RELEASE) $(CARGO_OPTS)

build-dev-mcp: ## Build the app with the in-process MCP host for agent/UI testing (never release)
	$(CARGO) build -p $(BIN_PACKAGE) --features $(DEV_MCP_FEATURES) $(CARGO_OPTS)

check: ## Type-check everything without producing binaries (fast, debug)
	$(CARGO) check --workspace --all-targets $(FEATURES) $(CARGO_OPTS)

check-release: ## Type-check everything with the release profile
	$(CARGO) check --workspace --all-targets $(RELEASE) $(FEATURES) $(CARGO_OPTS)


##@ Run

.PHONY: dev run run-release run-dev-mcp dev-mcp

dev: run ## Alias of run (SPEC §16)

run: ## Start the desktop app (debug). ARGS are passed to the program
	$(CARGO) run -p $(BIN_PACKAGE) $(call pkg_features,fauxfield) $(CARGO_OPTS) $(if $(ARGS),-- $(ARGS))

run-release: ## Start the desktop app (release). ARGS are passed to the program
	$(CARGO) run -p $(BIN_PACKAGE) $(RELEASE) $(call pkg_features,fauxfield) $(CARGO_OPTS) $(if $(ARGS),-- $(ARGS))

run-dev-mcp: dev-mcp ## Alias of dev-mcp

dev-mcp: ## Run the GUI with the MCP host on a socket [MCP_SOCKET=<path>] [ARGS=<workspace>]
	$(CARGO) run -p $(BIN_PACKAGE) --features $(DEV_MCP_FEATURES) $(CARGO_OPTS) -- $(ARGS) $(if $(MCP_SOCKET),--mcp-socket=$(MCP_SOCKET),--mcp-socket)


##@ Test

.PHONY: test test-release test-crate test-gui nextest smoke smoke-ui bench bench-crate

test: ## Run every test in the workspace (debug)
	$(CARGO) test --workspace $(FEATURES) $(CARGO_OPTS) $(if $(ARGS),-- $(ARGS))

test-release: ## Run every test with optimizations (catches release-only bugs, slower to compile)
	$(CARGO) test --workspace $(RELEASE) $(FEATURES) $(CARGO_OPTS) $(if $(ARGS),-- $(ARGS))

test-crate: ## Test one package: CRATE=<name> [ARGS=<filter>]
	$(call require_var,CRATE)
	$(CARGO) test -p $(CRATE) $(call pkg_features,$(CRATE)) $(CARGO_OPTS) $(if $(ARGS),-- $(ARGS))

test-gui: ## Test only the GPUI crate (fauxfield-gui), with the dev-mcp host [ARGS=<filter>]
	$(CARGO) test -p fauxfield-gui --features $(DEV_MCP_FEATURES) $(CARGO_OPTS) $(if $(ARGS),-- $(ARGS))

nextest: ## Run the workspace tests with cargo-nextest (faster, nicer output)
	$(call require_tool,cargo-nextest,cargo install cargo-nextest --locked)
	$(CARGO) nextest run --workspace $(FEATURES) $(CARGO_OPTS) $(ARGS)

smoke: ## Headless MCP smoke test: `fauxfield mcp` over stdio and the unix socket (no GPU, no display)
	$(CARGO) build -p $(BIN_PACKAGE) --no-default-features --features headless $(CARGO_OPTS)
	$(CARGO) test -p $(BIN_PACKAGE) --no-default-features --features headless --test mcp $(CARGO_OPTS)

smoke-ui: ## Run the UI scenarios against the dev-mcp app (needs a display) [SMOKE_ARGS=--no-capture]
	RUNTIME_SHADERS_FEATURE=$(if $(RUNTIME_SHADERS),runtime-shaders) scripts/smoke-ui.sh $(SMOKE_ARGS)

bench: ## (M4) Run all benchmarks (criterion; always release)
	$(call not_yet,M4,bench)

bench-crate: ## (M4) Benchmark one package: CRATE=<name> [ARGS=<filter>]
	$(call not_yet,M4,bench-crate)


##@ Code quality

.PHONY: fmt fmt-check clippy clippy-release clippy-crate lint lint-release fix doc doc-open doc-crate coverage

fmt: ## Format all code in place
	$(CARGO) fmt --all

fmt-check: ## Verify formatting without changing files (what CI runs)
	$(CARGO) fmt --all --check

clippy: ## Lint with clippy, warnings are errors (debug)
	$(CARGO) clippy --workspace --all-targets $(FEATURES) $(CARGO_OPTS) -- -D warnings

clippy-release: ## Lint with clippy using the release profile (catches cfg/optimization-dependent lints)
	$(CARGO) clippy --workspace --all-targets $(RELEASE) $(FEATURES) $(CARGO_OPTS) -- -D warnings

clippy-crate: ## Lint one package: CRATE=<name>
	$(call require_var,CRATE)
	$(CARGO) clippy -p $(CRATE) --all-targets $(call pkg_features,$(CRATE)) $(CARGO_OPTS) -- -D warnings

lint: fmt-check clippy ## Formatting check + clippy -D warnings (debug); the pre-commit gate

lint-release: fmt-check clippy-release ## Formatting check + clippy (release profile)

fix: ## Apply rustfmt and clippy's machine-applicable fixes (modifies files, needs a clean-ish tree)
	$(CARGO) clippy --workspace --all-targets $(FEATURES) $(CARGO_OPTS) --fix --allow-dirty --allow-staged
	$(CARGO) fmt --all

doc: ## Build API docs for workspace crates (no dependencies)
	$(CARGO) doc --workspace --no-deps $(FEATURES) $(CARGO_OPTS)

doc-open: ## Build API docs and open them in the browser
	$(CARGO) doc --workspace --no-deps $(FEATURES) $(CARGO_OPTS) --open

doc-crate: ## Build and open docs for one package: CRATE=<name>
	$(call require_var,CRATE)
	$(CARGO) doc -p $(CRATE) --no-deps $(CARGO_OPTS) --open

coverage: ## Line coverage report via cargo-llvm-cov (HTML in target/llvm-cov/html)
	$(call require_tool,cargo-llvm-cov,cargo install cargo-llvm-cov --locked)
	$(CARGO) llvm-cov --workspace $(FEATURES) $(CARGO_OPTS) --html


##@ Dependencies and policy

.PHONY: deny audit outdated tree verify-headless update schema

deny: ## Check licences, bans and sources with cargo-deny
	$(call require_tool,cargo-deny,cargo install cargo-deny --locked)
	$(CARGO) deny check

audit: ## Check Cargo.lock against the RustSec advisory database
	$(call require_tool,cargo-audit,cargo install cargo-audit --locked)
	$(CARGO) audit

outdated: ## List dependencies with newer versions available (cargo-outdated)
	$(call require_tool,cargo-outdated,cargo install cargo-outdated --locked)
	$(CARGO) outdated --workspace --root-deps-only

tree: ## Show the dependency tree of the main binary (duplicates highlighted: ARGS=-d)
	$(CARGO) tree -p $(BIN_PACKAGE) $(ARGS)

# Architecture rule (SPEC §3.2): only fauxfield-gui and fauxfield may depend on GPUI. The
# headless build of the binary must not pull it in (the Docker image and CI runner depend on it).
verify-headless: ## Fail if the headless build depends on GPUI (architecture rule, also checked in CI)
	@if $(CARGO) tree -p $(BIN_PACKAGE) --no-default-features --features headless -e normal $(CARGO_OPTS) | grep -qE '(^|[^a-z_-])gpui '; then \
		echo "error: the headless build of $(BIN_PACKAGE) depends on gpui" >&2; exit 1; \
	else echo "ok: headless build has no gpui dependency"; fi

update: ## Update Cargo.lock to the newest compatible versions (review the diff before committing)
	$(CARGO) update

schema: ## Regenerate the server-file JSON Schema (site/public/schema/v1/server.json)
	$(CARGO) xtask schema


##@ Aggregates

.PHONY: ci ci-release verify

# Mirrors .github/workflows/ci.yml: fmt, clippy, tests, release build, headless build.
ci: fmt-check clippy test build-release build-headless verify-headless ## Run what CI runs (fmt, clippy, tests, release + headless builds)

ci-release: lint-release test-release build-release ## Like ci, but lints and tests with the release profile

verify: lint test ## Quick pre-commit check: lint + tests (debug)


##@ Install and package

.PHONY: install uninstall dist

install: ## Install the release `fauxfield` binary into ~/.cargo/bin
	$(CARGO) install --path crates/$(BIN_PACKAGE) --locked $(call pkg_features,fauxfield) --force

uninstall: ## Remove the installed `fauxfield` binary
	$(CARGO) uninstall $(BIN_PACKAGE)

# Produces dist/fauxfield-<version>-<triple>.tar.gz plus a .sha256 next to it. The archive holds
# the release binaries and any of LICENSE, README.md, CHANGELOG.md that exist. This is a local
# convenience archive; signed installers and cargo-dist arrive in M19.
dist: build-release ## Package release binaries into dist/fauxfield-<version>-<triple>.tar.gz (+ sha256)
	@set -e; \
	triple="$(if $(TARGET),$(TARGET),$(HOST_TRIPLE))"; \
	name="fauxfield-$(VERSION)-$$triple"; \
	stage="$(DIST_DIR)/$$name"; \
	rm -rf "$$stage"; mkdir -p "$$stage"; \
	for bin in $(BINARIES); do \
		cp "$(call out_dir,release)/$$bin" "$$stage/"; \
	done; \
	for doc in LICENSE README.md CHANGELOG.md; do \
		[ -f "$$doc" ] && cp "$$doc" "$$stage/" || true; \
	done; \
	tar -czf "$(DIST_DIR)/$$name.tar.gz" -C "$(DIST_DIR)" "$$name"; \
	rm -rf "$$stage"; \
	( cd "$(DIST_DIR)" && shasum -a 256 "$$name.tar.gz" > "$$name.tar.gz.sha256" ); \
	ls -lh "$(DIST_DIR)/$$name.tar.gz"


##@ Website

.PHONY: site site-dev

site: ## (M18) Build the marketing page and docs site into site/dist
	$(call not_yet,M18,site)

site-dev: ## (M18) Serve the site with live reload
	$(call not_yet,M18,site-dev)


##@ Clean

.PHONY: clean clean-debug clean-release clean-incremental clean-intermediates clean-dist clean-docs clean-all

# Remove the whole target directory, binaries included.
clean: ## Remove the entire build (target/), binaries included
	$(CARGO) clean

clean-debug: ## Remove only debug build output (target/debug)
	rm -rf target/debug target/*/debug

clean-release: ## Remove only release build output (target/release)
	rm -rf target/release target/*/release

# Incremental caches are the cheapest thing to throw away: they rebuild quickly and are
# often tens of GB for a GPUI project.
clean-incremental: ## Remove incremental-compilation caches only (cheap, frees a lot of space)
	rm -rf target/debug/incremental target/release/incremental target/*/debug/incremental target/*/release/incremental

# Keeps the final binaries in target/<profile>/ and target/<triple>/<profile>/ and deletes the
# rest. On macOS and Linux the binaries are hardlinks into deps/, so removing deps/ leaves
# them intact. The next build recompiles all dependencies from scratch.
clean-intermediates: ## Remove everything except the final binaries (deps, build scripts, caches)
	@for dir in target/debug target/release target/*/debug target/*/release; do \
		[ -d "$$dir" ] || continue; \
		rm -rf "$$dir/deps" "$$dir/incremental" "$$dir/build" "$$dir/.fingerprint" "$$dir/examples"; \
	done
	@rm -rf target/criterion target/tmp
	@du -sh target 2>/dev/null || true

clean-dist: ## Remove packaged archives (dist/)
	rm -rf $(DIST_DIR)

clean-docs: ## Remove generated API docs (target/doc)
	rm -rf target/doc $(wildcard target/*/doc)

# Everything the Makefile can create: build output, archives, API docs, and the site build.
# Sources, fixtures and internal-docs/ are never touched.
clean-all: clean clean-dist clean-docs ## Remove every artifact the Makefile can create (target/, dist/, docs, site build)
	rm -rf site/dist site/.astro site/node_modules/.astro
	@echo "clean-all: done (sources and internal-docs/ untouched)"


##@ Information

.PHONY: size doctor version

size: ## Show built binary sizes and how much disk each part of target/ uses
	@echo "Binaries:"; \
	for dir in target/debug target/release target/*/debug target/*/release; do \
		for bin in $(BINARIES); do \
			[ -f "$$dir/$$bin" ] && ls -lh "$$dir/$$bin" | awk '{printf "  %-8s %s\n", $$5, $$9}'; \
		done; \
	done; true
	@echo "Disk usage:"; du -sh target 2>/dev/null | sed 's/^/  /'; \
	du -sh target/*/ 2>/dev/null | sed 's/^/  /'; true

doctor: ## Print toolchain, platform detection and optional-tool availability
	@echo "rustc:            $$(rustc --version)"
	@echo "cargo:            $$($(CARGO) --version)"
	@echo "host triple:      $(HOST_TRIPLE)"
	@echo "workspace:        $(VERSION)"
	@echo "runtime shaders:  $(if $(RUNTIME_SHADERS),on (Metal toolchain missing or forced),off)"
	@echo "feature flags:    $(if $(FEATURES),$(FEATURES),(none))"
	@echo "optional tools:"
	@for t in cargo-nextest cargo-deny cargo-audit cargo-llvm-cov cargo-outdated cargo-watch; do \
		if command -v $$t >/dev/null 2>&1; then printf '  %-18s yes\n' $$t; else printf '  %-18s no\n' $$t; fi; \
	done

version: ## Print the workspace version
	@echo $(VERSION)
