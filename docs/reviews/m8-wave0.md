# Review: M8 Wave 0, task S0 (branch `m8/s0-score-shell`)

Reviewer: L. Branch: three task commits on main a94f7bd (0.1.4): bb955d2 (score module, tests,
`tools/score_check.py`, CI step), a603c2f (Summary view shell), 8590cfe (e2e smoke test); plus four
`review(s0)` commits. Integrated as `integration/m8-wave0` (main + `--no-ff` merge of the branch).
Verdict: **accepted with fixes** (below). No version bump, no CHANGELOG entry yet (task X writes it for 0.2.0).

## What the branch does

- `web/js/score.js`: pure functions over `/api/compare`, `/api/runs` and `/api/groups`: section scores
  (100 x geometric mean of per-metric ratios against the baseline, `n`, `weak`, verdict counts), overall
  score (weighted geometric mean, `incomplete`), the four built-in profiles, `parseWeights` / `formatWeights`,
  median CV per section (`sideStability`, `baseStability`), `sidePrice`, `pricePerformance`, `sideVcpu`.
- `tools/score_check.py` (standard library only) recomputes section scores, overall scores of every profile
  plus custom weights, and the CVs in a different way (products to a power), and compares with score.js run
  in node to 1e-9. `tests/score/` (39 tests) checks score.js against it on the API examples, on a whole
  comparison of the fixtures, and on the edge cases.
