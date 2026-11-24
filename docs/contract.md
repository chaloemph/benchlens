# What benchlens reads from vmbench

benchlens and vmbench share no code. They agree only on the files of a vmbench result folder and on the
metric table, so either can change on its own as long as this stays true.

## Files

| File | Used for | Rules |
|---|---|---|
| `meta.json` | run id, label, arch, vCPU, RAM, vmbench version, stages, start/end time, pg_profile, web_cpus | must hold `"schema": 1`; any other value refuses the folder |
| `results.tsv` | every raw value: `id metric value unit round` | lines whose value is not a number are skipped, as vmbench's report.py does |
| `status.tsv` | `stage step result` | a stage with a `FAIL` line contributes no values |
| `hardware.json` | CPU model and the hardware table | optional; vmbench writes no serials, UUIDs, host names, IP or MAC addresses |
| `report.md` | kept for reading | never parsed |
| `benchlens-tags.json` | tags for benchlens, `{"key": "value"}` | optional, written by the user, not by vmbench |

Everything else in a folder (logs, raw tool output) is ignored, and is not copied from archives.
`summary.tsv` is not used: it rounds to four significant digits, so percentages would drift from vmbench's.

## Metric table

`vmbench run --metrics` prints one line per metric: `id|unit|direction|tool|label|description`. The tables of
known vmbench versions are built in (`crates/rules/catalog/`). For a newer vmbench:

```bash
vmbench run --metrics > metrics.txt
benchlens catalog add 0.3.0 metrics.txt
```

Runs of a version with no table are imported, but their values are not judged until the table is added.

## Verdict rule

Same as `judge()` in vmbench `report.py`: median and CV (population standard deviation over the mean) of the
values; difference = (other − baseline) / |baseline|; within noise when |difference| ≤ max(1 %, CV of either
side); direction `none` is never judged. `tests/golden/compare.json` holds vmbench's own verdicts for the
fixture pairs and `cargo test` fails if benchlens disagrees on any of them.

To refresh after a vmbench release: add `tests/fixtures/<version>/<run>/` (the four files), list the new pairs
in `tools/golden.py`, then `python3 -I tools/golden.py PATH/TO/vmbench` and commit the result.

## Scores

The Summary view of the web UI shows scores next to the verdicts. **Scores are summaries; verdicts stay
vmbench's.** A score only summarises numbers that `/api/compare` already returns (`rows[].base`, `value`,
`direction`, `cv_*`, `noise`, `cross_arch`, `verdict`); it never changes a verdict, and the verdict rule above
is untouched. The maths is in `web/js/score.js` (pure functions) and is recomputed independently by
`tools/score_check.py`; `node --test tests/score/` fails if the two disagree by more than 1e-9.

**Section score** of one compared side and one section (`cpu`, `disk`, `mem`, `net`, `pg`, `web`): take the
rows of that side and section with `cross_arch == false`, `base > 0` and `value > 0` (nulls are skipped). The
ratio of a row is `r = value / base` when `direction` is `higher` and `r = base / value` when it is `lower`, so
`r > 1` always means better than the baseline. Rows with direction `none` have no ratio and are not used.

```text
section score = 100 x exp(mean(ln r))        (the geometric mean of r, times 100)
```

The baseline is 100 on every axis. The result also carries `n` (rows used), `counts` (`better`, `worse`,
`not_clear` of those rows) and `weak = n < 3` (drawn faded, "few results"). A section with `n == 0` has score
`null` ("no data"), drawn as a gap and never as 0. The geometric mean keeps one metric that is five times off
from dominating, and makes A against B and B against C chain consistently.

**Overall score** under a profile (a weight per section, non-negative): the weighted geometric mean of the
section scores that are not `null`, with the weights renormalised over those sections.

```text
overall = exp( sum(w_s x ln score_s) / sum(w_s) )     over sections s with score_s != null
```

If a section with weight above 0 has no score, the result lists it as `incomplete`. Built-in profiles:

| Profile | cpu | disk | mem | net | pg | web |
|---|---|---|---|---|---|---|
| `balanced` (default) | 1 | 1 | 1 | 1 | 1 | 1 |
| `web` (web app) | 1 | 0.5 | 1 | 2 | 0.5 | 3 |
| `db` (database) | 1 | 3 | 2 | 0.5 | 3 | 0 |
| `compute` (batch) | 3 | 0.5 | 2 | 0 | 0 | 0 |

Custom weights travel in the URL (`profile=custom&w=cpu:1,disk:3,...`); nothing is stored on the server.

**Stability**: per side and section, the median of `cv_value` over the rows used for the section score (nulls
skipped); the baseline uses `cv_base`. In %, lower is better. When every CV of a side is null (measured once)
there is no stability figure.

**Price**: tags on runs. `price` is a positive decimal number with `.` as the decimal point and no thousands
separator (`4500`, `0.096`); anything else is an invalid price (shown, not used). `price_unit` is optional free
text. The price of a side needs every run of the side to carry the same `price` and `price_unit`, otherwise it is
inconsistent and not used. Sides are compared only with an identical unit (the unit of most priced sides, the
baseline's on a tie). **Performance per price** = overall score divided by price, shown as an index with the
baseline (or, without a usable baseline price, the cheapest priced side) at 100. The price block needs at least
two sides with a usable price in the same unit.

**Scaling**: the vCPU count of a side is `nproc` of its group or runs (a side mixing counts is skipped), its
architecture the side's `archs` (mixed sides are skipped). Points are x = vCPU, y = overall score; the dotted
reference is the smallest point of an architecture scaled linearly (`y x vCPU / vCPU0`); efficiency is the score
divided by that reference. The block needs an architecture with at least two sides of different vCPU counts.

## When vmbench changes its format

vmbench must raise `schema` in `meta.json`. benchlens then refuses those folders with a clear message until
it learns the new schema; it never reads them wrongly.
