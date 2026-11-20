#!/usr/bin/env bash
# End-to-end test: build the release binary, import the fixtures into a temporary database, start
# `benchlens serve` on a free port and run the Playwright suite in tests/e2e against it.
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
server_pid=""
cleanup() {
  local code=$?
  if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
    kill -INT "$server_pid" 2>/dev/null || true
    for _ in $(seq 50); do kill -0 "$server_pid" 2>/dev/null || break; sleep 0.1; done
    kill -KILL "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
  if [[ $code -ne 0 && -s "$tmp/serve.log" ]]; then
    echo "e2e: server log:" >&2
    sed 's/^/  /' "$tmp/serve.log" >&2
  fi
  rm -rf "$tmp"
  exit "$code"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

db="$tmp/e2e.db"
"$bin" --db "$db" import --registry examples/poc-2026-11/registry.tsv tests/fixtures/v0.2.1 tests/fixtures/v0.2.3

# port 0: the OS picks a free port; the server prints the address it got
"$bin" --db "$db" serve --listen 127.0.0.1:0 2>"$tmp/serve.log" &
server_pid=$!

url=""
for _ in $(seq 100); do
  url="$(grep -oE 'http://127\.0\.0\.1:[0-9]+' "$tmp/serve.log" | head -n1 || true)"
  [[ -n "$url" ]] && break
  kill -0 "$server_pid" 2>/dev/null || { echo "e2e: server exited" >&2; exit 1; }
  sleep 0.1
done
[[ -n "$url" ]] || { echo "e2e: server did not print its address" >&2; exit 1; }

healthy=""
for _ in $(seq 100); do
  if curl -fsS "$url/api/health" >/dev/null 2>&1; then healthy=1; break; fi
  kill -0 "$server_pid" 2>/dev/null || { echo "e2e: server exited" >&2; exit 1; }
  sleep 0.1
done
[[ -n "$healthy" ]] || { echo "e2e: $url/api/health did not answer" >&2; exit 1; }
echo "e2e: benchlens serving $url (database $db)"

cd tests/e2e
[[ -d node_modules/@playwright/test ]] || npm ci --no-audit --no-fund
BENCHLENS_URL="$url" BENCHLENS_FIXTURES="$root/tests/fixtures" npx playwright test "$@"