- Summary view shell: nav link first, `?view=summary` with the same sides as Compare (a "Sides: ... ·
  change" line linking to Compare, so no picker logic is duplicated), six slots and `#sum-hidden-notes`,
  `profile` / `w` in the URL state, six stub blocks, `table.js` (the "Show as table" fallback),
  `css/summary.css` with one block per task, an e2e smoke test, web/README.md.

## Checklist

- **No change in `crates/rules`, `compare.rs`, WASM, golden files, API shapes: yes.** `git diff main` is
  empty for `crates/`, `web/wasm/` and `tests/golden/`; no API or schema change.
- **All score maths in `score.js`: yes.** `summary/index.js` only calls `weightsFor` and `scoresFor`; the
  stubs and `table.js` compute nothing.
- **Scores labelled as summaries, counts next to each score**: no chart yet. `sectionScores` returns the
  counts each Wave 1 chart must show; web/README.md states the rule.
- **Hidden blocks**: `ctx.hide(reason)` empties and hides the slot and adds one `.sum-note` to
  `#sum-hidden-notes` (no empty box). Fixed: it could add a note twice, or add a stale note from a render a
  newer one had superseded.
- **`textContent` only**: yes (`h(..., { text })`, text nodes); tags (price, labels) are passed through as
  data and a test feeds `<img onerror>` style tags through `sidePrice`.
- **Dark mode, 400 px, table fallbacks, colour cue, radar axis order**: nothing drawn yet. `SECTIONS` fixes
  the axis order for the radar; the smoke test checks no sideways scroll at 1280 and 400 px.
- **Tests use fixtures; prices only via API and named "example"**: score tests use `docs/api-examples` and a
  real `benchlens compare --json` of `tests/fixtures`. Price unit tests put prices in run tags cloned from a
  real run, in node only (no server, nothing written to `registry.tsv`); the file says they are user input.
  The e2e price tests of task P must go through `PATCH /api/runs/{id}/tags`.
- **Suites**: see "Checks". `scripts/e2e.sh` could not run on this machine.

## Score maths, re-checked independently

`python3 tools/score_check.py` agrees on the API example (55 numbers; the example cuts long lists, so it
holds only three cpu rows of one side). On the whole fixtures (`benchlens compare group:x86-high
group:arm64-high x86-std-r2 --json`, 134 rows) it agrees on 83 numbers. A third recomputation written for this
review (plain loops, no code shared with either) gives the same values, for example:

| Side | Section | n | Score | Median CV |
|---|---|---|---|---|
| arm64-high | pg | 3 | 142.5293 | 1.33 % |
| arm64-high | mem | 3 | 68.5668 | 21.68 % |
| arm64-high | web | 1 | 50.0045 | 8.94 % |
| x86-std-r2 | cpu | 31 | 37.9735 | 0.67 % |
| x86-std-r2 | disk | 22 | 101.5398 | none (measured once) |

and arm64-high overall 81.5717 (balanced) and 110.0446 (db) by hand from its six section scores, as
score.js returns. The definitions match: rows with `cross_arch == false`, `base > 0`, `value > 0`; ratio
inverted for `direction: lower`; `n == 0` gives `null`; weights renormalised over non-null sections with
weight > 0; a missing section with weight > 0 lands in `incomplete`.

## Questions from the task, answered

1. **e2e cannot run here.** Chromium does not start on this machine (`libnspr4.so` missing, installing system
   packages needs sudo); no system package was installed. Instead: `index.js`, `main.js` and `state.js` were
   read for runtime errors (every import exists and is exported, slot ids match `index.html`, the nav
   `data-view`, `ALL_VIEWS`, `TITLES` and the `view-summary` section agree, the default-sides branch now
   covers `summary`); `benchlens serve` on the fixtures serves `?view=summary`, the eight `js/summary/*.js`
   modules, `js/score.js` and `css/summary.css` (200, right types); `GET /api/export.html` bundles
   `js/summary/index.js`, the six blocks and `js/score.js` and inlines `summary.css` (export still opens on
   Compare and hides the Summary link until task X). **The e2e job of this pull request is the first time
   the Summary view runs in a browser**; it must be green before Wave 1 starts.
2. **Exports beyond the list (`SECTIONS`, `weightsFor`, `scoresFor`): kept.** `SECTIONS` is the one fixed
   section order (radar axes, tables); `weightsFor` is the one place that turns `profile` / `w` into weights
   (and says when custom weights were invalid); `scoresFor` is the "scores precomputed once per render" the
   shell hands to every block. All three are score.js maths the blocks would otherwise repeat.
3. **Whole-fixtures test skipped without a binary: fixed.** It now fails instead of skipping when `CI` is set
   or `BENCHLENS_BIN` names a file, and the CI step sets `BENCHLENS_BIN=target/debug/benchlens`
   (`cargo test --all` builds it for the CLI integration tests, earlier in the same job).
4. **`tests/score/package.json` + `index.js`: kept, made robust.** Node 24 runs a directory argument as a
   module, so `node --test tests/score/` needs `index.js`. Without `package.json` Node 24 still detects the ES
   module syntax, but the file makes it explicit for any Node; five lines is the simplest option that keeps
   the plan's command. CI uses Node 24 (`actions/setup-node`). Fixed: `index.js` listed the test files by
   hand; it now imports every `*.test.mjs` next to it, so a later test file cannot be silently left out.
5. **`baseStability` and `sidePrice` semantics: match.** The baseline has no section score of its own, so
   its CVs are taken over the rows any compared side uses for a score, each metric once (`cv_base` is the
   same for a metric whatever the compared side: checked on the fixtures, 67 metrics, 0 differences).
   `sidePrice` returns `unit: ''` for "no unit"; two sides without a unit count as the same unit and
   `pricePerformance` names it "no unit" in problems, as the definition asks. Prices: `.` decimal, no
   separators, > 0; one priced run with another unpriced in a side is "inconsistent", listed by run label.

## Parallel work in Wave 1

- **Slots**: `index.html` has the six slots in the plan's order; each block writes only into its slot via
  `render(el, ctx)` and `ctx.hide(reason)`. `index.js` now documents the contract in one place, including the
  new `ctx.profile` (`{ profile, weights, invalid }`), so the leaderboard can say when custom weights were
  invalid without recomputing them.
- **CSS**: six empty blocks separated by their headers. Merging six branches that each add rules under their
  own header (or at the end of their block) was simulated with git: no conflict either way. The header now
  says to add rules right under the block's header and scope them to its slot.
- **Smoke test**: `summary.spec.js` required a placeholder in every slot and every slot visible, so every
  Wave 1 task would have broken it (price and scaling hide on the POC data). Fixed: it checks the order, that
  each slot is filled or hidden and empty with one note, and that the leaderboard shows.
- No Wave 1 task needs to touch `index.html`, `state.js`, `main.js`, `index.js` or `summary.spec.js`.

## Fixes (`review(s0)` commits)

- f243f50: whole-fixtures score test fails in CI instead of skipping; `index.js` loads every test file.
- 47b612b: `ctx.profile`; `hide()` once per block and never from a superseded render; contract documented in
  `index.js`; CSS header.
- 851ad3a: Summary smoke test independent of the stubs.
- eabaf3f: `score_check.py` docstring pointed to a docs section task X has not written; now points to
  web/README.md and shows how to check a whole comparison.

## Checks (on `integration/m8-wave0`)

| Suite | Result |
|---|---|
| `cargo fmt --check` | pass |
| `cargo clippy --all-targets -- -D warnings` | pass |
| `cargo test --all` | pass (23 test binaries, 0 failures) |
| `scripts/test-wasm.sh` | pass (336 verdicts in 6 pairs match) |
| `python3 tools/score_check.py` | agrees (55 numbers); on the whole fixtures comparison 83 numbers |
| `node --test tests/score/` (with `CI=1`) | pass (39 tests, 0 skipped) |
| `scripts/e2e.sh` | **could not run here**: Chromium does not launch on this machine (`libnspr4.so` missing). Tried once: the 12 API-only tests pass, the 32 browser tests (the summary spec among them) fail at the browser launch, `tests/export/run.sh` not reached. The CI e2e job of the pull request is the real run |

## Follow-ups

- F30: the export inlines `web/export/export.css` after every stylesheet link, so with `css/summary.css` it is
  in the file twice (harmless, same rules); inline it once (task X, `crates/server/src/export.rs`) (low).
- F31: `tools/score_check.py` does not recompute the price / performance index; task P's e2e compares with it
  per the plan, so add it there or in score_check (low).
- F32: Summary is first in the nav but a URL without `view` still opens Compare; decide with task X whether
  the live UI also opens on Summary (the export will) (owner decision).
