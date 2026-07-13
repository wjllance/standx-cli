use super::output::{emit_maker_cycle, log_maker_event};
use super::{
    is_current_run_order, is_maker_order, is_order_rejection, open_qty_adopts, pending_covers_slot,
    position_for_symbol, MakerFill, PendingPlace,
};
use crate::cli::*;
use anyhow::Result;
use standx_maker::{self as maker, MakerConfig, MakerStats, RestingQuote, VolBreaker};
use standx_sdk::account_stream::OrderUpdate;
use standx_sdk::client::order::CreateOrderParams;
use standx_sdk::client::StandXClient;
use standx_sdk::models::{Balance, Order, OrderSide, OrderType, TimeInForce, Trade};
use standx_sdk::order_response::OrderResponseHealth;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::time::Duration;

const MAKER_CLEANUP_VERIFY_DELAY: Duration = Duration::from_millis(500);
const MAKER_CLEANUP_RETRY_DELAY: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, Default)]
struct FillTotals {
    qty: f64,
    notional: f64,
}

/// Current-run fill ledger shared by the account WebSocket and REST polling.
/// Both sources report cumulative information in different shapes, so totals
/// are reconciled per order before mutating position or PnL.
#[derive(Debug)]
pub(super) struct MakerLedger {
    pub(super) expected_position: f64,
    pub(super) maker_order_ids: HashSet<u64>,
    pub(super) exit_order_ids: HashSet<u64>,
    seen_fill_ids: HashSet<u64>,
    accounted: HashMap<u64, FillTotals>,
    rest_seen: HashMap<u64, FillTotals>,
}

impl MakerLedger {
    pub(super) fn new(starting_position: f64) -> Self {
        Self {
            expected_position: starting_position,
            maker_order_ids: HashSet::new(),
            exit_order_ids: HashSet::new(),
            seen_fill_ids: HashSet::new(),
            accounted: HashMap::new(),
            rest_seen: HashMap::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn record_delta(
        &mut self,
        order_id: u64,
        side: OrderSide,
        cumulative: FillTotals,
        mark: f64,
        stats: &mut MakerStats,
        fills: &mut Vec<MakerFill>,
        origin: &'static str,
        trade_id: Option<u64>,
        trade_ts: Option<String>,
    ) -> Result<bool> {
        let previous = self.accounted.get(&order_id).copied().unwrap_or_default();
        let qty = cumulative.qty - previous.qty;
        if qty <= 1e-12 {
            return Ok(false);
        }
        let notional = cumulative.notional - previous.notional;
        let price = notional / qty;
        if !qty.is_finite() || !price.is_finite() || qty <= 0.0 || price <= 0.0 {
            return Err(anyhow::anyhow!(
                "invalid cumulative fill for maker order {order_id}: qty={qty}, price={price}"
            ));
        }
        stats.record_fill(side, price, qty, mark);
        self.expected_position += match side {
            OrderSide::Buy => qty,
            OrderSide::Sell => -qty,
        };
        self.accounted.insert(order_id, cumulative);
        fills.push(MakerFill {
            side,
            price,
            qty,
            trade_id,
            order_id: Some(order_id),
            trade_ts,
            origin,
        });
        Ok(self.exit_order_ids.contains(&order_id))
    }

    pub(super) fn apply_order_update(
        &mut self,
        update: &OrderUpdate,
        symbol: &str,
        run_order_prefix: &str,
        mark: f64,
        stats: &mut MakerStats,
        fills: &mut Vec<MakerFill>,
    ) -> Result<bool> {
        if update.symbol != symbol
            || !update
                .cl_ord_id
                .as_deref()
                .is_some_and(|id| id.starts_with(run_order_prefix))
        {
            return Ok(false);
        }
        self.maker_order_ids.insert(update.order_id);
        if update
            .cl_ord_id
            .as_deref()
            .is_some_and(|id| id.starts_with(&format!("{run_order_prefix}x")))
        {
            self.exit_order_ids.insert(update.order_id);
        }
        let qty = update.fill_qty.parse::<f64>().map_err(|_| {
            anyhow::anyhow!("account order {} has invalid fill_qty", update.order_id)
        })?;
        if qty <= 0.0 {
            return Ok(false);
        }
        let avg = update.fill_avg_price.parse::<f64>().map_err(|_| {
            anyhow::anyhow!(
                "account order {} has invalid fill_avg_price",
                update.order_id
            )
        })?;
        self.record_delta(
            update.order_id,
            update.side,
            FillTotals {
                qty,
                notional: qty * avg,
            },
            mark,
            stats,
            fills,
            "current_run_ws_order",
            None,
            Some(update.updated_at.clone()),
        )
    }
}

#[derive(Debug)]
pub(super) struct PositionReconciliationError {
    pub(super) expected: f64,
    pub(super) observed: f64,
}

impl fmt::Display for PositionReconciliationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "expected position {:+.8}, venue reported {:+.8}",
            self.expected, self.observed
        )
    }
}

