//! Pure quoting & reconcile logic for market making (SIP-5A maker yield).
//!
//! No I/O in this module: every function takes plain values and returns
//! decisions, so the whole strategy is unit-testable without a network.
//!
//! The core idea is an **anti-flicker** loop: SIP-5A rewards uptime (orders
//! resting on the book inside an eligibility band around mark price) and
//! penalizes flicker-cancels. So resting quotes are HELD as long as they stay
//! inside the band; re-quoting happens only when mark price drifts more than
//! `refresh_bps` from the mark recorded when the order was placed.
//!
//! Numeric representation: prices/quantities are `f64` internally and
//! formatted to the symbol's tick decimals only at the API edge
//! ([`format_decimals`]). This matches the rest of the codebase (which does
//! ad-hoc f64 math on the API's string values); if symbols with more than ~8
//! price decimals ever list, revisit with a decimal type.

use crate::models::OrderSide;

/// Static per-run configuration (CLI args + symbol metadata).
#[derive(Debug, Clone)]
pub struct MakerConfig {
    /// Half-spread from mark price, in basis points, for level 0.
    pub spread_bps: f64,
    /// Eligibility band: never quote outside `mark * (1 ± band_bps/1e4)`.
    pub band_bps: f64,
    /// Spacing between quote levels, in basis points.
    pub level_step_bps: f64,
    /// Anti-flicker threshold: re-quote only when mark has drifted more than
    /// this (bps) from the mark recorded at placement time.
    pub refresh_bps: f64,
    /// Number of quote levels per side.
    pub levels: u32,
    /// Per-side, per-level order quantity.
    pub size: f64,
    /// Max absolute position; the side that would grow it further is
    /// suppressed once exceeded.
    pub max_position: f64,
    /// Price precision (decimal places) from `SymbolInfo.price_tick_decimals`.
    pub price_decimals: u32,
    /// Quantity precision (decimal places) from `SymbolInfo.qty_tick_decimals`.
    pub qty_decimals: u32,
    /// Minimum order quantity from `SymbolInfo.min_order_qty`.
    pub min_order_qty: f64,
}

impl MakerConfig {
    /// One price tick: `10^-price_decimals`.
    pub fn price_tick(&self) -> f64 {
        10f64.powi(-(self.price_decimals as i32))
    }
}

/// A quote we want resting on the book (prices/qtys already tick-rounded).
#[derive(Debug, Clone, PartialEq)]
pub struct DesiredQuote {
    pub side: OrderSide,
    pub level: u32,
    pub price: f64,
    pub qty: f64,
}

/// A quote currently resting (a real order in live mode, simulated in paper
/// mode).
#[derive(Debug, Clone)]
pub struct RestingQuote {
    /// Exchange order id (None in paper mode / before adoption).
    pub order_id: Option<String>,
    pub side: OrderSide,
    pub level: u32,
    pub price: f64,
    pub qty: f64,
    /// Mark price when this quote was placed — the anti-flicker anchor.
    pub ref_mark: f64,
    pub placed_at_cycle: u64,
}

/// Why a resting quote is being cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelReason {
    /// Mark drifted more than `refresh_bps` from the quote's `ref_mark`.
    MarkMovedBeyondRefresh,
    /// The resting price left the eligibility band (earns nothing there).
    OutsideBand,
    /// The resting price now crosses the touch (would fill as taker).
    WouldCross,
    /// The quote's side is suppressed by the max-position limit.
    SideSuppressed,
    /// No desired quote exists at this (side, level) anymore.
    Stale,
}

impl CancelReason {
    /// Snake-case label for machine-readable output.
    pub fn as_str(&self) -> &'static str {
        match self {
            CancelReason::MarkMovedBeyondRefresh => "mark_moved",
            CancelReason::OutsideBand => "outside_band",
            CancelReason::WouldCross => "would_cross",
            CancelReason::SideSuppressed => "side_suppressed",
            CancelReason::Stale => "stale",
        }
    }
}

/// One reconcile decision.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Place(DesiredQuote),
    Cancel {
        order_id: Option<String>,
        side: OrderSide,
        level: u32,
        price: f64,
        reason: CancelReason,
    },
    Hold {
        side: OrderSide,
        level: u32,
        price: f64,
        age_cycles: u64,
        /// Current drift from the quote's ref_mark, in bps (for display).
        drift_bps: f64,
    },
}

