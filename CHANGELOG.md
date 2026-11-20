# Changelog

All notable changes to benchlens. Versions follow [semantic versioning](https://semver.org/); while the
version is 0.x, a minor release may change the API, the CLI or the database layout (layouts are migrated
automatically, see [docs/guide.md](docs/guide.md#upgrading)).

## [0.1.0] - unreleased

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

[0.1.0]: https://github.com/chaloemph/benchlens/releases/tag/v0.1.0
