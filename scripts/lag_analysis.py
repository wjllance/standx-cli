#!/usr/bin/env python3
"""Measure how far StandX's mark price lags Hyperliquid. Read-only, stdlib only.

Usage:
    python3 scripts/lag_analysis.py LAG.ndjson [--field mark|mid|index]
                                    [--grid-ms 250] [--max-lag-ms 4000]
                                    [--event-bps 8] [--event-window-ms 2000]
                                    [--cover-frac 0.5]

Input is the NDJSON produced by `standx lag-recorder`: one price observation per
line, each tagged `source` ("standx" | "hyperliquid") with a common-clock
`local_recv_ms` and price fields (mark/mid/index/last/best_bid/best_ask).

Two independent lag estimates are printed:

1. Cross-correlation of resampled price increments across candidate lags. The
   lag maximizing correlation is the typical co-movement offset (positive =
   StandX lags Hyperliquid).
2. Event response: when Hyperliquid jumps sharply, how long until StandX covers
   a fraction of that move. More robust than (1) because it isolates the moments
   that actually matter (the jumps that snipe stale quotes), and it is directly
   causal (measured from realized price paths).

HONEST CAVEATS (also printed at the end):
- The common local clock carries a FIXED differential-network-latency offset:
  the RTT from the recording host to StandX vs to Hyperliquid differs. The
  VARIABLE part (event-response spread, correlation shape) is robust; the
  ABSOLUTE offset is biased by that differential. Record from the SAME host /
  region as the maker for the number to be representative.
- Resolution floor: Hyperliquid activeAssetCtx (~0.5s/block) and StandX update
  cadence bound the smallest trustworthy lag. Treat sub-0.5s point estimates as
  "below resolution", not zero.
- Single window, single symbol. Re-record per symbol.
"""
import argparse
import bisect
import json
from statistics import mean, median


def load(path, field):
    """Return (standx, hyper): each a sorted list of (t_ms, price)."""
    series = {"standx": [], "hyperliquid": []}
    with open(path) as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except Exception:
                continue
            source = rec.get("source")
            if source not in series:
                continue
            price = rec.get(field)
            if price is None:
                continue
            t = rec.get("local_recv_ms")
            if t is None:
                continue
            series[source].append((int(t), float(price)))
    for source in series:
        series[source].sort(key=lambda row: row[0])
    return series["standx"], series["hyperliquid"]


def resample(series, grid_ms, t0, t1):
    """Forward-fill a (t_ms, price) series onto a fixed grid. Returns list of
    price (or None before the first sample)."""
    grid = list(range(t0, t1 + 1, grid_ms))
    times = [row[0] for row in series]
    out = []
    for g in grid:
        i = bisect.bisect_right(times, g) - 1
        out.append(series[i][1] if i >= 0 else None)
    return grid, out


def increments(prices):
    """First differences; None where either endpoint is missing."""
    out = []
    for a, b in zip(prices, prices[1:]):
        out.append(b - a if (a is not None and b is not None) else None)
    return out


def pearson(xs, ys):
    pairs = [(x, y) for x, y in zip(xs, ys) if x is not None and y is not None]
    if len(pairs) < 3:
        return None, len(pairs)
    mx = mean(p[0] for p in pairs)
    my = mean(p[1] for p in pairs)
    sxy = sum((p[0] - mx) * (p[1] - my) for p in pairs)
    sxx = sum((p[0] - mx) ** 2 for p in pairs)
    syy = sum((p[1] - my) ** 2 for p in pairs)
    if sxx <= 0 or syy <= 0:
        return None, len(pairs)
    return sxy / (sxx**0.5 * syy**0.5), len(pairs)


