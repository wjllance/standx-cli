#!/usr/bin/env python3
"""Pre-fill creep replay: is predictive cancel worth building?

Usage:
    python3 scripts/maker_creep_replay.py ARM.ndjson [ARM.ndjson ...]
        [--window 30]        # seconds of pre-fill book replay (default 30)
        [--reach-bps 2.0]    # "price reached us" threshold in bps (default 2)

For every passive fill, replays the opposite-side best (best_ask for our
buys, best_bid for our sells) distance to our quote over the pre-fill window
from cycle_summary book snapshots (~3s cadence), and buckets fills by the
"sustained lead": how long before the hit the opposite best stayed within
--reach-bps of our quote.

  no-warning (<3s)   — the hit teleported from outside the threshold;
  brief (3-8s)       — a short approach;
  sustained (>=8s)   — a visible creep a predictive cancel could act on.

Then joins each bucket with post-fill markout (30s/300s) and asks where the
toxic tail sits: if the worst fills are no-warning, predictive cancel cannot
see them and is dead; if they creep in, price what cancelling them saves.

Pure stdlib. Prints bucket shares, distance trajectories, markout by bucket,
and the warnable share of the mo300 worst decile.
"""

import argparse
import bisect
import json
from datetime import datetime, timezone
from statistics import mean, median


def parse_ts(s):
    return datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()


def load(path):
    cycles, fills = [], []
    for line in open(path):
        try:
            d = json.loads(line)
        except Exception:
            continue
        a = d.get("action")
        if a == "cycle_summary":
            bb, ba = d.get("best_bid"), d.get("best_ask")
            if bb is None or ba is None:
                continue
            cycles.append((parse_ts(d["ts"]), float(bb), float(ba), float(d["mark"])))
        elif a == "fill" and d.get("role") == "passive_maker":
            fills.append((parse_ts(d["ts"]), d["side"], float(d["price"])))
    return cycles, fills


def replay(cycles, fills, window, reach):
    ts_list = [c[0] for c in cycles]
    recs = []
    for t, side, price in fills:
        i0 = bisect.bisect_left(ts_list, t)
        i_start = bisect.bisect_left(ts_list, t - window)
        if i0 - i_start < 2:
            continue
        dist = []
        for c in cycles[i_start:i0]:
            d = ((c[2] - price) if side == "buy" else (price - c[1])) / price * 1e4
            dist.append((c[0], d))
        if len(dist) < 2:
            continue
        lead = 0.0
        for k in range(len(dist)):
            if all(x[1] <= reach for x in dist[k:]):
                lead = t - dist[k][0]
                break
        d_at = {}
        for w in (5, 10, 20):
            j = bisect.bisect_right(ts_list, t - w) - 1
            if i_start <= j < i0:
                c = cycles[j]
                d_at[w] = ((c[2] - price) if side == "buy" else (price - c[1])) / price * 1e4
        mo = {}
        if i0 < len(cycles):
            mark0 = cycles[i0][3]
            sign = 1.0 if side == "buy" else -1.0
            for h in (30.0, 300.0):
                j = bisect.bisect_left(ts_list, t + h)
                mo[h] = (cycles[j][3] - mark0) * sign / mark0 * 1e4 if j < len(cycles) else None
        recs.append(dict(side=side, lead=lead, d_last=dist[-1][1], d_at=d_at, mo=mo))
    return recs


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("arms", nargs="+")
    ap.add_argument("--window", type=float, default=30.0)
    ap.add_argument("--reach-bps", type=float, default=2.0)
    args = ap.parse_args()

    records = []
    for path in args.arms:
        cycles, fills = load(path)
        if cycles:
            records.extend(replay(cycles, fills, args.window, args.reach_bps))

    n = len(records)
    print(f"fills with pre-fill book replay: {n}")
    if not n:
        return
    no_warn = [r for r in records if r["lead"] < 3.0]
    brief = [r for r in records if 3.0 <= r["lead"] < 8.0]
    creep = [r for r in records if r["lead"] >= 8.0]
    for name, rs in (("no-warning (lead<3s)", no_warn), ("brief (3-8s)", brief),
                     ("sustained creep (>=8s)", creep)):
        extra = f" lead med {median([r['lead'] for r in rs]):.0f}s" if name.startswith("sust") and rs else ""
        print(f"{name:24s}: {len(rs):4d} ({100 * len(rs) / n:.0f}%)  "
              f"d_last med {median([r['d_last'] for r in rs]):+.1f}bps{extra}")

    print("\ndistance trajectory (median bps before fill), all fills:")
    for w in (20, 10, 5):
        v = [r["d_at"][w] for r in records if w in r["d_at"]]
        print(f"  t-{w:2d}s: {median(v):+.1f}bps (mean {mean(v):+.1f})")
    print(f"  last cycle pre-fill: {median([r['d_last'] for r in records]):+.1f}bps "
          f"(mean {mean([r['d_last'] for r in records]):+.1f})")

    print("\nmarkout by warning class:")
    tot300 = 0.0
    shares = {}
    for name, rs in (("no-warning", no_warn), ("brief", brief), ("creep", creep)):
        v30 = [r["mo"][30.0] for r in rs if r["mo"].get(30.0) is not None]
        v300 = [r["mo"][300.0] for r in rs if r["mo"].get(300.0) is not None]
        shares[name] = sum(v300)
        tot300 += sum(v300)
        print(f"{name:12s}: mo30 mean{mean(v30):+7.2f} med{median(v30):+7.2f} | "
              f"mo300 mean{mean(v300):+7.2f} med{median(v300):+7.2f}")
    v = sorted(((r["mo"][300.0], r) for r in records if r["mo"].get(300.0) is not None),
               key=lambda x: x[0])
    k = max(1, len(v) // 10)
    tail = v[:k]
    tw = sum(1 for _, r in tail if r["lead"] < 3.0)
    warn_mass = sum(x[0] for x in tail if x[1]["lead"] >= 3.0)
    print(f"\nmo300 worst decile n={k}: no-warning {tw} ({100 * tw / k:.0f}%), "
          f"warnable {k - tw} ({100 * (k - tw) / k:.0f}%)")
    print(f"worst-decile mo300 mass: {sum(x[0] for x in tail):+.1f}; "
          f"warnable part: {warn_mass:+.1f}")


if __name__ == "__main__":
    main()
