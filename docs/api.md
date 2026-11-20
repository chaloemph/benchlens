# benchlens HTTP API v1

The contract between the server (task A, C2) and the web UI (task B). Change it only in a PR that updates
this file, `docs/api-examples/`, the server and the UI together.

- Base path `/api`. Everything is JSON (UTF-8) except `GET /api/runs/{id}/files/{name}`.
- Every example in `docs/api-examples/` comes from the real code on `tests/fixtures/` (long lists cut).
  Regenerate with `cargo run -p benchlens-core --example api_examples` after changing a shape.
- Field names are those of the Rust types in `benchlens-core` / `benchlens-rules`; the UI must not depend on
  key order.
- **REF** (a side of a comparison): a run id, a unique run id prefix of 4+ characters, a folder name, a label
  that matches one run, `group:NAME` for every run of a group pooled, or `group:NAME@KEY=VALUE` for the runs
  of group NAME whose tag KEY is exactly VALUE, pooled. Example: with the POC registry,
  `group:x86-high@round=1` is x86-high and its web rerun x86-high-web, `group:x86-high@round=2` is
  x86-high-r2. Exactly one `@KEY=VALUE`: KEY follows the tag key rules (1-64 letters, digits, `_ - .`), VALUE
  is not empty, is compared as it is (no trimming, no case folding) and may not contain `@`; so a group whose
  name contains `@` cannot be referred to. The side's `label` is then `NAME · KEY=VALUE`
  (`x86-high · round=1`). A badly written REF (such as `group:`, `group:G@round`, `group:G@a=1@b=2`) is 400;
  a selection with no runs is 404 (`none of the 3 runs of group "x86-high" has tag round="9"`). A reference
  that matches several runs is refused with 400 and a message such as
  `"twin" matches 2 runs; use a run id, or group:NAME to pool them`. `%` and `_` are plain characters, not
  wildcards. In a query string, `@` and `=` inside a REF may be sent as they are or percent-encoded.
- **Query strings** are checked before anything is looked up: an unknown parameter, a single-valued parameter
  given twice, more than 32 repeats of `tag` / `other`, a value over 1024 bytes or with control characters
  all give 400. The UI sends only the parameters listed here.
- **`round`** means two things: in a `ValueRow` it is the repeat number inside one run (the `round` column of
  results.tsv, 1, 2, 3 ... for a metric measured several times); the `round` *tag* (from a registry, for
  example) is the test round a whole run belongs to. The dot plot colours by the tag.
