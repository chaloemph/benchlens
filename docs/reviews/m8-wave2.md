# Review: M8 Wave 2, task X (export, docs, release prep for 0.2.0)

Reviewer: L. Integration branch `integration/m8-wave2`, from main 0f2def8 (Waves 0 and 1 merged through pull
requests #14 and #15), then `m8/x-export-docs` merged with `--no-ff` (b53a315). The branch has two task
commits:

| Commit | What |
|---|---|
| 5a1889f | the static export offers Summary first and opens on it; `export/export.css` inlined once (F30); offline export test and e2e export spec extended |
| ca4daaa | guide section "Summary view" (with the F34 note), contract section "Scores", walkthrough step and screenshot `docs/img/summary.png`, README line, CHANGELOG `[Unreleased]`, workspace version 0.2.0 (no date) |

plus six `review(...)` commits (listed under "Fixes"). X stayed in its files: `web/export/offline.js`,
`crates/server/src/export.rs` (the stylesheet loop and two tests, no API or data change), `tests/export/**`,
`tests/e2e/tests/export.spec.js`, `docs/**`, `README.md`, `CHANGELOG.md`, `Cargo.toml`, `Cargo.lock`. Verdict:
**accepted with fixes**.

## Checklist

- **No change in `crates/rules`, `compare.rs`, WASM, golden files, API shapes: yes.** `git diff main` touches
  only `crates/server/src/export.rs` in `crates/` (the stylesheet inlining), nothing in `web/wasm/` or
  `tests/golden/`; `scripts/test-wasm.sh` matches 336 verdicts in 6 pairs. The committed WASM still reports
  `benchlens-wasm 0.1.4`; it is rebuilt by the release commit, as for every earlier release (see the checklist).
- **All score maths in `score.js`: yes.** X adds no maths to the page. The offline export test imports
  `web/js/score.js` itself (`weightsFor`, `scoresFor`) to know what the exported page must show.
- **Scores labelled as summaries, verdict counts next to each score: yes**, unchanged from Wave 1; the export
  runs the same modules.
- **Hidden blocks: yes.** The offline test checks, in the export, that every hidden slot is empty and has one
  note, and that an export of the same sides without prices hides the price block with its note.
- **`textContent` only: yes.** `offline.js` changed one array; the hostile-label export run still passes.
- **Tests use fixtures; prices named "example": yes.** `tests/export/run.sh` tags the fixture runs of a
  scratch database with example prices (`benchlens tag`, comment says so); the docs call every price an example
  price; `examples/poc-2026-11/registry.tsv` is untouched (`git diff main` is empty for `examples/`).
- **Suites**: all green on the integration branch, the browser suite included (see "Checks").

## Export (part 1 of X)

- **Summary first and opened on load: yes.** `offline.js` passes `views: ['summary', 'compare', 'metric',
  'hardware']`; with no `view` in the link the shared shell opens the default view, which is Summary since F32.
  The menu shows Summary first and current.
- **Offline and equal to the live view: yes.** Two independent checks:
  - `tests/export/offline.mjs` (run by `tests/export/run.sh`, network off, only the file itself may load):
    six slots in order, leaderboard names, order, scores and counts against score.js on the CLI comparison,
    the verdicts table against the CLI tallies, the price table against the example prices, for the exported
    link, `profile=db`, custom weights, a single-run baseline against a group, an export of the same sides
    without prices (price block hidden, its note shown) and the hostile-label export. Fixed: the score
    comparison allowed up to 0.55 off for scores away from 100; the page writes one decimal, so the test now
    wants exactly that.
  - `tests/e2e/tests/export.spec.js`: downloads the file from the Export button, opens it in an offline
    context, and compares the leaderboard rows, verdicts table, hidden slots and notes with the live Summary
    of the same sides.
  - By hand for this review: `benchlens export html` of x86-high against arm64-high and x86-std (with the
    example prices of the guide) opened from disk in Chromium; all six blocks drawn, 81.6 / 55.4 for Balanced,
    110.0 / 67.8 for `?profile=db`, 77.1 / 43.6 for `?profile=compute` after the file name, the numbers of
    `tools/score_check.py`.
- **F30 fixed properly: yes.** `export.css` is appended once, to the last stylesheet of `index.html` (so it
  still comes after `css/summary.css` and may override it); a Rust test checks it appears once and after
  summary.css, and that the Summary modules are bundled. Fixed: an `index.html` without a stylesheet now
  stops the export with an error instead of leaving `export.css` out silently, and the e2e check counts the
  whole of `web/export/export.css` instead of one copied rule.

## Docs (part 2 of X)

- **Contract "Scores" against `web/js/score.js`.** Section score, ratio per direction, `n` / `counts` /
  `weak`, null for `n == 0`, weighted geometric mean with renormalised weights, `incomplete`, the four
  profiles and their weights (identical to `PROFILES`), stability (median `cv_value`, baseline `cv_base`),
  price parsing and consistency, unit choice, the index (baseline, else cheapest = 100), the two-price rule,
  scaling points and efficiency: all as the code does it. The sentence "Scores are summaries; verdicts stay
  vmbench's." is there. Fixed (the code's behaviour, now written down): direction `none` has no row at all
  (the text said such rows are "not used"); the overall score is `null` when no weighted section has a
  score; weights missing from `w` are 0 and unreadable or all-zero custom weights fall back to balanced; a
  side with no `price` tag has no price and sides in another unit are listed; vCPU comes from the runs and only
  then from the group; the baseline is a scaling point at 100.
