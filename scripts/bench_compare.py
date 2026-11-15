#!/usr/bin/env python3
"""Compare criterion results in target/criterion against a saved baseline copy.
Usage: bench_compare.py BASELINE_DIR [--gate]   (fails on > 10 % slower mean only with --gate)."""
import json, pathlib, sys

base, gate = pathlib.Path(sys.argv[1]), "--gate" in sys.argv
cur = pathlib.Path("target/criterion")
bad = 0
for f in sorted(cur.glob("*/new/estimates.json")):
    name = f.parent.parent.name
    b = base / name / "new" / "estimates.json"
    if not b.exists():
        print(f"{name}: no baseline"); continue
    new = json.load(open(f))["mean"]["point_estimate"]
    old = json.load(open(b))["mean"]["point_estimate"]
    pct = (new - old) / old * 100
    flag = " REGRESSION" if pct > 10 else ""
    print(f"{name}: {old:.0f} -> {new:.0f} ns ({pct:+.1f}%){flag}")
    bad += pct > 10
if bad and gate:
    sys.exit(1)
