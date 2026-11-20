# Wave 2 review: AB (end-to-end suite and CI job)

Branch `m2/ab-e2e` at d41e22e, reviewed against docs/plan.md (row AB) and docs/api.md. Verdict: **ok,
extended in review**.

## What was checked

- `scripts/e2e.sh`: release build, fixtures imported with the POC registry into a temp database, `serve` on
  port 0 (address read from the log), health wait, cleanup on every exit. CI job `e2e` installs the pinned
  Playwright (lock file) with `--with-deps`, caches browsers, uploads traces on failure. actionlint is clean.
- Specs read every expected number from the server at run time (no hard-coded data): API shapes (no key
  missing, none undocumented), errors, every view at 1280/400 px light/dark with no console error and no
  sideways overflow, tally / heatmap / dot plot equal `/api/compare` and `/api/values`, URL state, tag editing.

## Fixed or extended in review

- Item 6, api.md: `direction` values listed (`higher` / `lower` / `none`, null without a metric table; compare
  rows only higher / lower). `run-detail.json`: `meta` was a placeholder string, now the real meta.json object
  (`stages` was already the real string); api.md says the example cuts `hardware` to `cpu`.
- Item 6, `body { overflow-x: hidden }`: removed. It cut wide content off silently; the views pass without it
  and the suite now sees overflow as sideways scroll.
- Item 6 / item 9 / item 10: the suite grew to cover what smoke.mjs and the export test did, and
  `web/dev/smoke.mjs` is gone. `e2e.sh` starts three servers (open; token with public reads; token with
  `read_public=false`), each on its own database copy, and stops them with SIGTERM. New specs: `runs.spec.js`
  (W's checks and the report.md test), `auth.spec.js` (401s, upload through the dialog with the token prompt,
  `already_present` via the API, private reads and the token-carrying Export button), `export.spec.js` (Export
  button download and endpoint headers), and a round quick-pick test in `compare.spec.js`. After the suite,
  `tests/export/run.sh` checks the offline export. 34 tests plus the export test pass locally.

## Notes

- The placeholder report.md files are written only into the temp copy of the fixtures and say they are
  placeholders; no fixture or number is invented.
