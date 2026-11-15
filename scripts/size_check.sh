#!/usr/bin/env bash
# SPEC §15: headless binary (stripped) < 15 MB.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -q --release -p fauxfield --no-default-features --features headless
BIN=target/release/fauxfield
BYTES=$(wc -c < "$BIN" | tr -d ' ')
echo "headless_size: $((BYTES / 1024)) KiB (budget 15360 KiB)"
[ "$BYTES" -lt $((15 * 1024 * 1024)) ] || { echo "FAIL: headless binary >= 15 MB" >&2; exit 1; }
