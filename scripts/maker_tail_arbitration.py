#!/usr/bin/env python3
"""Stage-3 arbitration: what mechanism would actually cut the markout tail?

Usage:
    python3 scripts/maker_tail_arbitration.py ARM.ndjson [ARM.ndjson ...]
        [--val-last N]          # last N files form the validation split (default 0)
        [--tail-frac 0.10]      # worst fraction of fills by mo300 = "tail"
        [--episode-gap 300]     # seconds between fills that starts a new episode
        [--taker-fee-bps 4.0]   # taker fee assumption for exit cost
        [--fallback-half-spread-bps 4.0]  # used when touch is missing at trigger

Reads maker arm NDJSON logs (cycle_summary + fill events, passive_maker only)
and answers the three questions that decide the Stage-3 v0 scope:

1. POSITION AT TAIL FILLS — was inventory already loaded when the big losses
   hit? High |position| at tail fills => size skew (reduce add-side qty) is on
   target. Tail losses on the first fill of an episode (|position| ~ 0) =>
   size skew cannot help; only a drawdown-triggered exit can.
2. HALT OVERLAP — do the tail windows coincide with volatility halts? The
   current policy suppresses taker exits while halted (stage 5-b open item), so
   if exit value concentrates inside halt windows, that policy decision is a
   prerequisite, not a footnote.
3. CONDITIONAL-EXIT PRICING — sweep (trigger drawdown T, window W, exit
   fraction F): value per fill = F * (mo_trigger - cost - mo600), i.e. bleed
   avoided from trigger to 600s minus taker cost. Train on the earlier files,
   validate on --val-last files.

HONEST LIMITS (printed): per-fill simulation double-counts overlapping
inventory inside one episode — read the per-episode aggregates, not the raw
per-fill sum; the tail is a handful of independent episodes, so treat train
numbers as upper bounds and expect the out-of-sample capture to be roughly
half; fills within 600s of the arm end are censored out.
"""
import argparse
import bisect
import json
from datetime import datetime
from statistics import median

MO_TAIL_HORIZON = 300.0  # tail ranking horizon (matches prior analyses)
MO_EXIT_HORIZON = 600.0  # holding horizon the exit rule is priced against
FOLLOW_HALT_WINDOW = 600.0

TRIGGER_BPS = [5.0, 8.0, 12.0, 20.0]
WINDOW_S = [60.0, 120.0, 300.0]
EXIT_FRAC = [0.5, 1.0]


def parse_ts(s):
    return datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()


def load_arm(path):
    """Return (cycles, fills). cycles: sorted (t, mark, position, halted,
    half_spread_bps|None). fills: sorted passive-maker (t, sign, price, qty)."""
    cycles, fills = [], []
    with open(path) as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            try:
                d = json.loads(line)
            except Exception:
                continue
            action = d.get("action")
            if action == "cycle_summary":
                try:
                    t = parse_ts(d["ts"])
                    mark = float(d["mark"])
                except Exception:
                    continue
                half = None
                bid, ask = d.get("best_bid"), d.get("best_ask")
                if isinstance(bid, (int, float)) and isinstance(ask, (int, float)) and ask > bid > 0:
                    half = (ask - bid) / 2.0 / ((ask + bid) / 2.0) * 1e4
                position = d.get("position")
                position = float(position) if isinstance(position, (int, float)) else None
                cycles.append((t, mark, position, bool(d.get("halted")), half))
            elif action == "fill" and d.get("role") == "passive_maker":
                try:
                    fills.append((
                        parse_ts(d["ts"]),
                        1.0 if d["side"] == "buy" else -1.0,
                        float(d["price"]),
                        float(d["qty"]),
                    ))
                except Exception:
                    continue
    cycles.sort(key=lambda c: c[0])
    fills.sort(key=lambda f: f[0])
    return cycles, fills


class Arm:
    def __init__(self, path):
        self.path = path
        self.cycles, self.fills = load_arm(path)
        self.ts = [c[0] for c in self.cycles]
        self.marks = [c[1] for c in self.cycles]

    def mark_at_or_after(self, t):
        i = bisect.bisect_left(self.ts, t)
        return (self.ts[i], self.marks[i], i) if i < len(self.ts) else (None, None, None)

    def position_before(self, t):
        i = bisect.bisect_right(self.ts, t) - 1
        return self.cycles[i][2] if i >= 0 else None

    def halted_in(self, t0, t1):
        """(any_halt, seconds_to_first_halt|None) for cycles in [t0, t1]."""
        i = bisect.bisect_left(self.ts, t0)
        while i < len(self.ts) and self.ts[i] <= t1:
            if self.cycles[i][3]:
                return True, self.ts[i] - t0
            i += 1
        return False, None


