# benchlens web UI

Plain HTML, CSS and JavaScript modules, with d3 v7 vendored. No bundler, no build step, no framework, no
requests outside the benchlens server. `benchlens serve` embeds this folder and serves it next to the JSON API
(`docs/api.md`).

```bash
benchlens --db benchlens.db serve          # then open http://127.0.0.1:8080
```

## Views

All state is in the query string, so any page can be shared as a link:
`view` (`summary`, `compare`, `metric`, `runs`, `hardware`), `base` (REF), `other` (REF, repeated), `section`
(`key`, `all`, `cpu`, ...), `metric` (metric id), `run` (run id, repeated, for hardware), the runs filters
`q`, `arch`, `group`, `tag` (`KEY:VALUE`), and for the Summary view `profile` (`balanced`, `web`, `db`, `compute`,
`custom`) and `w` (custom section weights, `cpu:1,disk:3,...`). A REF is a run id or `group:NAME`, as in the API.

| View | Data | What it shows |
|---|---|---|
| Summary | `/api/compare`, `/api/runs`, `/api/groups` | the same sides as Compare (picked there; a "Sides: ... · change" line links to it), then six blocks: leaderboard with workload profiles, radar, price / performance, scaling, stability, verdicts by section. The last blocks are built one by one (see `js/summary/`); blocks without data hide themselves and say why in one muted line at the end |
| Compare | `/api/groups`, `/api/runs`, `/api/compare` | baseline picker, compare-with checkboxes (groups and single runs), quick picks derived from the data, takeaway sentence, API warnings, tally table, heatmap (section filter incl. key metrics; sysbench across architectures hatched and not counted) |
| Metric | `/api/compare`, `/api/values` | dot plot of every raw value per side, coloured by the run's `round` tag (or by run), median tick from the comparison, axis not from zero; statistics table |
| Runs | `/api/runs`, `/api/runs/{id}`, `PATCH /api/runs/{id}/tags`, `DELETE /api/runs/{id}` | search and filters, tags with inline editing (drafts survive re-renders), delete with confirmation, report.md link when the run kept one (`/api/runs/{id}` file list, fetched when the row scrolls into view), flags for failed stages / excluded stages / no metric table |
| Hardware | `/api/runs/{id}` | run facts and hardware.json of the selected runs side by side, rows that differ highlighted |

Compare, Metric, Runs and Hardware never compute medians, CV, differences or verdicts: every number is shown as
`/api/compare` returns it, so it equals `benchlens compare --json` for the same sides. The Summary view has
**one** exception: the scores of `js/score.js`, built only from `rows[].base`, `value`, `direction`, `cv_*`,
`cross_arch` and `verdict` of `/api/compare` (section score = 100 x geometric mean of the per-metric ratios
against the baseline; overall score = weighted geometric mean of the sections; median CV per section; price and
vCPU of a side from the tags and `nproc` of its runs). Scores are summaries, not verdicts: every chart that shows
one is labelled "score (summary)" and shows the better / worse / within noise counts behind it, and no chart
re-judges a metric. All of that maths lives in `js/score.js` (pure functions, no DOM); the chart modules call it.
`node --test tests/score/` checks it against `tools/score_check.py`, an independent recomputation in Python.

Upload: the header's Upload button opens a dialog (archive, optional tag rows) that posts to `POST /api/runs`
and lists the import report of every folder; the runs reload after it.

Token: when the server answers 401 (a write, or any read on a server with `read_public = false`), a dialog
asks for the token; it is kept in `sessionStorage` for the tab and sent as `Authorization: Bearer <token>` on
every request after that. Requests refused at the same time share one dialog.

## Files

```
index.html          page skeleton (every view, dialogs)
css/app.css         styles; light and dark themes via prefers-color-scheme and custom properties
css/summary.css     styles of the Summary view, one comment-delimited block per chart
js/app.js           entry point of the served UI (main.js + upload dialog)
js/api.js           fetch wrapper, API errors, 401 token prompt
js/state.js         URL query string state
js/data.js          runs/groups store, picker entries, quick picks
js/compare.js       compare view
js/score.js         scores of the Summary view (pure functions; tests/score/, tools/score_check.py)
js/summary/         Summary view: index.js (loads the data, one slot per block), board, radar, price, scaling,
                    stability, verdicts (one file per block), table.js ("Show as table" fallback)
js/detail.js        metric view (dot plot)
js/runs.js          runs view
js/hardware.js      hardware view
js/upload.js        upload dialog
js/format.js        number and label formatting
js/ui.js            DOM helpers (textContent only), tooltip, dialogs
vendor/             d3 v7.9.0 (dist/d3.min.js, licence header kept; LICENSE-d3.txt)
js/main.js          UI shell shared with the static export: loading, routing between views
export/             static HTML export entry (offline.js: API answered from inlined data, WASM compare)
```

Rules: values go into the page with `textContent` (the `h()` helper), never `innerHTML`; SVG charts carry
`role="img"` and an `aria-label`; wide tables scroll inside their own box so the page never scrolls sideways
at 400 px.

## Developing

Edit the files and reload: there is nothing to build. Develop against the real server; a debug build
(`cargo run`) reads `web/` from disk, so a reload shows the edit:

```bash
cargo run -p benchlens -- --db /tmp/poc.db import --registry examples/poc-2026-11/registry.tsv tests/fixtures/v0.2.1 tests/fixtures/v0.2.3
cargo run -p benchlens -- --db /tmp/poc.db serve          # http://127.0.0.1:8080
```

To try the token prompt, start the server with a token (`BENCHLENS_TOKEN=some-long-dev-token cargo run -p
benchlens -- --db /tmp/poc.db serve`), and add `--read-public false` for a private server. The READMEs are not
built into the binary.

Browser tests: `scripts/e2e.sh` (Playwright, `tests/e2e/`; see tests/e2e/README.md) starts the server on the
fixtures (open, with a token, and private) and checks every view at 1280 and 400 px in light and dark, the
numbers on screen against the API, URL state, tag editing and drafts, report.md links, the delete dialog,
upload with the token prompt, private reads and the Export button. It replaced the old `web/dev/smoke.mjs`.

The static HTML export (`web/export/offline.js`, built by `crates/server/src/export.rs`) reuses `js/main.js`
and the views; keep `web/js/` in the plain module style the export bundler accepts (one-line imports,
`export function|const|class`). `tests/export/run.sh` (run by `scripts/e2e.sh`) opens an export from disk with the
network off and checks it against `benchlens compare --json`.
