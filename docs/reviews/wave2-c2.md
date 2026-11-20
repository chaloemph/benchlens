# Wave 2 review: C2 (upload, token auth, benchlens.toml)

Branch `m2/c2-upload-auth` at 17606ff, reviewed against docs/plan.md (row C2, F9) and docs/api.md.
Verdict: **ok, fixed in review** (one cross-site upload hole on loopback; Docker docs).

## What was checked

- Auth (`auth.rs`): token compared as SHA-256 digests with `subtle::ct_eq` (no length or content timing);
  `Bearer` scheme case-insensitive, empty token refused. Loopback = `SocketAddr::ip().is_loopback()` of the
  listen address (127.0.0.0/8, ::1; an IPv4-mapped `::ffff:127.0.0.1` counts as not loopback, the safe side).
  Write check is the `WriteAccess` extractor on every write handler and runs before the body is read; the read
  guard is a `route_layer` over every `/api` route except `/api/health` and the 404 fallback (export included
  after the merge with D). 401 carries `WWW-Authenticate: Bearer realm="benchlens"`, 403 does not.
- Config (`config.rs`): flags > `BENCHLENS_*` > benchlens.toml > defaults; `deny_unknown_fields`; empty env
  counts as unset; token >= 16 visible ASCII characters, redacted in `Debug`, no `--token` flag, never printed;
  `read_public = false` without a token refuses to start; upload limit 1..4096 MiB. toml parse errors are
  reported with `message()` only (no source line echoed, so a token cannot leak through an error).
- Upload (`upload.rs`): multipart with `DefaultBodyLimit` = limit + 256 KiB; the `file` field is streamed into a
  `TempDir` and counted, one byte over is 413; at most one `file`, at most 64 `tag` fields of <= 1089 bytes,
  unknown fields 400; client file name reduced to `[A-Za-z0-9._-]`, no leading dot, 128 chars; the temp folder is
  dropped on every path (moved into the blocking import). Tar safety is core's (no extraction; `..`, absolute,
  links, devices refused). Raw tar error text is cut to printable ASCII. Checked in Docker (distroless): an
  upload with the token imports, without it 401, without a token configured 403.
- No SQL built from strings; the server goes through `Store`.
- Tests (`upload_auth.rs`, 10, plus unit tests): idempotent upload equals CLI import, 413, unsafe and broken
  archives 400, bad fields import nothing, every write without/with wrong token 401, private reads 401 except
  health and UI, loopback without token writes open, open address without token 403. They test their claims.
- F9: `web/dev/server` removed.

## Fixed in review

- `review(C2)`: without a token on loopback, writes are open, and a multipart POST is a CORS "simple request":
  any web page the local user visited could upload to `http://127.0.0.1:8080/api/runs` (PATCH / DELETE need a
  preflight and were blocked). Open writes now answer 403 when `Sec-Fetch-Site: cross-site` or `Origin` does
  not name `Host`; no `Origin` (curl) and the UI itself pass. Unit test; e2e tag/upload tests still pass.
- `review(C2)`: Dockerfile, docker-compose.yml (passes `BENCHLENS_TOKEN` through) and the README Docker section
  say that the container is read-only without a token; the README status line no longer says upload comes next.
- `review(C2,D)`: test that `/api/export.html` and `/api/values` need the token on a private server.
- Merge with D: `/api/export.html` sits inside the read guard; the upload dialog is wired through a `setup`
  hook of D's `main.js` (the export page hides the Upload button).
- `refactor`: tag key/value rules in one place (`store::check_tag` / `parse_tag`), used by upload fields, the
  store, `inbox_tags` and the CLI.

## Notes and follow-ups (not blocking)

- DNS rebinding: a loopback server without a token trusts any `Host`; a rebinding page has the same origin and
  can read, and write with the Origin check passing. Fix later with a Host allow-list (localhost, 127.0.0.1,
  [::1]) in no-token mode, or require a token always. Added to the plan.
- On a private server the Runs view's report.md link opens in a new tab without the token (401). Fetch it with
  the token into a blob instead. Plan follow-up.
- Uploads get no registry: a run uploaded without a `group` tag lands in its automatic group. Applying the
  server's `inbox_registry` to uploads would match the inbox. Plan follow-up.
- Reverse proxy: README documents "set a token behind a proxy"; a proxy on the same host makes loopback mean
  "writes open". Plan follow-up to consider refusing open writes when `X-Forwarded-For` / `Forwarded` is present.