def fill_rows(arm, arm_idx):
    """Per passive fill: dict with markouts, position, censoring."""
    rows = []
    for (t, sign, price, qty) in arm.fills:
        t0, m0, i0 = arm.mark_at_or_after(t)
        if m0 is None:
            continue
        row = {
            "arm": arm_idx, "t": t, "sign": sign, "price": price, "qty": qty,
            "m0": m0, "i0": i0,
            "pos_before": arm.position_before(t),
        }
        for name, h in (("mo300", MO_TAIL_HORIZON), ("mo600", MO_EXIT_HORIZON)):
            _, mh, _ = arm.mark_at_or_after(t + h)
            row[name] = sign * (mh / m0 - 1.0) * 1e4 if mh is not None else None
        rows.append(row)
    return rows


def simulate_exit(arm, row, trig_bps, window_s, taker_fee_bps, fallback_half):
    """First cycle within `window_s` of the fill whose adverse move >= trig_bps.
    Returns (mo_trigger_bps, cost_bps, trigger_halted) or None."""
    t, sign, m0 = row["t"], row["sign"], row["m0"]
    i = row["i0"]
    while i < len(arm.ts) and arm.ts[i] - t <= window_s:
        mo_now = sign * (arm.marks[i] / m0 - 1.0) * 1e4
        if mo_now <= -trig_bps:
            half = arm.cycles[i][4]
            cost = (half if half is not None else fallback_half) + taker_fee_bps
            return mo_now, cost, arm.cycles[i][3]
        i += 1
    return None


def episodes_of(rows, gap_s):
    """Cluster fills into episodes by time gap (per arm)."""
    out = []
    for row in sorted(rows, key=lambda r: (r["arm"], r["t"])):
        if out and row["arm"] == out[-1][-1]["arm"] and row["t"] - out[-1][-1]["t"] <= gap_s:
            out[-1].append(row)
        else:
            out.append([row])
    return out


def quantile(xs, q):
    if not xs:
        return None
    s = sorted(xs)
    pos = q * (len(s) - 1)
    lo = int(pos)
    hi = min(lo + 1, len(s) - 1)
    return s[lo] + (s[hi] - s[lo]) * (pos - lo)


def fmt(x, nd=2):
    return "n/a" if x is None else f"{x:.{nd}f}"


