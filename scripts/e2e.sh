#!/usr/bin/env bash
# End-to-end test: build the release binary, import the fixtures into temporary databases, start three
# `benchlens serve` on free ports (no token; a token with public reads; a token with read_public=false), run the
# Playwright suite in tests/e2e against them, then the offline test of the static HTML export (tests/export).
#
#   scripts/e2e.sh                      # whole suite
#   scripts/e2e.sh --grep heatmap       # extra arguments go to `playwright test`
#
# Env: BENCHLENS_E2E_SKIP_BUILD=1 uses the existing target/release/benchlens.
# The server is always stopped and the temporary files removed, also on failure or Ctrl-C.
# Screenshots and traces of failed tests are kept in tests/e2e/test-results/.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if [[ "${BENCHLENS_E2E_SKIP_BUILD:-}" != 1 ]]; then
  cargo build --release --locked -p benchlens
fi
bin="$root/target/release/benchlens"
[[ -x "$bin" ]] || { echo "e2e: $bin not found" >&2; exit 1; }

tmp="$(mktemp -d "${TMPDIR:-/tmp}/benchlens-e2e.XXXXXX")"
pids=()
cleanup() {
  local code=$?
  for pid in "${pids[@]}"; do
    kill -TERM "$pid" 2>/dev/null || true
  done
  for pid in "${pids[@]}"; do
    for _ in $(seq 50); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
    kill -KILL "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  done
  if [[ $code -ne 0 ]]; then
    for log in "$tmp"/*.log; do
      [[ -s "$log" ]] || continue
      echo "e2e: $(basename "$log" .log) server log:" >&2
      sed 's/^/  /' "$log" >&2
    done
  fi
  rm -rf "$tmp"
  exit "$code"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# A copy of the fixtures where every run but one kept a report.md (the fixtures keep none), so the Runs view's
# report.md link can be checked both ways. The file is a marker, not a vmbench report.
fixtures="$tmp/fixtures"
cp -R tests/fixtures "$fixtures"
no_report=x86-std-r2-20261119-103253
for meta in "$fixtures"/*/*/meta.json; do
  dir="$(dirname "$meta")"
  [[ "$(basename "$dir")" == "$no_report" ]] && continue
  printf '# report.md placeholder written by scripts/e2e.sh for %s\n' "$(basename "$dir")" >"$dir/report.md"
done

db="$tmp/e2e.db"
"$bin" --db "$db" import --registry examples/poc-2026-11/registry.tsv "$fixtures/v0.2.1" "$fixtures/v0.2.3"
# the token servers start without x86-std-r2, which the upload test sends
"$bin" --db "$tmp/token.db" import --registry examples/poc-2026-11/registry.tsv "$fixtures/v0.2.1" \
  $(find "$fixtures/v0.2.3" -mindepth 1 -maxdepth 1 -type d ! -name "$no_report") >/dev/null
cp "$tmp/token.db" "$tmp/private.db"
tar -czf "$tmp/upload.tar.gz" -C "$fixtures/v0.2.3" "$no_report"

token="e2e-token-$(od -An -N8 -tx1 /dev/urandom | tr -d ' \n')"
# start NAME DB [ENV=VALUE...]: `benchlens serve` on a free loopback port (port 0: the OS picks one and the
# server prints the address it got); sets URL to that address once /api/health answers
start() {
  local name="$1" dbfile="$2"; shift 2
  env -u BENCHLENS_TOKEN -u BENCHLENS_READ_PUBLIC -u BENCHLENS_CONFIG "$@" \
    "$bin" --db "$dbfile" serve --listen 127.0.0.1:0 2>"$tmp/$name.log" &
  local pid=$!
  pids+=("$pid")
  URL=""
  for _ in $(seq 100); do
    URL="$(grep -oE 'http://127\.0\.0\.1:[0-9]+' "$tmp/$name.log" | head -n1 || true)"
    [[ -n "$URL" ]] && break
    kill -0 "$pid" 2>/dev/null || { echo "e2e: $name server exited" >&2; exit 1; }
    sleep 0.1
  done
  [[ -n "$URL" ]] || { echo "e2e: $name server did not print its address" >&2; exit 1; }
  for _ in $(seq 100); do
    if curl -fsS "$URL/api/health" >/dev/null 2>&1; then
      echo "e2e: $name server on $URL"
      return
    fi
    kill -0 "$pid" 2>/dev/null || { echo "e2e: $name server exited" >&2; exit 1; }
    sleep 0.1
  done
  echo "e2e: $URL/api/health did not answer" >&2
  exit 1
}

start open "$db"
open_url="$URL"
start token "$tmp/token.db" BENCHLENS_TOKEN="$token"
token_url="$URL"
start private "$tmp/private.db" BENCHLENS_TOKEN="$token" BENCHLENS_READ_PUBLIC=false
private_url="$URL"

(
  cd tests/e2e
  [[ -d node_modules/@playwright/test ]] || npm ci --no-audit --no-fund
  BENCHLENS_URL="$open_url" BENCHLENS_TOKEN_URL="$token_url" BENCHLENS_PRIVATE_URL="$private_url" \
    BENCHLENS_E2E_TOKEN="$token" BENCHLENS_UPLOAD="$tmp/upload.tar.gz" BENCHLENS_NO_REPORT="$no_report" \
    BENCHLENS_FIXTURES="$root/tests/fixtures" npx playwright test "$@"
)

# the static HTML export opened from disk with the network off (tests/export); skipped with --grep etc.
if [[ $# -eq 0 ]]; then
  BENCHLENS_BIN="$bin" NODE_PATH="$root/tests/e2e/node_modules" tests/export/run.sh "$tmp/export"
fi
