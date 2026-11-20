# M4 follow-ups review: F19–F22 (Host check with a token on loopback, Grafana login, release notes, MSRV)

Branch `m4/followups` at ee967ca (one commit on main 768714f), reviewed against docs/plan.md (F19–F22),
docs/api.md (Auth) and SECURITY.md. Verdict: **ok, fixed in review** (no blocking issue; one coverage gap
documented and turned into a follow-up, small workflow hardening, behaviour change made explicit).

## F19: Host check with a token on a loopback listener

`Access::for_listen` adds `on_loopback()` when the listen IP `is_loopback()` and a token is set;
`checks_host()` is `no token || loopback || allowed_hosts set`. `host_guard` is layered over the whole
router, so `/`, the static files, `/api/health` and every `/api` route get the same check (router test hits
`/`, `/api/health`, `/api/runs` and an upload with a valid token: all 403 for a foreign `Host`, before the
token is looked at).

Checked by hand on a live server (`--listen [::1]:18191` and `127.0.0.1:18192`, token set):

| `Host` | result |
|---|---|
| `localhost:18191`, `LOCALHOST.`, `[::1]:18191`, `127.0.0.1` | 200 |
| `evil.example`, `bench.lan:1` | 403, detail names `allowed_hosts = ["<name>"]` / `BENCHLENS_ALLOWED_HOSTS=<name>` |
| `/` with `evil.example` | 403 (static files covered) |
| `Secret-Name.internal:443` with `BENCHLENS_ALLOWED_HOSTS=secret-name.internal` | 200 |

Checklist points:

- **Reverse proxy on the same host passing the public Host** (Caddy default, nginx `proxy_set_header Host
  $host`): now 403 until the name is in `allowed_hosts`; the 403 says exactly what to set. nginx's default
  (`Host` = upstream IP) passes. Documented in guide, README, api.md, SECURITY.md, CHANGELOG. Intended.
- **Docker with a published port**: the container listens on 0.0.0.0, so it is not loopback and keeps
  serving any `Host` with a token, also when compose publishes `127.0.0.1:8080:8080`. A rebinding page can
  still read the public data of a token-protected container. Not a regression (same as 0.1.0), but the main
  deployment path is not covered by F19. **Fixed in review (docs)**: SECURITY.md, the compose comment and
  CHANGELOG give the workaround `BENCHLENS_ALLOWED_HOSTS=localhost` (any non-empty `allowed_hosts` turns the
  check on; `localhost` and IPs are always served); unit test added for it; `localhost` is no longer listed
  twice in "Host names served". Changing the default is left as **F23**.
- **IPv6 loopback**: `[::1]` listener is loopback (unit test and live check); `[::1]` as `Host` passes.
  `127.0.0.0/8` is loopback. An IPv4-mapped listen address (`[::ffff:127.0.0.1]`) is *not*
  (`Ipv6Addr::is_loopback` is false for it): with a token it serves any `Host`, as in 0.1.0; without a token
  it is treated as non-loopback (writes closed, fail-safe). Pre-existing and used in three places
  (`for_listen`, config validation, start-up message); left as **F24** rather than changing write semantics
  here.
- **`allowed_hosts` with a token**: adds names on loopback and non-loopback; names are lower-cased and
  matched without trailing dot and port (unit, router and live tests).
- **No leak in the 403**: `error` echoes the request's own `Host` (first 100 chars, Debug-quoted); `detail`
  echoes only the request's own name and only if it is a plain host name (`[A-Za-z0-9._-]`, ≤ 253),
  otherwise a generic text. Configured names and the token are never included (checked live with a
  configured `secret-name.internal` and the token string: neither appears in the body). The JSON body is
  `application/json`, so the echo is not an HTML injection.
- **Health and static files**: same middleware, consistent (see above).

## Behaviour change

With a token on loopback, opening the server by a name (an `/etc/hosts` alias, a tunnel or same-host proxy
that passes another name) now gets 403. The guide explained the proxy case but not the alias case, and the
CHANGELOG had it only inside the Security entry. **Fixed in review**: guide paragraph (aliases and tunnels
get 403 until listed; `localhost` / `127.0.0.1`, also through `ssh -L`, keep working) and an explicit
**Behaviour change** bullet under `### Changed` in CHANGELOG `[Unreleased]`.

## F20: Grafana login by default

`GF_AUTH_ANONYMOUS_ENABLED: ${GRAFANA_ANONYMOUS:-false}`. Re-run once: `docker compose -p benchlens-l
--profile grafana up -d` → env `GF_AUTH_ANONYMOUS_ENABLED=false`; anonymous `/api/search` 401,
`/api/ds/query` 401, `/` 302 to `/login`; `admin:admin` `/api/search` 200. Then `down -v` and the pulled
images (`grafana/grafana:13.3.1`, `ghcr.io/chaloemph/benchlens:latest`) removed. Docs (README, guide,
SECURITY.md, deploy/grafana/README.md, compose comments) say to change the `admin` password before the first
start and that Grafana bypasses the token and `read_public`. No secrets committed (the only password is the
documented `admin` default, overridable by `GRAFANA_ADMIN_PASSWORD`).

## F21: release notes from CHANGELOG

`scripts/release-notes.sh` (mode 100755 in git) exercised: `0.1.0` and `v0.1.0` ok; missing `0.1.1`,
`Unreleased`, prefix `0.1`, `## [0.2.0] - unreleased`, an empty section: exit 1 with a clear message;
link definitions at the end and a `[0.2.0-rc1]` section before `[0.2.0]` handled (the `]` in the match
prevents prefix hits); works from another cwd (path relative to the script).

release.yml: `notes` has a checkout step; `build` and `docker` `needs: notes`, `release` `needs: [notes,
build]`, so a missing section fails before any build or push. The dispatch path creates no release (`if:
github.ref_type == 'tag'`) and pushes no image (`push: ${{ github.ref_type == 'tag' }}`, login only on tags).
No new permissions (workflow `contents: read`; `release` keeps `contents: write`, `docker` `packages: write`
as before). **Fixed in review**: `notes` states `permissions: contents: read` explicitly, and on a tag it
checks the tag against the workspace version too, so a mismatched tag also fails before any build.

## F22: MSRV job

Reads `rust-version` within `[workspace.package]` only (the sed range ends at the next `[` section), so the
crate values 1.80 / 1.81 are never picked; empty result is an error. **Fixed in review**: accepts
`rust-version="1.89"` without spaces and a trailing comment, digits and dots only (tested on a sample
manifest with a `[package]` 1.80 before and another section 1.70 after). Cache key `msrv-<version>` on top
of rust-cache's own toolchain hash: separate from the stable cache, sensible.

## Tests run (after the review commit)

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --all`: ok.
- `scripts/test-wasm.sh`: 336 verdicts in 6 pairs match.
- `scripts/e2e.sh`: ok (export offline, hostile labels).
- `cargo +1.89 check --locked --workspace --all-targets`: ok.
- actionlint on ci.yml and release.yml: clean.
- Release dry run on GitHub (`gh workflow run release.yml --ref m4/followups`): see the PR description.

## Not verified

- A real tag push (the release job's `--notes-file` path runs only on a tag); the dispatch run covers
  `notes` and the artifact hand-off up to `release`'s download step.
- Caddy / nginx in front of benchlens: the proxy cases are covered by router tests with the `Host` they
  send, not with a real proxy.
