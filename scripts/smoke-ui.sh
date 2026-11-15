#!/usr/bin/env bash
# Runs the shipped UI scenarios against a dev-mcp app (SPEC §11.4, §18). Used by `make smoke-ui`
# and CI. Needs a display (xvfb-run on Linux CI). Usage: scripts/smoke-ui.sh [--no-capture] [-- extra cargo args]
set -euo pipefail
cd "$(dirname "$0")/.."

capture=()
features="dev-mcp${RUNTIME_SHADERS_FEATURE:+,$RUNTIME_SHADERS_FEATURE}"
if [[ "${1:-}" == "--no-capture" ]]; then capture=(--no-capture); shift; fi

sock="$(mktemp -u "${TMPDIR:-/tmp}/ffsmoke.XXXXXX")"   # short path: unix sockets cap near 104 bytes
out="${SMOKE_OUT:-target/smoke}"
mkdir -p "$out"
cargo build -p fauxfield --features "$features"
bin="${CARGO_TARGET_DIR:-target}/debug/fauxfield"

# A throwaway copy of the example workspace: scenarios edit it (autosave writes files).
ws="$(mktemp -d "${TMPDIR:-/tmp}/ffws.XXXXXX")"
cp -R fixtures/example-workspace "$ws/example-workspace"
"$bin" "$ws/example-workspace" --mcp-socket "$sock" >"$out/app.log" 2>&1 &
app=$!
trap 'kill "$app" 2>/dev/null || true; rm -rf "$sock" "$ws"' EXIT
for _ in $(seq 1 100); do [[ -S "$sock" ]] && break; sleep 0.2; done
[[ -S "$sock" ]] || { echo "app did not open $sock"; cat "$out/app.log"; exit 2; }
sleep 1

status=0
for f in fixtures/scenarios/*.yaml; do
  name="$(basename "$f" .yaml)"
  [[ "$name" == "broken-assertion" ]] && continue
  # Scenarios that need another fixture say so in a `# workspace: <fixture>` header line.
  grep -q '^# workspace: ' "$f" && continue
  echo "== $name"
  "$bin" mcp scenario "$f" --socket "$sock" --out "$out" ${capture[@]+"${capture[@]}"} || status=1
done

# Scenarios with their own fixture workspace (one app per fixture).
for f in $(grep -l '^# workspace: ' fixtures/scenarios/*.yaml 2>/dev/null); do
  name="$(basename "$f" .yaml)"
  fixture="$(sed -n 's/^# workspace: //p' "$f" | head -1)"
  echo "== $name ($fixture)"
  sock2="$(mktemp -u "${TMPDIR:-/tmp}/ffsmoke.XXXXXX")"
  cp -R "fixtures/$fixture" "$ws/$fixture"
  # `# traffic_rows: N` sizes the synthetic Traffic (perf scenario: 100000).
  rows="$(sed -n 's/^# traffic_rows: //p' "$f" | head -1)"
  FAUXFIELD_TRAFFIC_ROWS="${rows:-10000}" "$bin" "$ws/$fixture" --mcp-socket "$sock2" >"$out/app-$fixture.log" 2>&1 &
  app2=$!
  for _ in $(seq 1 100); do [[ -S "$sock2" ]] && break; sleep 0.2; done
  sleep 1
  "$bin" mcp scenario "$f" --socket "$sock2" --out "$out" ${capture[@]+"${capture[@]}"} || status=1
  kill "$app2" 2>/dev/null || true
  rm -f "$sock2"
done

echo "== broken-assertion (must fail with exit 1)"
set +e
"$bin" mcp scenario fixtures/scenarios/broken-assertion.yaml --socket "$sock" --out "$out" ${capture[@]+"${capture[@]}"}
code=$?
set -e
if [[ $code -ne 1 ]]; then echo "broken-assertion exited $code, expected 1"; status=1; fi
exit $status
