# Wave 1 review: B (web UI)

Branch `m2/b-web` at e5ab36b. Verdict: **fixed in review; three items for the task agent**.

## What was checked

- HTML injection: every value goes in through `h()` / `textContent` or d3 `.text()`; there is no
  `innerHTML`, `outerHTML`, `insertAdjacentHTML` or d3 `.html()` in `web/js`. Links are built from fixed paths
  and `encodeURIComponent`'d run ids. Tag keys are checked in the browser with the server's rule; the server
  checks again.
- API use: only documented endpoints and query keys (`base`, `other`, `ref`, `id`, `version`), so A's strict
  query check never trips; run filters are applied client side. `/api/compare` is fetched without `section`
  and filtered in the browser, which keeps one cached comparison for every section view (fine).
- No maths in the UI: medians, CV, diff and verdicts are shown as the API returns them. The key-metrics list
  (from the prototype) names ids that all exist in the 0.2.3 catalog; missing ones are skipped.
- Against the real server (release build, full POC: 18 runs of round 1 and 2, registry tags) with
  `web/dev/smoke.mjs`: every view at 1280 and 400 px, light and dark, no console or page errors, no sideways
  scroll; the tally (2 rows) and the verdict class of all 134 heatmap cells equal `benchlens compare
  group:x86-high group:arm64-high group:x86-med --json`. Screenshots looked right (compare, metric dot plot).
  The smoke test does not check the percentage text in the cells or the dot plot values.

## Fixed in review

- `review(B)`: rust-embed embedded all of `web/`, including `web/dev/` (dev server sources, smoke test) and the
  READMEs. Now `#[exclude = "dev/*"]` and `#[exclude = "*.md"]` (feature `include-exclude`); the d3 licence is
  still served. Test `dev_tools_and_readmes_are_not_embedded`, in debug (files from disk) and release
  (embedded) builds.
- `review(B)`: the smoke test and web/README.md now point at `benchlens serve` (8080). The dev server stays
  for what the real server cannot do before C2 (token prompt, slow answers); remove it after C2.

## For the task agent

1. Tag editing: an open tag form loses what was typed when the runs table re-renders (selecting a checkbox,
   changing a filter, a reload after another write). Keep the draft per run id, or do not re-render rows that
   are being edited.
2. The runs table links `report.md` for every run, but a run may not have kept one (the server answers 404
   JSON in a new tab). Either show the link only when the run has the file (needs the file list: fetch
   `/api/runs/{id}` lazily, or add `files` to `RunSummary` in a contract change) or point it at the run's
   detail.
3. The delete confirmation says "Importing the folder again brings it back"; the values come back, but tags
   set by a registry or edited in the UI do not (unless they are in `benchlens-tags.json`). Say so.

## Follow-up (feature, not a review fix; in the plan)

- The API has no reference for part of a group, so the "Round 1 vs 2" quick picks compare one run per round
  rather than pooled rounds. Proposed: `group:NAME@tag=value`.