- **Access**: see [Auth](#auth). In short: with a token configured, writes need `Authorization: Bearer
  <token>` (and reads too when `read_public = false`); without one, writes are open only on a loopback
  address.

## Errors

Any failure: HTTP status plus `{"error": "message for people", "detail": "optional"}`
([error.json](api-examples/error.json)).

| Status | When |
|---|---|
| 400 | bad parameter, bad tag key, badly written REF, a REF that matches several runs, archive not a vmbench result, unsafe archive, upload not a tar / tar.gz |
| 401 | token needed and missing or wrong (with `WWW-Authenticate: Bearer realm="benchlens"`) |
| 403 | write on a server that listens on a non-loopback address without a token |
| 404 | unknown run, group, file, metric table or endpoint; a `group:NAME@KEY=VALUE` that selects no run |
| 413 | upload larger than `max_upload_mb`; a `PATCH .../tags` body over 64 KiB |
| 500 | anything else (the message says what) |
| 501 | endpoint not built yet (T0 skeleton only) |

## Auth

Settings of `benchlens serve` (flag > `BENCHLENS_*` variable > `benchlens.toml` > default): `token`
(`BENCHLENS_TOKEN`; no command-line flag) and `read_public` (`--read-public`, `BENCHLENS_READ_PUBLIC`,
default `true`).

| Configuration | Reads (`/api/...`) | Writes (`PATCH .../tags`, `DELETE /api/runs/{id}`, `POST /api/runs`) |
|---|---|---|
| no token, listening on loopback (`127.0.0.1`, `::1`) | open | open |
| no token, any other address | open | **403** (the server prints a warning at start) |
| token, `read_public = true` (default) | open | `Authorization: Bearer <token>`, else **401** |
| token, `read_public = false` | `Authorization: Bearer <token>`, else **401** | same |

- Always open: `GET /api/health` and the web UI files (everything outside `/api`), so the page and its token
  dialog load on a private server. An unknown `/api/...` path is 404 without a token check.
- 401 bodies are the usual JSON error and carry `WWW-Authenticate: Bearer realm="benchlens"`. The scheme
  `Bearer` is case-insensitive. The token is compared in constant time (SHA-256 of both sides).
- The server refuses to start with a token shorter than 16 characters (or with spaces or non-ASCII), and with
  `read_public = false` but no token. It never prints the token. An empty `BENCHLENS_TOKEN` counts as unset.
- The token check runs before the body is read, so a refused upload is not received.
- A reverse proxy in front of a loopback listener makes it reachable from outside while writes stay open:
  set a token whenever anything but the local machine can reach the server.

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
([run-detail.json](api-examples/run-detail.json)). `{id}` is a run id, unique prefix, folder or label (a run
reference as above, but not `group:`, which is 400).

### `GET /api/runs/{id}/files/{name}`
The kept file as stored, `text/plain; charset=utf-8` (`.json` files as `application/json`). Names:
`meta.json`, `results.tsv`, `status.tsv`, `hardware.json`, `report.md`, `benchlens-tags.json`. Anything else
is 404, and so is a name the run did not keep (`files` in `GET /api/runs/{id}` lists what it has).

### `PATCH /api/runs/{id}/tags` (write)
Body `{"key": "value", ...}`; an empty value removes the tag. Keys: 1-64 of `A-Z a-z 0-9 _ - .`; values: one
line (no `\n` or `\r`), at most 1024 bytes; at most 64 tags per request. One bad tag refuses the whole request
(400) and changes nothing. Answers the updated `RunSummary`. Special keys: `group` (changes the run's group),
`exclude` (comma list of stages left out).

### `DELETE /api/runs/{id}` (write)
Removes the run and its values. `204` with no body.

### `POST /api/runs` (write)
`multipart/form-data` with exactly one `file` field (a `.tar` or `.tar.gz` of one or more result folders;
gzip is detected from the content, not the name) and optional repeated `tag` fields `KEY=VALUE` (at most 64,
same key/value rules as `PATCH .../tags`; key and value are trimmed; an empty value removes that tag, e.g.
one from `benchlens-tags.json`). Any other field name is 400.

```bash
curl -H "Authorization: Bearer $BENCHLENS_TOKEN" -F file=@x86-std-r2.tar.gz -F tag=set=poc -F tag=round=2 \
     http://127.0.0.1:8080/api/runs
```

The file is streamed to a fresh folder under the system temp dir, counted as it arrives (one byte over
`max_upload_mb`, default 256 MiB, ends the request with 413; the whole request body may be at most the
limit plus 256 KiB), then imported with the same code as `benchlens import`, source `upload`. A folder at
the root of the archive is named after the uploaded file name, as with `benchlens import name.tar.gz`. The
temp folder is removed on every path.

Answers `ImportReport[]` ([import-report.json](api-examples/import-report.json)) with status 200 even when
some folders failed (each report says `outcome`: `imported`, `already_present`, `failed`); uploading a run
that is already there answers `already_present` and applies the tags. 400 for: no `file`, an empty file,
two `file` fields, a bad tag, an archive with an absolute path, `..`, a link or device entry, an archive
holding no result folder (no `meta.json`), or data that is not tar / tar.gz. 413 when too large.

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
with `/api/runs` to show which test round a dot belongs to (`round` here is the repeat inside the run, see
above). The values are exactly the rows of that metric among all the side's values (only that metric is
read from the database). A metric id with no values on that side (or not in any table) gives 200 with
`values: []`.
`side.archs` is empty here; it is filled only in `/api/compare`.

### `GET /api/metrics?version=0.2.3`
The metric table of a vmbench version: `Metric[]` (`id`, `unit`, `direction`, `tool`, `label`,
`description`) ([metrics.json](api-examples/metrics.json)). `version` is required (400 without it); a leading
`v` is accepted. 404 when there is no table for that version.

## Not in v1

- Pagination (lists are small until thousands of runs; add `limit`/`after` then), editing values, users and
  roles beyond one write token, websockets.
