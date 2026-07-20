#!/usr/bin/env python3
"""Post-fill markout curve to 900s, drift buckets, and worst-decile split.

Usage:
    python3 scripts/maker_mo_curve_tail.py ARM.ndjson [ARM.ndjson ...]

Pools passive fills across the given arm logs and prints:
  - the signed markout curve (30/60/120/300/600/900s) with mean/median/neg%;
  - the same curve split by pre-fill signed drift bucket (drift15, bps;
    negative = mark drifted into our quote before the fill);
  - the mo300 worst decile: drift/age/cap profile and its share of total
    mo300 loss mass (how tail-dominated the bleed is).

Answers: does the bleed saturate (holding is free) or keep growing (there is
a tail to cut)? Pure stdlib; reuses maker_markout_ab loaders.
"""

import os
import sys
from statistics import mean, median

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import maker_markout_ab as m  # noqa: E402

H = [30.0, 60.0, 120.0, 300.0, 600.0, 900.0]


def stat_line(rs):
    out = [f"n{len(rs):3d} cap{mean([r['cap'] for r in rs]):+6.2f}"]
    for h in H:
        v = [r["mo"][h] for r in rs if r["mo"].get(h) is not None]
        if not v:
            out.append(f"mo{h:>4.0f}s   n/a  ")
            continue
        neg = 100 * sum(1 for x in v if x < 0) / len(v)
        out.append(f"mo{h:>4.0f}s {mean(v):+6.2f}/med{median(v):+6.2f}/neg{neg:3.0f}%")
    return " ".join(out)


def main(paths):
    m.MO_CURVE_HORIZONS = H
    rows = []
    for path in paths:
        cycles, fills, _lp, _pnl, timeline, _ha = m.load_arm(path)
        if not cycles:
            continue
        arows, _, _ = m.attribution_rows(cycles, timeline)
        rows.extend(arows)
    print(f"total passive fills: {len(rows)}")
    print("ALL :", stat_line(rows))

    buckets = [("d<-4 ", lambda d: d is not None and d < -4),
               ("-4..-2", lambda d: d is not None and -4 <= d < -2),
               ("-2..0 ", lambda d: d is not None and -2 <= d < 0),
               ("d>=0  ", lambda d: d is not None and d >= 0)]
    for name, pred in buckets:
        rs = [r for r in rows if pred(r["drift"].get(15))]
        if rs:
            print(f"{name}:", stat_line(rs))

    v = sorted(((r["mo"][300.0], r) for r in rows if r["mo"].get(300.0) is not None),
               key=lambda x: x[0])
    if not v:
        return
    k = max(1, len(v) // 10)
    tail = [r for _, r in v[:k]]
    rest = [r for _, r in v[k:]]

    def dr(rs):
        vals = [r["drift"][15] for r in rs if r["drift"].get(15) is not None]
        return f"{mean(vals):+.2f}" if vals else "n/a"

    def ag(rs):
        vals = [r["age"] for r in rs if r["age"] is not None]
        return f"{median(vals):.0f}s" if vals else "n/a"

    print(f"\nmo300 worst-decile (n={len(tail)}): mo300 mean "
          f"{mean([r['mo'][300.0] for r in tail]):+.2f} drift15 mean {dr(tail)} "
          f"age med {ag(tail)} cap mean {mean([r['cap'] for r in tail]):+.2f}")
    print(f"rest              (n={len(rest)}): mo300 mean "
          f"{mean([r['mo'][300.0] for r in rest]):+.2f} drift15 mean {dr(rest)} "
          f"cap mean {mean([r['cap'] for r in rest]):+.2f}")
    tot = sum(x[0] for x in v)
    tl = sum(x[0] for x in v[:k])
    print(f"mo300 mass in worst decile: {tl:+.1f} / {tot:+.1f} = {100 * tl / tot:.0f}%")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(__doc__)
        raise SystemExit(64)
    main(sys.argv[1:])
