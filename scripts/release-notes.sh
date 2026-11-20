#!/usr/bin/env bash
# Print the release notes of VERSION: the body of its `## [VERSION]` section in CHANGELOG.md, without the
# heading, up to the next `## ` heading or the link definitions at the end. Used by .github/workflows/release.yml.
#
#   scripts/release-notes.sh 0.1.0 [CHANGELOG.md]      (a leading `v` is accepted: v0.1.0)
#
# Fails (exit 1, message on stderr) when the section is missing, empty, or its heading still says
# "unreleased" (e.g. `## [0.1.1] - unreleased`): date the section before tagging.
set -euo pipefail

if [ $# -lt 1 ] || [ $# -gt 2 ] || [ -z "$1" ]; then
  echo "usage: $0 VERSION [CHANGELOG]" >&2
  exit 2
fi
version="${1#v}"
changelog="${2:-$(dirname "$0")/../CHANGELOG.md}"
[ -f "$changelog" ] || { echo "error: $changelog not found" >&2; exit 1; }

heading=$(awk -v h="## [$version]" 'index($0, h) == 1 { print; exit }' "$changelog")
if [ -z "$heading" ]; then
  echo "error: no \`## [$version]\` section in $changelog: add one before releasing" >&2
  exit 1
fi
if grep -qi 'unreleased' <<<"$heading"; then
  echo "error: the $version section of $changelog still says unreleased ($heading): date it before releasing" >&2
  exit 1
fi

notes=$(awk -v h="## [$version]" '
  !found { if (index($0, h) == 1) found = 1; next }
  /^## / || /^\[[^]]+\]: / { exit }
  { print }
' "$changelog" | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;ba' -e '}')

if [ -z "$(tr -d '[:space:]' <<<"$notes")" ]; then
  echo "error: the $version section of $changelog is empty" >&2
  exit 1
fi
printf '%s\n' "$notes"
