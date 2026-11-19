# Wave 1 review: A (server read endpoints, tags, delete)

Branch `m2/a-server` at 7f54e71, reviewed against docs/plan.md (row A), docs/api.md and docs/contract.md.
Verdict: **ok, fixed in review** (one bug in core exposed through the API).

## What was checked

- Every read endpoint of API v1 answers as docs/api.md says; `/api/compare` and `/api/values` return exactly
  what `benchlens_core::compare` returns (the tests compare the JSON with core's own output).
- SQL: the server builds none; every lookup goes through `Store` with bound parameters.
- Query strings: `check_query` refuses unknown keys, values over 1024 bytes, control characters, more than 32
  repeats; single-valued keys given twice are 400. The UI (B) sends only documented keys (checked in
  `web/js/api.js`: `base`, `other`, `ref`, `id`, `version`; run filters are applied in the browser).
- Path segments: run references are length- and control-checked; file names are matched against the fixed
  list of kept files before the store is asked.
- Bodies: `PATCH .../tags` is limited to 64 KiB (413) and 64 tags (400); one bad tag rolls back the whole
  transaction (the store's `set_tags` runs in one transaction).
- Static files (T0 code, exercised here): release builds read only the embedded files; debug builds go through
  rust-embed's canonicalize-and-prefix check. `curl --path-as-is /../../Cargo.toml` and `%2e%2e` both get
  index.html.
- Tests (`crates/server/tests/api.rs`, 17 + 1 added): 404 for unknown runs, groups, files, metric tables;
  400 for bad refs, bad filters, bad tag keys and bodies; 413; section filter keeps tallies; delete removes
  values. They test what they claim.
- Verdict rule: untouched.

## Fixed in review

- `review(A)`: `Store::find_run` matched the id prefix with `LIKE ?1 || '%'`, so `%` and `_` in a reference
  were wildcards: `/api/runs/%%%%` matched every run. Now `substr(run_id, 1, length(?1)) = ?1`. Test
  `id_prefix_is_not_a_like_pattern` fails without the fix.
- `review(A)`: tag values allowed `\r`; "one line" now refuses `\r` as well as `\n` (test added).

## Interpretations of docs/api.md (now written into docs/api.md)

1. A reference matching several runs is 404 with a message saying how many (core has no Ambiguous error).
2. `/api/values` with no values (or an unknown metric id) is 200 with an empty list.
3. `{id}` in `/api/runs/{id}` also accepts a unique folder or label; `group:` is 400.
4. `/api/metrics` without `version` is 400; a leading `v` is accepted.
5. `side.archs` is empty in `/api/values`.
6. Unknown query parameters are 400.

All six are reasonable; none needed a code change.

## Notes and follow-ups (not blocking)

- `/api/values` loads every value of the side and filters by metric id in Rust; fine at POC size, a
  `Store::values_of(run_ids, id)` would avoid it for big groups.
- `v_values` (core, M1) excludes stages with `NOT LIKE '%,' || section || ',%'`, where the section comes from
  results.tsv; a section containing `_` or `%` would match too loosely. Use `instr()`; needs a schema bump
  because the view is created with `IF NOT EXISTS`. Added to the plan.
- `WriteAccess` is the single place for C2's token check (TODO(C2) in write.rs).
