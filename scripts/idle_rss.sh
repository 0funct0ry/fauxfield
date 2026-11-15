#!/usr/bin/env bash
# SPEC §15: idle memory, GUI, 10 servers < 150 MB. Starts the release GUI binary on the perf
# workspace, lets it idle 10 s, samples RSS, asserts < 150 MB (set BUDGET_MB to override).
# Needs a display; on a headless box pass HEADLESS=1 to measure `fauxfield run` instead (report only).
# Usage: scripts/idle_rss.sh [path/to/fauxfield]
set -euo pipefail
cd "$(dirname "$0")/.."
BIN=${1:-target/release/fauxfield}
BUDGET_MB=${BUDGET_MB:-150}
WS=fixtures/perf-workspace
[ -d "$WS" ] || cargo run -q -p xtask -- perf-workspace
[ -x "$BIN" ] || { echo "error: $BIN not built (make release)" >&2; exit 1; }
if [ "${HEADLESS:-0}" = 1 ]; then "$BIN" run "$WS" >/dev/null 2>&1 & else "$BIN" "$WS" >/dev/null 2>&1 & fi
PID=$!
trap 'kill $PID 2>/dev/null || true' EXIT
sleep 10
kill -0 $PID 2>/dev/null || { echo "error: process exited" >&2; exit 1; }
RSS_MB=$(( $(ps -o rss= -p $PID | tr -d ' ') / 1024 ))
if [ "$(uname -s)" = Darwin ]; then
  # macOS RSS counts shared system-framework pages (Metal, AppKit) that every app maps; the
  # budget uses the physical footprint, which is what Activity Monitor reports as Memory.
  MB=$(vmmap -summary $PID 2>/dev/null | sed -n 's/^Physical footprint: *\([0-9.]*\)M.*/\1/p' | head -1 | cut -d. -f1)
  MB=${MB:-$RSS_MB}
else
  MB=$RSS_MB   # Linux: VmRSS
fi
echo "idle_rss: ${MB} MB footprint, ${RSS_MB} MB RSS (budget ${BUDGET_MB} MB)"
if [ "${HEADLESS:-0}" = 1 ]; then exit 0; fi
[ "$MB" -lt "$BUDGET_MB" ] || { echo "FAIL: idle RSS ${MB} MB >= ${BUDGET_MB} MB" >&2; exit 1; }
