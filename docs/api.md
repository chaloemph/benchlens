# benchlens HTTP API v1

The contract between the server (tasks A, C2, D) and the web UI (tasks B, W). Change it only in a PR that updates
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
| 403 | write on a server that listens on a non-loopback address without a token; without a token, a write from a web page of another origin |
| 404 | unknown run, group, file, metric table or endpoint; a `group:NAME@KEY=VALUE` that selects no run |
| 413 | upload larger than `max_upload_mb`; a `PATCH .../tags` body over 64 KiB |
| 500 | anything else (the message says what) |
| 501 | endpoint not built yet (T0 skeleton only) |

## Auth

Settings of `benchlens serve` (flag > `BENCHLENS_*` variable > `benchlens.toml` > default): `token`
(`BENCHLENS_TOKEN`; no command-line flag), `read_public` (`--read-public`, `BENCHLENS_READ_PUBLIC`,
default `true`) and `allowed_hosts` (`BENCHLENS_ALLOWED_HOSTS` as a comma list, a TOML array in the file;
names only, no scheme or port).

| Configuration | Reads (`/api/...`) | Writes (`PATCH .../tags`, `DELETE /api/runs/{id}`, `POST /api/runs`) |
|---|---|---|
| no token, listening on loopback (`127.0.0.1`, `::1`) | open | open |
| no token, any other address | open | **403** (the server prints a warning at start) |
| token, `read_public = true` (default) | open | `Authorization: Bearer <token>`, else **401** |
| token, `read_public = false` | `Authorization: Bearer <token>`, else **401** | same |

- **Host check** (DNS rebinding), before anything else and for every path, the web UI files included:
  without a token, a request is served only when its `Host` (HTTP/2: `:authority`) is `localhost`, an IP
  address (`127.0.0.1`, `[::1]`, `192.168.1.5`, ...) or a name in `allowed_hosts`, with any port; anything
  else is **403** with a JSON error naming `allowed_hosts`. Names are compared without case and without a
  trailing dot; when a request gives more than one name (two `Host` headers, or an absolute-form target such
  as `GET http://name/ HTTP/1.1`), every one must pass. A rebinding page (its domain re-pointed at
  127.0.0.1) sends its own domain as `Host`, so it is refused; an IP address cannot be rebound, and the port
  is not checked so port mappings (`docker -p 9000:8080`) keep working. With a token every `Host` is served
  (a rebinding page never sees the token), unless `allowed_hosts` is set: then the same rule applies. A
  request without `Host` (HTTP/1.0 tools) is served; browsers always send one.
- **Reverse proxy**: without a token, a write carrying `Forwarded`, `X-Forwarded-For` or `X-Forwarded-Host` is
  **403** ("set a token when serving behind a proxy"): a proxy on the same machine would otherwise make
  loopback's open writes reachable from outside. Reads through a proxy are not affected; with a token the
  headers are ignored.
- Always open (once past the Host check): `GET /api/health` and the web UI files (everything outside `/api`), so the page and its token
  dialog load on a private server. An unknown `/api/...` path is 404 without a token check.
- 401 bodies are the usual JSON error and carry `WWW-Authenticate: Bearer realm="benchlens"`. The scheme
  `Bearer` is case-insensitive. The token is compared in constant time (SHA-256 of both sides).
- The server refuses to start with a token shorter than 16 characters (or with spaces or non-ASCII), and with
  `read_public = false` but no token. It never prints the token. An empty `BENCHLENS_TOKEN` counts as unset.
- The token check runs before the body is read, so a refused upload is not received.
- Without a token, a write that a browser marks as coming from another site (`Sec-Fetch-Site: cross-site`, or
  an `Origin` that does not name the `Host`) is 403: a multipart upload needs no CORS preflight, so any page
  the local user visits could otherwise post to a loopback server. Requests without `Origin` (curl, scripts)
  are not affected.
- `/api/export.html` is a read like the others: with `read_public = false` it needs the token.
- Kept files (`/api/runs/{id}/files/{name}`) are reads too; on a private server the UI's plain report.md link
  opens in a new tab without the token and gets 401 (known gap, docs/plan.md).