- **Guide.** The block table and each block's text match the code's hiding rules: price / performance hides
  with fewer than two usable prices (fixed: "in the same unit" added to the table, as the block and the
  contract say), scaling hides unless one architecture has two different vCPU counts (baseline included); the
  other four always show; the hidden-note example is the block's exact text. The `price` / `price_unit` rules
  match `sidePrice` / `pricePerformance`. The F34 note (counts in the radar and stability tooltips vs the
  verdicts block) is there and correct.
- **Commands re-run for this review** (0.2.0 release build of this branch, scratch databases):
  - the price-tag session of the guide (`import ... --tag price=1300 --tag price_unit=THB/month`, `runs --json`,
    `tag x86-std-r2 price=1,200`): output identical to the guide;
  - a copy of the POC registry with `price` / `price_unit` columns: the seven runs get 4500 / 3900 / 1200
    THB/month as the guide says;
  - `PATCH /api/runs/{id}/tags` with the guide's body and the upload with `-F tag=price=1100`: tags set,
    `already imported; tags updated`;
  - `benchlens compare group:x86-high group:arm64-high group:x86-std --json` and `python3 tools/score_check.py
    --compare`: 74.6844 for arm64-high cpu, overall 81.5716 / 63.7664 / 110.0445 / 77.0989 and 55.3578 / ...,
    89 numbers agree; tallies 23 / 37 / 5 and 7 / 34 / 26. Walkthrough figures (81.6, 55.4, 110) match.
  - `benchlens export html` of the guide and of the walkthrough: **the pasted sizes were 0.1.4's** (752 KB,
    about 230 KB gzipped, 786 KB); the Summary modules add about 100 KB. Fixed with the real output of this
    branch (849 KB, about 255 KB gzipped, 883 KB). They are re-pasted again from the release binary (checklist).
- **Example prices: yes, named as such** in the guide, walkthrough caption, README line and the export test.
  Fixed: both screenshot captions said the prices were set "through the registry on the POC data", which reads
  as the committed registry; they now say a copy of it and link the guide's new "Price tags" heading.
- **Screenshot**: `docs/img/summary.png`, 900 px wide, leaderboard and radar of the POC data (as decided: two
  blocks, the same size as `compare.png`). Not broken; kept.
