#!/usr/bin/env bash
# Print the release notes for a version from CHANGELOG.md.
# Usage: scripts/release-notes.sh 1.0.0   (reads "## [1.0.0]" or "## 1.0.0"; falls back to "## Unreleased")
set -euo pipefail
ver="${1:?usage: release-notes.sh <version>}"; ver="${ver#v}"
cl="$(dirname "$0")/../CHANGELOG.md"
notes="$(awk -v v="$ver" '
  /^## / { if (on) exit; h=$0; sub(/^## /,"",h); gsub(/[\[\]]/,"",h); split(h,a," ");
           if (a[1]==v || (tolower(a[1])=="unreleased" && !seen)) { on=1; next } }
  on { print }' "$cl")"
[ -n "$notes" ] || { echo "no notes found for $ver in CHANGELOG.md" >&2; exit 1; }
printf '%s\n' "$notes"