impl std::error::Error for PositionReconciliationError {}

fn quote_client_order_id(
    run_order_prefix: &str,
    cycle: u64,
    side: OrderSide,
    level: u32,
) -> String {
    let side_code = match side {
        OrderSide::Buy => 'b',
        OrderSide::Sell => 's',
    };
    format!(
        "{run_order_prefix}q{:08x}{side_code}{level:x}",
        cycle as u32
    )
}

fn exit_client_order_id(run_order_prefix: &str, cycle: u64) -> String {
    format!("{run_order_prefix}x{:08x}", cycle as u32)
}

fn trade_is_in_session(trade: &Trade, session_started_at: i64, now: i64) -> Result<bool> {
    let timestamp = chrono::DateTime::parse_from_rfc3339(&trade.time).map_err(|_| {
        anyhow::anyhow!(
            "maker trade {} has invalid RFC3339 timestamp '{}'",
            trade.id,
            trade.time
        )
    })?;
    let timestamp = timestamp.timestamp();
    Ok(timestamp >= session_started_at && timestamp <= now)
}

#[allow(clippy::too_many_arguments)]
fn collect_current_run_fills(
    trades: Vec<Trade>,
    ledger: &mut MakerLedger,
    session_started_at: i64,
    now: i64,
    mark: f64,
    stats: &mut MakerStats,
    fills: &mut Vec<MakerFill>,
) -> Result<bool> {
    let mut exit_fill_observed = false;
    for trade in trades {
        let Some(order_id) = trade.order_id else {
            continue;
        };
        if !ledger.maker_order_ids.contains(&order_id) {
            continue;
        }
        if trade.id == 0 {
            return Err(anyhow::anyhow!(
                "maker fill for order {} has no stable trade ID",
                order_id
            ));
        }
        if !trade_is_in_session(&trade, session_started_at, now)? {
            return Err(anyhow::anyhow!(
                "current-run maker trade {} falls outside the session time boundary",
                trade.id
            ));
        }
        if !ledger.seen_fill_ids.insert(trade.id) {
            continue;
        }
        let (side, price, qty) = maker_trade_fill(&trade)?;
        let totals = {
            let totals = ledger.rest_seen.entry(order_id).or_default();
            totals.qty += qty;
            totals.notional += qty * price;
            *totals
        };
        exit_fill_observed |= ledger.record_delta(
            order_id,
            side,
            totals,
            mark,
            stats,
            fills,
            "current_run_rest_trade",
            Some(trade.id),
            Some(trade.time),
        )?;
    }
    Ok(exit_fill_observed)
}

fn adopt_current_run_order(
    order: &Order,
    run_order_prefix: &str,
    maker_order_ids: &mut HashSet<u64>,
    exit_order_ids: &mut HashSet<u64>,
) -> Result<bool> {
    if !is_current_run_order(order, run_order_prefix) {
        return Ok(false);
    }
    let order_id = order.id.parse::<u64>().map_err(|_| {
        anyhow::anyhow!(
            "current-run maker order has non-integer exchange ID '{}'",
            order.id
        )
    })?;
    maker_order_ids.insert(order_id);
    if order
        .cl_ord_id
        .as_deref()
        .is_some_and(|id| id.starts_with(&format!("{run_order_prefix}x")))
    {
        exit_order_ids.insert(order_id);
    }
    Ok(true)
}