/// Round half-up to `decimals` decimal places.
pub fn round_to_decimals(value: f64, decimals: u32) -> f64 {
    let factor = 10f64.powi(decimals as i32);
    (value * factor).round() / factor
}

/// Round DOWN to `decimals` decimal places (used for buy prices).
pub fn floor_to_decimals(value: f64, decimals: u32) -> f64 {
    let factor = 10f64.powi(decimals as i32);
    // Nudge by a hair to avoid f64 representation artifacts like
    // 99.90 * 100 = 9989.999999... flooring to 99.89.
    ((value * factor) + 1e-9).floor() / factor
}

/// Round UP to `decimals` decimal places (used for sell prices).
pub fn ceil_to_decimals(value: f64, decimals: u32) -> f64 {
    let factor = 10f64.powi(decimals as i32);
    ((value * factor) - 1e-9).ceil() / factor
}

/// Format for API strings with exactly `decimals` decimal places.
pub fn format_decimals(value: f64, decimals: u32) -> String {
    format!("{:.*}", decimals as usize, value)
}

/// Absolute difference between `a` and `b` in basis points of `b`.
/// Returns 0.0 when `b` is 0 (avoids division blowup on degenerate input).
pub fn bps_diff(a: f64, b: f64) -> f64 {
    if b == 0.0 {
        return 0.0;
    }
    ((a - b) / b).abs() * 10_000.0
}

/// Divergence between mark price and the book mid, in bps of mark.
///
/// A large value means the two data sources disagree (stale feed, bad print,
/// or a dislocated book) — quotes anchored to mark would sit nonsensically
/// relative to the book, so callers should skip acting on such a snapshot.
pub fn mark_mid_divergence_bps(mark: f64, best_bid: f64, best_ask: f64) -> f64 {
    bps_diff((best_bid + best_ask) / 2.0, mark)
}

/// Compute the desired quote set for the current market snapshot.
///
/// Applies, in order: the spread/level ladder, the band clamp, the no-cross
/// clamp, directional tick rounding (with band re-entry), the min-qty filter,
/// and max-position side suppression. Quotes that fail a guard are dropped;
/// duplicate prices after clamping/rounding are collapsed (outer level wins
/// nothing — the inner level is kept).
pub fn compute_desired_quotes(
    cfg: &MakerConfig,
    mark: f64,
    best_bid: Option<f64>,
    best_ask: Option<f64>,
    position: f64,
) -> Vec<DesiredQuote> {
    let mut out = Vec::new();
    if mark <= 0.0 {
        return out;
    }

    let qty = round_to_decimals(cfg.size, cfg.qty_decimals);
    if qty < cfg.min_order_qty || qty <= 0.0 {
        return out;
    }

    let tick = cfg.price_tick();
    let band_lo = mark * (1.0 - cfg.band_bps / 1e4);
    let band_hi = mark * (1.0 + cfg.band_bps / 1e4);

    let suppress_buy = position >= cfg.max_position;
    let suppress_sell = position <= -cfg.max_position;

    for side in [OrderSide::Buy, OrderSide::Sell] {
        if (side == OrderSide::Buy && suppress_buy) || (side == OrderSide::Sell && suppress_sell) {
            continue;
        }
        let mut last_price: Option<f64> = None;
        for level in 0..cfg.levels {
            let offset_bps = cfg.spread_bps + level as f64 * cfg.level_step_bps;
            let mut price = match side {
                OrderSide::Buy => mark * (1.0 - offset_bps / 1e4),
                OrderSide::Sell => mark * (1.0 + offset_bps / 1e4),
            };

            // Band clamp: quoting outside the band earns nothing, so clamp
            // back to the edge (still eligible).
            price = price.clamp(band_lo, band_hi);

            // No-cross clamp: without relying solely on ALO rejection, never
            // price through the touch. One tick of safety margin.
            match side {
                OrderSide::Buy => {
                    if let Some(ask) = best_ask {
                        price = price.min(ask - tick);
                    }
                }
                OrderSide::Sell => {
                    if let Some(bid) = best_bid {
                        price = price.max(bid + tick);
                    }
                }
            }

            // Directional tick rounding: away from mark, so rounding never
            // pushes us through the touch.
            price = match side {
                OrderSide::Buy => floor_to_decimals(price, cfg.price_decimals),
                OrderSide::Sell => ceil_to_decimals(price, cfg.price_decimals),
            };

            // Rounding may have pushed the price just outside the band —
            // nudge one tick back toward mark.
            if price < band_lo {
                price = floor_to_decimals(price + tick, cfg.price_decimals);
            } else if price > band_hi {
                price = ceil_to_decimals(price - tick, cfg.price_decimals);
            }

            if price <= 0.0 {
                continue;
            }

            // Collapse duplicate levels (clamping can flatten the ladder).
            if last_price == Some(price) {
                continue;
            }
            last_price = Some(price);

            out.push(DesiredQuote {
                side,
                level,
                price,
                qty,
            });
        }
    }

    out
}

