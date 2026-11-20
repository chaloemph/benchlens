#!/usr/bin/env bash
# End-to-end test of `benchlens export html`: import the fixtures with the POC registry, export
# group:x86-high vs group:arm64-high, open the file from disk in headless Chromium with the network off
# (tests/export/offline.mjs) and check that the page shows what `benchlens compare --json` prints: on first
# load, after changing the baseline in the page, for one run against a group, and with hostile labels.
#
#   NODE_PATH=/path/to/node_modules tests/export/run.sh [OUT_DIR]
#
# Needs the `playwright` npm package and its chromium (not a project dependency), like web/dev/smoke.mjs.
set -euo pipefail
cd "$(dirname "$0")/../.."
out=${1:-target/export-test}
rm -rf "$out"
mkdir -p "$out"
cargo build -q -p benchlens
bl() { target/debug/benchlens --db "$out/poc.db" "$@"; }

bl import tests/fixtures/v0.2.1 tests/fixtures/v0.2.3 --registry examples/poc-2026-11/registry.tsv >/dev/null
X=group:x86-high A=group:arm64-high
bl export html --base $X --other $A -o "$out/report.html"
bl compare $X $A --json >"$out/exported.json"
bl compare $A $X --json >"$out/swapped.json"
run=$(bl runs --json | node -e 'const r=JSON.parse(require("fs").readFileSync(0));console.log(r.find(x=>x.label==="x86-high-r2").run_id)')
bl compare "$run" $A --json >"$out/run.json"
node tests/export/offline.mjs "$out/report.html" "$out/exported.json" "$out/swapped.json" \
  "base=$run&other=$A" "$out/run.json"

# hostile group name: move one ARM run into a group whose name tries to break out of the page
evil='</script><img src=x onerror="window.__benchlens_pwned=true"><!--'
arm=$(bl runs --json | node -e 'const r=JSON.parse(require("fs").readFileSync(0));console.log(r.find(x=>x.label==="arm64-high-r2").run_id)')
bl tag "$arm" "group=$evil" >/dev/null
bl export html --base $X --other "group:$evil" -o "$out/hostile.html" --title "$evil"
bl compare $X "group:$evil" --json >"$out/hostile.json"
node tests/export/offline.mjs "$out/hostile.html" "$out/hostile.json" --hostile
