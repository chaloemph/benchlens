# benchlens web UI

Plain HTML, CSS and JavaScript modules, with d3 v7 vendored. No bundler, no build step, no framework, no
requests outside the benchlens server. `benchlens serve` embeds this folder and serves it next to the JSON API
(`docs/api.md`).

```bash
benchlens --db benchlens.db serve          # then open http://127.0.0.1:8080
```

## Views

All state is in the query string, so any page can be shared as a link:
`view` (`compare`, `metric`, `runs`, `hardware`), `base` (REF), `other` (REF, repeated), `section`
(`key`, `all`, `cpu`, ...), `metric` (metric id), `run` (run id, repeated, for hardware), and the runs filters
`q`, `arch`, `group`, `tag` (`KEY:VALUE`). A REF is a run id or `group:NAME`, as in the API.

| View | Data | What it shows |
|---|---|---|
| Compare | `/api/groups`, `/api/runs`, `/api/compare` | baseline picker, compare-with checkboxes (groups and single runs), quick picks derived from the data, takeaway sentence, API warnings, tally table, heatmap (section filter incl. key metrics; sysbench across architectures hatched and not counted) |
| Metric | `/api/compare`, `/api/values` | dot plot of every raw value per side, coloured by the run's `round` tag (or by run), median tick from the comparison, axis not from zero; statistics table |
| Runs | `/api/runs`, `PATCH /api/runs/{id}/tags`, `DELETE /api/runs/{id}` | search and filters, tags with inline editing, delete with confirmation, report.md link, flags for failed stages / excluded stages / no metric table |
| Hardware | `/api/runs/{id}` | run facts and hardware.json of the selected runs side by side, rows that differ highlighted |

The UI never computes medians, CV, differences or verdicts: every number is shown as `/api/compare` returns
it, so it equals `benchlens compare --json` for the same sides.

Writes: when the server answers 401, a dialog asks for a token; it is kept in `sessionStorage` for the tab
and sent as `Authorization: Bearer <token>` on every request after that.

## Files

```
index.html          page skeleton (every view, dialogs)
css/app.css         styles; light and dark themes via prefers-color-scheme and custom properties
js/app.js           entry point: loading, routing between views
js/api.js           fetch wrapper, API errors, 401 token prompt
js/state.js         URL query string state
js/data.js          runs/groups store, picker entries, quick picks
js/compare.js       compare view
js/detail.js        metric view (dot plot)
js/runs.js          runs view
js/hardware.js      hardware view
js/format.js        number and label formatting
js/ui.js            DOM helpers (textContent only), tooltip, dialogs
vendor/             d3 v7.9.0 (dist/d3.min.js, licence header kept; LICENSE-d3.txt)
dev/                development tools only (below); not used by the UI
```

Rules: values go into the page with `textContent` (the `h()` helper), never `innerHTML`; SVG charts carry
`role="img"` and an `aria-label`; wide tables scroll inside their own box so the page never scrolls sideways
at 400 px.

## Developing

Edit the files and reload: there is nothing to build. To work before the real server answers every endpoint,
`web/dev/server` is a small stand-in for API v1 built on `benchlens-core`, so every number comes from the real
code (it is its own Cargo project, outside the workspace, and serves `web/` from disk without caching):

```bash
benchlens --db /tmp/poc.db import --registry examples/poc-2026-11/registry.tsv tests/fixtures/v0.2.1 tests/fixtures/v0.2.3
cargo run --manifest-path web/dev/server/Cargo.toml -- --db /tmp/poc.db --listen 127.0.0.1:8090
# BENCHLENS_DEV_TOKEN=secret      writes need a token (tries the 401 prompt)
# BENCHLENS_DEV_DELAY_MS=800      slow answers (shows the loading states)
```

Once `benchlens serve` implements the API, use it instead (debug builds of rust-embed read `web/` from disk).

Smoke test in a headless browser (needs the `playwright` npm package and its chromium; it is not a project
dependency):

```bash
benchlens --db /tmp/poc.db compare group:x86-high group:arm64-high group:x86-med --json > /tmp/cmp.json
NODE_PATH=/path/to/node_modules node web/dev/smoke.mjs http://127.0.0.1:8090 /tmp/shots /tmp/cmp.json
```

It opens every view at 1280 and 400 px in light and dark, saves screenshots, fails on page errors or sideways
page scroll, and checks that the tally table and every heatmap cell match the CLI's JSON. Other sides:
`SIDES='base=...&other=...'`.
