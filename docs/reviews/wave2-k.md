# Wave 2 review: K (group subset refs, ambiguous refs, exact exclude, one-metric values)

Branch `m2/k-core-refs` at cd4c80a, reviewed against docs/plan.md (F1, F2, F7, F10), docs/api.md and
docs/contract.md. Verdict: **ok, fixed in review** (two UI/CLI follow-ons of F1).

## What was checked

- F1 `group:NAME@KEY=VALUE`: `Reference::parse` is the one reader (core `resolve`, server `check_ref`). Exactly
  one `@`, KEY by the tag key rule, VALUE non-empty, exact (no trim, no case folding), no second `@`. Lookup
  `runs_in_group_tagged` is one bound-parameter query on `v_runs` joined with `tags`; empty selection is 404
  with a message that says whether the group or the tag value is missing. Label `NAME · KEY=VALUE`.
  `@` and `=` in query strings work raw and percent-encoded (server test).
- F2: `Error::Ambiguous` and `Error::BadReference` map to 400; `NotFound` stays 404. api.md updated.
- F7: `v_values` uses `instr(',' || list || ',', ',' || stage || ',') = 0`; DB_VERSION 2. Opening a layout 1
  file drops the three views (constant names, dependents first) and recreates them in one transaction; tables
  untouched. **Checked on a real layout 1 file**: the full POC (round1 + round2, 18 runs) imported with the
  Wave 1 binary (store.rs identical to 403f30b), then opened with the integrated build: `user_version` 1 → 2,
  `v_values` now uses `instr`, 1704 rows before and after, `compare group:x86-high group:arm64-high --json`
  byte-identical. A newer layout is still refused.
- F10: `Store::metric_values` adds `AND (?2 IS NULL OR id = ?2)` to the same query; the test compares it with
  the filtered full side for every metric.
- Verdict rule: untouched. `group:x86-high@round=1` vs `@round=2` on the POC gives 13 / 18 / 36 (CLI, API, UI).
- `web/js/data.js` (outside the brief, item 8): `sideInfo` learns subset refs and the "Round 1 vs 2" quick pick
  pools each round. F1 says "then switch B's quick picks to it", so this is in scope; the code only reads tags
  already in `/api/runs` and builds refs with `@round=` only when names hold no `@`. Kept.

## Fixed in review

- `review(K)`: the CLI compare table cut both value-column heads to 12 characters, so subset labels became
  identical ("x86-high · r"). Heads now widen to the label (12..32 characters, cut with `…`); labels still alike
  become "baseline" / "other". Unit test.
- `review(K)`: after a round quick pick the baseline drop-down showed `group:x86-high@round=1 (from the link)`;
  it now names the side like the listed ones (`x86-high · round=1 (2 runs · x86_64)`). e2e test for the quick
  pick (tally equals the API, picker label).
- `review(D)` (cross-task): the export looked up the group of a side by stripping `group:`, so subset sides had
  no `GroupSummary`; it now uses `Reference::parse`.

## Notes and follow-ups

- The compare picker has no checkbox for subset sides: they come from quick picks or links only. Fine for now;
  a "split by tag" control would be a UI feature (plan follow-up).
- A group whose name contains `@` cannot be referred to as a group (documented).
