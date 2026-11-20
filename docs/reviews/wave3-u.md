# Wave 3 review: U (kept_files, report.md with the token, split a group by tag)

Branch `m3/u-web-followups` at 7721a23, reviewed against docs/plan.md (F12, F13, F17) and docs/api.md.
Verdict: **ok**, one cosmetic fix.

## What was checked

- **F12 `kept_files`** in `RunSummary`: one query over `files` (no query per run), sorted by name; named
  `kept_files` so it does not clash with `RunDetail.files` (which flattens `RunSummary`). api.md table and
  both api-examples updated; the server test checks it equals RunDetail's names for every run. The Runs
  view drops the IntersectionObserver + RunDetail fetch per row. Additive API change, fine for v0.1.0.
- **F13 report.md**: a plain left click fetches the file through `api.file` (the shared `request`, so the
  Bearer token and the 401 dialog apply) and shows it with `h('pre', {text})`, i.e. `textContent`; nothing
  is parsed as Markdown or HTML, no `innerHTML` anywhere in web/js. Modified clicks keep the browser's
  default (new tab: 401 on a private server, fails closed). Out-of-order answers are dropped (`reportSeq`),
  focus returns to the link on close. `request` returns `res.json()` only for a JSON content type; kept
  `report.md` is served as text, so the dialog gets the raw text.
- **F17 split by tag**: group / key / values come from `/api/runs`; keys exclude `group`, values with `@`
  and groups whose name has `@` are skipped (they cannot be written as a reference). One
  `group:NAME@KEY=VALUE` per value, optional first-as-baseline; sides not in the lists are shown and can be
  unticked; toggling a listed side keeps them (`inPickerOrder`). All DOM through `h()`.
- e2e: kept_files shape, link only when kept and no RunDetail request, dialog by keyboard, private server
  with the token, split.spec.js (keyboard, labels, URL, drop, baseline, 400 px). 38 tests pass after the
  merge.
- docs/api.md: U changed the report.md "known gap" line in the Auth section that S also edited; merged so
  both edits stand (report.md now opens with the token; S's proxy wording kept).

## Fixed in review

- `review(U)`: `web/export/offline.js` had no `api.file`; it now rejects with a 404 `ApiError` "kept files
  such as report.md are not part of a static export". Harmless before (the export has no Runs view).

## Checked by hand on the full POC (round1 + round2, 18 runs)

- Split control: group `x86-high`, tag `round`, values 1 and 2, first as baseline: URL
  `base=group:x86-high@round=1&other=group:x86-high@round=2`, tally 13 / 18 / 36, equal to
  `benchlens compare 'group:x86-high@round=1' 'group:x86-high@round=2'`.
- report.md of a run uploaded to a private server (token, `read_public = false`): the dialog text equals
  the file byte for byte; the same URL without the token is 401.