def cross_correlation(standx, hyper, grid_ms, max_lag_ms):
    """Scan integer lags; StandX increments vs Hyperliquid increments shifted by
    `lag` grid steps. Positive lag = StandX lags Hyperliquid."""
    t0 = max(standx[0][0], hyper[0][0])
    t1 = min(standx[-1][0], hyper[-1][0])
    if t1 - t0 < grid_ms * 4:
        return None
    _, sx = resample(standx, grid_ms, t0, t1)
    _, hy = resample(hyper, grid_ms, t0, t1)
    sx_incr = increments(sx)
    hy_incr = increments(hy)
    max_steps = max(1, max_lag_ms // grid_ms)
    best = None
    curve = []
    for lag in range(-max_steps, max_steps + 1):
        # StandX increment at t vs Hyperliquid increment at t-lag.
        if lag >= 0:
            xs = sx_incr[lag:]
            ys = hy_incr[: len(hy_incr) - lag] if lag > 0 else hy_incr
        else:
            xs = sx_incr[: len(sx_incr) + lag]
            ys = hy_incr[-lag:]
        r, n = pearson(xs, ys)
        if r is None:
            continue
        curve.append((lag * grid_ms, r, n))
        if best is None or r > best[1]:
            best = (lag * grid_ms, r, n)
    return best, curve


def event_response(standx, hyper, event_bps, window_ms, cover_frac):
    """For each sharp Hyperliquid jump, measure ms until StandX covers
    `cover_frac` of the move in the same direction."""
    st_times = [row[0] for row in standx]
    st_px = [row[1] for row in standx]

    def standx_at(t):
        i = bisect.bisect_right(st_times, t) - 1
        return st_px[i] if i >= 0 else None

    responses = []
    already = 0  # StandX had already moved by event time
    never = 0    # StandX never covered within a generous follow window
    follow_ms = max(window_ms * 4, 8000)

    n = len(hyper)
    j = 0
    for i in range(n):
        t0, p0 = hyper[i]
        # find the first later sample within window that constitutes a jump
        k = i + 1
        while k < n and hyper[k][0] - t0 <= window_ms:
            t1, p1 = hyper[k]
            move_bps = (p1 / p0 - 1.0) * 1e4
            if abs(move_bps) >= event_bps:
                direction = 1.0 if p1 > p0 else -1.0
                target = p0 + cover_frac * (p1 - p0)
                s0 = standx_at(t0)
                if s0 is not None and direction * (s0 - target) >= 0:
                    already += 1
                else:
                    # find first StandX sample at/after t0 that reaches target
                    idx = bisect.bisect_left(st_times, t0)
                    hit = None
                    while idx < len(standx) and st_times[idx] - t0 <= follow_ms:
                        if direction * (st_px[idx] - target) >= 0:
                            hit = st_times[idx] - t0
                            break
                        idx += 1
                    if hit is None:
                        never += 1
                    else:
                        responses.append(hit)
                break
            k += 1
        j = i
    _ = j
    return responses, already, never


def quantile(xs, q):
    if not xs:
        return None
    s = sorted(xs)
    pos = q * (len(s) - 1)
    lo = int(pos)
    hi = min(lo + 1, len(s) - 1)
    return s[lo] + (s[hi] - s[lo]) * (pos - lo)


def main():
    ap = argparse.ArgumentParser(description="Measure StandX price lag vs Hyperliquid.")
    ap.add_argument("path")
    ap.add_argument("--field", default="mark", choices=["mark", "mid", "index"])
    ap.add_argument("--grid-ms", type=int, default=250)
    ap.add_argument("--max-lag-ms", type=int, default=4000)
    ap.add_argument("--event-bps", type=float, default=8.0)
    ap.add_argument("--event-window-ms", type=int, default=2000)
    ap.add_argument("--cover-frac", type=float, default=0.5)
    args = ap.parse_args()

    standx, hyper = load(args.path, args.field)
    print(f"loaded: standx={len(standx)} hyperliquid={len(hyper)} samples "
          f"(field={args.field})")
    if len(standx) < 5 or len(hyper) < 5:
        print("not enough samples on both sources; record a longer window.")
        return
    span_s = (min(standx[-1][0], hyper[-1][0]) - max(standx[0][0], hyper[0][0])) / 1000.0
    print(f"overlapping span: {span_s:.1f}s")

    print("\n== 1. Cross-correlation of price increments ==")
    xc = cross_correlation(standx, hyper, args.grid_ms, args.max_lag_ms)
    if xc is None or xc[0] is None:
        print("  insufficient overlap for cross-correlation.")
    else:
        best, curve = xc
        lag_ms, r, n = best
        sign = "StandX LAGS Hyperliquid" if lag_ms > 0 else (
            "StandX LEADS Hyperliquid" if lag_ms < 0 else "no offset")
        print(f"  peak correlation r={r:.3f} at lag={lag_ms:+d}ms ({sign}), n={n}")
        # show the correlation near the peak for shape
        near = [c for c in curve if abs(c[0] - lag_ms) <= 3 * args.grid_ms]
        print("  nearby: " + ", ".join(f"{c[0]:+d}ms:{c[1]:.2f}" for c in near))
        if abs(lag_ms) < 500:
            print("  NOTE: |lag| < 500ms is below the feed-cadence resolution floor;"
                  " treat as 'no resolvable lead'.")

    print("\n== 2. Event response (Hyperliquid jumps -> StandX follow time) ==")
    responses, already, never = event_response(
        standx, hyper, args.event_bps, args.event_window_ms, args.cover_frac)
    total = len(responses) + already + never
    print(f"  jumps >= {args.event_bps:g}bps within {args.event_window_ms}ms: {total}")
    if total:
        print(f"    StandX already ahead at jump: {already}")
        print(f"    StandX never covered {args.cover_frac:.0%} within follow window: {never}")
    if responses:
        print(f"    follow-time to cover {args.cover_frac:.0%} of the move "
              f"(n={len(responses)}):")
        print(f"      median={median(responses):.0f}ms  "
              f"p25={quantile(responses,0.25):.0f}ms  "
              f"p75={quantile(responses,0.75):.0f}ms  "
              f"mean={mean(responses):.0f}ms")
    else:
        print("    no measurable follow events (need a longer / more volatile window).")

    print("\n== Caveats ==")
    print("  - Absolute lag carries a fixed differential-network-latency bias "
          "(host->StandX vs host->Hyperliquid). Variable part is robust; run on "
          "the maker's host/region.")
    print("  - Resolution floor set by feed cadence (~0.5s). Sub-0.5s estimates "
          "are 'below resolution', not zero.")
    print("  - Single window / single symbol; re-record per symbol.")


if __name__ == "__main__":
    main()