- **`web/README.md`** (owner's decision d): "The last blocks are built one by one" removed; it now says which
  two blocks hide themselves and that the export opens on Summary; `summary/common.js` and
  `export/export.css` added to the file list.

## CHANGELOG and version (part 3 of X)

`[Unreleased]` has, under Added, the Summary view with its six blocks, profiles and custom weights (`profile`,
`w`), the `price` / `price_unit` tags, the hiding rule, scores as summaries with counts, table fallbacks; the
export opening on Summary offline with profiles in the link; the docs. Under Changed: F32, bookmarks without
`view` open Summary and UI links to Compare carry `view=compare`. Under Fixed: F30. Complete for M8. Workspace
version 0.2.0 in `Cargo.toml` and `Cargo.lock` (all five crates); no date, as asked.

Left for the release commit (owner's decision a): the `0.1.4` in the README status line, the README
`VERSION=` install snippet, the guide intro, and every pasted `benchlens 0.1.4 on ...` / `{"benchlens":"0.1.4"}`
output in the guide and walkthrough.

## Fixes (`review(...)` commits)

- 699f3cd `review(x)`: the export refuses an `index.html` without a stylesheet instead of dropping
  `export.css`; the e2e check reads `web/export/export.css`.
- 13eacb8 `review(x)`: the offline export test compares scores exactly as the page writes them (one decimal).
- 6589aea `review(x)`: contract "Scores" covers score.js's edge cases (above).
- 2f1a8f4 `review(x)`: guide and walkthrough: screenshot captions (copy of the registry, link to "Price
  tags"), the same-unit rule in the block table, export sizes re-run.
- 3270c84 `review(w2)`: web/README.md (decision d).
- dee787a `review(x)`: `tests/export/run.sh` keeps the example prices in one place (`prices.json`).

## Checks (on `integration/m8-wave2`)

| Suite | Result |
|---|---|
| `cargo fmt --check` | pass |
| `cargo clippy --all-targets -- -D warnings` | pass |
| `cargo test --all` | pass (146 tests, 0 failures) |
| `scripts/test-wasm.sh` | pass (336 verdicts in 6 pairs match) |
| `node --test tests/score/` (with `CI=1`) | pass (46 tests, 0 skipped) |
| `python3 tools/score_check.py` | agrees (56 numbers); on the three-side fixture comparison 89 numbers |
| `scripts/e2e.sh` (whole suite, release build of this branch) | **pass: 88 browser and API tests, then `tests/export/run.sh` ok** (Summary checked in the export: 5 blocks shown with example prices, 4 without; scaling hidden for two 16 vCPU sides, as it should). Chromium with its NSS libraries unpacked into a scratch folder. `tests/export/run.sh` re-run alone after dee787a: ok |

## Follow-ups

| ID | Follow-up | Where | Status |
|---|---|---|---|
| F30 | The export inlined `web/export/export.css` after every stylesheet | `crates/server/src/export.rs` | **closed**: inlined once (5a1889f, 699f3cd) |
| F33 | The stability sentence names the noisiest side-and-section cell; the task row says "the side with the highest median CV overall". A per-side figure would be new score.js maths (low, owner decision) | `score.js`, `stability.js` | open, after the release |
| F34 | Radar / stability tooltip counts vs `tallies[].by_section`: say so in the guide | `docs/guide.md` | **closed**: guide, "Counts in the tooltips and in the verdicts block" (ca4daaa) |
| F35 | Only the scaling chart redraws on width change; price, stability and verdicts scale with the viewBox. Shared resize helper (low) | `web/js/summary/` | open, after the release |
| F36 | Links accept any non-negative weight (`parseWeights`), inputs only 0 to 5 in steps of 0.5 (`checkWeights`); clamp links or widen inputs (low) | `score.js`, `board.js` | open, after the release |
| F37 | Only the board restores keyboard focus after a re-render; restore focus by id in `summary/index.js` for every block (low) | `web/js/summary/index.js` | open, after the release |
| F38 | No Export button in the Summary view: the button lives in Compare, so a reader on Summary goes to Compare to download the report (which then opens on Summary). Add the button to the Summary view (owner's decision c: not in 0.2.0) (low) | `web/index.html`, `web/js/summary/index.js` | open, after the release |
| F39 | In an export, a link to a view it does not carry (`?view=runs`) falls back to Compare (`main.js`), while a link without `view` opens Summary; falling back to the first offered view would be consistent (low) | `web/js/main.js` | open |

## Release commit checklist (0.2.0)

Not in this pull request; for the commit that releases 0.2.0 after it is merged:

1. CHANGELOG: `## [Unreleased]` becomes `## [0.2.0] - <date>` (and a new empty `[Unreleased]` above it).
2. README: the status line (`Status: v0.1.4 ...`) and the install snippet `VERSION=0.1.4`.
3. Guide intro ("does in v0.1.4").
4. Re-paste every pasted output from the 0.2.0 binary: `benchlens --version`, `benchlens 0.1.4 on http://...`
   start-up lines and `{"benchlens":"0.1.4",...}` health answers in the guide and walkthrough, and the export
   sizes (`wrote report.html (... KB)`, "about ... KB gzipped", `wrote poc-high.html (... KB)`); regenerate
   `docs/api-examples/health.json` (still `"benchlens": "0.1.4"`).
5. Rebuild `web/wasm` with `scripts/build-wasm.sh` (it still reports `benchlens-wasm 0.1.4`), as the 0.1.x
   release commits did.
6. Tag `v0.2.0` (the release workflow builds the binaries, the Docker image and the release notes from the
   CHANGELOG section).
