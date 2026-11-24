# Changelog

All notable changes to benchlens. Versions follow [semantic versioning](https://semver.org/); while the
version is 0.x, a minor release may change the API, the CLI or the database layout (layouts are migrated
automatically, see [docs/guide.md](docs/guide.md#upgrading)).

## [Unreleased]

### Added

- **Summary view** (`?view=summary`, first in the menu): a comparison as six charts for decision makers, using
  the sides of Compare: a leaderboard with workload profiles (balanced, web app, database, compute, or custom
  weights in the URL as `profile` and `w`), a radar of the six sections, price / performance, scaling with
  vCPUs, stability (median CV) and verdicts by section. Price / performance needs `price` (and optionally
  `price_unit`) tags on the runs of at least two sides, scaling needs two sizes of one architecture; a block
  without data hides itself and says why. Scores (section score: 100 times the geometric mean of the ratios
  to the baseline) are summaries, labelled as such, with the verdict counts next to them; verdicts and the
  API are unchanged. Every chart has a "Show as table" fallback. See the guide, the contract and the
  walkthrough.
- The static HTML export opens on the Summary view, also offline, with profiles and custom weights in the link.
- Docs: guide section "Summary view" (including the price tags), contract section "Scores", a Summary step in
  the walkthrough.

### Changed

- A bookmark or link without `view` now opens the Summary view (before: Compare). Links that name a view are
  unchanged, and the links the UI makes to Compare now carry `view=compare`.

### Fixed

- The static HTML export inlines its own stylesheet once, not after every stylesheet.

## [0.1.4] - 2026-11-21

### Added

- `benchlens serve` warns at start-up when the Host check is off: a token, a non-loopback listener and no
  `allowed_hosts` list (for example `benchlens serve --listen 0.0.0.0:8080` with a token, or a container
  started with the opt-out `-e BENCHLENS_ALLOWED_HOSTS=` and no `allowed_hosts` in a mounted file). Any `Host`
  is then served, so a DNS rebinding page can read what `read_public = true` makes public; the warning names
  `allowed_hosts` / `BENCHLENS_ALLOWED_HOSTS=name` and `read_public = false`. Before, `Host names served: any`
  was the only hint. Quiet with a list (also the Docker image default), with `read_public = false`, on
  loopback, and without a token (which has its own warning). Behaviour unchanged.

## [0.1.3] - 2026-11-21

### Changed

- Docker: the image default for the Host check moved from `BENCHLENS_ALLOWED_HOSTS=localhost` to the new
  `BENCHLENS_DEFAULT_ALLOWED_HOSTS=localhost`, which has the lowest priority: flag > `BENCHLENS_ALLOWED_HOSTS`
  > `allowed_hosts` in benchlens.toml > `BENCHLENS_DEFAULT_ALLOWED_HOSTS` > none. **Behaviour change for
  mounted-file users**: `allowed_hosts` in a mounted `benchlens.toml` now replaces the image default without
  `-e BENCHLENS_ALLOWED_HOSTS=`; in 0.1.2 it was ignored (with a start-up warning), so names listed there are
  now served. Nothing changes for a container without a file (`localhost` and IP addresses only) or for
  anyone who sets `-e BENCHLENS_ALLOWED_HOSTS=...`: a list still wins over the file, and an empty value is
  still the opt-out (it counts as not set and also drops the image default, so with no list in a file and a
  token every `Host` is served, as in 0.1.2). An empty `allowed_hosts = []` in the file counts as not set and
  keeps the image default.
- `BENCHLENS_ALLOWED_HOSTS` holding no name (only blanks or commas, e.g. `" "`) now counts as empty, like
  `BENCHLENS_ALLOWED_HOSTS=`: a config file's `allowed_hosts` is used when there is one. In 0.1.2 such a value
  turned the Host check off and also hid the file's list.
- The Host-check 403 names only the setting that would work, given where the current list came from:
  `BENCHLENS_ALLOWED_HOSTS=name` when the list comes from that variable (the file's list has no effect then),
  `allowed_hosts = ["name"]` when it comes from the config file, either one for the image default or no
  list. Configured names and the token are still never shown. Before, it always suggested both. Without a
  token on a non-loopback address it adds "or set a token" only when no list is set (with a list, such as the
  image default, a token would not serve the name; 0.1.2 suggested it anyway).
- The start-up line names the source of the list, e.g. `Host names served: localhost, IP addresses, bench.lan
  (from allowed_hosts in the config file)`. The warning about a config file's `allowed_hosts` being ignored now
  appears only when `BENCHLENS_ALLOWED_HOSTS` (or a flag) really hides a different, non-empty list; it no
  longer mentions the image.

## [0.1.2] - 2026-11-21

### Security

- Docker: the image sets `BENCHLENS_ALLOWED_HOSTS=localhost`, so a container checks the `Host` header also
  with a token: only `localhost`, IP addresses and the listed names are served, anything else is 403. The
  container listens on 0.0.0.0, which is not loopback, so the loopback Host check of 0.1.1 did not apply:
  with a token any `Host` was served, and a DNS rebinding page could read the data that `read_public = true`
  makes public through a published port, also one published as `127.0.0.1:8080:8080` (it could not write).
- A listen address in IPv4-mapped form (`[::ffff:127.0.0.1]`, any `::ffff:127.x.y.z`) now counts as loopback,
  the same as `127.0.0.1`: the Host check applies with a token, and without a token writes are open and no
  warning is printed. Before, such a listener served any `Host` with a token (writes without a token were
  refused, fail-safe).

### Changed

- **Behaviour change** (Docker, from the default above): opening a container by a DNS name (a LAN name such
  as `http://bench.lan:8080`, or a reverse proxy that passes its public name as `Host`) now gets 403 until
  that name is listed: `-e BENCHLENS_ALLOWED_HOSTS=bench.lan` or, in compose, `BENCHLENS_ALLOWED_HOSTS:
  localhost,bench.lan`; the 403 names the exact setting. A value you set replaces the image default
  (`localhost` and IP addresses are always served, so nothing is lost). `http://localhost:8080` and opening the
  container by IP address work as before. An explicit `BENCHLENS_ALLOWED_HOSTS` in the container also wins
  over `allowed_hosts` in a mounted `benchlens.toml` (the environment beats the file); the image default does
  too, so put the names in the variable, or start with `-e BENCHLENS_ALLOWED_HOSTS=` to use the file's list.
  A reverse proxy in another container that reaches benchlens by its service name (nginx
  `proxy_pass http://benchlens:8080;`) sends that name as `Host`: list it too.
- `benchlens serve` warns at start-up when `BENCHLENS_ALLOWED_HOSTS` hides a different `allowed_hosts` from
  the config file (`warning: allowed_hosts in the config file (...) is ignored: ...`), so a mounted
  `benchlens.toml` is not ignored silently in the container.
- `docker-compose.yml` has a commented `BENCHLENS_ALLOWED_HOSTS: localhost,bench.example.org` example; the
  container's start-up line reads `Host names served: localhost, IP addresses` with a token (0.1.1: `any`).

## [0.1.1] - 2026-11-20

### Security

- On a loopback address (`127.0.0.1`, `::1`) the server now checks the `Host` header also when a token is
  set: only `localhost`, IP addresses and `allowed_hosts` are served, anything else is 403. Before, with a
  token any `Host` was served, so a DNS rebinding page could read the data that `read_public = true` makes
  public from a server on 127.0.0.1 (it could not write). A reverse proxy on the same host that passes its
  public name as `Host` (Caddy by default, nginx with `proxy_set_header Host $host`) needs that name in
  `allowed_hosts` / `BENCHLENS_ALLOWED_HOSTS`; the 403 names the exact setting. Non-loopback listeners are
  unchanged.
- Grafana (compose profile `grafana`): anonymous access is off by default (`GRAFANA_ANONYMOUS` defaults to
  `false`), so a login is needed. Grafana reads the database directly and bypasses benchlens' token and
  `read_public`; set `GRAFANA_ANONYMOUS=true` only on a local machine, and change the default `admin`
  password with `GRAFANA_ADMIN_PASSWORD`.

### Changed

- **Behaviour change** (from the Host check above): with a token on a loopback address, opening the server by
  a name other than `localhost` (an `/etc/hosts` alias, a tunnel or same-host proxy that passes another name as
  `Host`) now gets 403 until that name is in `allowed_hosts`. `localhost` and IP addresses work as before.
  Docker containers listen on 0.0.0.0 and are not affected; set `BENCHLENS_ALLOWED_HOSTS=localhost` there to
  get the same check.
- The start-up line `Host names served:` reflects the loopback Host check (`localhost, IP addresses` on
  loopback with a token, where 0.1.0 printed `any`).
- The Host-check 403 says exactly what to set (`allowed_hosts = ["name"]` / `BENCHLENS_ALLOWED_HOSTS=name`)
  and suggests a token only where a token would help.

### CI

- Release notes come from the tag's section of this file (`scripts/release-notes.sh`) instead of GitHub's
  generated notes; a missing, empty or "unreleased" section fails the release before anything is built or
  pushed, and the dry run prints the notes.
- New CI job `msrv`: `cargo check --locked --workspace --all-targets` with the workspace `rust-version`
  (1.89), read from `Cargo.toml`.
- The release's `notes` job also checks that the tag matches the workspace version, so a mismatched tag fails
  before any build.

## [0.1.0] - 2026-11-20

First public release.

### Import

- `benchlens import` of vmbench result folders, directories of them and `.tar` / `.tar.gz` archives (read in
  memory, never extracted; unsafe entries refuse the archive). `--dry-run`, `--json`.
- The same run imported again by any route keeps its id and is not stored twice.
- Tags from `--tag`, a registry TSV (`--registry`), a `benchlens-tags.json` in the result folder, or
  `benchlens tag` later. `group` and `exclude` have a meaning; failed stages are always left out.
- Inbox: `benchlens watch` and `benchlens serve --inbox` import folders and archives dropped into a folder,
  then move them to `done/` or `failed/` (with a reason). Registry and tags for inbox imports, re-read when
  the registry changes; one watcher per inbox (lock file); clean stop on Ctrl-C / SIGTERM.
- Upload through the web UI or `POST /api/runs`. The server's registry (`registry` setting,
  `BENCHLENS_REGISTRY`, `--registry`; the older name `inbox_registry` still works) tags uploads as it tags
  inbox imports; the upload's own tags win.
- Metric tables of vmbench 0.2.1, 0.2.2 and 0.2.3 built in; `benchlens catalog add` for newer versions.

### Compare

- `benchlens compare` with the verdict rule of `vmbench report compare` (median, CV, within noise when
  |difference| ≤ max(1 %, CV of either side)), checked by golden tests against vmbench's own verdicts.
- Sides: a run (id, id prefix, folder, label), `group:NAME` (all runs of a group pooled) and
  `group:NAME@KEY=VALUE` (the runs of a group with one tag value). Ambiguous references are refused.
- Automatic groups from label, CPU, vCPU and RAM; sysbench across architectures shown but not counted.
- `--section`, `--json`.
- Output piped into a reader that stops early (`benchlens compare ... | head`) ends quietly by SIGPIPE
  (status 141), like other Unix tools; `serve` is not affected.

### Server and web UI

- `benchlens serve`: JSON API v1 ([docs/api.md](docs/api.md)) and the web UI in one binary: Compare (picker,
  quick picks, tally, heatmap, "split a group by tag" for `group:NAME@KEY=VALUE` sides), Metric (dot plot
  of every value), Runs (filters, tag editing, delete, report.md shown in a dialog and fetched with the token)
  and Hardware views; state in the URL; light and dark themes; works at 400 px.
- `GET /api/runs` lists each run's `kept_files`.
- Settings from flags, `BENCHLENS_*` variables and `benchlens.toml`.
- Token auth for writes (`BENCHLENS_TOKEN`), optional private reads (`read_public = false`), writes refused
  without a token on non-loopback addresses, from pages of other origins and through reverse proxies
  (`Forwarded` / `X-Forwarded-*` headers); upload size limit.
- DNS rebinding guard: without a token only `localhost`, IP addresses and `allowed_hosts`
  (`BENCHLENS_ALLOWED_HOSTS`) are served as `Host`; with a token, `allowed_hosts` when set.

### Export

- `benchlens export html`: one self-contained HTML file that opens offline, lets the reader change baseline
  and sides, and computes verdicts with the same rule compiled to WebAssembly. Also the Export button and
  `GET /api/export.html`.
- `benchlens export csv` and `export json` of values and statistics.

### Packaging

- Release archives for Linux (x86_64, aarch64, static musl), macOS (arm64, x86_64) and Windows (x86_64), with
  `SHA256SUMS`.
- Docker image `ghcr.io/chaloemph/benchlens` (linux/amd64, linux/arm64, distroless, non-root) and
  `docker-compose.yml`.
- SQL views `v_runs`, `v_values`, `v_stats` for other tools.
- Grafana: compose profile `grafana` with the SQLite datasource on the database (read-only) and three
  dashboards (metric over time, group vs group, runs). benchlens keeps the SQLite `-wal` and `-shm` files
  when it closes the database, so a read-only reader keeps working while benchlens is stopped.

[Unreleased]: https://github.com/chaloemph/benchlens/compare/v0.1.4...main
[0.1.4]: https://github.com/chaloemph/benchlens/releases/tag/v0.1.4
[0.1.3]: https://github.com/chaloemph/benchlens/releases/tag/v0.1.3
[0.1.2]: https://github.com/chaloemph/benchlens/releases/tag/v0.1.2
[0.1.1]: https://github.com/chaloemph/benchlens/releases/tag/v0.1.1
[0.1.0]: https://github.com/chaloemph/benchlens/releases/tag/v0.1.0
