#!/usr/bin/env python3
"""Independent recomputation of the Summary view scores (web/js/score.js), standard library only.

Reads a Comparison as `/api/compare` returns it (default: docs/api-examples/compare.json) and recomputes, per
compared side, the section scores (n, counts, weak), the median CV per section, the overall score of every
built-in profile and of optional custom weights, plus the baseline's median CV and the median noise threshold
per section. The definitions are the ones web/README.md summarises, written here without looking at score.js:
geometric means are taken as products to a power (score.js uses sums of logarithms).

  python3 tools/score_check.py                  # recompute, run web/js/score.js in node on the same file, compare (1e-9)
  benchlens compare group:x86-high group:arm64-high x86-std-r2 --json > c.json
  python3 tools/score_check.py --compare c.json # the same on a whole comparison (docs/api-examples cut long lists)
  python3 tools/score_check.py --json           # print the recomputed numbers as JSON (tests/score uses this)
  options: --compare FILE   --weights cpu:1,disk:3,...   --tol 1e-9
"""
import argparse
import json
import math
import statistics
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SECTIONS = ["cpu", "disk", "mem", "net", "pg", "web"]
PROFILES = {
    "balanced": dict(cpu=1, disk=1, mem=1, net=1, pg=1, web=1),
    "web": dict(cpu=1, disk=0.5, mem=1, net=2, pg=0.5, web=3),
    "db": dict(cpu=1, disk=3, mem=2, net=0.5, pg=3, web=0),
    "compute": dict(cpu=3, disk=0.5, mem=2, net=0, pg=0, web=0),
}


def used_rows(cmp, idx):
    """Rows of one compared side that count: not cross-architecture, base > 0, value > 0 (nulls skipped)."""
    out = []
    for r in cmp["rows"]:
        if not isinstance(r, dict):  # docs/api-examples cut long lists with a "... N more" string
            continue
        if r["other"] != idx or r["cross_arch"] is not False:
            continue
        b, v = r.get("base"), r.get("value")
        if b is None or v is None or not (b > 0) or not (v > 0):
            continue
        out.append(r)
    return out


def ratio(r):
    return r["value"] / r["base"] if r["direction"] == "higher" else r["base"] / r["value"]


def median_or_none(xs):
    return statistics.median(xs) if xs else None


def section_scores(cmp, idx):
    out = {}
    for sec in SECTIONS:
        rows = [r for r in used_rows(cmp, idx) if r["section"] == sec]
        n = len(rows)
        counts = {k: sum(1 for r in rows if r["verdict"] == k) for k in ("better", "worse", "not_clear")}
        score = 100 * math.prod(ratio(r) for r in rows) ** (1 / n) if n else None
        out[sec] = {"score": score, "n": n, "weak": n < 3, "counts": counts}
    return out


def overall(scores, weights):
    live = [(s, weights.get(s, 0)) for s in SECTIONS if weights.get(s, 0) > 0]
    have = [(s, w) for s, w in live if scores[s]["score"] is not None]
    incomplete = [s for s, w in live if scores[s]["score"] is None]
    total = sum(w for _, w in have)
    if not total:
        return {"score": None, "incomplete": incomplete}
    prod = math.prod((scores[s]["score"] / 100) ** w for s, w in have)
    return {"score": 100 * prod ** (1 / total), "incomplete": incomplete}


def side_stability(cmp, idx):
    rows = used_rows(cmp, idx)
    return {s: median_or_none([r["cv_value"] for r in rows if r["section"] == s and r.get("cv_value") is not None])
            for s in SECTIONS}


def base_stability(cmp):
    """Baseline CV: every metric once, when some compared side uses its row for a score."""
    seen = {}
    for i in range(len(cmp["others"])):
        for r in used_rows(cmp, i):
            seen.setdefault(r["id"], r)
    rows = list(seen.values())
    return {s: median_or_none([r["cv_base"] for r in rows if r["section"] == s and r.get("cv_base") is not None])
            for s in SECTIONS}


def median_noise(cmp):
    """Median `noise` per section over the rows the section scores use, every compared side pooled."""
    rows = [r for i in range(len(cmp["others"])) for r in used_rows(cmp, i)]
    return {s: median_or_none([r["noise"] for r in rows if r["section"] == s and r.get("noise") is not None])
            for s in SECTIONS}


def parse_weights(text):
    w = {s: 0.0 for s in SECTIONS}
    for part in text.split(","):
        k, v = part.split(":")
        w[k.strip()] = float(v)
    return w


