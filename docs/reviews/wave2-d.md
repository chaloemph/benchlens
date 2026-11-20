# Wave 2 review: D (static HTML export, JSON / CSV export, Export button)

Branch `m2/d-export` at bb24d19, reviewed against docs/plan.md (row D) and docs/api.md. Verdict: **ok, fixed
in review** (subset sides lacked their group; no change needed in the export itself).

## What was checked

- One generator for `benchlens export html` and `GET /api/export.html` (`crates/server/src/export.rs`). The page
  is the live UI: index.html with CSS and d3 inlined, the ES modules reachable from `web/export/offline.js`
  bundled by a deliberately narrow bundler that refuses any module style it would bundle wrongly (tests run it on
  the real `web/js`), the WASM build as base64, the data as JSON.
- Escaping: data JSON with `<`, `>`, `&`, U+2028/2029 as `\uXXXX`; title HTML-escaped; inlined code and CSS
  refused if they contain `</script`, `</style` or `<!--`; CSP `default-src 'none'` with inline script/style and
  `wasm-unsafe-eval` only, as meta tag and (endpoint) header, plus `nosniff`. `Content-Disposition` file name
  reduced to `[A-Za-z0-9._-]`. The hostile-label test (Rust and the offline browser test) checks nothing runs.
- Numbers: the inlined sides through the WASM crate's compare equal `benchlens compare --json` byte for byte
  (Rust test); the offline browser test checks first load, a changed baseline and a run-vs-group against the CLI.
  Checked by hand on the full POC: `group:x86-high@round=1` vs `@round=2` vs `group:arm64-high`, 134 heatmap
  cells equal the CLI with the network off.
- Read guard: after the merge with C2 the endpoint is inside the guard; test added (item 4).
- CSV: RFC 4180 quoting, CRLF, formula guard (`= + - @`, tab, CR get a leading `'`) on text fields only, numbers
  untouched; documented in README; kept (item 11). JSON export: stats equal the medians compare judges.
- `web/js/app.js` split into `main.js` (shell shared with the export) + `app.js` (entry): behaviour unchanged,
  the e2e suite passes.

## Fixed in review

- `review(D)`: `collect()` found a side's group by stripping `group:`, so a K subset side
  (`group:x86-high@round=1`) carried no `GroupSummary`. Uses `Reference::parse` now; test added.
- `refactor(AB,W,C2,D)`: `tests/export/run.sh` runs inside `scripts/e2e.sh` (release binary via
  `BENCHLENS_BIN`, Playwright from `tests/e2e/node_modules`), so CI runs the offline export test (item 10).
- Merge with C2: `main.js` gets a `setup` hook for the upload dialog; the export passes none and hides Upload.

## Notes

- The export of large groups is built in memory (750 KB for two POC groups); fine at this scale.
- `runs[]` carries meta.json and hardware.json of every exported run, as the API already does; say so when
  sharing exports outside the team (H docs).