def main():
    ap = argparse.ArgumentParser(description="Stage-3 tail arbitration analysis.")
    ap.add_argument("paths", nargs="+")
    ap.add_argument("--val-last", type=int, default=0)
    ap.add_argument("--tail-frac", type=float, default=0.10)
    ap.add_argument("--episode-gap", type=float, default=300.0)
    ap.add_argument("--taker-fee-bps", type=float, default=4.0)
    ap.add_argument("--fallback-half-spread-bps", type=float, default=4.0)
    args = ap.parse_args()

    arms = [Arm(p) for p in args.paths]
    all_rows, censored = [], 0
    for idx, arm in enumerate(arms):
        for row in fill_rows(arm, idx):
            if row["mo300"] is None or row["mo600"] is None:
                censored += 1
                continue
            all_rows.append(row)
    n = len(all_rows)
    print(f"arms={len(arms)} passive fills usable={n} censored(<{int(MO_EXIT_HORIZON)}s "
          f"from arm end or no mark)={censored}")
    if n < 20:
        print("too few fills for arbitration; aborting.")
        return

    # ---- tail definition: worst tail-frac by mo300 ----
    ranked = sorted(all_rows, key=lambda r: r["mo300"])
    k = max(1, int(n * args.tail_frac))
    tail, rest = ranked[:k], ranked[k:]
    tail_loss = sum(r["mo300"] * r["qty"] for r in tail)
    total_loss = sum(r["mo300"] * r["qty"] for r in all_rows if r["mo300"] < 0)
    print(f"\ntail = worst {args.tail_frac:.0%} by mo300: n={k}, mean mo300 "
          f"{fmt(sum(r['mo300'] for r in tail)/k)}bps, "
          f"share of qty-weighted mo300 loss mass "
          f"{fmt(100*tail_loss/total_loss if total_loss else None, 1)}%")

    # ---- 1. position at tail fills ----
    print("\n== 1. |position| immediately BEFORE the fill ==")
    for label, group in (("tail", tail), ("rest", rest)):
        pos = [abs(r["pos_before"]) for r in group if r["pos_before"] is not None]
        missing = len(group) - len(pos)
        print(f"  {label}: n={len(pos)}"
              + (f" (+{missing} missing position)" if missing else "")
              + f"  median={fmt(quantile(pos, 0.5))}  p75={fmt(quantile(pos, 0.75))}"
              + f"  p90={fmt(quantile(pos, 0.90))}")
    buckets = [(0.0, 0.1), (0.1, 0.3), (0.3, 0.5), (0.5, 99.0)]
    print("  tail loss-mass by |position_before| bucket "
          "(low bucket => first-fill losses, size skew can't reach them):")
    for lo, hi in buckets:
        sub = [r for r in tail
               if r["pos_before"] is not None and lo <= abs(r["pos_before"]) < hi]
        mass = sum(r["mo300"] * r["qty"] for r in sub)
        share = 100 * mass / tail_loss if (tail_loss and sub) else 0.0
        print(f"    [{lo:.1f},{hi:.1f}): n={len(sub)}  loss-share={fmt(share, 1)}%")
    adding = [r for r in tail if r["pos_before"] is not None
              and abs(r["pos_before"]) > 1e-9
              and r["sign"] * r["pos_before"] > 0]
    print(f"  tail fills ADDING to an existing same-direction position: "
          f"{len(adding)}/{k}")

    # ---- 2. halt overlap ----
    print("\n== 2. volatility-halt overlap (current policy suppresses taker "
          "exits while halted) ==")
    halted_n, lead_times = 0, []
    for r in tail:
        any_halt, lead = arms[r["arm"]].halted_in(r["t"], r["t"] + FOLLOW_HALT_WINDOW)
        if any_halt:
            halted_n += 1
            lead_times.append(lead)
    print(f"  tail fills with a halt within +{int(FOLLOW_HALT_WINDOW)}s: "
          f"{halted_n}/{k}")
    if lead_times:
        print(f"  seconds fill->first halt: median={fmt(quantile(lead_times, 0.5), 0)} "
              f"p25={fmt(quantile(lead_times, 0.25), 0)} "
              f"p75={fmt(quantile(lead_times, 0.75), 0)}")

    # ---- 3. conditional-exit pricing sweep ----
    n_train_files = len(arms) - args.val_last
    train_rows = [r for r in all_rows if r["arm"] < n_train_files]
    val_rows = [r for r in all_rows if r["arm"] >= n_train_files]
    print(f"\n== 3. drawdown-triggered exit pricing "
          f"(train files=0..{n_train_files - 1} n={len(train_rows)}, "
          f"val files={n_train_files}..{len(arms) - 1} n={len(val_rows)}) ==")
    if not args.val_last:
        print("  WARNING: no --val-last given; every number below is in-sample.")
    print("  value/fill = F * (mo_trigger - cost - mo600); cost = touch half-"
          f"spread at trigger + {args.taker_fee_bps:g}bps taker fee")

    def sweep(rows):
        out = {}
        for T in TRIGGER_BPS:
            for W in WINDOW_S:
                for F in EXIT_FRAC:
                    total_v, fired, halted_trig = 0.0, 0, 0
                    per_row = []
                    for r in rows:
                        hit = simulate_exit(arms[r["arm"]], r, T, W,
                                            args.taker_fee_bps,
                                            args.fallback_half_spread_bps)
                        v = 0.0
                        if hit is not None:
                            mo_trig, cost, is_halted = hit
                            fired += 1
                            halted_trig += is_halted
                            v = F * (mo_trig - cost - r["mo600"])
                        total_v += v
                        per_row.append((r, v))
                    out[(T, W, F)] = (total_v / len(rows) if rows else 0.0,
                                      fired, halted_trig, per_row)
        return out

    train = sweep(train_rows)
    val = sweep(val_rows) if val_rows else None
    header = (" T(bps) | W(s) |  F  | train v/fill | fired | trig-halted"
              + (" | val v/fill | val fired" if val else ""))
    print("  " + header)
    best_key = max(train, key=lambda key: train[key][0])
    for key in sorted(train, key=lambda key: -train[key][0])[:8]:
        T, W, F = key
        tv, fired, ht, _ = train[key]
        line = (f"  {T:6.0f} | {W:4.0f} | {F:3.1f} | {tv:12.2f} | {fired:5d} | "
                f"{ht:11d}")
        if val:
            vv, vf, _, _ = val[key]
            line += f" | {vv:10.2f} | {vf:9d}"
        if key == best_key:
            line += "   <= train best"
        print(line)

    # effective-n honesty on the train best cell
    _, _, _, per_row = train[best_key]
    episodes = episodes_of([r for r, _ in per_row], args.episode_gap)
    values = {id(r): v for r, v in per_row}
    ep_vals = sorted((sum(values[id(r)] for r in ep) for ep in episodes),
                     reverse=True)
    pos_total = sum(v for v in ep_vals if v > 0)
    cum, top = 0.0, 0
    for v in ep_vals:
        if v <= 0 or cum >= 0.8 * pos_total:
            break
        cum += v
        top += 1
    print(f"\n  train best cell episode structure: episodes={len(episodes)}, "
          f"{top} episodes carry 80% of the positive value"
          + ("  <-- FEWER THAN 15: treat as anecdotal, expect ~half out-of-sample"
         if top < 15 else ""))
    med_ep = median(ep_vals) if ep_vals else None
    print(f"  episode value median={fmt(med_ep)} (bps-per-fill units, summed in-episode)")

    print("\n== caveats ==")
    print("  - per-fill simulation double-counts overlapping inventory inside an "
          "episode; use per-episode aggregates for judgment.")
    print("  - tail estimates rest on few independent trend episodes; train-best "
          "is an upper bound, expect ~half out-of-sample.")
    print("  - exits are priced at cycle marks (~2.5s grid) with touch half-spread "
          "at the trigger cycle; real slippage in a fast market is worse.")
    print("  - fills within 600s of an arm end are censored out entirely.")


if __name__ == "__main__":
    main()