def recompute(cmp, custom=None):
    profiles = dict(PROFILES)
    if custom:
        profiles["custom"] = custom
    sides = []
    for i, side in enumerate(cmp["others"]):
        sc = section_scores(cmp, i)
        sides.append({
            "label": side["label"],
            "sections": sc,
            "stability": side_stability(cmp, i),
            "overall": {name: overall(sc, w) for name, w in profiles.items()},
        })
    return {"base_stability": base_stability(cmp), "median_noise": median_noise(cmp), "sides": sides}


JS_DUMP = """
import { sectionScores, sideStability, baseStability, medianNoise, overallScore, PROFILES, parseWeights } from %(mod)s;
const cmp = JSON.parse(%(cmp)s);
const custom = %(custom)s;
const profiles = { ...Object.fromEntries(Object.entries(PROFILES).map(([k, v]) => [k, v.weights])) };
if (custom) profiles.custom = parseWeights(custom);
const sides = cmp.others.map((side, i) => {
  const sections = sectionScores(cmp, i);
  return { label: side.label, sections, stability: sideStability(cmp, i),
    overall: Object.fromEntries(Object.entries(profiles).map(([k, w]) => [k, overallScore(sections, w)])) };
});
console.log(JSON.stringify({ base_stability: baseStability(cmp), median_noise: medianNoise(cmp), sides }));
"""


def compare_values(path, a, b, tol, bad):
    """Collect differences between the python (a) and node (b) results."""
    if isinstance(a, dict):
        if not isinstance(b, dict) or set(a) != set(b):
            bad.append(f"{path}: keys differ")
            return
        for k in a:
            compare_values(f"{path}.{k}", a[k], b[k], tol, bad)
    elif isinstance(a, list):
        if not isinstance(b, list) or len(a) != len(b):
            bad.append(f"{path}: lists differ")
            return
        for i, (x, y) in enumerate(zip(a, b)):
            compare_values(f"{path}[{i}]", x, y, tol, bad)
    elif isinstance(a, (int, float)) and not isinstance(a, bool):
        if not isinstance(b, (int, float)) or abs(a - b) > tol * max(1.0, abs(a)):
            bad.append(f"{path}: python {a!r} vs js {b!r}")
    elif a != b:
        bad.append(f"{path}: python {a!r} vs js {b!r}")


def count_numbers(x):
    if isinstance(x, dict):
        return sum(count_numbers(v) for v in x.values())
    if isinstance(x, list):
        return sum(count_numbers(v) for v in x)
    return 1 if isinstance(x, (int, float)) and not isinstance(x, bool) else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--compare", default=str(ROOT / "docs/api-examples/compare.json"))
    ap.add_argument("--weights", help="custom weights, cpu:1,disk:3,...")
    ap.add_argument("--tol", type=float, default=1e-9)
    ap.add_argument("--json", action="store_true", help="print the recomputed numbers and stop")
    args = ap.parse_args()
    raw = Path(args.compare).read_text()
    cmp = json.loads(raw)
    expected = recompute(cmp, parse_weights(args.weights) if args.weights else None)
    if args.json:
        print(json.dumps(expected))
        return 0
    js = JS_DUMP % {
        "mod": json.dumps((ROOT / "web/js/score.js").as_uri()),
        "cmp": json.dumps(raw),
        "custom": json.dumps(args.weights) if args.weights else "null",
    }
    try:
        res = subprocess.run(["node", "--input-type=module", "-e", js], capture_output=True, text=True, check=False)
    except FileNotFoundError:
        print("score_check: node not found; cannot run web/js/score.js", file=sys.stderr)
        return 2
    if res.returncode != 0:
        print("score_check: node failed:\n" + res.stderr, file=sys.stderr)
        return 2
    actual = json.loads(res.stdout)
    bad = []
    compare_values("", expected, actual, args.tol, bad)
    for side in expected["sides"]:
        print(side["label"])
        for sec in SECTIONS:
            s = side["sections"][sec]
            score = "no data" if s["score"] is None else f"{s['score']:.4f}"
            c = s["counts"]
            print(f"  {sec:<5} score {score:>9}  n={s['n']:<3} better {c['better']} worse {c['worse']} within noise {c['not_clear']}")
        print("  overall " + ", ".join(
            f"{k} {('n/a' if v['score'] is None else format(v['score'], '.4f'))}" for k, v in side["overall"].items()))
    if bad:
        print(f"score_check: score.js DISAGREES at {len(bad)} place(s):")
        for line in bad[:20]:
            print("  " + line)
        return 1
    print(f"score_check: score.js agrees with the recomputation ({count_numbers(expected)} numbers, tolerance {args.tol:g})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
