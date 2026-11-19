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

## When vmbench changes its format

vmbench must raise `schema` in `meta.json`. benchlens then refuses those folders with a clear message until
it learns the new schema; it never reads them wrongly.