/// Diff desired vs resting quotes, applying the anti-flicker hold rule.
///
/// Decision table per resting quote (checked in order):
///
/// | # | Condition                                        | Action                        |
/// |---|--------------------------------------------------|-------------------------------|
/// | 1 | side suppressed by max-position                  | Cancel (SideSuppressed)       |
/// | 2 | no desired quote at (side, level)                | Cancel (Stale)                |
/// | 3 | price outside current band                       | Cancel (OutsideBand)          |
/// | 4 | price crosses current touch                      | Cancel (WouldCross)           |
/// | 5 | mark drifted > refresh_bps from ref_mark         | Cancel (MarkMovedBeyondRefresh) |
/// | 6 | otherwise                                        | Hold (anti-flicker)           |
///
/// Every desired quote without a surviving resting counterpart yields a
/// `Place`. The returned Vec orders all Cancels before all Places so the
/// executor frees margin before re-placing; Holds come last.
pub fn reconcile(
    cfg: &MakerConfig,
    mark: f64,
    best_bid: Option<f64>,
    best_ask: Option<f64>,
    desired: &[DesiredQuote],
    resting: &[RestingQuote],
    cycle: u64,
) -> Vec<Action> {
    let band_lo = mark * (1.0 - cfg.band_bps / 1e4);
    let band_hi = mark * (1.0 + cfg.band_bps / 1e4);

    let desired_has = |side: OrderSide, level: u32| -> bool {
        desired.iter().any(|d| d.side == side && d.level == level)
    };
    // A side with zero desired quotes this cycle is suppressed (either by
    // max-position or because every quote failed a guard).
    let side_live = |side: OrderSide| -> bool { desired.iter().any(|d| d.side == side) };

    let mut cancels = Vec::new();
    let mut holds = Vec::new();
    // (side, level) pairs covered by a surviving (held) resting quote.
    let mut covered: Vec<(OrderSide, u32)> = Vec::new();

    for r in resting {
        let reason = if !side_live(r.side) {
            Some(CancelReason::SideSuppressed)
        } else if !desired_has(r.side, r.level) {
            Some(CancelReason::Stale)
        } else if r.price < band_lo || r.price > band_hi {
            Some(CancelReason::OutsideBand)
        } else if match r.side {
            OrderSide::Buy => best_ask.map(|a| r.price >= a).unwrap_or(false),
            OrderSide::Sell => best_bid.map(|b| r.price <= b).unwrap_or(false),
        } {
            Some(CancelReason::WouldCross)
        } else if bps_diff(mark, r.ref_mark) > cfg.refresh_bps {
            Some(CancelReason::MarkMovedBeyondRefresh)
        } else {
            None
        };

        match reason {
            Some(reason) => cancels.push(Action::Cancel {
                order_id: r.order_id.clone(),
                side: r.side,
                level: r.level,
                price: r.price,
                reason,
            }),
            None => {
                covered.push((r.side, r.level));
                holds.push(Action::Hold {
                    side: r.side,
                    level: r.level,
                    price: r.price,
                    age_cycles: cycle.saturating_sub(r.placed_at_cycle),
                    drift_bps: bps_diff(mark, r.ref_mark),
                });
            }
        }
    }

    let places: Vec<Action> = desired
        .iter()
        .filter(|d| !covered.contains(&(d.side, d.level)))
        .map(|d| Action::Place(d.clone()))
        .collect();

    // Cancels first (free margin), then places, then holds (display only).
    let mut actions = cancels;
    actions.extend(places);
    actions.extend(holds);
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    /// mark=100-friendly config: 2 price decimals, 4 qty decimals.
    fn cfg() -> MakerConfig {
        MakerConfig {
            spread_bps: 10.0,
            band_bps: 20.0,
            level_step_bps: 2.0,
            refresh_bps: 3.0,
            levels: 1,
            size: 0.01,
            max_position: 0.05,
            price_decimals: 2,
            qty_decimals: 4,
            min_order_qty: 0.001,
        }
    }

    fn resting(side: OrderSide, level: u32, price: f64, ref_mark: f64) -> RestingQuote {
        RestingQuote {
            order_id: Some("1".into()),
            side,
            level,
            price,
            qty: 0.01,
            ref_mark,
            placed_at_cycle: 0,
        }
    }

    fn find(quotes: &[DesiredQuote], side: OrderSide, level: u32) -> &DesiredQuote {
        quotes
            .iter()
            .find(|q| q.side == side && q.level == level)
            .expect("quote missing")
    }

    // 1. Basic two-sided quoting.
    #[test]
    fn basic_two_sided() {
        let quotes = compute_desired_quotes(&cfg(), 100.0, Some(99.99), Some(100.01), 0.0);
        assert_eq!(quotes.len(), 2);
        assert_eq!(find(&quotes, OrderSide::Buy, 0).price, 99.90);
        assert_eq!(find(&quotes, OrderSide::Sell, 0).price, 100.10);
        assert_eq!(find(&quotes, OrderSide::Buy, 0).qty, 0.01);
    }

    // 2. Spread wider than band: clamp to band edges, not dropped.
    #[test]
    fn band_clamp() {
        let mut c = cfg();
        c.spread_bps = 30.0; // > band 20
        let quotes = compute_desired_quotes(&c, 100.0, Some(99.5), Some(100.5), 0.0);
        assert_eq!(find(&quotes, OrderSide::Buy, 0).price, 99.80);
        assert_eq!(find(&quotes, OrderSide::Sell, 0).price, 100.20);
    }

    // 3. Directional tick rounding: buy floors, sell ceils.
    #[test]
    fn tick_rounding_directional() {
        let mut c = cfg();
        c.price_decimals = 1;
        c.spread_bps = 5.0;
        // mark=100.03: raw buy = 99.979985 -> floor(1dp) 99.9
        //              raw sell = 100.080015 -> ceil(1dp) 100.1
        let quotes = compute_desired_quotes(&c, 100.03, None, None, 0.0);
        assert_eq!(find(&quotes, OrderSide::Buy, 0).price, 99.9);
        assert_eq!(find(&quotes, OrderSide::Sell, 0).price, 100.1);
        assert_eq!(format_decimals(99.9, 1), "99.9");
    }

    // 4. Rounding that exits the band is nudged back inside.
    #[test]
    fn rounding_reenters_band() {
        let mut c = cfg();
        c.price_decimals = 0; // whole-number ticks
        c.spread_bps = 20.0; // == band: raw buy exactly at band edge 99.8
        c.band_bps = 20.0;
        // raw buy = 99.8, floor(0dp) = 99 < band_lo 99.8 -> nudge +1 tick = 100
        // (floor(99.8+1) = 100)... still >= band_lo, inside band.
        let quotes = compute_desired_quotes(&c, 100.0, None, None, 0.0);
        let buy = find(&quotes, OrderSide::Buy, 0);
        assert!(buy.price >= 99.8, "price {} left the band", buy.price);
        assert_eq!(buy.price, 100.0);
    }

    // 5. No-cross clamp on both sides.
    #[test]
    fn no_cross_clamp() {
        // Best ask (99.85) sits BELOW our raw buy (99.90): buy must clamp
        // down to ask - tick.
        let quotes = compute_desired_quotes(&cfg(), 100.0, Some(99.83), Some(99.85), 0.0);
        let buy = find(&quotes, OrderSide::Buy, 0);
        let sell = find(&quotes, OrderSide::Sell, 0);
        // buy clamped to ask - tick = 99.84
        assert_eq!(buy.price, 99.84);
        // sell raw 100.10 already > bid + tick; unchanged
        assert_eq!(sell.price, 100.10);

        // Symmetric: bid above our raw sell forces sell up to bid + tick.
        let quotes = compute_desired_quotes(&cfg(), 100.0, Some(100.15), Some(100.20), 0.0);
        let sell = find(&quotes, OrderSide::Sell, 0);
        assert_eq!(sell.price, 100.16);
    }

    // 6. Size below min_order_qty -> no quotes at all.
    #[test]
    fn min_qty_rejection() {
        let mut c = cfg();
        c.size = 0.00001; // rounds to 0.0000 at 4dp -> below min 0.001
        let quotes = compute_desired_quotes(&c, 100.0, None, None, 0.0);
        assert!(quotes.is_empty());
    }

    // 7. Max-position suppression, both directions.
    #[test]
    fn max_position_suppresses_buy() {
        let quotes = compute_desired_quotes(&cfg(), 100.0, None, None, 0.05);
        assert!(quotes.iter().all(|q| q.side == OrderSide::Sell));
        assert_eq!(quotes.len(), 1);
    }

    #[test]
    fn max_position_suppresses_sell() {
        let quotes = compute_desired_quotes(&cfg(), 100.0, None, None, -0.05);
        assert!(quotes.iter().all(|q| q.side == OrderSide::Buy));
        assert_eq!(quotes.len(), 1);
    }

    // 8. Anti-flicker: drift within refresh threshold -> Hold.
    #[test]
    fn reconcile_hold_within_refresh() {
        let c = cfg();
        let mark = 100.02; // 2 bps from ref 100.0, refresh = 3
        let desired = compute_desired_quotes(&c, mark, None, None, 0.0);
        let rest = vec![
            resting(OrderSide::Buy, 0, 99.90, 100.0),
            resting(OrderSide::Sell, 0, 100.10, 100.0),
        ];
        let actions = reconcile(&c, mark, None, None, &desired, &rest, 7);
        assert!(
            actions.iter().all(|a| matches!(a, Action::Hold { .. })),
            "{actions:?}"
        );
        assert_eq!(actions.len(), 2);
        if let Action::Hold { age_cycles, .. } = &actions[0] {
            assert_eq!(*age_cycles, 7);
        }
    }

    // 9. Drift beyond refresh -> Cancel(mark_moved) + Place, cancel first.
    #[test]
    fn reconcile_requote_beyond_refresh() {
        let c = cfg();
        let mark = 100.05; // 5 bps > refresh 3
        let desired = compute_desired_quotes(&c, mark, None, None, 0.0);
        let rest = vec![resting(OrderSide::Buy, 0, 99.90, 100.0)];
        let actions = reconcile(&c, mark, None, None, &desired, &rest, 1);
        // Expect: cancel(buy, mark_moved), then places for buy+sell.
        assert!(matches!(
            actions[0],
            Action::Cancel {
                reason: CancelReason::MarkMovedBeyondRefresh,
                ..
            }
        ));
        let cancel_idx = 0;
        let place_idx = actions
            .iter()
            .position(|a| matches!(a, Action::Place(_)))
            .unwrap();
        assert!(cancel_idx < place_idx);
    }

    // 10. Outside band takes precedence over refresh drift.
    #[test]
    fn reconcile_cancel_outside_band_precedence() {
        let c = cfg();
        // Mark gapped 30 bps: resting buy at 99.90 with ref 100.0 is now
        // outside band [100.10, 100.50] around mark 100.30.
        let mark = 100.30;
        let desired = compute_desired_quotes(&c, mark, None, None, 0.0);
        let rest = vec![resting(OrderSide::Buy, 0, 99.90, 100.0)];
        let actions = reconcile(&c, mark, None, None, &desired, &rest, 1);
        assert!(
            matches!(
                actions[0],
                Action::Cancel {
                    reason: CancelReason::OutsideBand,
                    ..
                }
            ),
            "{actions:?}"
        );
    }

    // 11. Touch moved through a resting quote -> WouldCross.
    #[test]
    fn reconcile_cancel_would_cross() {
        let c = cfg();
        let mark = 100.01; // tiny drift, within refresh
        let desired = compute_desired_quotes(&c, mark, Some(100.12), Some(100.14), 0.0);
        // Resting sell at 100.10 now BELOW best bid 100.12 -> crossed.
        let rest = vec![resting(OrderSide::Sell, 0, 100.10, 100.0)];
        let actions = reconcile(&c, mark, Some(100.12), Some(100.14), &desired, &rest, 1);
        assert!(
            matches!(
                actions[0],
                Action::Cancel {
                    reason: CancelReason::WouldCross,
                    ..
                }
            ),
            "{actions:?}"
        );
    }

    // 12. Level removed from config -> Stale.
    #[test]
    fn reconcile_stale_level() {
        let c = cfg(); // levels = 1 -> only level 0 desired
        let mark = 100.0;
        let desired = compute_desired_quotes(&c, mark, None, None, 0.0);
        let rest = vec![
            resting(OrderSide::Buy, 0, 99.90, 100.0),
            resting(OrderSide::Buy, 1, 99.88, 100.0), // stale level
        ];
        let actions = reconcile(&c, mark, None, None, &desired, &rest, 1);
        let stale: Vec<_> = actions
            .iter()
            .filter(|a| {
                matches!(
                    a,
                    Action::Cancel {
                        reason: CancelReason::Stale,
                        level: 1,
                        ..
                    }
                )
            })
            .collect();
        assert_eq!(stale.len(), 1, "{actions:?}");
    }

    // 13. Multi-level ladder + duplicate collapse.
    #[test]
    fn multi_level_ladder() {
        let mut c = cfg();
        c.levels = 3;
        let quotes = compute_desired_quotes(&c, 100.0, None, None, 0.0);
        // Buys descending: 99.90, 99.88, 99.86; sells ascending mirrored.
        assert_eq!(find(&quotes, OrderSide::Buy, 0).price, 99.90);
        assert_eq!(find(&quotes, OrderSide::Buy, 1).price, 99.88);
        assert_eq!(find(&quotes, OrderSide::Buy, 2).price, 99.86);
        assert_eq!(find(&quotes, OrderSide::Sell, 2).price, 100.14);
        assert_eq!(quotes.len(), 6);

        // Ladder flattened by band: spread 18, step 2, band 20 -> levels 1+
        // clamp to the band edge and duplicates collapse.
        let mut c2 = cfg();
        c2.levels = 3;
        c2.spread_bps = 18.0;
        let quotes = compute_desired_quotes(&c2, 100.0, None, None, 0.0);
        let buys: Vec<_> = quotes.iter().filter(|q| q.side == OrderSide::Buy).collect();
        assert_eq!(buys.len(), 2, "{buys:?}"); // 99.82, then 99.80 (L1) and L2 dup dropped
        assert_eq!(buys[1].price, 99.80);
    }

    // 14. Helper edge cases.
    #[test]
    fn helper_edge_cases() {
        assert_eq!(bps_diff(100.0, 0.0), 0.0);
        assert!((bps_diff(100.05, 100.0) - 5.0).abs() < 1e-9); // ~5 bps
        assert!((bps_diff(99.95, 100.0) - 5.0).abs() < 1e-9); // symmetric
        assert_eq!(round_to_decimals(1.23456, 0), 1.0);
        assert_eq!(round_to_decimals(-1.235, 2), -1.24);
        assert_eq!(floor_to_decimals(99.90, 2), 99.90); // representation artifact guard
        assert_eq!(ceil_to_decimals(100.10, 2), 100.10);
        assert_eq!(format_decimals(99.9, 2), "99.90");
        assert_eq!(format_decimals(0.0123, 4), "0.0123");
    }

    // 15. Mark/mid divergence guard helper.
    #[test]
    fn mark_mid_divergence() {
        // mid = 100.0 == mark -> no divergence
        assert_eq!(mark_mid_divergence_bps(100.0, 99.9, 100.1), 0.0);
        // mid = 100.25 vs mark 100.0 -> 25 bps
        assert!((mark_mid_divergence_bps(100.0, 100.2, 100.3) - 25.0).abs() < 1e-9);
        // symmetric below
        assert!((mark_mid_divergence_bps(100.0, 99.7, 99.8) - 25.0).abs() < 1e-9);
        // degenerate mark = 0 -> 0.0, no blowup
        assert_eq!(mark_mid_divergence_bps(0.0, 99.9, 100.1), 0.0);
    }
}
