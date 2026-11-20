# Wave 3 review: S (Host check, proxy headers, registry for uploads, SIGPIPE)

Branch `m3/s-server-hardening` at 8d0fcf8, reviewed against docs/plan.md (F14, F15, F16, F18) and docs/api.md.
Verdict: **ok, fixed in review** (two small gaps in the Host check, one env-name inconsistency).

## What was checked

- **F14 Host check** (`auth::check_host`, `host_guard` layered over the whole router, static files included).
  Without a token only `localhost`, IP addresses and `allowed_hosts` are served; with a token any `Host`
  unless `allowed_hosts` is set. Re-checked the design choices:
  - *Every IP accepted*: a rebinding page's `Host` is its own DNS name; an IP literal cannot be re-pointed,
    and a page whose origin is an IP is served from that IP, not from this server. Browsers canonicalise
    `http://2130706433/`, `0x7f.1`, `127.0.0.1.` to `127.0.0.1` before sending, and Rust's strict
    `IpAddr` parser refuses those spellings, so they are refused (fail closed). IPv4-mapped and long IPv6
    forms are addresses and pass. Sound.
  - *Port not checked*: rebinding keeps the attacker's name, so the name alone decides; port mappings
    (`docker -p 9000:8080`) keep working. Sound.
  - Trailing dot, upper case: normalised (`LocalHost.:8080` passes, `localhost.evil.example` does not).
  - Userinfo (`user@localhost`, `evil.example:80@127.0.0.1`): never sent by browsers; refused because the
    name keeps the `@`. Unbracketed IPv6 (`::1`) refused.
  - **Gap (fixed)**: only the first of `Host` / URI authority was looked at. A request with two `Host`
    headers, or an HTTP/1.1 absolute-form target (`GET http://evil.example/ HTTP/1.1` + `Host: localhost`)
    passed on one name. Browsers do not send these, but nothing should pass with one name and be handled
    under another; now every name must pass. Checked through hyper for real:
    `curl --request-target http://evil.example/api/health http://127.0.0.1:18084/` is 403.
  - **Gap (fixed)**: `[::1` (no closing bracket) and `[::1]evil` were read as `::1`. Now refused.
  - Remaining design gap, documented (SECURITY.md, README): with a token and `read_public = true`, any
    `Host` is served, so a rebinding page can read the public data of a server on 127.0.0.1 (never the
    token, never write). `allowed_hosts` closes it. Applying the check by default in token mode would break
    reverse proxies that pass the public name; left as is for v0.1.0.
- **F15 proxy headers**: open writes (loopback, no token) carrying `Forwarded`, `X-Forwarded-For` or
  `X-Forwarded-Host` are 403; with a token they are ignored; reads unaffected. Documented as a safety net
  only (nginx's plain `proxy_pass` adds none of them). Correct.
- **F16 registry for uploads**: read at every upload (edits apply at once), parse/read failure is 500 with
  the path in `detail` (server's own config, not user data), form tags win. `registry` / `inbox_registry`:
  both in one TOML layer is an error; across layers the higher layer wins whichever name it uses; flag has
  `--inbox-registry` as a visible alias. Tests cover precedence. One upload-dialog hint line (text only).
- **F18 SIGPIPE**: `SIG_DFL` is set first thing in `main` (single-threaded, before clap), then back to
  `SIG_IGN` for `serve` before the tokio runtime and the inbox thread start, so a client that disconnects
  cannot kill the server and the inbox thread runs in that same process. Child processes (`hostname`,
  `kill -0` in the inbox lock) only matter in `watch`, where the default action is the normal Unix one.
  The test pipes into a closed reader and checks signal 13 for compare/runs/--help. README keeps the
  `set -o pipefail` note.

## Fixed in review

- `review(S)`: `check_host` checks every `Host` header and the request-target authority; `host_name`
  refuses malformed brackets. 20 more cases in `host_check_without_token` (upper case, trailing dot,
  IPv4-mapped, long IPv6, decimal / hex IPv4, userinfo, unbracketed IPv6, two Host headers, absolute form).
- `review(S)`: `benchlens watch` read only `BENCHLENS_INBOX_REGISTRY`, while `serve` documents
  `BENCHLENS_REGISTRY`. `watch` now takes `BENCHLENS_REGISTRY` and still falls back to the old name; README
  line fixed. api.md says every name must pass.
- `review(S)` (on the integration branch): the start-up line "serving Host any Host name" reads
  "Host names served: any" / "Host names served: localhost, IP addresses, bench.lan".

## Notes

- User-visible: a token-less server (including the Docker image without `BENCHLENS_TOKEN`) reached by a
  DNS name now answers 403 until the name is in `BENCHLENS_ALLOWED_HOSTS`. Called out in the PR.
- In the environment, both `BENCHLENS_REGISTRY` and `BENCHLENS_INBOX_REGISTRY` set is not an error (the new
  name wins), unlike the TOML file. Harmless.
