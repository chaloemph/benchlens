#!/usr/bin/env python3
"""tools/golden.py VMBENCH_DIR: write tests/golden/compare.json from vmbench's own report.py.

Every pair of fixture folders listed in PAIRS is compared with report.judge(); benchlens must give the same
difference and verdict for every metric (tests/golden.rs). Rerun when a fixture or vmbench's rule changes.
Run with python3 -I from the repo root: python3 -I tools/golden.py ../vmbench
"""
import glob, json, os, sys

vmbench = os.path.abspath(sys.argv[1])
sys.path.insert(0, vmbench)
import report as R  # noqa: E402

PAIRS = [
    ("v0.2.3/x86-high-r2-20261119-145026", "v0.2.3/arm64-high-r2-20261119-074530"),
    ("v0.2.3/x86-std-r2-20261119-103253", "v0.2.3/x86-high-r2-20261119-145026"),
    ("v0.2.1/x86-high-20261119-033546", "v0.2.1/arm64-high-20261118-203447"),
    ("v0.2.1/x86-high-20261119-033546", "v0.2.3/x86-high-r2-20261119-145026"),
    ("v0.2.1/arm64-high-20261118-203447", "v0.2.3/arm64-high-r2-20261119-074530"),
    ("v0.2.3/x86-high-web-20261119-101442", "v0.2.3/arm64-high-web-20261119-030942"),
]
out = []
for b, o in PAIRS:
    rb, ro = R.Run(os.path.join("tests/fixtures", b)), R.Run(os.path.join("tests/fixtures", o))
    mb, mo = {m.id: m for m in rb.metrics}, {m.id: m for m in ro.metrics}
    for ident, m in mb.items():
        if ident in mo:
            diff, verdict = R.judge(m, mo[ident])
            out.append({"base": b.split("/")[1], "other": o.split("/")[1], "id": ident,
                        "median_base": m.median, "median_other": mo[ident].median,
                        "diff": diff, "verdict": verdict})
os.makedirs("tests/golden", exist_ok=True)
with open("tests/golden/compare.json", "w") as f:
    json.dump({"vmbench": open(os.path.join(vmbench, "VERSION")).read().strip() if os.path.exists(os.path.join(vmbench, "VERSION")) else "see git", "rows": out}, f, indent=1)
print(len(out), "rows")
