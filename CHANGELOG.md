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
- Upload through the web UI or `POST /api/runs`.
- Metric tables of vmbench 0.2.1, 0.2.2 and 0.2.3 built in; `benchlens catalog add` for newer versions.

### Compare

- `benchlens compare` with the verdict rule of `vmbench report compare` (median, CV, within noise when
  |difference| ≤ max(1 %, CV of either side)), checked by golden tests against vmbench's own verdicts.
- Sides: a run (id, id prefix, folder, label), `group:NAME` (all runs of a group pooled) and
  `group:NAME@KEY=VALUE` (the runs of a group with one tag value). Ambiguous references are refused.
- Automatic groups from label, CPU, vCPU and RAM; sysbench across architectures shown but not counted.
- `--section`, `--json`.

### Server and web UI

- `benchlens serve`: JSON API v1 ([docs/api.md](docs/api.md)) and the web UI in one binary: Compare (picker,
  quick picks, tally, heatmap), Metric (dot plot of every value), Runs (filters, tag editing, delete) and
  Hardware views; state in the URL; light and dark themes; works at 400 px.
- Settings from flags, `BENCHLENS_*` variables and `benchlens.toml`.
- Token auth for writes (`BENCHLENS_TOKEN`), optional private reads (`read_public = false`), writes refused
  without a token on non-loopback addresses and from pages of other origins; upload size limit.

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

[0.1.0]: https://github.com/chaloemph/benchlens/releases/tag/v0.1.0