fn adopt_order_into_ledger(
    order: &Order,
    run_order_prefix: &str,
    ledger: &mut MakerLedger,
) -> Result<bool> {
    adopt_current_run_order(
        order,
        run_order_prefix,
        &mut ledger.maker_order_ids,
        &mut ledger.exit_order_ids,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn recover_current_run_order_ids_for_reconciliation(
    client: &StandXClient,
    trades: &[Trade],
    expected_position: f64,
    observed_position: f64,
    qty_tolerance: f64,
    run_order_prefix: &str,
    ledger: &mut MakerLedger,
) {
    // A trade can settle into position before its order is visible in either
    // open-order polling or `query_orders`. Only inspect unknown trades that
    // could explain the signed reconciliation gap, then require a direct
    // `query_order` lookup to prove the current run's client-order prefix.
    const MAX_ORDER_LOOKUPS: usize = 8;
    let position_gap = observed_position - expected_position;
    if position_gap.abs() <= qty_tolerance {
        return;
    }

    let mut candidate_ids = HashSet::new();
    for trade in trades {
        let Some(order_id) = trade.order_id else {
            continue;
        };
        if ledger.maker_order_ids.contains(&order_id) {
            continue;
        }
        let side = match trade
            .side
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("buy") => 1.0,
            Some("sell") => -1.0,
            _ => continue,
        };
        let Ok(qty) = trade.qty.parse::<f64>() else {
            continue;
        };
        if !qty.is_finite()
            || qty <= 0.0
            || side * position_gap <= 0.0
            || qty > position_gap.abs() + qty_tolerance
        {
            continue;
        }
        candidate_ids.insert(order_id);
        if candidate_ids.len() == MAX_ORDER_LOOKUPS {
            break;
        }
    }

    for order_id in candidate_ids {
        match client.get_order(order_id).await {
            Ok(order) => {
                if let Err(error) = adopt_order_into_ledger(&order, run_order_prefix, ledger) {
                    eprintln!(
                        "⚠️  reconciliation order lookup returned invalid order {}: {}",
                        order_id, error
                    );
                }
            }
            Err(error) => eprintln!(
                "⚠️  reconciliation order lookup for {} failed: {}",
                order_id, error
            ),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn reconcile_ledger_snapshot(
    client: &StandXClient,
    symbol: &str,
    session_started_at: i64,
    run_order_prefix: &str,
    qty_tolerance: f64,
    mark: f64,
    ledger: &mut MakerLedger,
    stats: &mut MakerStats,
) -> Result<(f64, Vec<MakerFill>)> {
    let now = chrono::Utc::now().timestamp();
    let (orders, filled_orders, trades, positions) = tokio::join!(
        client.get_open_orders(Some(symbol)),
        client.get_order_history(Some(symbol), Some(100)),
        client.get_user_trades(symbol, session_started_at, now, Some(500)),
        client.get_positions(Some(symbol)),
    );
    let orders = orders?;
    let filled_orders = filled_orders?;
    for order in orders.iter().chain(filled_orders.iter()) {
        adopt_order_into_ledger(order, run_order_prefix, ledger)?;
    }
    let trades = trades?;
    let observed = position_for_symbol(&positions?, symbol)?;
    recover_current_run_order_ids_for_reconciliation(
        client,
        &trades,
        ledger.expected_position,
        observed,
        qty_tolerance,
        run_order_prefix,
        ledger,
    )
    .await;
    let mut fills = Vec::new();
    collect_current_run_fills(
        trades,
        ledger,
        session_started_at,
        now,
        mark,
        stats,
        &mut fills,
    )?;
    Ok((observed, fills))
}

fn unhealthy_order_response(health: Option<&OrderResponseHealth>) -> Option<String> {
    match health {
        Some(health) if health.is_healthy() => None,
        Some(health) => Some(health.failure_reason().unwrap_or_else(|| {
            "order-response stream became unhealthy without a recorded reason".to_string()
        })),
        None => Some("order-response health state is unavailable".to_string()),
    }
}

/// One reconcile cycle over an already-acquired market snapshot.
/// Returns (places, cancels, holds, fills) counts. `sim_position` carries the
/// paper-mode simulated inventory across cycles (unused in live).
#[allow(clippy::too_many_arguments)]
pub(super) async fn maker_cycle(
    client: &StandXClient,
    symbol: &str,
    cfg: &MakerConfig,
    live: bool,
    cycle: u64,
    mark: f64,
    best_bid: Option<f64>,
    best_ask: Option<f64>,
    max_divergence_bps: f64,
    inventory_exit_pct: f64,
    inventory_exit_qty: f64,
    resting: &mut Vec<RestingQuote>,
    adopted: &mut HashMap<String, (u32, f64, u64)>,
    pending: &mut Vec<PendingPlace>,
    inventory_exit_pending: &mut bool,
    ledger: &mut MakerLedger,
    session_started_at: i64,
    run_order_prefix: &str,
    starting_position: f64,
    sim_position: &mut f64,
    stats: &mut MakerStats,
    breaker: &mut VolBreaker,
    output_format: OutputFormat,
    order_response_health: Option<&OrderResponseHealth>,
) -> Result<(u64, u64, u64, u64)> {
    use maker::{
        format_decimals, paper_quote_filled, Action, CycleInput, CycleSkip, MarketSnapshot,
    };

    // 0. Run all market-only guards before any account/order I/O. The pure
    // planner owns breaker observation and data-consistency policy; this
    // adapter only renders the resulting skip decision.
    let market = MarketSnapshot {
        mark,
        best_bid,
        best_ask,
    };
    let preflight = maker::preflight_cycle(breaker, market, max_divergence_bps, live);
    let halted = match preflight.skip {
        Some(CycleSkip::MarkMidDivergence { divergence_bps }) => {
            let live_str = if live { "live" } else { "paper" };
            match output_format {
                OutputFormat::Json => {
                    println!(
                        "{}",
                        serde_json::json!({
                            "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                            "cycle": cycle, "mode": live_str, "symbol": symbol,
                            "action": "skip", "reason": "mark_mid_divergence",
                            "mark": format_decimals(mark, cfg.price_decimals),
                            "divergence_bps": (divergence_bps * 100.0).round() / 100.0,
                            "max_divergence_bps": max_divergence_bps,
                        })
                    );
                }
                _ => {
                    eprintln!(
                        "⚠️  #{} mark/mid divergence {:.1}bps > {}bps — skipping cycle (no actions)",
                        cycle, divergence_bps, max_divergence_bps
                    );
                }
            }
            return Ok((0, 0, 0, 0));
        }
        Some(CycleSkip::MissingTouch) => {
            // Fail-safe: without a touch we cannot guarantee no-cross pricing.
            eprintln!("⚠️  empty order book on {}; skipping this cycle", symbol);
            return Ok((0, 0, 0, 0));
        }
        None => preflight.halted,
    };

    // 2. Rebuild resting + position from the exchange (live) or keep the
    //    simulated book (paper).
    let position: f64;
    let mut account_balance: Option<Balance> = None;
    let mut fills: Vec<MakerFill> = Vec::new();
    let mut exit_fill_observed = false;
    if live {
        let now = chrono::Utc::now().timestamp();
        let (orders, filled_orders, trades, balance) = tokio::join!(
            client.get_open_orders(Some(symbol)),
            client.get_order_history(Some(symbol), Some(100)),
            client.get_user_trades(symbol, session_started_at, now, Some(500)),
            client.get_balance(),
        );
        let mut orders = orders?;
        let filled_orders = filled_orders?;
        let trades = trades?;
        account_balance = Some(balance?);

        // Open maker orders identify partial fills; historical maker orders
        // identify a quote that fully filled between two polling cycles.
        for order in orders.iter().chain(filled_orders.iter()) {
            adopt_order_into_ledger(order, run_order_prefix, ledger)?;
        }

        exit_fill_observed |= collect_current_run_fills(
            trades,
            ledger,
            session_started_at,
            now,
            mark,
            stats,
            &mut fills,
        )?;

        let positions = client.get_positions(Some(symbol)).await?;
        let mut observed_position = position_for_symbol(&positions, symbol)?;
        let qty_tolerance = 10_f64.powi(-(cfg.qty_decimals as i32)) / 2.0;
        if (observed_position - ledger.expected_position).abs() > qty_tolerance {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let retry_now = chrono::Utc::now().timestamp();
            let (retry_orders, retry_filled_orders, retry_trades) = tokio::join!(
                client.get_open_orders(Some(symbol)),
                client.get_order_history(Some(symbol), Some(100)),
                client.get_user_trades(symbol, session_started_at, retry_now, Some(500)),
            );
            orders = retry_orders?;
            let retry_filled_orders = retry_filled_orders?;
            for order in orders.iter().chain(retry_filled_orders.iter()) {
                adopt_order_into_ledger(order, run_order_prefix, ledger)?;
            }
            let retry_trades = retry_trades?;
            recover_current_run_order_ids_for_reconciliation(
                client,
                &retry_trades,
                ledger.expected_position,
                observed_position,
                qty_tolerance,
                run_order_prefix,
                ledger,
            )
            .await;
            exit_fill_observed |= collect_current_run_fills(
                retry_trades,
                ledger,
                session_started_at,
                retry_now,
                mark,
                stats,
                &mut fills,
            )?;
            let retry_positions = client.get_positions(Some(symbol)).await?;
            observed_position = position_for_symbol(&retry_positions, symbol)?;
            if (observed_position - ledger.expected_position).abs() > qty_tolerance {
                if output_format == OutputFormat::Json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                            "symbol": symbol,
                            "cycle": cycle,
                            "action": "position_reconciliation",
                            "event": "failed",
                            "expected_position": ledger.expected_position,
                            "observed_position": observed_position,
                            "message": "venue position cannot be explained by current-run maker fills",
                        })
                    );
                } else {
                    eprintln!(
                        "⚠️  position reconciliation failed: expected {:+.8}, observed {:+.8}",
                        ledger.expected_position, observed_position
                    );
                }
                return Err(anyhow::Error::new(PositionReconciliationError {
                    expected: ledger.expected_position,
                    observed: observed_position,
                }));
            }
        }
        position = observed_position;

        let tick = cfg.price_tick();
        *resting = orders
            .into_iter()
            .filter(|order| is_current_run_order(order, run_order_prefix))
            .map(|o| {
                let price: f64 = o.price.parse().unwrap_or(0.0);
                let qty: f64 = o.qty.parse().unwrap_or(0.0);
                let (level, ref_center, placed_at_cycle) = match adopted.get(&o.id) {
                    Some(&meta) => meta,
                    None => {
                        // Try to adopt from a recent place by side + price,
                        // tolerating a shrunk qty from a partial fill (see
                        // open_qty_adopts).
                        let matched = o
                            .cl_ord_id
                            .as_ref()
                            .and_then(|cl_ord_id| {
                                pending.iter().position(|p| p.cl_ord_id == *cl_ord_id)
                            })
                            .or_else(|| {
                                // Backward-compatible fallback for orders
                                // created before client IDs were enabled.
                                pending.iter().position(|p| {
                                    p.side == o.side
                                        && (p.price - price).abs() < tick / 2.0
                                        && open_qty_adopts(qty, p.qty)
                                })
                            });
                        let meta = match matched {
                            Some(idx) => {
                                let p = pending.remove(idx);
                                (p.level, p.ref_center, p.cycle)
                            }
                            // An older maker order without in-memory state:
                            // sentinel level makes reconciliation replace it.
                            // Manual orders were filtered above and cannot
                            // enter the strategy state.
                            None => (u32::MAX, mark, cycle),
                        };
                        adopted.insert(o.id.clone(), meta);
                        meta
                    }
                };
                RestingQuote {
                    order_id: Some(o.id),
                    side: o.side,
                    level,
                    price,
                    qty,
                    ref_center,
                    placed_at_cycle,
                }
            })
            .collect();
        // Places older than 2 cycles never showed up as open orders —
        // likely rejected (e.g. ALO would-cross) or fully filled on arrival.
        pending.retain(|p| cycle.saturating_sub(p.cycle) <= 2);
        adopted.retain(|id, _| resting.iter().any(|r| r.order_id.as_deref() == Some(id)));
    } else {
        // Paper mode: simulate fills against the touch so inventory (and thus
        // skew) is observable without going live. A crossed resting quote is
        // taken off the book and its signed qty folded into the position; the
        // reconcile below then re-quotes the vacated level.
        let mut i = 0;
        while i < resting.len() {
            if paper_quote_filled(resting[i].side, resting[i].price, best_bid, best_ask) {
                let q = resting.remove(i);
                *sim_position += match q.side {
                    OrderSide::Buy => q.qty,
                    OrderSide::Sell => -q.qty,
                };
                stats.record_fill(q.side, q.price, q.qty, mark);
                fills.push(MakerFill {
                    side: q.side,
                    price: q.price,
                    qty: q.qty,
                    trade_id: None,
                    order_id: None,
                    trade_ts: None,
                    origin: "paper",
                });
            } else {
                i += 1;
            }
        }
        position = *sim_position;
    }

    // 3. Build the pure quote/exit plan from the synchronized state.
    let pending_slots = pending
        .iter()
        .map(|place| (place.side, place.level))
        .collect::<Vec<_>>();
    let plan = maker::plan_cycle(
        cfg,
        CycleInput {
            cycle,
            market,
            position,
            resting,
            pending_slots: &pending_slots,
            active_exit_enabled: live,
            inventory_exit_pct,
            inventory_exit_qty,
        },
        halted,
    );
    let raw_inventory_exit = plan.requested_inventory_exit;
    if exit_fill_observed {
        *inventory_exit_pending = false;
    }
    if raw_inventory_exit.is_none() {
        *inventory_exit_pending = false;
    }
    if raw_inventory_exit.is_some() && *inventory_exit_pending {
        return Err(anyhow::anyhow!(
            "inventory exit is still awaiting venue confirmation; refusing to submit another"
        ));
    }

    let inventory_exit = plan.inventory_exit;
    // The pure reconciler intentionally knows nothing about transport state.
    // Remove desired placements whose slots are still reserved by an HTTP
    // submission before both execution and telemetry, so output never claims
    // a duplicate place occurred.
    let actions: Vec<Action> = plan
        .actions
        .into_iter()
        .filter(|action| match action {
            Action::Place(q) if live && pending_covers_slot(pending, q.side, q.level) => {
                log_maker_event(
                    output_format,
                    symbol,
                    cycle,
                    "place_pending",
                    q.side,
                    q.level,
                    q.price,
                    cfg.price_decimals,
                    "awaiting asynchronous order confirmation",
                );
                false
            }
            _ => true,
        })
        .collect();

    // The pure planner provides the anti-flicker anchor for new placements.
    let ref_center = plan.ref_center;

    // 4. Execute. Business rejections (post-only would-cross, order already
    //    gone) are expected and logged inline; only transient failures
    //    propagate as cycle errors toward the fail-safe.
    let mut places: u64 = 0;
    let mut cancels: u64 = 0;
    let mut holds: u64 = 0;
    for action in &actions {
        match action {
            Action::Cancel {
                order_id,
                side,
                level,
                price,
                ..
            } => {
                if live {
                    if let Some(id) = order_id {
                        match client.cancel_order(symbol, id).await {
                            Ok(()) => {
                                adopted.remove(id);
                                cancels += 1;
                            }
                            Err(e) if is_order_rejection(&e) => {
                                // Order already gone (filled or cancelled
                                // out from under us) — that IS the goal.
                                adopted.remove(id);
                                cancels += 1;
                                log_maker_event(
                                    output_format,
                                    symbol,
                                    cycle,
                                    "cancel_noop",
                                    *side,
                                    *level,
                                    *price,
                                    cfg.price_decimals,
                                    "order already gone",
                                );
                            }
                            // Transient (network / 5xx) → fail-safe path.
                            Err(e) => return Err(e.into()),
                        }
                    }
                } else {
                    resting.retain(|r| !(r.side == *side && r.level == *level));
                    cancels += 1;
                }
            }
            Action::Place(q) => {
                if live {
                    if let Some(reason) = unhealthy_order_response(order_response_health) {
                        return Err(anyhow::anyhow!("{reason}; refusing live placement"));
                    }
                    let cl_ord_id = quote_client_order_id(run_order_prefix, cycle, q.side, q.level);
                    match client
                        .create_order(CreateOrderParams {
                            symbol: symbol.to_string(),
                            cl_ord_id: Some(cl_ord_id.clone()),
                            side: q.side,
                            order_type: OrderType::Limit,
                            quantity: format_decimals(q.qty, cfg.qty_decimals),
                            price: Some(format_decimals(q.price, cfg.price_decimals)),
                            // Post-only: reject instead of taking if the
                            // price would cross by arrival time.
                            time_in_force: Some(TimeInForce::Alo),
                            reduce_only: false,
                            stop_price: None,
                            sl_price: None,
                            tp_price: None,
                        })
                        .await
                    {
                        Ok(submission) => {
                            pending.push(PendingPlace {
                                request_id: submission.id,
                                cl_ord_id,
                                side: q.side,
                                price: q.price,
                                qty: q.qty,
                                level: q.level,
                                ref_center,
                                cycle,
                            });
                            places += 1;
                        }
                        Err(e) if is_order_rejection(&e) => {
                            // Post-only would-cross etc. — expected in fast
                            // markets. Re-quote next cycle, don't fail-safe.
                            log_maker_event(
                                output_format,
                                symbol,
                                cycle,
                                "place_rejected",
                                q.side,
                                q.level,
                                q.price,
                                cfg.price_decimals,
                                "post-only rejected",
                            );
                        }
                        Err(e) => return Err(e.into()),
                    }
                } else {
                    resting.push(RestingQuote {
                        order_id: None,
                        side: q.side,
                        level: q.level,
                        price: q.price,
                        qty: q.qty,
                        ref_center,
                        placed_at_cycle: cycle,
                    });
                    places += 1;
                }
            }
            Action::Hold { .. } => holds += 1,
        }
    }

    if let Some(exit) = inventory_exit {
        // Do not race a reduce-only market order against quote cancellations.
        // The next cycle must observe an empty maker book before the single
        // exit request can be submitted.
        if resting.is_empty() && pending.is_empty() {
            if let Some(reason) = unhealthy_order_response(order_response_health) {
                return Err(anyhow::anyhow!("{reason}; refusing inventory exit"));
            }
            let cl_ord_id = exit_client_order_id(run_order_prefix, cycle);
            client
                .create_order(CreateOrderParams {
                    symbol: symbol.to_string(),
                    cl_ord_id: Some(cl_ord_id),
                    side: exit.side,
                    order_type: OrderType::Market,
                    quantity: format_decimals(exit.qty, cfg.qty_decimals),
                    price: None,
                    time_in_force: None,
                    reduce_only: true,
                    stop_price: None,
                    sl_price: None,
                    tp_price: None,
                })
                .await?;
            *inventory_exit_pending = true;
            log_maker_event(
                output_format,
                symbol,
                cycle,
                "inventory_exit_submitted",
                exit.side,
                0,
                mark,
                cfg.price_decimals,
                "reduce-only market order submitted after maker book cleared",
            );
        }
    }

    // 5. Telemetry uses exact ledger fills in live mode and simulated fills
    // in paper mode; never infer a fill from a position delta.
    let two_sided = resting.iter().any(|r| r.side == OrderSide::Buy)
        && resting.iter().any(|r| r.side == OrderSide::Sell);
    stats.end_cycle(position, two_sided);

    // 6. Emit.
    emit_maker_cycle(
        output_format,
        live,
        symbol,
        cycle,
        mark,
        best_bid,
        best_ask,
        position,
        starting_position,
        account_balance.as_ref(),
        &actions,
        &fills,
        stats,
        halted.then(|| breaker.vol_bps()),
        cfg,
    );

    Ok((places, cancels, holds, fills.len() as u64))
}