- A reverse proxy in front of a loopback listener makes it reachable from outside; the forwarded-header check
  above catches the usual proxies, not all: set a token whenever anything but the local machine can reach the
  server.

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
([run-detail.json](api-examples/run-detail.json); `stages` is the space-separated list from meta.json, `meta`
the meta.json object as stored; the example cuts `hardware` to its `cpu` part). `{id}` is a run id, unique prefix, folder or label (a run
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
limit plus 256 KiB), then imported with the same code as `benchlens import`, source `upload`. When the
server has a registry (`registry` in benchlens.toml, `BENCHLENS_REGISTRY`, `--registry`; old name
`inbox_registry` / `BENCHLENS_INBOX_REGISTRY` / `--inbox-registry` still accepted), it applies to uploads as
it does to inbox imports: read at every upload, rows matched by result folder name; the upload's `tag` fields
win over it. A registry that cannot be read or parsed refuses the upload with 500 (nothing imported). A folder at
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
| `rows[]` | one per metric and compared side: `other` (index into `others`), `id`, `name`, `section`, `unit`, `direction` (`higher`/`lower`; metrics with direction `none` are never judged and have no row), `base`, `value` (medians), `cv_base`, `cv_value` (% or null when measured once), `n_base`, `n_value`, `diff` (% or null), `noise` (% threshold used), `verdict` (`better` / `worse` / `not_clear`, from the compared side's point of view), `cross_arch` (sysbench across architectures: show, do not count) |
| `tallies[]` | one per `others` entry: `better`, `worse`, `not_clear`, `by_section` (`{section: [better, worse, not_clear]}`); `cross_arch` rows are not counted |
| `warnings[]` | sentences to show as they are |

`section` keeps only rows of that section (tallies stay complete). At least one `other` is required (400).
The verdict rule is vmbench's; see docs/contract.md.

### `GET /api/values?ref=REF&id=METRIC`
Every raw value of one metric on one side, for the dot plot ([values.json](api-examples/values.json)):
`{side, id, values: ValueRow[]}`, `ValueRow` = `run_id`, `id`, `name`, `section`, `unit`, `direction`,
`tool`, `arch`, `round`, `value` (`direction` as in `/api/metrics`, or null for a vmbench version without a
metric table). Stages that failed or are excluded are already left out. Use `run_id`
with `/api/runs` to show which test round a dot belongs to (`round` here is the repeat inside the run, see
above). The values are exactly the rows of that metric among all the side's values (only that metric is
read from the database). A metric id with no values on that side (or not in any table) gives 200 with
`values: []`.
`side.archs` is empty here; it is filled only in `/api/compare`.

### `GET /api/metrics?version=0.2.3`
The metric table of a vmbench version: `Metric[]` (`id`, `unit`, `direction`, `tool`, `label`,
`description`) ([metrics.json](api-examples/metrics.json)). `direction` is `higher` (higher is better),
`lower` (lower is better) or `none` (a context value such as CPU steal: shown, never judged). `version` is
required (400 without it); a leading `v` is accepted. 404 when there is no table for that version.

### `GET /api/export.html?base=REF&other=REF[&other=REF...][&title=T]`
The static HTML report of a comparison, the same file as `benchlens export html`: `text/html` with
`Content-Disposition: attachment; filename="benchlens-<base>-vs-<other>.html"` and a
`Content-Security-Policy` that allows no network access. A read: with `read_public = false` it needs the token
(401). Same parameter checks as `/api/compare` (400 without
`base` or `other`, 404 for an unknown side); `title` (one line, at most 1024 bytes) defaults to
`"BASE vs OTHER, ..."`. The file opens from disk with no server: it inlines `web/index.html`, the CSS, d3,
the modules of `web/js/` bundled with `web/export/offline.js`, the WASM verdict rule (base64, in
`<script type="application/octet-stream" id="benchlens-wasm">`) and this JSON in
`<script type="application/json" id="benchlens-data">` (`<`, `>`, `&`, U+2028, U+2029 written as `\uXXXX`):

| Field | |
|---|---|
| `format` | 1 (raised when this shape changes) |
| `benchlens`, `exported_at`, `source`, `title` | version, UTC time, database file name (`benchlens server` from this endpoint), title |
| `base`, `others[]` | the exported references, as given |
| `sides[]` | baseline first: `{side: Side, values: ValueRow[]}`, what `/api/compare` judges for that reference |
| `runs[]` | `RunDetail` of every run of those sides, `files` empty |
| `groups[]` | `GroupSummary` of the groups named by `group:NAME` and `group:NAME@KEY=VALUE` sides |

In the page the reader can pick any exported side, or any single run of one, as baseline or compared side;
comparisons are computed by the WASM `compare` and equal `/api/compare` for the same sides.

## Not in v1

- Pagination (lists are small until thousands of runs; add `limit`/`after` then), editing values, users and
  roles beyond one write token, websockets.
