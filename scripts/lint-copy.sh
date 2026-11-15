#!/usr/bin/env bash
# Fails on user-facing string literals in view code of files listed in scripts/lint-copy.files
# (CLAUDE.md / SPEC §9.10: UI copy goes through crates/fauxfield-gui/src/copy.rs).
set -euo pipefail
cd "$(dirname "$0")/.."
pat='\.child\("[A-Za-z]|\.tooltip\(.*"[A-Z]|format!\("[A-Z]'
bad=0
while IFS= read -r f; do
  case "$f" in ''|\#*) continue ;; esac
  # Ignore everything from the test module on.
  hits=$(awk '/#\[cfg\(test\)\]/{exit} {print FNR": "$0}' "$f" | grep -E "$pat" || true)
  if [ -n "$hits" ]; then echo "$f:"; echo "$hits"; bad=1; fi
done < scripts/lint-copy.files
[ "$bad" = 0 ] || { echo "lint-copy: move these strings to copy.rs"; exit 1; }
echo "lint-copy: ok"
