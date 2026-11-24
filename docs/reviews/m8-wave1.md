# Review: M8 Wave 1, tasks B, R, P, Z, T, D (the six Summary blocks)

Reviewer: L. Integration branch `integration/m8-wave1`, from main 2df99fa (Wave 0 merged through pull request
#14). In order: F32 cherry-picked from `integration/m8-wave0` (f899598, see below), then each task branch merged
with `--no-ff`:

| Task | Branch | Task commit | Merge |
|---|---|---|---|
| B leaderboard + profiles | `m8/b-board` | 60fc57a | ccafd10 |
| R radar | `m8/r-radar` | be9e844 | 92e64dd |
| P price / performance | `m8/p-price` | 8aa7513 | ff55f1a |
| Z scaling | `m8/z-scaling` | 0963396 | 8159585 |
| T stability | `m8/t-stability` | 9d62505 | 7f0c99c |
| D verdicts by section | `m8/d-verdicts` | 2f46ffe | 424ab76 |

plus six `review(...)` commits (listed under "Fixes"). Every task touched only its own `web/js/summary/<x>.js`,
its own block of `web/css/summary.css` and its own `tests/e2e/tests/summary-<x>.spec.js`; the six merges had no
conflict (the CSS blocks are separate). Verdict: **accepted with fixes**. No version bump, no CHANGELOG entry
(task X writes them for 0.2.0).

## Checklist

- **No change in `crates/rules`, `compare.rs`, WASM, golden files, API shapes: yes.** `git diff main` is empty
  for `crates/`, `web/wasm/` and `tests/golden/`; `scripts/test-wasm.sh` matches 336 verdicts in 6 pairs.
- **All score maths in `score.js`: yes, after the review.** Four helpers that computed lived in block files and
  moved: T's `medianNoise`, Z's `efficiency` (now `scalingEfficiency`), P's Pareto frontier (`paretoFrontier`)
  and B's custom-weight check (`checkWeights`, next to `parseWeights`). The blocks now only lay out what
  `ctx.scores` and those functions return; D reads `tallies[].by_section` and computes nothing.
- **Scores labelled as summaries, verdict counts next to each score: yes.** Leaderboard bars, radar points,
  price points, scaling points: "score (summary)" in the label, axis or tooltip, with better / worse / within
  noise in the tooltip, under the bar or in the table. Fixed: the price block took its counts from the rows
  behind the section scores while the leaderboard, scaling and verdicts blocks show `tallies`; it now shows the
  tally too, so a side has one set of counts on the page (on the fixtures both are equal: 65 and 67 rows).
- **Hidden blocks: yes.** P hides with fewer than two usable prices in one unit, Z when no architecture has two
  different vCPU counts; both through `ctx.hide(reason)`, so no heading and no empty box, one note in
  `#sum-hidden-notes`. Fixed: P's note showed literal backticks around `price`.
- **`textContent` only: yes.** No `innerHTML` in `web/js/summary/` or `score.js`; P's test feeds the unit
  `<i>THB</i>/month` and checks it is shown as text; labels, units and prices go through `h()` / d3 `.text()`.
- **Dark mode, 400 px, table fallbacks, colour not the only cue, fixed radar axis order: yes.** Each block has a
  `Show as table` with the chart's numbers and an e2e test at 400 px (no sideways scroll) and in the dark scheme.
  Radar: fixed `cpu, disk, mem, net, pg, web`, dash + marker per side; stability: marker shape per side;
  scaling: dash + marker per architecture, hollow marker for the baseline; price: filled circle on the frontier,
  hollow square off it; verdicts: counts written at the bar ends, worse bars hatched. Fixed: three blocks had
  three palettes (see "Visual consistency").
- **Tests use fixtures; prices only via API and named "example": yes.** P's spec sets `price` / `price_unit`
  through `PATCH /api/runs/{id}/tags`, calls them example prices and puts the tags back in `finally`; the
  config runs one worker, so no other spec sees them.
- **Suites**: all green on the integration branch, the browser suite included (see "Checks").

## Score maths, re-checked

`score.js` itself did not change in the task branches; the moved helpers are covered by new unit tests and
`tools/score_check.py` now also recomputes the median noise threshold (pooled over the rows the section scores
use), so `node --test tests/score/` compares it at 1e-9. On whole fixture comparisons (`benchlens compare ...
--json` of `tests/fixtures`):

| Comparison | Number | Value |
|---|---|---|
| x86-high vs arm64-high | arm64-high overall, balanced / db | 81.5716 / 110.0445 (the order of the leaderboard flips between the two profiles, as task B asks) |
| x86-std vs x86-high, arm64-high | x86-high overall, balanced | 180.6431: scaling from 2 to 16 vCPU = 180.6431 / (100 x 16 / 2) = 22.6 %, the efficiency the scaling table shows (23 %) |
| x86-high vs arm64-high, x86-std-r2 | score_check agrees | 89 numbers (section scores, CVs, median noise, overall of every profile) |
| the same | score rows vs tallies | 65 / 65 (arm64-high) and 67 / 67 (x86-std-r2); every section's counts equal `by_section` |

The price index is checked in P's e2e test with plain arithmetic (score / price against the baseline's), not with
`pricePerformance`, so F31 is closed there.