/// Decode a venue fill strictly enough for accounting. A maker fill with
/// missing side, price, or quantity is not silently guessed from position.
fn maker_trade_fill(trade: &Trade) -> Result<(OrderSide, f64, f64)> {
    let side = match trade.side.as_deref() {
        Some(side) if side.eq_ignore_ascii_case("buy") => OrderSide::Buy,
        Some(side) if side.eq_ignore_ascii_case("sell") => OrderSide::Sell,
        _ => {
            return Err(anyhow::anyhow!(
                "maker trade {} is missing a valid side",
                trade.id
            ));
        }
    };
    let price = trade.price.parse::<f64>().map_err(|_| {
        anyhow::anyhow!(
            "maker trade {} has invalid price '{}'",
            trade.id,
            trade.price
        )
    })?;
    let qty = trade
        .qty
        .parse::<f64>()
        .map_err(|_| anyhow::anyhow!("maker trade {} has invalid qty '{}'", trade.id, trade.qty))?;
    if !price.is_finite() || price <= 0.0 || !qty.is_finite() || qty <= 0.0 {
        return Err(anyhow::anyhow!(
            "maker trade {} has non-positive price/qty",
            trade.id
        ));
    }
    Ok((side, price, qty))
}

/// Cancel maker-owned orders with retries, preserving manual/API orders.
pub(super) async fn cancel_maker_orders_with_retry(
    client: &StandXClient,
    symbol: &str,
    attempts: u32,
    output_format: OutputFormat,
) -> Result<()> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=attempts {
        let result = async {
            let orders = client.get_open_orders(Some(symbol)).await?;
            let order_ids = orders
                .iter()
                .filter(|order| is_maker_order(order))
                .map(|order| {
                    order.id.parse::<i64>().map_err(|_| {
                        anyhow::anyhow!(
                            "maker-owned order has non-integer exchange ID '{}'",
                            order.id
                        )
                    })
                })
                .collect::<Result<Vec<_>>>()?;

            if order_ids.is_empty() {
                return Ok::<_, anyhow::Error>(());
            }

            client.cancel_orders(&order_ids).await?;
            // Cancellation is accepted asynchronously by the venue. Do not
            // treat an immediately stale open-orders response as a reason to
            // abandon a safe reconnect; verify after a short grace period and
            // reissue cancellation only for still-visible maker orders.
            tokio::time::sleep(MAKER_CLEANUP_VERIFY_DELAY).await;
            let residual_ids = client
                .get_open_orders(Some(symbol))
                .await?
                .iter()
                .filter(|order| is_maker_order(order))
                .map(|order| order.id.clone())
                .collect::<Vec<_>>();
            if residual_ids.is_empty() {
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "RESIDUAL MAKER ORDERS on {} after cancellation: [{}]",
                    symbol,
                    residual_ids.join(", ")
                ))
            }
        }
        .await;
        match result {
            Ok(()) => {
                if output_format == OutputFormat::Json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                            "symbol": symbol,
                            "action": "maker_cleanup",
                            "event": "complete",
                            "remaining_maker_orders": 0,
                        })
                    );
                } else {
                    println!("✅ All maker-owned {} orders cancelled", symbol);
                }
                return Ok(());
            }
            Err(e) => {
                eprintln!(
                    "⚠️  maker-order cancellation attempt {}/{} incomplete: {}",
                    attempt, attempts, e
                );
                last_err = Some(e);
                if attempt < attempts {
                    tokio::time::sleep(MAKER_CLEANUP_RETRY_DELAY).await;
                }
            }
        }
    }

    Err(last_err.unwrap_or_else(|| {
        anyhow::anyhow!(
            "maker-order cancellation had no attempts — inspect or cancel manually with 'standx order cancel-all {}'",
            symbol
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trade(side: Option<&str>, price: &str, qty: &str) -> Trade {
        Trade {
            id: 42,
            time: "2026-07-10T00:00:00Z".to_string(),
            price: price.to_string(),
            qty: qty.to_string(),
            side: side.map(str::to_string),
            is_buyer_taker: false,
            fee_asset: None,
            fee_qty: None,
            pnl: None,
            order_id: Some(7),
            symbol: Some("BTC-USD".to_string()),
            value: None,
        }
    }

    #[test]
    fn maker_trade_fill_requires_complete_venue_fields() {
        assert_eq!(
            maker_trade_fill(&trade(Some("buy"), "99.5", "0.02")).unwrap(),
            (OrderSide::Buy, 99.5, 0.02)
        );
        assert!(maker_trade_fill(&trade(None, "99.5", "0.02"))
            .unwrap_err()
            .to_string()
            .contains("valid side"));
        assert!(maker_trade_fill(&trade(Some("sell"), "bad", "0.02"))
            .unwrap_err()
            .to_string()
            .contains("invalid price"));
    }

    #[test]
    fn current_run_fill_is_recorded_once_with_trade_identity() {
        let trade = trade(Some("buy"), "59.50", "0.20");
        let start = chrono::DateTime::parse_from_rfc3339("2026-07-10T00:00:00Z")
            .unwrap()
            .timestamp();
        let mut stats = MakerStats::default();
        let mut ledger = MakerLedger::new(0.0);
        ledger.maker_order_ids.insert(7);
        let mut fills = Vec::new();

        collect_current_run_fills(
            vec![trade.clone()],
            &mut ledger,
            start,
            start + 60,
            59.50,
            &mut stats,
            &mut fills,
        )
        .unwrap();
        collect_current_run_fills(
            vec![trade],
            &mut ledger,
            start,
            start + 60,
            59.50,
            &mut stats,
            &mut fills,
        )
        .unwrap();

        assert_eq!(stats.fills(), 1);
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].trade_id, Some(42));
        assert_eq!(fills[0].order_id, Some(7));
        assert_eq!(fills[0].origin, "current_run_rest_trade");
        assert!((ledger.expected_position - 0.2).abs() < 1e-9);
    }

    fn order_update(fill_qty: &str, avg: &str) -> OrderUpdate {
        OrderUpdate {
            seq: 10,
            order_id: 7,
            cl_ord_id: Some("sxmk-0123456789ab-q00000001b0".to_string()),
            symbol: "BTC-USD".to_string(),
            side: OrderSide::Buy,
            qty: "0.20".to_string(),
            fill_qty: fill_qty.to_string(),
            fill_avg_price: avg.to_string(),
            price: "59.50".to_string(),
            status: standx_sdk::models::OrderStatus::PartiallyFilled,
            reduce_only: false,
            updated_at: "2026-07-10T00:00:01Z".to_string(),
        }
    }

    #[test]
    fn websocket_then_rest_trade_is_not_double_counted() {
        let start = chrono::DateTime::parse_from_rfc3339("2026-07-10T00:00:00Z")
            .unwrap()
            .timestamp();
        let mut ledger = MakerLedger::new(0.0);
        let mut stats = MakerStats::default();
        let mut fills = Vec::new();
        ledger
            .apply_order_update(
                &order_update("0.20", "59.50"),
                "BTC-USD",
                "sxmk-0123456789ab-",
                59.50,
                &mut stats,
                &mut fills,
            )
            .unwrap();
        collect_current_run_fills(
            vec![trade(Some("buy"), "59.50", "0.20")],
            &mut ledger,
            start,
            start + 60,
            59.50,
            &mut stats,
            &mut fills,
        )
        .unwrap();
        assert_eq!(stats.fills(), 1);
        assert_eq!(fills.len(), 1);
        assert!((ledger.expected_position - 0.20).abs() < 1e-9);
    }

    #[test]
    fn rest_then_websocket_only_accounts_cumulative_delta() {
        let start = chrono::DateTime::parse_from_rfc3339("2026-07-10T00:00:00Z")
            .unwrap()
            .timestamp();
        let mut ledger = MakerLedger::new(0.0);
        ledger.maker_order_ids.insert(7);
        let mut stats = MakerStats::default();
        let mut fills = Vec::new();
        collect_current_run_fills(
            vec![trade(Some("buy"), "59.50", "0.10")],
            &mut ledger,
            start,
            start + 60,
            59.50,
            &mut stats,
            &mut fills,
        )
        .unwrap();
        ledger
            .apply_order_update(
                &order_update("0.20", "59.50"),
                "BTC-USD",
                "sxmk-0123456789ab-",
                59.50,
                &mut stats,
                &mut fills,
            )
            .unwrap();
        assert_eq!(stats.fills(), 2);
        assert_eq!(fills.len(), 2);
        assert!((fills[1].qty - 0.10).abs() < 1e-9);
        assert!((ledger.expected_position - 0.20).abs() < 1e-9);
    }

    #[test]
    fn historical_trade_without_current_run_order_is_ignored() {
        let mut stats = MakerStats::default();
        let mut fills = Vec::new();
        let mut ledger = MakerLedger::new(-0.13);
        collect_current_run_fills(
            vec![trade(Some("sell"), "59.50", "0.20")],
            &mut ledger,
            1_783_000_000,
            1_784_000_000,
            59.50,
            &mut stats,
            &mut fills,
        )
        .unwrap();
        assert_eq!(stats.fills(), 0);
        assert!(fills.is_empty());
        assert_eq!(ledger.expected_position, -0.13);
    }

    #[test]
    fn current_run_trade_outside_session_is_rejected() {
        let mut stats = MakerStats::default();
        let mut fills = Vec::new();
        let mut ledger = MakerLedger::new(0.0);
        ledger.maker_order_ids.insert(7);
        let error = collect_current_run_fills(
            vec![trade(Some("buy"), "59.50", "0.20")],
            &mut ledger,
            1_783_700_000,
            1_783_700_100,
            59.50,
            &mut stats,
            &mut fills,
        )
        .unwrap_err();
        assert!(error.to_string().contains("outside the session"));
    }

    #[test]
    fn current_run_client_order_ids_are_bounded_and_scoped() {
        let prefix = "sxmk-0123456789ab-";
        let quote = quote_client_order_id(prefix, u64::MAX, OrderSide::Sell, u32::MAX);
        let exit = exit_client_order_id(prefix, u64::MAX);
        assert!(quote.starts_with(prefix));
        assert!(exit.starts_with(prefix));
        assert!(quote.len() <= 41, "{quote}");
        assert!(exit.len() <= 41, "{exit}");
    }
}
