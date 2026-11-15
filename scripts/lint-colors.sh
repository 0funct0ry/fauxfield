#!/usr/bin/env bash
# Fails on hard-coded colour literals in fauxfield-gui outside theme*.rs (CLAUDE.md: no hard-coded colours).
# Allow-list: scripts/lint-colors.allow, one `path:justification` per line.
set -euo pipefail
cd "$(dirname "$0")/.."
allow=scripts/lint-colors.allow
pat='\b(rgb|rgba|hsla)\(|\b0x[0-9a-fA-F]{6}([0-9a-fA-F]{2})?\b'
bad=0
while IFS= read -r hit; do
  file=${hit%%:*}
  if [ -f "$allow" ] && grep -q "^${file}:" "$allow"; then continue; fi
  echo "colour literal: $hit"; bad=1
done < <(grep -rnE "$pat" crates/fauxfield-gui/src --include='*.rs' \
  | grep -vE '/theme[a-z_]*\.rs:' || true)
[ "$bad" = 0 ] || { echo "lint-colors: use theme tokens, or justify in $allow"; exit 1; }
echo "lint-colors: ok"