## Per task

**B, leaderboard + profiles.** Bars sorted by overall score with the baseline at 100, the line at 100, counts
from `tallies`, "incomplete: ..." when the profile needs a missing section, the one-line takeaway, radio
buttons for the four profiles and Custom with six inputs (0 to 5, steps of 0.5, inline error, at least one
above 0), `profile` / `w` in the URL, invalid links reported. Fixed:
- **Keyboard focus was lost** after every profile or weight change: `setState` re-renders every block and the
  board's controls were new elements. The board now remembers the last control focused inside it and focuses
  the control with the same id after a re-render it caused (the radios got ids). The six weight inputs are kept
  as they are while the URL holds the weights they produced, so a value typed in the next input survives. The
  state is set one task after the `change` event: a Tab out of an input fires `change` before the focus lands
  on the next input, and the re-render must see where it landed.
- **The custom-weights e2e test could not pass**: it zeroed the inputs one by one and expected the URL to keep
  the old weights, but each input applies when it loses the focus (cpu:0 with the others still 1 is valid).
  Run in a browser for this review, it failed exactly there. It now expects the last valid weights
  (`cpu:0,disk:0,mem:0,net:0,pg:0,web:1`), checks the typed zeros survive, and a new test walks the radios with
  the arrow keys and Tabs between weights, checking the focus each time.
- `checkWeights` moved to score.js; it returns the offending section, and the board writes the message (it
  used to find the input by matching the start of the message text).
- Scores printed as whole numbers away from 100 (112 here, 112.3 in the price and scaling blocks); now one
  decimal like every block.

**R, radar.** Fixed axis order, axis labels "CPU (31)" with a range when the sides differ, the baseline dashed
at 100, up to four sides with dash + marker, a selector beyond four (first four checked, the fifth disabled
until one is unchecked), radial scale `max(160, ceil(max / 20) x 20)` with rings every 20 or 40, gaps (never a
dip to 0) for null sections, faded "few results" axes, tooltips with counts. Fixed: the selector test needed six
groups and the fixtures have three, so it always skipped; it now compares six runs. Colour now follows the side
(`--sum-c<i>` by side index), so a side keeps its colour when the selector changes.

**P, price / performance.** `sidePrice` / `pricePerformance` from score.js, scatter with the Pareto frontier,
the index list (baseline, or the cheapest side, = 100), the list of sides that cannot be priced (no price,
invalid, inconsistent with the run labels, different unit), the unit on the x axis. Fixed: counts from tallies;
the frontier moved to score.js; the hidden note without backticks. The e2e tests cover hidden with 0 and 1
price, shown with 2, inconsistent prices inside a group, another unit, an invalid price and no price.

**Z, scaling.** `sideVcpu` + the overall score, a line per architecture with a dotted linear reference from its
smallest point, x ticks only at real vCPU counts, the efficiency column, the sides left out (mixed vCPU, mixed
architecture, no score) named. It is the only block that redraws when its slot changes width. Fixed: the
profile was printed as its key (`db`) where the others say "Database"; its own section-name and "incomplete"
copies replaced by the shared ones; `efficiency` moved to score.js; y axis label "Score (summary)" like P.

**T, stability.** Dot chart of median CV per section and side, baseline included, a band up to the median noise
threshold, sides measured once listed and not drawn at 0 (`x86-std-r2` in the fixtures), the noisiest value in
one sentence. Fixed: `medianNoise` moved to score.js (and into `scoresFor`, so the block reads `ctx.scores.noise`
like the CVs) and into the independent recomputation. The sentence names the noisiest side *and section*
rather than a per-side overall figure (F33).

**D, verdicts by section.** Diverging bars from `tallies[].by_section` only, one shared scale, worse hatched,
counts at the bar ends, an "All sections" row from the tally totals, the sysbench note exactly when a
`cross_arch` row exists. Its e2e test asserts every count against `/api/compare` (the numbers of `benchlens
compare --json`). No fix needed beyond the shared heading and counts text.

## Visual consistency (all blocks)

- **Headings**: every block starts with one `h2.sum-title` from `blockTitle()` in the new
  `web/js/summary/common.js`; titles are short noun phrases (Leaderboard, Section scores, Price / performance,
  Scaling with vCPUs, Stability, Verdicts by section). The smoke test checks one heading first in every shown
  block. Subheadings (`h3` in P and D) share one shell rule.
