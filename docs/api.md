# benchlens HTTP API v1

The contract between the server (task A, C2) and the web UI (task B). Change it only in a PR that updates
this file, `docs/api-examples/`, the server and the UI together.

- Base path `/api`. Everything is JSON (UTF-8) except `GET /api/runs/{id}/files/{name}`.
- Every example in `docs/api-examples/` comes from the real code on `tests/fixtures/` (long lists cut).
  Regenerate with `cargo run -p benchlens-core --example api_examples` after changing a shape.
- Field names are those of the Rust types in `benchlens-core` / `benchlens-rules`; the UI must not depend on
  key order.
- **REF** (a side of a comparison): a run id, a unique run id prefix of 4+ characters, a folder name, a label
  that matches one run, or `group:NAME` for every run of a group pooled.
- **Write** endpoints need `Authorization: Bearer <token>` when the server has a token configured (C2);
  without a token configured they are open (local use).

## Errors

Any failure: HTTP status plus `{"error": "message for people", "detail": "optional"}`
([error.json](api-examples/error.json)).

| Status | When |
|---|---|
| 400 | bad parameter, bad tag key, archive not a vmbench result, unsafe archive |
| 401 | write without a valid token (C2) |
| 404 | unknown run, group, file, metric table or endpoint |
| 413 | upload larger than the configured limit (C2) |
| 500 | anything else (the message says what) |
| 501 | endpoint not built yet (T0 skeleton only) |

## Endpoints

### `GET /api/health`
`{benchlens, db_layout, runs}` ([health.json](api-examples/health.json)). Works from T0 on.

### `GET /api/runs`
Every run, oldest first: `RunSummary[]` ([runs.json](api-examples/runs.json)).

| Field | |
|---|---|
| `run_id` | stable id (same run imported again by any route keeps it) |
| `folder`, `label` | result folder name and vmbench label |
| `group` | effective group: the `group` tag, else the automatic one |
| `arch`, `nproc`, `ram_gb`, `cpu_model`, `vmbench_version`, `start_utc` | from meta.json / hardware.json |
| `source` | `cli`, `inbox`, `upload`, `api` |
| `catalog_known` | false: no metric table for that vmbench version, values not judged |
| `failed_stages` | stages whose values are left out |
| `tags` | `{key: value}` |
| `values` | number of raw values stored |

Optional filters (all must match): `tag=KEY:VALUE` (repeatable), `arch=`, `group=`, `q=` (substring of
label, folder, group or any tag value, case-insensitive).

### `GET /api/runs/{id}`
`RunDetail`: every `RunSummary` field plus `stages`, `end_utc`, `imported_at`, `pg_profile`, `web_cpus`,
`meta` (meta.json), `hardware` (hardware.json or null) and `files` (`[{name, bytes}]`)
([run-detail.json](api-examples/run-detail.json)). `{id}` is a run id or unique prefix.

### `GET /api/runs/{id}/files/{name}`
The kept file as stored, `text/plain; charset=utf-8` (`.json` files as `application/json`). Names:
`meta.json`, `results.tsv`, `status.tsv`, `hardware.json`, `report.md`, `benchlens-tags.json`. Anything else
is 404.

### `PATCH /api/runs/{id}/tags` (write)
Body `{"key": "value", ...}`; an empty value removes the tag. Keys: 1-64 of `A-Z a-z 0-9 _ - .`; values: one
line, at most 1024 bytes. Answers the updated `RunSummary`. Special keys: `group` (changes the run's group),
`exclude` (comma list of stages left out).

### `DELETE /api/runs/{id}` (write)
Removes the run and its values. `204` with no body.

### `POST /api/runs` (write, C2)
`multipart/form-data`: `file` = a `.tar` / `.tar.gz` of one or more result folders; optional repeated
`tag` = `KEY=VALUE`. Same import path and rules as `benchlens import`. Answers `ImportReport[]`
([import-report.json](api-examples/import-report.json)) with status 200 even when some folders failed (each
report says `outcome`: `imported`, `already_present`, `failed`); 400 for an unsafe or empty archive, 413 when
too large.

### `GET /api/groups`
`GroupSummary[]` ([groups.json](api-examples/groups.json)): `group`, `runs`, `labels`, `archs`, `nproc`,
`cpu_models`, `vmbench_versions`, `first_start_utc`, `last_start_utc`. A group with more than one entry in
`archs`, `nproc` or `cpu_models` mixes machines; the UI should flag it.

### `GET /api/compare?base=REF&other=REF[&other=REF...][&section=cpu]`
`Comparison` ([compare.json](api-examples/compare.json)):

| Field | |
|---|---|
| `base`, `others[]` | `Side`: `reference`, `label`, `run_ids`, `archs` |
| `rows[]` | one per metric and compared side: `other` (index into `others`), `id`, `name`, `section`, `unit`, `direction` (`higher`/`lower`), `base`, `value` (medians), `cv_base`, `cv_value` (% or null when measured once), `n_base`, `n_value`, `diff` (% or null), `noise` (% threshold used), `verdict` (`better` / `worse` / `not_clear`, from the compared side's point of view), `cross_arch` (sysbench across architectures: show, do not count) |
| `tallies[]` | one per `others` entry: `better`, `worse`, `not_clear`, `by_section` (`{section: [better, worse, not_clear]}`); `cross_arch` rows are not counted |
| `warnings[]` | sentences to show as they are |

`section` keeps only rows of that section (tallies stay complete). At least one `other` is required (400).
The verdict rule is vmbench's; see docs/contract.md.

### `GET /api/values?ref=REF&id=METRIC`
Every raw value of one metric on one side, for the dot plot ([values.json](api-examples/values.json)):
`{side, id, values: ValueRow[]}`, `ValueRow` = `run_id`, `id`, `name`, `section`, `unit`, `direction`,
`tool`, `arch`, `round`, `value`. Stages that failed or are excluded are already left out. Use `run_id`
with `/api/runs` to show which test round a dot belongs to.

### `GET /api/metrics?version=0.2.3`
The metric table of a vmbench version: `Metric[]` (`id`, `unit`, `direction`, `tool`, `label`,
`description`) ([metrics.json](api-examples/metrics.json)). 404 when there is none.

## Not in v1

Pagination (lists are small until thousands of runs; add `limit`/`after` then), editing values, users and
roles beyond one write token, websockets.