- **Texts**: `common.js` also holds `fmtScore` (one decimal), `countsText`, `incompleteText` and `profileName`;
  section names come from `format.js` everywhere (B and Z had lower-case copies).
- **Colours**: one set of six series tokens `--sum-c0..5` with a dark-mode set, in the shell block of
  `summary.css`, used by the radar, stability and scaling charts (they had three palettes, each with its own
  dark copy). A compared side has the same colour in the radar and the stability chart. Bars and price points
  keep the app tokens (`--accent`, `--good`, `--bad`, `--neutral`, `--hatch`).
- **Spacing**: one margin between blocks and one for the table fallback instead of six different ones.

## F32

Owner's decision: a URL without `view` opens Summary. Commit 4944b8e of `integration/m8-wave0` (not merged with
pull request #14) is cherry-picked as f899598 without conflict: `DEFAULTS.view` is `summary`, links to Compare
carry `view=compare`, the e2e specs that meant Compare say so, and a test checks both. Bookmarks without `view`
now open Summary: task X notes it in the CHANGELOG. The export does not offer Summary yet and still opens on
Compare until task X adds it. The Wave 0 review's F32 line now points here.

## Fixes (`review(...)` commits)

- cab61bf `review(w1)`: `medianNoise`, `scalingEfficiency`, `paretoFrontier`, `checkWeights` into score.js with
  unit tests (`tests/score/helpers.test.mjs`); `scoresFor` carries `noise`; score_check recomputes it;
  web/README.md names the `noise` field.
- 1031b62 `review(b)`: focus and typed weights kept across the re-render; custom-weights test corrected; new
  focus test.
- c36842e `review(p)`: counts from tallies; the hidden note's text.
- 3aee263 `review(w1)`: `common.js` (heading, score format, counts, incomplete, profile name) used by all six.
- 4f29be9 `review(w1)`: shared series colours, spacing, subheadings in `summary.css`; radar colour by side.
- b83e5d5 `review(r)`: the more-than-four-sides test runs on the fixtures.

## Checks (on `integration/m8-wave1`)

| Suite | Result |
|---|---|
| `cargo fmt --check` | pass |
| `cargo clippy --all-targets -- -D warnings` | pass |
| `cargo test --workspace` | pass (every test binary, 0 failures) |
| `scripts/test-wasm.sh` | pass (336 verdicts in 6 pairs match) |
| `node --test tests/score/` (with `CI=1`) | pass (46 tests, 0 skipped) |
| `python3 tools/score_check.py` | agrees (56 numbers); on whole fixture comparisons 89 and 52 numbers |
| `scripts/e2e.sh` (whole suite) | **pass: 86 browser and API tests, then `tests/export/run.sh` ok.** Chromium ran on this machine for the first time in M8 (its NSS libraries unpacked from the distribution packages into a scratch folder, no system change). The first run of the merged branches found the failing custom-weights test above; everything else passed as the tasks wrote it |

## Follow-ups

| ID | Follow-up | Where | Status |
|---|---|---|---|
| F30 | The export inlines `web/export/export.css` after every stylesheet link, so with `css/summary.css` it is in the file twice (harmless); inline it once (low) | `crates/server/src/export.rs` | open, task X |
| F31 | The price / performance index needs an independent check | task P | **closed**: P's e2e recomputes the index with plain arithmetic |
| F32 | A URL without `view` opens Summary | `web/js/state.js`, e2e | **done** (f899598); task X: CHANGELOG line |
| F33 | The stability sentence names the noisiest side-and-section cell; the task row says "the side with the highest median CV overall". If a per-side figure is wanted it is new maths for score.js (median of the CVs of all rows a side's scores use) (low, owner decision) | `score.js`, `stability.js` | open |
| F34 | Counts per section in the radar and stability tooltips are the rows behind the section score; the verdicts block shows `tallies[].by_section`. They differ when a section has rows with a null or non-positive value (equal on the fixtures). Say so in the guide's Summary section (low) | `docs/guide.md` | open, task X |
| F35 | Only the scaling chart redraws when its slot changes width; price, stability and verdicts size their SVG at render and then scale with the viewBox, so text shrinks if the window narrows after loading. A shared resize helper in `web/js/summary/` would cover all (low) | `web/js/summary/` | open |
| F36 | A link accepts any non-negative weight (`parseWeights`), the inputs only 0 to 5 in steps of 0.5 (`checkWeights`): `w=cpu:7` is shown and applied, and becomes an error once the user edits an input. Clamp links or widen the inputs (low) | `score.js`, `board.js` | open |
| F37 | Only the board restores the keyboard focus after a re-render; focus on a radar marker or price point is lost when another change re-renders the view. Restoring focus by id in `summary/index.js` would cover every block (low) | `web/js/summary/index.js` | open |
