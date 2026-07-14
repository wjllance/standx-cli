use super::*;
use standx_sdk::order_response::OrderResponse;

fn next_runtime_effect(runtime_state: &mut MakerState) -> Option<MakerEffect> {
    runtime_state.next_effect().map(|effect| match effect {
        MakerEffect::RunCycle(token) => MakerEffect::RunCycle(token),
        MakerEffect::AbortInFlight(token) => MakerEffect::AbortInFlight(token),
        MakerEffect::CommitCycle(token) => MakerEffect::CommitCycle(token),
        MakerEffect::Cleanup { token, target } => MakerEffect::Cleanup { token, target },
        MakerEffect::Recover { token, target } => MakerEffect::Recover { token, target },
        MakerEffect::Stop(reason) => MakerEffect::Stop(reason),
    })
}

fn take_cleanup_effect(
    runtime_state: &mut MakerState,
    expected_target: RecoveryTarget,
) -> Result<WorkToken> {
    loop {
        match next_runtime_effect(runtime_state) {
            Some(MakerEffect::AbortInFlight(_)) => {}
            Some(MakerEffect::Cleanup { token, target }) if target == expected_target => {
                return Ok(token);
            }
            Some(effect) => {
                return Err(anyhow::anyhow!(
                    "runtime expected {expected_target:?} cleanup, got {effect:?}"
                ));
            }
            None => {
                return Err(anyhow::anyhow!(
                    "runtime did not emit {expected_target:?} cleanup"
                ));
            }
        }
    }
}

fn take_recovery_effect(
    runtime_state: &mut MakerState,
    expected_target: RecoveryTarget,
) -> Result<WorkToken> {
    match next_runtime_effect(runtime_state) {
        Some(MakerEffect::Recover { token, target }) if target == expected_target => Ok(token),
        Some(effect) => Err(anyhow::anyhow!(
            "runtime expected {expected_target:?} recovery, got {effect:?}"
        )),
        None => Err(anyhow::anyhow!(
            "runtime did not emit {expected_target:?} recovery"
        )),
    }
}

fn take_stop_effect(runtime_state: &mut MakerState) -> Result<MakerExit> {
    loop {
        match next_runtime_effect(runtime_state) {
            Some(MakerEffect::AbortInFlight(_)) => {}
            Some(MakerEffect::Stop(reason)) => return Ok(reason.into()),
            Some(effect) => {
                return Err(anyhow::anyhow!(
                    "runtime expected stop effect, got {effect:?}"
                ));
            }
            None => return Err(anyhow::anyhow!("runtime did not emit stop effect")),
        }
    }
}

fn recovery_failed_exit(
    runtime_state: &mut MakerState,
    token: WorkToken,
    reason: String,
) -> MakerExit {
    runtime_state.handle(MakerEvent::RecoveryFailed { token, reason });
    take_stop_effect(runtime_state)
        .unwrap_or_else(|error| MakerExit::PositionReconciliation(error.to_string()))
}

fn stop_requested_exit(runtime_state: &mut MakerState, reason: RuntimeStopReason) -> MakerExit {
    runtime_state.handle(MakerEvent::StopRequested(reason));
    take_stop_effect(runtime_state)
        .unwrap_or_else(|error| MakerExit::PositionReconciliation(error.to_string()))
}

pub(super) fn apply_order_responses(
    receiver: &mut tokio::sync::mpsc::Receiver<OrderResponse>,
    projection: &mut MakerAccountProjection,
    runtime_state: &mut MakerState,
    output_format: OutputFormat,
    symbol: &str,
    cycle: u64,
    price_decimals: u32,
) -> Result<()> {
    loop {
        let response = match receiver.try_recv() {
            Ok(response) => response,
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => return Ok(()),
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                return Err(anyhow::anyhow!(
                    "order-response stream disconnected; refusing further live orders"
                ));
            }
        };
        let request_id = response.request_id.clone();
        let matched = apply_order_response(
            response,
            projection,
            output_format,
            symbol,
            cycle,
            price_decimals,
        );
        if let Some(request_id) = request_id {
            if !matched {
                runtime_state.handle(MakerEvent::OrderResponseUnmatched { request_id });
            }
        }
        if matches!(
            runtime_state.pending_effect(),
            Some(MakerEffect::AbortInFlight(_))
                | Some(MakerEffect::Cleanup {
                    target: RecoveryTarget::OrderResponse,
                    ..
                })
        ) {
            return Err(anyhow::anyhow!("order-response correlation failed closed"));
        }
    }
}

fn apply_order_response(
    response: OrderResponse,
    projection: &mut MakerAccountProjection,
    output_format: OutputFormat,
    symbol: &str,
    cycle: u64,
    price_decimals: u32,
) -> bool {
    let Some(request_id) = response.request_id.as_deref() else {
        return false;
    };
    let Some(pending) = projection.pending_request(request_id).cloned() else {
        return false;
    };
    let generation = projection.generation();
    match pending {
        ProjectionPendingRequest::Cancel(cancelled) => {
            projection.apply(
                generation,
                AccountProjectionEvent::CancelResolved {
                    request_id: request_id.to_string(),
                },
            );
            // A cancellation rejected because the order is already gone is an
            // acceptable terminal outcome. The next venue snapshot remains the
            // authority for whether the order actually disappeared.
            if !response.accepted() {
                output::log_maker_event(output::MakerLogEvent {
                    output_format,
                    symbol,
                    cycle,
                    action: "cancel_noop",
                    side: cancelled.side,
                    level: cancelled.level,
                    price: cancelled.price,
                    price_decimals,
                    detail: "order already gone",
                });
            }
        }
        ProjectionPendingRequest::Place(place) => {
            if response.accepted() {
                projection.apply(
                    generation,
                    AccountProjectionEvent::PlaceAccepted {
                        request_id: request_id.to_string(),
                    },
                );
            } else {
                projection.apply(
                    generation,
                    AccountProjectionEvent::PlaceRejected {
                        request_id: request_id.to_string(),
                    },
                );
                output::log_maker_event(output::MakerLogEvent {
                    output_format,
                    symbol,
                    cycle,
                    action: "place_rejected_async",
                    side: place.side,
                    level: place.level,
                    price: place.price,
                    price_decimals,
                    detail: &response.message,
                });
            }
        }
    }
    true
}

struct AccountEventContext<'a> {
    symbol: &'a str,
    run_order_prefix: &'a str,
    mark: f64,
    cycle: u64,
    output_format: OutputFormat,
}

struct AccountEventState<'a> {
    ledger: &'a mut MakerLedger,
    stats: &'a mut MakerStats,
    projection: &'a mut MakerAccountProjection,
}

#[derive(Debug, Default)]
struct AccountEventOutcome {
    fills: u64,
    latest_position: Option<f64>,
    exit_fill_observed: bool,
}

impl AccountEventOutcome {
    fn merge(&mut self, other: Self) {
        self.fills += other.fills;
        if other.latest_position.is_some() {
            self.latest_position = other.latest_position;
        }
        self.exit_fill_observed |= other.exit_fill_observed;
    }
}

fn apply_account_events(
    receiver: &mut tokio::sync::mpsc::Receiver<AccountEvent>,
    state: &mut AccountEventState<'_>,
    context: &AccountEventContext<'_>,
) -> Result<AccountEventOutcome> {
    let mut outcome = AccountEventOutcome::default();
    loop {
        let event = match receiver.try_recv() {
            Ok(event) => event,
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                return Ok(outcome);
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                return Err(anyhow::anyhow!("authenticated account stream disconnected"));
            }
        };
        outcome.merge(apply_account_event(event, state, context)?);
    }
}

fn account_event_invalidates_cycle(event: &AccountEvent) -> bool {
    matches!(
        event,
        AccountEvent::Position(_)
            | AccountEvent::Trade(_)
            | AccountEvent::Disconnected { .. }
            | AccountEvent::Error { .. }
    )
}

fn apply_account_event(
    event: AccountEvent,
    state: &mut AccountEventState<'_>,
    context: &AccountEventContext<'_>,
) -> Result<AccountEventOutcome> {
    match event {
        AccountEvent::Connected { epoch } => {
            if epoch != state.projection.generation() {
                return Err(anyhow::anyhow!(
                    "stale account-stream generation {epoch}; current projection generation is {}",
                    state.projection.generation()
                ));
            }
            Ok(AccountEventOutcome::default())
        }
        AccountEvent::Order(update) => {
            let mut fills = Vec::new();
            let observation = model::stream_order_observation(&update)?;
            let exit_fill_observed = ledger::apply_order_update(
                state.ledger,
                &update,
                context.symbol,
                context.run_order_prefix,
                context.mark,
                state.stats,
                &mut fills,
            )?;
            let generation = state.projection.generation();
            let projection_outcome = state.projection.apply(
                generation,
                AccountProjectionEvent::OrderObserved(observation),
            );
            if projection_outcome.unknown_current_run_order {
                return Err(anyhow::anyhow!(
                    "account stream reported an unknown current-run maker order"
                ));
            }
            for fill in &fills {
                if let Some(order_id) = fill.order_id {
                    state.projection.apply(
                        generation,
                        AccountProjectionEvent::TradeApplied {
                            order_id,
                            qty: fill.qty,
                        },
                    );
                }
                emit_live_fill(fill, context.symbol, context.cycle, context.output_format);
            }
            Ok(AccountEventOutcome {
                fills: fills.len() as u64,
                latest_position: None,
                exit_fill_observed,
            })
        }
        AccountEvent::Position(update) => {
            if !update.symbol.eq_ignore_ascii_case(context.symbol) {
                return Ok(AccountEventOutcome::default());
            }
            let qty =
                model::signed_position_quantity(&update.qty, update.side).map_err(|error| {
                    anyhow::anyhow!("account position update has invalid qty: {error}")
                })?;
            let generation = state.projection.generation();
            state.projection.apply(
                generation,
                AccountProjectionEvent::PositionObserved { position: qty },
            );
            Ok(AccountEventOutcome {
                fills: 0,
                latest_position: Some(qty),
                exit_fill_observed: false,
            })
        }
        AccountEvent::Trade(trade) => {
            let mut fills = Vec::new();
            let exit_fill_observed = ledger::apply_account_trade(
                state.ledger,
                trade,
                context.symbol,
                context.mark,
                state.stats,
                &mut fills,
            )?;
            let generation = state.projection.generation();
            for fill in &fills {
                if let Some(order_id) = fill.order_id {
                    state.projection.apply(
                        generation,
                        AccountProjectionEvent::TradeApplied {
                            order_id,
                            qty: fill.qty,
                        },
                    );
                }
                emit_live_fill(fill, context.symbol, context.cycle, context.output_format);
            }
            Ok(AccountEventOutcome {
                fills: fills.len() as u64,
                latest_position: None,
                exit_fill_observed,
            })
        }
        // Raw wallet fields are projected independently. The derived unified
        // margin snapshot used by existing output remains REST-backed.
        AccountEvent::Balance(update) => {
            let generation = state.projection.generation();
            state.projection.apply(
                generation,
                AccountProjectionEvent::BalanceObserved(model::projected_balance(update)),
            );
            Ok(AccountEventOutcome::default())
        }
        AccountEvent::Disconnected { reason } | AccountEvent::Error { reason } => Err(
            anyhow::anyhow!("authenticated account stream unhealthy: {reason}"),
        ),
    }
}

fn emit_live_fill(fill: &MakerFill, symbol: &str, cycle: u64, output_format: OutputFormat) {
    match output_format {
        OutputFormat::Json => println!(
            "{}",
            serde_json::json!({
                "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "symbol": symbol,
                "cycle": cycle,
                "action": "fill",
                "origin": fill.origin,
                "order_id": fill.order_id,
                "trade_id": fill.trade_id,
                "trade_ts": fill.trade_ts,
                "side": fill.side,
                "price": fill.price,
                "qty": fill.qty,
            })
        ),
        _ => eprintln!(
            "⚡ account fill {:?} {} @ {} (order {})",
            fill.side,
            fill.qty,
            fill.price,
            fill.order_id.unwrap_or_default()
        ),
    }
}

fn emit_reconciliation_state(
    output_format: OutputFormat,
    symbol: &str,
    cycle: u64,
    event: &str,
    expected: f64,
    observed: f64,
) {
    if output_format == OutputFormat::Json {
        println!(
            "{}",
            serde_json::json!({
                "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "symbol": symbol,
                "cycle": cycle,
                "action": "position_reconciliation",
                "event": event,
                "expected_position": expected,
                "observed_position": observed,
            })
        );
    } else {
        eprintln!(
            "⚠️  position reconciliation {event}: expected {expected:+.8}, observed {observed:+.8}"
        );
    }
}

fn emit_stop_loss_triggered(
    output_format: OutputFormat,
    symbol: &str,
    cycle: u64,
    pnl: f64,
    stop_loss: f64,
) {
    if output_format == OutputFormat::Json {
        println!(
            "{}",
            serde_json::json!({
                "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "symbol": symbol,
                "cycle": cycle,
                "action": "stop_loss",
                "event": "triggered",
                "pnl": pnl,
                "stop_loss": stop_loss,
            })
        );
    } else {
        eprintln!(
            "🛑 stop-loss triggered: session PnL {pnl:+.2} breached -{stop_loss:.2}; shutting down"
        );
    }
}

fn accounting_position_mismatch(
    expected_position: f64,
    stats_position: f64,
    qty_tolerance: f64,
) -> bool {
    let delta = (stats_position - expected_position).abs();
    // Fail closed: a non-finite delta (NaN from a poisoned position) would make
    // a bare `>` comparison false and silently pass the invariant, so treat any
    // non-finite value as a mismatch.
    !delta.is_finite() || delta > qty_tolerance
}

async fn accounting_invariant_exit(
    notifier: &MakerNotifier,
    symbol: &str,
    cycle: u64,
    expected_position: f64,
    stats_position: f64,
    qty_tolerance: f64,
) -> Option<MakerExit> {
    if !accounting_position_mismatch(expected_position, stats_position, qty_tolerance) {
        return None;
    }
    let detail = format!(
        "stats position {stats_position:+.8} differs from ledger expected {expected_position:+.8} beyond tolerance {qty_tolerance:.8}"
    );
    notifier
        .risk(
            RiskNotice {
                kind: "accounting_invariant",
                severity: "critical",
                event: "mismatch",
                message: &detail,
                symbol,
                cycle,
                position_before: None,
                position_after: Some(expected_position),
                expected: Some(expected_position),
                observed: Some(stats_position),
            },
            true,
        )
        .await;
    Some(MakerExit::AccountingInvariant(detail))
}

fn emit_reconciliation_snapshot_error(
    output_format: OutputFormat,
    symbol: &str,
    cycle: u64,
    message: &str,
) {
    // Precursor signal: a failed reconciliation snapshot inside the freeze
    // window is an early warning that the fail-safe may not converge. Surface
    // it on stdout (JSON mode) so ingest uploads it rather than losing it to
    // local stderr only.
    if output_format == OutputFormat::Json {
        println!(
            "{}",
            serde_json::json!({
                "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "symbol": symbol,
                "cycle": cycle,
                "action": "position_reconciliation",
                "event": "snapshot_failed",
                "severity": "warning",
                "message": message,
            })
        );
    } else {
        eprintln!("⚠️  bounded position reconciliation snapshot failed: {message}");
    }
}

fn emit_ledger_sync(
    output_format: OutputFormat,
    symbol: &str,
    starting_position: f64,
    baseline_mark: f64,
    historical_orders: usize,
    historical_trades: usize,
) {
    if output_format == OutputFormat::Json {
        println!(
            "{}",
            serde_json::json!({
                "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "symbol": symbol,
                "action": "ledger_sync",
                "event": "complete",
                "starting_position": starting_position,
                "baseline_mark": baseline_mark,
                "pnl_baseline": 0.0,
                "historical_maker_orders": historical_orders,
                "historical_maker_trades_ignored": historical_trades,
                "history_window_seconds": 24 * 60 * 60,
                "history_order_limit": 100,
                "history_trade_limit": 500,
                "current_run_fills": 0,
            })
        );
        if starting_position.abs() > f64::EPSILON {
            println!(
                "{}",
                serde_json::json!({
                    "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    "symbol": symbol,
                    "action": "inventory_adopted",
                    "event": "complete",
                    "starting_position": starting_position,
                    "baseline_mark": baseline_mark,
                    "pnl_baseline": 0.0,
                })
            );
        }
    } else {
        eprintln!(
            "✅ maker ledger synchronized: position={starting_position:+.8}, baseline mark={baseline_mark:.8}, ignored historical fills={historical_trades}"
        );
    }
}

fn emit_startup_rejected(
    output_format: OutputFormat,
    symbol: &str,
    position: f64,
    max_position: f64,
) {
    let message = format!(
        "starting position {position:+.8} exceeds max_position {max_position:.8}; refusing live maker"
    );
    if output_format == OutputFormat::Json {
        println!(
            "{}",
            serde_json::json!({
                "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "symbol": symbol,
                "action": "startup_rejected",
                "event": "position_over_limit",
                "position": position,
                "max_position": max_position,
                "message": message,
            })
        );
    } else {
        eprintln!("⚠️  {message}");
    }
}

/// Reject alert thresholds that silently defeat the guard they configure.
///
/// A negative threshold turns the alert off without warning, and a percentage
/// above 100 can never fire; both leave an operator believing a protection is
/// armed when it is not.
pub(super) fn validate_alert_thresholds(
    alert_loss: f64,
    alert_inventory_pct: f64,
    alert_position_change_pct: f64,
    alert_uptime: f64,
) -> Result<()> {
    if alert_loss < 0.0 {
        return Err(anyhow::anyhow!("--alert-loss must be >= 0"));
    }
    if !(0.0..=100.0).contains(&alert_inventory_pct) {
        return Err(anyhow::anyhow!("--alert-inventory-pct must be 0..=100"));
    }
    if !(0.0..=100.0).contains(&alert_position_change_pct) {
        return Err(anyhow::anyhow!(
            "--alert-position-change-pct must be 0..=100"
        ));
    }
    if alert_uptime < 0.0 {
        return Err(anyhow::anyhow!("--alert-uptime must be >= 0"));
    }
    Ok(())
}

pub(super) async fn run_maker(
    symbol: String,
    args: MakerRunArgs,
    output_format: OutputFormat,
) -> Result<()> {
    let order_session_id = args.live.then(|| uuid::Uuid::new_v4().to_string());
    let run_uuid = uuid::Uuid::new_v4().simple().to_string();
    let run_order_prefix = format!("{}{}-", MAKER_CL_ORD_ID_PREFIX, &run_uuid[..12]);
    let mut starting_position = 0.0_f64;
    let mut baseline_mark = 0.0_f64;
    let mut session_started_at = chrono::Utc::now().timestamp();

    if let Some(after) = args.controlled_disconnect_after {
        if !args.live {
            return Err(anyhow::anyhow!(
                "--controlled-disconnect-after requires --live"
            ));
        }
        if after == 0 || after > 60 {
            return Err(anyhow::anyhow!(
                "--controlled-disconnect-after must be between 1 and 60 seconds"
            ));
        }
    }
    if args.order_response_reconnect_attempts > 10 {
        return Err(anyhow::anyhow!(
            "--order-response-reconnect-attempts must be between 0 and 10"
        ));
    }
    if args.order_response_reconnect_attempts > 0
        && !(1..=60).contains(&args.order_response_reconnect_backoff)
    {
        return Err(anyhow::anyhow!(
            "--order-response-reconnect-backoff must be between 1 and 60 seconds when reconnect is enabled"
        ));
    }
    if args.account_stream_reconnect_attempts > 10 {
        return Err(anyhow::anyhow!(
            "--account-stream-reconnect-attempts must be between 0 and 10"
        ));
    }
    if args.account_stream_reconnect_attempts > 0
        && !(1..=60).contains(&args.account_stream_reconnect_backoff)
    {
        return Err(anyhow::anyhow!(
            "--account-stream-reconnect-backoff must be between 1 and 60 seconds when reconnect is enabled"
        ));
    }
    let mut client = match order_session_id.as_deref() {
        Some(session_id) => StandXClient::new()?.with_session_id(session_id),
        None => StandXClient::new()?,
    };

    // ---- Startup: symbol metadata + invariants (fail fast) ----
    let infos = client.get_symbol_info().await?;
    let info = infos
        .iter()
        .find(|i| i.symbol.eq_ignore_ascii_case(&symbol))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Unknown symbol '{}'. Available: {}",
                symbol,
                infos
                    .iter()
                    .map(|i| i.symbol.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    if info.status != "trading" {
        return Err(anyhow::anyhow!(
            "Symbol {} is not trading (status: {})",
            info.symbol,
            info.status
        ));
    }
    let symbol = info.symbol.clone(); // canonical casing

    let min_order_qty: f64 = info.min_order_qty.parse().unwrap_or(0.0);
    let cfg = MakerConfig {
        spread_bps: args.spread_bps,
        band_bps: args.band_bps,
        level_step_bps: args.level_step_bps,
        refresh_bps: args.refresh_bps,
        levels: args.levels.max(1),
        size: args.size,
        max_position: args.max_position,
        skew_bps: args.skew_bps,
        price_decimals: info.price_tick_decimals,
        qty_decimals: info.qty_tick_decimals,
        min_order_qty,
    };

    if cfg.spread_bps <= 0.0 {
        return Err(anyhow::anyhow!("--spread-bps must be > 0"));
    }
    if cfg.skew_bps < 0.0 {
        return Err(anyhow::anyhow!("--skew-bps must be >= 0"));
    }
    if !(0.0..=100.0).contains(&args.inventory_exit_pct) || args.inventory_exit_qty < 0.0 {
        return Err(anyhow::anyhow!(
            "--inventory-exit-pct must be 0..=100 and --inventory-exit-qty must be >= 0"
        ));
    }
    if (args.inventory_exit_pct > 0.0) != (args.inventory_exit_qty > 0.0) {
        return Err(anyhow::anyhow!(
            "active inventory exit requires both --inventory-exit-pct and --inventory-exit-qty"
        ));
    }
    validate_alert_thresholds(
        args.alert_loss,
        args.alert_inventory_pct,
        args.alert_position_change_pct,
        args.alert_uptime,
    )?;
    if cfg.band_bps <= cfg.spread_bps {
        return Err(anyhow::anyhow!(
            "--band-bps ({}) must be greater than --spread-bps ({}): quotes clamped to the band edge would sit exactly at the boundary",
            cfg.band_bps,
            cfg.spread_bps
        ));
    }
    let rounded_size = maker::round_to_decimals(cfg.size, cfg.qty_decimals);
    if rounded_size < cfg.min_order_qty || rounded_size <= 0.0 {
        return Err(anyhow::anyhow!(
            "--size {} (rounded to {} at {} decimals) is below min order qty {} for {}",
            cfg.size,
            rounded_size,
            cfg.qty_decimals,
            cfg.min_order_qty,
            symbol
        ));
    }
    if cfg.refresh_bps >= cfg.spread_bps {
        eprintln!(
            "⚠️  --refresh-bps ({}) >= --spread-bps ({}): quotes will be held through large drifts",
            cfg.refresh_bps, cfg.spread_bps
        );
    }
    if cfg.levels > 1
        && cfg.spread_bps + (cfg.levels - 1) as f64 * cfg.level_step_bps >= cfg.band_bps
    {
        eprintln!("⚠️  outer quote levels exceed the band and will be clamped/collapsed");
    }

    let notifier = MakerNotifier::new(
        output_format,
        args.alert_webhook.clone(),
        args.alert_webhook_format,
    );

    // ---- Live gating & clean start ----
    let mut order_responses = None;
    let mut order_commands = None;
    let mut order_response_health = None;
    let mut order_response_handle = None;
    let mut account_events = None;
    let mut account_stream_health: Option<AccountStreamHealth> = None;
    let mut account_stream_handle = None;
    let mut account_stream_epoch = 1_u64;
    let mut live_account_poll = None;
    if args.live {
        if std::env::var(LIVE_MAKER_ENV).ok().as_deref() != Some("1") {
            return Err(anyhow::anyhow!(
                "live mode not yet enabled: it has not been supervised-tested against production. Set {}=1 to unlock (at your own risk).",
                LIVE_MAKER_ENV
            ));
        }
        // A live run with no push channel is how #220 happens: if the process
        // dies (SIGKILL/OOM/panic/host down) nobody is notified and resting
        // orders are left on the venue. Refuse to start live without a
        // webhook, and refuse if the webhook can never fire because every
        // alert threshold is disabled.
        if args.alert_webhook.is_none() {
            return Err(anyhow::anyhow!(
                "live mode requires --alert-webhook so the maker can push risk/stop notifications; refusing to run live with no push channel"
            ));
        }
        if args.alert_loss <= 0.0
            && args.alert_inventory_pct <= 0.0
            && args.alert_position_change_pct <= 0.0
            && args.alert_uptime <= 0.0
        {
            return Err(anyhow::anyhow!(
                "live mode requires at least one alert threshold (--alert-loss, --alert-inventory-pct, --alert-position-change-pct, or --alert-uptime); all are 0 so the webhook would never fire"
            ));
        }
        let creds = Credentials::load()?;
        if creds.is_expired() {
            return Err(anyhow::anyhow!(
                "Credentials expired. Run 'standx auth login' first."
            ));
        }
        if creds.private_key.is_empty() {
            return Err(anyhow::anyhow!(
                "Live mode requires a private key for order signing. Run 'standx auth login' with --private-key."
            ));
        }
        let open_orders = client.get_open_orders(Some(&symbol)).await?;
        let manual_orders = open_orders
            .iter()
            .filter(|order| !is_maker_order(order))
            .count();
        if manual_orders > 0 {
            eprintln!(
                "ℹ️  preserving {} manually-managed order(s) on {}; only {} orders are managed",
                manual_orders, symbol, MAKER_CL_ORD_ID_PREFIX
            );
        }
        // Clean only leftover orders owned by this maker. Manual/API orders
        // are not part of the strategy's reconciliation state and must never
        // be adopted or cancelled as stale.
        cancel_maker_orders_with_retry(&client, &symbol, 3, output_format).await?;

        // Establish the session ledger boundary before any new order can be
        // submitted. Existing inventory is adopted at the current mark, so
        // maker-session PnL starts at zero while account upnl remains intact.
        let history_to = chrono::Utc::now().timestamp();
        let history_from = history_to.saturating_sub(24 * 60 * 60);
        let (positions, startup_market, filled_orders, historical_trades, balance) = tokio::join!(
            client.get_positions(Some(&symbol)),
            market_snapshot(&client, &symbol, None),
            client.get_order_history(Some(&symbol), Some(100)),
            client.get_user_trades(&symbol, history_from, history_to, Some(500)),
            client.get_balance(),
        );
        let positions = positions?;
        let (mark, _, _, _) = startup_market?;
        let filled_orders = filled_orders?;
        let historical_trades = historical_trades?;
        let balance = balance?;
        starting_position = position_for_symbol(&positions, &symbol)?;
        baseline_mark = mark;
        live_account_poll = Some(LiveAccountPollState::new(
            balance,
            std::time::Instant::now(),
        ));

        let historical_order_ids = filled_orders
            .iter()
            .filter(|order| is_maker_order(order))
            .map(|order| {
                order.id.parse::<u64>().map_err(|_| {
                    anyhow::anyhow!(
                        "historical maker order has non-integer exchange ID '{}'",
                        order.id
                    )
                })
            })
            .collect::<Result<HashSet<_>>>()?;
        let historical_maker_orders = historical_order_ids.len();
        let historical_maker_trades = historical_trades
            .iter()
            .filter(|trade| {
                trade
                    .order_id
                    .is_some_and(|order_id| historical_order_ids.contains(&order_id))
            })
            .count();

        if !maker::position_within_limit(starting_position, cfg.max_position, cfg.qty_decimals) {
            emit_startup_rejected(output_format, &symbol, starting_position, cfg.max_position);
            let message = format!(
                "starting position {starting_position:+.8} exceeds max_position {:.8}",
                cfg.max_position
            );
            notifier
                .risk(
                    RiskNotice {
                        kind: "startup_position_limit",
                        severity: "critical",
                        event: "startup_rejected",
                        message: &message,
                        symbol: &symbol,
                        cycle: 0,
                        position_before: None,
                        position_after: Some(starting_position),
                        expected: None,
                        observed: Some(starting_position),
                    },
                    true,
                )
                .await;
            return Err(anyhow::anyhow!(
                "starting position {:+.8} exceeds max_position {:.8}",
                starting_position,
                cfg.max_position
            ));
        }
        session_started_at = chrono::Utc::now().timestamp();

        // Authenticated account state is a hard live dependency. Connect it
        // before order-response readiness, then require a second REST
        // snapshot so events buffered during authentication cannot create an
        // unobserved startup gap.
        let account_stream = AccountStream::new(account_stream_epoch)?;
        let (events, health, handle) = account_stream
            .connect(&[
                AccountChannel::Order,
                AccountChannel::Position,
                AccountChannel::Trade,
                AccountChannel::Balance,
            ])
            .await?;
        let post_auth_positions = client.get_positions(Some(&symbol)).await?;
        let post_auth_position = position_for_symbol(&post_auth_positions, &symbol)?;
        let qty_tolerance = 10_f64.powi(-(cfg.qty_decimals as i32)) / 2.0;
        if (post_auth_position - starting_position).abs() > qty_tolerance {
            handle.abort();
            notifier
                .risk(
                    RiskNotice {
                        kind: "position_reconciliation",
                        severity: "critical",
                        event: "startup_sync_failed",
                        message: "position changed while the account stream was authenticating",
                        symbol: &symbol,
                        cycle: 0,
                        position_before: Some(starting_position),
                        position_after: Some(post_auth_position),
                        expected: Some(starting_position),
                        observed: Some(post_auth_position),
                    },
                    true,
                )
                .await;
            return Err(anyhow::anyhow!(
                "position changed while account stream was authenticating: baseline {starting_position:+.8}, snapshot {post_auth_position:+.8}"
            ));
        }
        account_events = Some(events);
        account_stream_health = Some(health);
        account_stream_handle = Some(handle);

        let stream = OrderResponseStream::new(
            order_session_id
                .as_deref()
                .expect("live maker must have an order session"),
        )?;
        let (commands, responses, health, handle) = stream.connect().await?;
        if let Some(after) = args.controlled_disconnect_after {
            let health_for_fault = health.clone();
            let abort = handle.abort_handle();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(after)).await;
                // Aborting drops the local WebSocket halves; set health first
                // so the maker exits through its existing fail-safe path even
                // if the runtime delays observing the socket close.
                health_for_fault.mark_unhealthy(format!(
                    "controlled fault injection closed the order-response stream after {after}s"
                ));
                abort.abort();
            });
            eprintln!(
                "⚠️ controlled fault injection armed: closing order-response stream after {after}s"
            );
        }
        order_responses = Some(responses);
        order_commands = Some(commands);
        order_response_health = Some(health);
        order_response_handle = Some(handle);
        emit_ledger_sync(
            output_format,
            &symbol,
            starting_position,
            baseline_mark,
            historical_maker_orders,
            historical_maker_trades,
        );
        if starting_position.abs() > 10_f64.powi(-(cfg.qty_decimals as i32)) / 2.0 {
            let message = format!("adopted non-zero starting inventory {starting_position:+.8}");
            notifier
                .risk(
                    RiskNotice {
                        kind: "inventory_adopted",
                        severity: "warning",
                        event: "startup",
                        message: &message,
                        symbol: &symbol,
                        cycle: 0,
                        position_before: Some(0.0),
                        position_after: Some(starting_position),
                        expected: Some(starting_position),
                        observed: Some(starting_position),
                    },
                    false,
                )
                .await;
        }
    }

    let mode = if args.live { "LIVE" } else { "PAPER" };
    if output_format == OutputFormat::Table {
        println!("┌──────────────────────────────────────────────────────────┐");
        println!("│ standx maker — {} mode on {}", mode, symbol);
        println!(
            "│ spread {}bps | band {}bps | refresh {}bps | {} level(s)",
            cfg.spread_bps, cfg.band_bps, cfg.refresh_bps, cfg.levels
        );
        println!(
            "│ size {} | max-position {} | interval {}s",
            cfg.size, cfg.max_position, args.interval
        );
        if cfg.skew_bps > 0.0 {
            println!(
                "│ inventory skew {}bps (live only; paper holds no position)",
                cfg.skew_bps
            );
        }
        if args.inventory_exit_pct > 0.0 {
            println!(
                "│ active exit: {}% of max, reduce-only chunks of {} (live only)",
                args.inventory_exit_pct, args.inventory_exit_qty
            );
        }
        println!(
            "│ ticks: price {}dp, qty {}dp | min qty {}",
            cfg.price_decimals, cfg.qty_decimals, cfg.min_order_qty
        );
        if !args.live {
            println!("│ paper mode: no real orders; fills are simulated when the");
            println!("│ touch crosses a quote, so position & skew move. --live for real.");
        } else {
            println!(
                "│ ⚠️  LIVE: the bot manages only {} orders on {}",
                MAKER_CL_ORD_ID_PREFIX, symbol
            );
            println!("│ manual/API orders are preserved and ignored.");
            println!(
                "│ order-response recovery: {} attempt(s), {}s base backoff",
                args.order_response_reconnect_attempts, args.order_response_reconnect_backoff
            );
            println!(
                "│ account-stream recovery: {} attempt(s), {}s base backoff",
                args.account_stream_reconnect_attempts, args.account_stream_reconnect_backoff
            );
        }
        if args.no_ws {
            println!("│ feed: REST polling (--no-ws)");
        } else {
            println!(
                "│ feed: websocket (REST fallback) | divergence guard {}bps",
                args.max_divergence_bps
            );
        }
        if args.vol_pause_bps > 0.0 {
            println!(
                "│ vol breaker: halt at {}bps range / {} cycles (resume < {}bps)",
                args.vol_pause_bps,
                args.vol_window.max(1),
                args.vol_pause_bps / 2.0
            );
        }
        if args.stop_loss > 0.0 {
            println!(
                "│ stop-loss: session PnL -{} → fail-safe shutdown",
                args.stop_loss
            );
        }
        if args.alert_loss > 0.0
            || args.alert_inventory_pct > 0.0
            || args.alert_position_change_pct > 0.0
            || args.alert_uptime > 0.0
            || args.alert_equity_below > 0.0
            || args.alert_margin_below > 0.0
        {
            let mut parts = Vec::new();
            if args.alert_loss > 0.0 {
                parts.push(format!("loss -{}", args.alert_loss));
            }
            if args.alert_inventory_pct > 0.0 {
                parts.push(format!("inv {}%", args.alert_inventory_pct));
            }
            if args.alert_position_change_pct > 0.0 {
                parts.push(format!("position Δ {}%", args.alert_position_change_pct));
            }
            if args.alert_uptime > 0.0 {
                parts.push(format!("uptime {}%", args.alert_uptime));
            }
            if args.alert_equity_below > 0.0 {
                parts.push(format!("equity <{}", args.alert_equity_below));
            }
            if args.alert_margin_below > 0.0 {
                parts.push(format!("margin <{}", args.alert_margin_below));
            }
            let sink = if args.alert_webhook.is_some() {
                format!("stderr + webhook ({:?})", args.alert_webhook_format).to_lowercase()
            } else {
                "stderr".to_string()
            };
            println!("│ risk alerts: {} → {}", parts.join(", "), sink);
        }
        println!("│ Ctrl+C to stop (cancels maker-owned resting orders on exit)");
        println!("└──────────────────────────────────────────────────────────┘");
    }

    // Notify start (fire-and-forget; the process keeps running).
    notifier
        .lifecycle(
            "started",
            &format!(
            "🟢 maker started — {} {} | spread {}bps band {}bps size {} | {} | order-response reconnects {}",
            mode,
            symbol,
            cfg.spread_bps,
            cfg.band_bps,
            cfg.size,
            if args.no_ws { "REST" } else { "WS" },
            if args.live {
                args.order_response_reconnect_attempts
            } else {
                0
            }
            ),
            &symbol,
            false,
        )
    .await;

    // ---- Market feed (WS primary, REST fallback) ----
    let (feed, mut updates, feed_handle) = if args.no_ws {
        (None, None, None)
    } else {
        let (state, rx, handle) = spawn_market_feed(symbol.clone(), args.verbose);
        (Some(state), Some(rx), Some(handle))
    };

    // ---- Loop state ----
    let mut cycle: u64 = 0;
    let mut resting: Vec<RestingQuote> = Vec::new(); // paper-mode book
    let mut account_projection = args.live.then(|| {
        MakerAccountProjection::new(
            account_stream_epoch,
            run_order_prefix.clone(),
            starting_position,
        )
    });
    let mut inventory_exit_pending = false;
    let mut ledger = MakerLedger::new(starting_position);
    let mut position_alert_anchor =
        PositionAlertAnchor::new(starting_position, args.alert_position_change_pct);
    let mut consecutive_errors: u32 = 0;
    let mut total_places: u64 = 0;
    let mut total_cancels: u64 = 0;
    let mut total_holds: u64 = 0;
    let mut total_fills: u64 = 0;
    let mut total_halted: u64 = 0;
    let mut sim_position: f64 = 0.0; // paper-mode simulated inventory
    let mut stats = if args.live {
        MakerStats::with_inventory_baseline(starting_position, baseline_mark)
    } else {
        MakerStats::default()
    };
    let qty_tolerance = 10_f64.powi(-(cfg.qty_decimals as i32)) / 2.0;
    let mut breaker = VolBreaker::new(args.vol_window.max(1) as usize, args.vol_pause_bps);
    let mut alerts =
        AlertMonitor::new(args.alert_loss, args.alert_inventory_pct, args.alert_uptime)
            .with_account_floors(args.alert_equity_below, args.alert_margin_below);
    let mut last_mark: Option<f64> = None;
    let mut last_src: Option<&'static str> = None;
    let mut order_response_reconnect_attempts_used = 0_u32;
    let mut account_stream_reconnect_attempts_used = 0_u32;
    let mut account_position_mismatch: Option<f64> = None;
    // JWT expiry monitor: highest severity already alerted, plus a throttle so
    // credentials are only reloaded from disk/env periodically.
    let mut token_expiry_alerted = TokenExpiryLevel::Ok;
    let mut last_token_expiry_check: Option<std::time::Instant> = None;
    let mut runtime_state = MakerState::starting();
    runtime_state.handle(MakerEvent::StartupReady);

    let exit = 'main: loop {
        if args.live {
            // JWT expiry monitor. There is no renewal endpoint, so we can only
            // warn: escalate through Warning → Critical and alert once per band.
            let due = last_token_expiry_check
                .map(|last| last.elapsed() >= TOKEN_EXPIRY_CHECK_INTERVAL)
                .unwrap_or(true);
            if due {
                last_token_expiry_check = Some(std::time::Instant::now());
                if let Ok(creds) = Credentials::load() {
                    let remaining = creds.remaining_seconds();
                    let level = token_expiry_level(
                        remaining,
                        TOKEN_EXPIRY_WARN_SECS,
                        TOKEN_EXPIRY_CRITICAL_SECS,
                    );
                    if level > token_expiry_alerted {
                        token_expiry_alerted = level;
                        let (severity, event) = match level {
                            TokenExpiryLevel::Critical => ("critical", "token_expiry_critical"),
                            _ => ("warning", "token_expiry_warning"),
                        };
                        let minutes = remaining / 60;
                        let message = format!(
                            "auth token expires in ~{minutes}m ({}); no renewal endpoint — run 'standx auth login' before it lapses or the bot will halt",
                            creds.expires_at_string()
                        );
                        notifier
                            .risk(
                                RiskNotice {
                                    kind: "token_expiry",
                                    severity,
                                    event,
                                    message: &message,
                                    symbol: &symbol,
                                    cycle,
                                    position_before: None,
                                    position_after: None,
                                    expected: None,
                                    observed: None,
                                },
                                false,
                            )
                            .await;
                    }
                }
            }
            if let Some(health) = account_stream_health
                .as_ref()
                .filter(|health| !health.is_healthy())
            {
                let detail = health.failure_reason().unwrap_or_else(|| {
                    "account stream became unhealthy without a recorded reason".to_string()
                });
                runtime_state.handle(MakerEvent::AccountStreamDisconnected(detail.clone()));
                let cleanup_token =
                    match take_cleanup_effect(&mut runtime_state, RecoveryTarget::AccountStream) {
                        Ok(token) => token,
                        Err(error) => {
                            break stop_requested_exit(
                                &mut runtime_state,
                                RuntimeStopReason::CleanupFailure {
                                    target: RecoveryTarget::AccountStream,
                                    reason: error.to_string(),
                                },
                            );
                        }
                    };
                let message = format!(
                    "account stream unavailable; placements frozen and cleanup starting: {detail}"
                );
                notifier
                    .risk(
                        RiskNotice {
                            kind: "account_stream",
                            severity: "warning",
                            event: "disconnected_frozen",
                            message: &message,
                            symbol: &symbol,
                            cycle,
                            position_before: None,
                            position_after: None,
                            expected: Some(ledger.expected_position),
                            observed: None,
                        },
                        false,
                    )
                    .await;
                // Freeze immediately: no further cycle can place while the
                // authoritative account stream is unavailable.
                if let Err(cleanup_error) =
                    cancel_maker_orders_with_retry(&client, &symbol, 3, output_format).await
                {
                    runtime_state.handle(MakerEvent::CleanupFailed {
                        token: cleanup_token,
                        reason: format!(
                            "account stream disconnected ({detail}); freeze cleanup failed: {cleanup_error}"
                        ),
                    });
                    break match take_stop_effect(&mut runtime_state) {
                        Ok(exit) => exit,
                        Err(error) => MakerExit::PositionReconciliation(error.to_string()),
                    };
                }
                resting.clear();
                if let Some(projection) = account_projection.as_mut() {
                    projection.clear_orders_and_pending();
                }
                inventory_exit_pending = false;
                if let Some(handle) = account_stream_handle.take() {
                    handle.abort();
                }
                account_events.take();
                runtime_state.handle(MakerEvent::CleanupCompleted(cleanup_token));
                let recovery_token =
                    match take_recovery_effect(&mut runtime_state, RecoveryTarget::AccountStream) {
                        Ok(token) => token,
                        Err(error) => {
                            break stop_requested_exit(
                                &mut runtime_state,
                                RuntimeStopReason::PositionReconciliation(error.to_string()),
                            );
                        }
                    };

                let qty_tolerance = 10_f64.powi(-(cfg.qty_decimals as i32)) / 2.0;
                if account_stream_reconnect_attempts_used >= args.account_stream_reconnect_attempts
                {
                    break recovery_failed_exit(
                        &mut runtime_state,
                        recovery_token,
                        format!(
                            "account stream disconnected ({detail}); reconnect disabled or budget exhausted ({}/{})",
                            account_stream_reconnect_attempts_used,
                            args.account_stream_reconnect_attempts
                        ),
                    );
                }

                let mut last_connect_error: Option<String> = None;
                let mut reconnected = None;
                while account_stream_reconnect_attempts_used
                    < args.account_stream_reconnect_attempts
                {
                    account_stream_reconnect_attempts_used += 1;
                    let attempt = account_stream_reconnect_attempts_used;
                    account_stream_epoch = account_stream_epoch.saturating_add(1);
                    let reconnect = async {
                        let stream = AccountStream::new(account_stream_epoch)?;
                        stream
                            .connect(&[
                                AccountChannel::Order,
                                AccountChannel::Position,
                                AccountChannel::Trade,
                                AccountChannel::Balance,
                            ])
                            .await
                            .map_err(anyhow::Error::from)
                    };
                    match tokio::time::timeout(Duration::from_secs(15), reconnect).await {
                        Ok(Ok(triple)) => {
                            reconnected = Some(triple);
                            break;
                        }
                        Ok(Err(error)) => {
                            last_connect_error = Some(format!("connect failed: {error}"));
                        }
                        Err(_) => {
                            last_connect_error =
                                Some("connect timed out after 15 seconds".to_string());
                        }
                    }
                    eprintln!(
                        "⚠️  account stream reconnect attempt {}/{} failed: {}",
                        attempt,
                        args.account_stream_reconnect_attempts,
                        last_connect_error.as_deref().unwrap_or("unknown error")
                    );
                    if attempt < args.account_stream_reconnect_attempts {
                        let multiplier = 1_u32 << attempt.saturating_sub(1).min(4);
                        tokio::time::sleep(
                            Duration::from_secs(args.account_stream_reconnect_backoff)
                                .saturating_mul(multiplier),
                        )
                        .await;
                    }
                }

                let Some((mut events, health, handle)) = reconnected else {
                    runtime_state.handle(MakerEvent::RecoveryFailed {
                        token: recovery_token,
                        reason: format!(
                            "account stream disconnected ({detail}); reconnect exhausted: {}",
                            last_connect_error
                                .unwrap_or_else(|| "no attempts available".to_string())
                        ),
                    });
                    break match take_stop_effect(&mut runtime_state) {
                        Ok(exit) => exit,
                        Err(error) => MakerExit::PositionReconciliation(error.to_string()),
                    };
                };

                let projection = account_projection
                    .as_mut()
                    .expect("live account reconnect requires initialized projection");
                projection.reset(account_stream_epoch, ledger.expected_position);

                let mut reconnect_fills = match apply_account_events(
                    &mut events,
                    &mut AccountEventState {
                        ledger: &mut ledger,
                        stats: &mut stats,
                        projection,
                    },
                    &AccountEventContext {
                        symbol: &symbol,
                        run_order_prefix: &run_order_prefix,
                        mark: last_mark.unwrap_or(baseline_mark),
                        cycle,
                        output_format,
                    },
                ) {
                    Ok(outcome) => outcome.fills,
                    Err(error) => {
                        handle.abort();
                        break recovery_failed_exit(
                            &mut runtime_state,
                            recovery_token,
                            format!("account stream reconnect event validation failed: {error}"),
                        );
                    }
                };
                let positions = match client.get_positions(Some(&symbol)).await {
                    Ok(positions) => positions,
                    Err(error) => {
                        handle.abort();
                        break recovery_failed_exit(
                            &mut runtime_state,
                            recovery_token,
                            format!("account stream reconnect snapshot failed: {error}"),
                        );
                    }
                };
                let mut observed = match position_for_symbol(&positions, &symbol) {
                    Ok(position) => position,
                    Err(error) => {
                        handle.abort();
                        break recovery_failed_exit(
                            &mut runtime_state,
                            recovery_token,
                            error.to_string(),
                        );
                    }
                };

                if (observed - ledger.expected_position).abs() > qty_tolerance {
                    // WS events can lag REST settlement across a reconnect: give
                    // a bounded window to explain the gap with REST trades
                    // (mirrors the in-cycle freeze-path reconciliation) before
                    // failing closed.
                    let mut gap_closed = false;
                    for delay in [500_u64, 1_000, 1_500] {
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                        match apply_account_events(
                            &mut events,
                            &mut AccountEventState {
                                ledger: &mut ledger,
                                stats: &mut stats,
                                projection,
                            },
                            &AccountEventContext {
                                symbol: &symbol,
                                run_order_prefix: &run_order_prefix,
                                mark: last_mark.unwrap_or(baseline_mark),
                                cycle,
                                output_format,
                            },
                        ) {
                            Ok(outcome) => reconnect_fills += outcome.fills,
                            Err(error) => {
                                handle.abort();
                                break 'main recovery_failed_exit(
                                    &mut runtime_state,
                                    recovery_token,
                                    format!(
                                        "account stream reconnect event validation failed during REST backfill: {error}"
                                    ),
                                );
                            }
                        }
                        match reconcile_ledger_snapshot(
                            &client,
                            ReconcileRequest {
                                symbol: &symbol,
                                session_started_at,
                                run_order_prefix: &run_order_prefix,
                                qty_tolerance,
                                mark: last_mark.unwrap_or(baseline_mark),
                            },
                            &mut ledger,
                            &mut stats,
                        )
                        .await
                        {
                            Ok((obs, fills)) => {
                                observed = obs;
                                reconnect_fills += fills.len() as u64;
                                for fill in &fills {
                                    emit_live_fill(fill, &symbol, cycle, output_format);
                                }
                                if (observed - ledger.expected_position).abs() <= qty_tolerance {
                                    gap_closed = true;
                                    break;
                                }
                            }
                            Err(error) => eprintln!(
                                "⚠️  account stream reconnect REST trade backfill failed: {error}"
                            ),
                        }
                    }
                    if !gap_closed {
                        handle.abort();
                        break recovery_failed_exit(
                            &mut runtime_state,
                            recovery_token,
                            format!(
                                "account stream reconnect snapshot expected {:+.8}, observed {:+.8} (REST trade backfill did not close the gap)",
                                ledger.expected_position, observed
                            ),
                        );
                    }
                }

                account_events = Some(events);
                account_stream_health = Some(health);
                account_stream_handle = Some(handle);
                if let Some(projection) = account_projection.as_mut() {
                    let generation = projection.generation();
                    projection.apply(
                        generation,
                        AccountProjectionEvent::PositionObserved { position: observed },
                    );
                }
                total_fills += reconnect_fills;
                runtime_state.handle(MakerEvent::RecoverySucceeded(recovery_token));
                notifier
                    .risk(
                        RiskNotice {
                            kind: "account_stream",
                            severity: "resolved",
                            event: "reconnected",
                            message: "account stream reauthenticated; buffered events and REST trades reconciled against the venue position",
                            symbol: &symbol,
                            cycle,
                            position_before: None,
                            position_after: None,
                            expected: Some(ledger.expected_position),
                            observed: Some(observed),
                        },
                        false,
                    )
                .await;
                continue;
            }
            if let Some(health) = order_response_health
                .as_ref()
                .filter(|health| !health.is_healthy())
            {
                let detail = health.failure_reason().unwrap_or_else(|| {
                    "order-response stream became unhealthy without a recorded reason".to_string()
                });
                runtime_state.handle(MakerEvent::OrderResponseDisconnected(detail.clone()));
                let cleanup_token =
                    match take_cleanup_effect(&mut runtime_state, RecoveryTarget::OrderResponse) {
                        Ok(token) => token,
                        Err(error) => {
                            break stop_requested_exit(
                                &mut runtime_state,
                                RuntimeStopReason::OrderResponse(error.to_string()),
                            );
                        }
                    };
                let controlled_fault = detail.starts_with("controlled fault injection");
                let reconnect_available = order_response_reconnect_available(
                    &detail,
                    order_response_reconnect_attempts_used,
                    args.order_response_reconnect_attempts,
                );
                // Mirror the account-stream path: the order-response stream was
                // previously silent on the webhook across disconnect/reconnect.
                let disconnect_message =
                    format!("order-response stream unavailable; placements frozen: {detail}");
                notifier
                    .risk(
                        RiskNotice {
                            kind: "order_response",
                            severity: "warning",
                            event: "disconnected_frozen",
                            message: &disconnect_message,
                            symbol: &symbol,
                            cycle,
                            position_before: None,
                            position_after: None,
                            expected: Some(ledger.expected_position),
                            observed: None,
                        },
                        false,
                    )
                    .await;
                if let Err(error) =
                    cancel_maker_orders_with_retry(&client, &symbol, 3, output_format).await
                {
                    runtime_state.handle(MakerEvent::CleanupFailed {
                        token: cleanup_token,
                        reason: format!("order-response freeze cleanup failed: {error}"),
                    });
                    break match take_stop_effect(&mut runtime_state) {
                        Ok(exit) => exit,
                        Err(error) => MakerExit::OrderResponse(error.to_string()),
                    };
                }
                resting.clear();
                if let Some(projection) = account_projection.as_mut() {
                    projection.clear_orders_and_pending();
                }
                inventory_exit_pending = false;
                runtime_state.handle(MakerEvent::CleanupCompleted(cleanup_token));
                let recovery_token =
                    match take_recovery_effect(&mut runtime_state, RecoveryTarget::OrderResponse) {
                        Ok(token) => token,
                        Err(error) => {
                            break stop_requested_exit(
                                &mut runtime_state,
                                RuntimeStopReason::OrderResponse(error.to_string()),
                            );
                        }
                    };
                if reconnect_available {
                    if let Some(handle) = order_response_handle.take() {
                        handle.abort();
                    }
                    order_responses.take();
                    order_commands.take();
                    match reconnect_order_response(ReconnectRequest {
                        cleanup_client: client.clone(),
                        symbol: &symbol,
                        session_started_at,
                        run_order_prefix: &run_order_prefix,
                        expected_position: ledger.expected_position,
                        qty_tolerance: 10_f64.powi(-(cfg.qty_decimals as i32)) / 2.0,
                        output_format,
                        attempts_used: order_response_reconnect_attempts_used,
                        max_attempts: args.order_response_reconnect_attempts,
                        base_backoff: Duration::from_secs(args.order_response_reconnect_backoff),
                        original_failure: &detail,
                    })
                    .await
                    {
                        Ok((reconnected, attempts_used)) => {
                            client = reconnected.client;
                            order_commands = Some(reconnected.commands);
                            order_responses = Some(reconnected.responses);
                            order_response_health = Some(reconnected.health);
                            order_response_handle = Some(reconnected.handle);
                            order_response_reconnect_attempts_used = attempts_used;
                            // Cleanup verified an empty maker book. The next
                            // cycle rebuilds exchange state before it may place.
                            resting.clear();
                            if let Some(projection) = account_projection.as_mut() {
                                projection.clear_orders_and_pending();
                            }
                            consecutive_errors = 0;
                            runtime_state.handle(MakerEvent::RecoverySucceeded(recovery_token));
                            notifier
                                .risk(
                                    RiskNotice {
                                        kind: "order_response",
                                        severity: "resolved",
                                        event: "reconnected",
                                        message: "order-response stream reconnected; maker book verified empty before quoting resumes",
                                        symbol: &symbol,
                                        cycle,
                                        position_before: None,
                                        position_after: None,
                                        expected: Some(ledger.expected_position),
                                        observed: None,
                                    },
                                    false,
                                )
                                .await;
                            continue;
                        }
                        Err(error) => {
                            if let Some(reconciliation) =
                                error.downcast_ref::<PositionReconciliationError>()
                            {
                                if output_format == OutputFormat::Json {
                                    println!(
                                        "{}",
                                        serde_json::json!({
                                            "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                                            "symbol": symbol,
                                            "action": "position_reconciliation",
                                            "event": "failed_during_reconnect",
                                            "expected_position": reconciliation.expected,
                                            "observed_position": reconciliation.observed,
                                            "message": "post-reconnect venue position cannot be explained by current-run maker fills",
                                        })
                                    );
                                }
                                let reconciliation_message = format!(
                                    "order-response reconnect failed reconciliation: {error}"
                                );
                                notifier
                                    .risk(
                                        RiskNotice {
                                            kind: "order_response",
                                            severity: "critical",
                                            event: "reconnect_failed",
                                            message: &reconciliation_message,
                                            symbol: &symbol,
                                            cycle,
                                            position_before: None,
                                            position_after: None,
                                            expected: Some(ledger.expected_position),
                                            observed: None,
                                        },
                                        true,
                                    )
                                    .await;
                                runtime_state.handle(MakerEvent::RecoveryFailed {
                                    token: recovery_token,
                                    reason: error.to_string(),
                                });
                                break match take_stop_effect(&mut runtime_state) {
                                    Ok(exit) => exit,
                                    Err(runtime_error) => {
                                        MakerExit::PositionReconciliation(runtime_error.to_string())
                                    }
                                };
                            }
                            let reconnect_failed_message = format!(
                                "{detail}; safe reconnect failed: {error}; refusing further live orders"
                            );
                            notifier
                                .risk(
                                    RiskNotice {
                                        kind: "order_response",
                                        severity: "critical",
                                        event: "reconnect_failed",
                                        message: &reconnect_failed_message,
                                        symbol: &symbol,
                                        cycle,
                                        position_before: None,
                                        position_after: None,
                                        expected: Some(ledger.expected_position),
                                        observed: None,
                                    },
                                    true,
                                )
                                .await;
                            runtime_state.handle(MakerEvent::RecoveryFailed {
                                token: recovery_token,
                                reason: reconnect_failed_message,
                            });
                            break match take_stop_effect(&mut runtime_state) {
                                Ok(exit) => exit,
                                Err(error) => MakerExit::OrderResponse(error.to_string()),
                            };
                        }
                    }
                }
                let reconnect_note = if controlled_fault {
                    "controlled fault injection requires fail-safe shutdown".to_string()
                } else if args.order_response_reconnect_attempts == 0 {
                    "safe reconnect is disabled".to_string()
                } else {
                    format!(
                        "safe reconnect budget exhausted ({}/{})",
                        order_response_reconnect_attempts_used,
                        args.order_response_reconnect_attempts
                    )
                };
                let refuse_message =
                    format!("{detail}; {reconnect_note}; refusing further live orders");
                notifier
                    .risk(
                        RiskNotice {
                            kind: "order_response",
                            severity: "critical",
                            event: "reconnect_unavailable",
                            message: &refuse_message,
                            symbol: &symbol,
                            cycle,
                            position_before: None,
                            position_after: None,
                            expected: Some(ledger.expected_position),
                            observed: None,
                        },
                        true,
                    )
                    .await;
                runtime_state.handle(MakerEvent::RecoveryFailed {
                    token: recovery_token,
                    reason: refuse_message,
                });
                break match take_stop_effect(&mut runtime_state) {
                    Ok(exit) => exit,
                    Err(error) => MakerExit::OrderResponse(error.to_string()),
                };
            }
        }
        if let Some(receiver) = order_responses.as_mut() {
            let projection = account_projection
                .as_mut()
                .expect("live order responses require initialized account projection");
            if let Err(error) = apply_order_responses(
                receiver,
                projection,
                &mut runtime_state,
                output_format,
                &symbol,
                cycle,
                cfg.price_decimals,
            ) {
                if let Some(health) = order_response_health.as_ref() {
                    health.mark_unhealthy(error.to_string());
                    continue;
                }
                break stop_requested_exit(
                    &mut runtime_state,
                    RuntimeStopReason::OrderResponse(error.to_string()),
                );
            }
        }
        if let Some(receiver) = account_events.as_mut() {
            match apply_account_events(
                receiver,
                &mut AccountEventState {
                    ledger: &mut ledger,
                    stats: &mut stats,
                    projection: account_projection
                        .as_mut()
                        .expect("live account events require initialized projection"),
                },
                &AccountEventContext {
                    symbol: &symbol,
                    run_order_prefix: &run_order_prefix,
                    mark: last_mark.unwrap_or(baseline_mark),
                    cycle,
                    output_format,
                },
            ) {
                Ok(outcome) => {
                    total_fills += outcome.fills;
                    if outcome.exit_fill_observed {
                        inventory_exit_pending = false;
                    }
                    let position = outcome.latest_position;
                    if let Some(position) = position {
                        notifier
                            .position_jump(
                                &mut position_alert_anchor,
                                PositionChange {
                                    observed: position,
                                    expected: ledger.expected_position,
                                    max_position: cfg.max_position,
                                    inventory_exit_pct: args.inventory_exit_pct,
                                    qty_tolerance,
                                    symbol: &symbol,
                                    cycle,
                                },
                            )
                            .await;
                    }
                    if let Some(position) = position.filter(|position| {
                        (*position - ledger.expected_position).abs() > qty_tolerance
                    }) {
                        account_position_mismatch = Some(position);
                    }
                }
                Err(error) => {
                    if let Some(health) = account_stream_health.as_ref() {
                        health.mark_unhealthy(error.to_string());
                        continue;
                    }
                    break stop_requested_exit(
                        &mut runtime_state,
                        RuntimeStopReason::PositionReconciliation(error.to_string()),
                    );
                }
            }
        }
        if account_position_mismatch
            .is_some_and(|position| (position - ledger.expected_position).abs() <= qty_tolerance)
        {
            account_position_mismatch = None;
        }
        if args.live {
            if let Some(exit) = accounting_invariant_exit(
                &notifier,
                &symbol,
                cycle,
                ledger.expected_position,
                stats.position(),
                qty_tolerance,
            )
            .await
            {
                break 'main exit;
            }
        }

        // Work phase raced against Ctrl+C so a slow API call can be
        // interrupted (mirrors run_watch_loop).
        let mismatch = account_position_mismatch.take();
        let exit_pending_before = inventory_exit_pending;
        let breaker_halted_before = breaker.halted();
        if runtime_state.pending_effect().is_none() {
            runtime_state.handle(MakerEvent::Timer);
        }
        let cycle_work_token = match next_runtime_effect(&mut runtime_state) {
            Some(MakerEffect::RunCycle(token)) => token,
            Some(MakerEffect::Stop(reason)) => break reason.into(),
            Some(effect) => {
                break stop_requested_exit(
                    &mut runtime_state,
                    RuntimeStopReason::PositionReconciliation(format!(
                        "runtime emitted unexpected effect before cycle: {effect:?}"
                    )),
                );
            }
            None => continue,
        };
        let work = async {
            if let Some(observed) = mismatch {
                return Err(anyhow::Error::new(PositionReconciliationError {
                    expected: ledger.expected_position,
                    observed,
                }));
            }
            let (mark, best_bid, best_ask, src) =
                market_snapshot(&client, &symbol, feed.as_ref()).await?;
            let result = maker_cycle(
                CycleRequest {
                    client: &client,
                    symbol: &symbol,
                    cfg: &cfg,
                    live: args.live,
                    cycle,
                    mark,
                    best_bid,
                    best_ask,
                    max_divergence_bps: args.max_divergence_bps,
                    inventory_exit_pct: args.inventory_exit_pct,
                    inventory_exit_qty: args.inventory_exit_qty,
                    session_started_at,
                    run_order_prefix: &run_order_prefix,
                    starting_position,
                    output_format,
                    order_commands: order_commands.as_ref(),
                    order_response_health: order_response_health.as_ref(),
                    account_stream_health: account_stream_health.as_ref(),
                },
                CycleState {
                    resting: &mut resting,
                    account_projection: account_projection.as_mut(),
                    inventory_exit_pending: &mut inventory_exit_pending,
                    ledger: &mut ledger,
                    sim_position: &mut sim_position,
                    stats: &mut stats,
                    breaker: &mut breaker,
                    live_account_poll: live_account_poll.as_mut(),
                },
            )
            .await?;
            Ok::<_, anyhow::Error>((
                result.places,
                result.cancels,
                result.holds,
                result.fills,
                mark,
                src,
                breaker.halted(),
                inventory_exit_pending,
                result.balance,
            ))
        };
        // Order lifecycle and balance events are buffered so a cycle's own
        // acknowledgement cannot tear apart a multi-order plan. Position,
        // trade, and stream-failure events can change risk or invalidate the
        // plan. They freeze the reducer before this future is dropped so the
        // queued Cleanup effect compensates for any request that may already
        // have reached the venue.
        let mut buffered_account: Vec<AccountEvent> = Vec::new();
        let mut buffered_orders: Vec<OrderResponse> = Vec::new();
        let mut cycle_invalidated_by_account = false;
        // Scope the pinned work future so it (and its ledger/pending borrows)
        // is dropped once it resolves, before the buffered events are applied.
        let cycle_result = {
            tokio::pin!(work);
            loop {
                let account_during_work = async {
                    match account_events.as_mut() {
                        Some(receiver) => receiver.recv().await,
                        None => std::future::pending().await,
                    }
                };
                let order_during_work = async {
                    match order_responses.as_mut() {
                        Some(receiver) => receiver.recv().await,
                        None => std::future::pending().await,
                    }
                };
                tokio::select! {
                    biased;
                    _ = signal::ctrl_c() => {
                        runtime_state.handle(MakerEvent::CtrlC);
                        break 'main match take_stop_effect(&mut runtime_state) {
                            Ok(exit) => exit,
                            Err(error) => MakerExit::PositionReconciliation(error.to_string()),
                        };
                    },
                    event = account_during_work => {
                        let Some(event) = event else {
                            let reason = "authenticated account stream disconnected during cycle".to_string();
                            runtime_state.handle(MakerEvent::AccountStreamDisconnected(reason.clone()));
                            if let Some(health) = account_stream_health.as_ref() {
                                health.mark_unhealthy(reason);
                            }
                            continue 'main;
                        };
                        let invalidates = account_event_invalidates_cycle(&event);
                        buffered_account.push(event);
                        if invalidates {
                            runtime_state.handle(MakerEvent::CycleInvalidated {
                                reason: "account state changed during maker cycle".to_string(),
                            });
                            cycle_invalidated_by_account = true;
                            break None;
                        }
                    },
                    response = order_during_work => {
                        let Some(response) = response else {
                            let reason = "order-response stream disconnected during cycle".to_string();
                            runtime_state.handle(MakerEvent::OrderResponseDisconnected(reason.clone()));
                            if let Some(health) = order_response_health.as_ref() {
                                health.mark_unhealthy(reason);
                            }
                            continue 'main;
                        };
                        buffered_orders.push(response);
                    },
                    result = &mut work => break Some(result),
                }
            }
        };
        if cycle_invalidated_by_account {
            if let Some(receiver) = account_events.as_mut() {
                while let Ok(event) = receiver.try_recv() {
                    buffered_account.push(event);
                }
            }
        }
        // Apply the events buffered during work, ordering order-responses
        // before account events to mirror the top-of-loop drain.
        for response in buffered_orders {
            let request_id = response.request_id.clone();
            let matched = apply_order_response(
                response,
                account_projection
                    .as_mut()
                    .expect("live order responses require initialized projection"),
                output_format,
                &symbol,
                cycle,
                cfg.price_decimals,
            );
            if let Some(request_id) = request_id {
                if !matched {
                    runtime_state.handle(MakerEvent::OrderResponseUnmatched { request_id });
                }
            }
            if matches!(
                runtime_state.pending_effect(),
                Some(MakerEffect::AbortInFlight(_))
                    | Some(MakerEffect::Cleanup {
                        target: RecoveryTarget::OrderResponse,
                        ..
                    })
            ) {
                if let Some(health) = order_response_health.as_ref() {
                    health.mark_unhealthy("order-response correlation failed closed");
                }
            }
        }
        for event in buffered_account {
            match apply_account_event(
                event,
                &mut AccountEventState {
                    ledger: &mut ledger,
                    stats: &mut stats,
                    projection: account_projection
                        .as_mut()
                        .expect("live account events require initialized projection"),
                },
                &AccountEventContext {
                    symbol: &symbol,
                    run_order_prefix: &run_order_prefix,
                    mark: last_mark.unwrap_or(baseline_mark),
                    cycle,
                    output_format,
                },
            ) {
                Ok(outcome) => {
                    total_fills += outcome.fills;
                    if outcome.exit_fill_observed {
                        inventory_exit_pending = false;
                    }
                    let position = outcome.latest_position;
                    if let Some(position) = position {
                        notifier
                            .position_jump(
                                &mut position_alert_anchor,
                                PositionChange {
                                    observed: position,
                                    expected: ledger.expected_position,
                                    max_position: cfg.max_position,
                                    inventory_exit_pct: args.inventory_exit_pct,
                                    qty_tolerance,
                                    symbol: &symbol,
                                    cycle,
                                },
                            )
                            .await;
                        if (position - ledger.expected_position).abs() > qty_tolerance {
                            account_position_mismatch = Some(position);
                        } else {
                            account_position_mismatch = None;
                        }
                    }
                }
                Err(error) => {
                    runtime_state.handle(MakerEvent::AccountStreamDisconnected(error.to_string()));
                    if let Some(health) = account_stream_health.as_ref() {
                        health.mark_unhealthy(error.to_string());
                    }
                }
            }
        }
        if args.live {
            if let Some(exit) = accounting_invariant_exit(
                &notifier,
                &symbol,
                cycle,
                ledger.expected_position,
                stats.position(),
                qty_tolerance,
            )
            .await
            {
                break 'main exit;
            }
        }

        let cycle_result = if let Some(observed) = mismatch.or(account_position_mismatch.take()) {
            Err(anyhow::Error::new(PositionReconciliationError {
                expected: ledger.expected_position,
                observed,
            }))
        } else if let Some(cycle_result) = cycle_result {
            cycle_result
        } else {
            continue 'main;
        };

        if !matches!(
            runtime_state.pending_effect(),
            None | Some(MakerEffect::RunCycle(_))
        ) && cycle_result.is_ok()
        {
            // A fail-closed event invalidated the generation while cycle work
            // was running. Do not commit its counters/alerts; the queued
            // abort/cleanup effects are consumed by the recovery path.
            continue 'main;
        }

        match cycle_result {
            Ok((places, cancels, holds, fills, mark, src, halted, exit_pending_after, balance)) => {
                runtime_state.handle(MakerEvent::CycleCompleted(cycle_work_token));
                if !matches!(
                    next_runtime_effect(&mut runtime_state),
                    Some(MakerEffect::CommitCycle(token)) if token == cycle_work_token
                ) {
                    continue 'main;
                }
                consecutive_errors = 0;
                total_places += places;
                total_cancels += cancels;
                total_holds += holds;
                total_fills += fills;
                total_halted += halted as u64;
                last_mark = Some(mark);
                if halted != breaker_halted_before {
                    let (severity, event, message) = if halted {
                        (
                            "warning",
                            "entered",
                            "volatility breaker entered; maker quotes are being pulled",
                        )
                    } else {
                        (
                            "resolved",
                            "cleared",
                            "volatility breaker cleared; quoting may resume",
                        )
                    };
                    notifier
                        .risk(
                            RiskNotice {
                                kind: "volatility_breaker",
                                severity,
                                event,
                                message,
                                symbol: &symbol,
                                cycle,
                                position_before: None,
                                position_after: Some(ledger.expected_position),
                                expected: Some(ledger.expected_position),
                                observed: None,
                            },
                            false,
                        )
                        .await;
                }
                if !exit_pending_before && exit_pending_after {
                    notifier
                        .risk(
                            RiskNotice {
                                kind: "inventory_exit",
                                severity: "warning",
                                event: "submitted",
                                message: "reduce-only inventory exit submitted",
                                symbol: &symbol,
                                cycle,
                                position_before: None,
                                position_after: Some(ledger.expected_position),
                                expected: Some(ledger.expected_position),
                                observed: None,
                            },
                            false,
                        )
                        .await;
                } else if exit_pending_before && !exit_pending_after {
                    notifier
                        .risk(
                            RiskNotice {
                                kind: "inventory_exit",
                                severity: "resolved",
                                event: "confirmed",
                                message: "reduce-only inventory exit is no longer pending after ledger reconciliation",
                                symbol: &symbol,
                                cycle,
                                position_before: None,
                                position_after: Some(ledger.expected_position),
                                expected: Some(ledger.expected_position),
                                observed: Some(ledger.expected_position),
                            },
                            false,
                        )
                    .await;
                }
                if !args.no_ws && last_src != Some(src) {
                    match src {
                        "ws" => eprintln!("✅ market feed: websocket live"),
                        _ => eprintln!(
                            "⚠️  market feed: REST fallback (websocket warming up or stale)"
                        ),
                    }
                    last_src = Some(src);
                }
                // Risk alerts: evaluate over the just-updated stats and
                // deliver any state changes (stderr always; webhook if set).
                let session_position = if args.live {
                    ledger.expected_position
                } else {
                    stats.position()
                };
                if alerts.enabled() {
                    let fired =
                        alerts.evaluate(&stats, session_position, mark, cfg.max_position, cycle);
                    for alert in fired {
                        // Await firing alerts so a breach raised on the final
                        // cycle before shutdown is not dropped with its task.
                        let await_delivery = alert.firing;
                        notifier.alert(&alert, &symbol, await_delivery).await;
                    }
                }
                // Account equity / available-margin floors. The snapshot is
                // only fetched in live mode, so these stay quiet in paper.
                if alerts.account_enabled() {
                    if let Some(balance) = balance.as_ref() {
                        let equity = balance.equity.parse::<f64>().ok();
                        let available = balance.cross_available.parse::<f64>().ok();
                        if let (Some(equity), Some(available)) = (equity, available) {
                            let fired = alerts.evaluate_account(equity, available);
                            for alert in fired {
                                let await_delivery = alert.firing;
                                notifier.alert(&alert, &symbol, await_delivery).await;
                            }
                        }
                    }
                }
                // Financial brake: a session loss breaching --stop-loss routes
                // through the fail-safe shutdown (freeze, cancel the maker
                // book, await the critical webhook, exit) — the same path the
                // other MakerExit variants use.
                if args.stop_loss > 0.0 {
                    let pnl = stats.pnl(session_position, mark);
                    if pnl <= -args.stop_loss {
                        emit_stop_loss_triggered(
                            output_format,
                            &symbol,
                            cycle,
                            pnl,
                            args.stop_loss,
                        );
                        notifier
                            .risk(
                                RiskNotice {
                                    kind: "stop_loss",
                                    severity: "critical",
                                    event: "triggered",
                                    message: &format!(
                                        "session PnL {pnl:+.2} breached stop-loss -{:.2}; shutting down",
                                        args.stop_loss
                                    ),
                                    symbol: &symbol,
                                    cycle,
                                    position_before: None,
                                    position_after: Some(ledger.expected_position),
                                    expected: Some(ledger.expected_position),
                                    observed: None,
                                },
                                true,
                            )
                            .await;
                        runtime_state.handle(MakerEvent::StopRequested(
                            RuntimeStopReason::StopLoss(format!(
                                "session PnL {pnl:+.2} <= -{:.2}",
                                args.stop_loss
                            )),
                        ));
                        break 'main match take_stop_effect(&mut runtime_state) {
                            Ok(exit) => exit,
                            Err(error) => MakerExit::PositionReconciliation(error.to_string()),
                        };
                    }
                }
            }
            Err(e) => {
                if e.downcast_ref::<ProjectionRegistryError>().is_some() {
                    let detail = format!("order-response correlation failed closed: {e}");
                    if let Some(health) = order_response_health.as_ref() {
                        health.mark_unhealthy(detail.clone());
                    }
                    runtime_state.handle(MakerEvent::OrderResponseDisconnected(detail));
                    continue 'main;
                }
                if let Some(mismatch) = e.downcast_ref::<PositionReconciliationError>() {
                    runtime_state.handle(MakerEvent::PositionMismatch);
                    let cleanup_token = match take_cleanup_effect(
                        &mut runtime_state,
                        RecoveryTarget::PositionReconciliation,
                    ) {
                        Ok(token) => token,
                        Err(error) => {
                            break 'main stop_requested_exit(
                                &mut runtime_state,
                                RuntimeStopReason::CleanupFailure {
                                    target: RecoveryTarget::PositionReconciliation,
                                    reason: error.to_string(),
                                },
                            );
                        }
                    };
                    // A mismatch is not a normal cycle error. Freeze quoting,
                    // empty the maker book, and give account-order callbacks
                    // plus REST settlement a bounded three-second window to
                    // converge before failing closed.
                    emit_reconciliation_state(
                        output_format,
                        &symbol,
                        cycle,
                        "frozen",
                        mismatch.expected,
                        mismatch.observed,
                    );
                    notifier
                        .risk(
                            RiskNotice {
                                kind: "position_reconciliation",
                                severity: "warning",
                                event: "frozen",
                                message: "position mismatch detected; placements frozen and maker cleanup starting",
                                symbol: &symbol,
                                cycle,
                                position_before: None,
                                position_after: None,
                                expected: Some(mismatch.expected),
                                observed: Some(mismatch.observed),
                            },
                            false,
                        )
                    .await;
                    if let Err(cleanup_error) =
                        cancel_maker_orders_with_retry(&client, &symbol, 3, output_format).await
                    {
                        runtime_state.handle(MakerEvent::CleanupFailed {
                            token: cleanup_token,
                            reason: format!("freeze cleanup failed: {cleanup_error}"),
                        });
                        break 'main match take_stop_effect(&mut runtime_state) {
                            Ok(exit) => exit,
                            Err(error) => MakerExit::PositionReconciliation(error.to_string()),
                        };
                    }
                    resting.clear();
                    if let Some(projection) = account_projection.as_mut() {
                        projection.clear_orders_and_pending();
                    }
                    inventory_exit_pending = false;
                    runtime_state.handle(MakerEvent::CleanupCompleted(cleanup_token));
                    let recovery_token = match take_recovery_effect(
                        &mut runtime_state,
                        RecoveryTarget::PositionReconciliation,
                    ) {
                        Ok(token) => token,
                        Err(error) => {
                            break 'main stop_requested_exit(
                                &mut runtime_state,
                                RuntimeStopReason::PositionReconciliation(error.to_string()),
                            );
                        }
                    };
                    let qty_tolerance = 10_f64.powi(-(cfg.qty_decimals as i32)) / 2.0;
                    let mut recovered = false;
                    let mut last_observed = mismatch.observed;
                    for delay in [500_u64, 1_000, 1_500] {
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                        if let Some(receiver) = account_events.as_mut() {
                            match apply_account_events(
                                receiver,
                                &mut AccountEventState {
                                    ledger: &mut ledger,
                                    stats: &mut stats,
                                    projection: account_projection.as_mut().expect(
                                        "live account events require initialized projection",
                                    ),
                                },
                                &AccountEventContext {
                                    symbol: &symbol,
                                    run_order_prefix: &run_order_prefix,
                                    mark: last_mark.unwrap_or(baseline_mark),
                                    cycle,
                                    output_format,
                                },
                            ) {
                                Ok(outcome) => {
                                    total_fills += outcome.fills;
                                    if outcome.exit_fill_observed {
                                        inventory_exit_pending = false;
                                    }
                                    let position = outcome.latest_position;
                                    if let Some(position) = position {
                                        notifier
                                            .position_jump(
                                                &mut position_alert_anchor,
                                                PositionChange {
                                                    observed: position,
                                                    expected: ledger.expected_position,
                                                    max_position: cfg.max_position,
                                                    inventory_exit_pct: args.inventory_exit_pct,
                                                    qty_tolerance,
                                                    symbol: &symbol,
                                                    cycle,
                                                },
                                            )
                                            .await;
                                        last_observed = position;
                                    }
                                }
                                Err(error) => {
                                    if let Some(health) = account_stream_health.as_ref() {
                                        health.mark_unhealthy(error.to_string());
                                    }
                                }
                            }
                        }
                        match reconcile_ledger_snapshot(
                            &client,
                            ReconcileRequest {
                                symbol: &symbol,
                                session_started_at,
                                run_order_prefix: &run_order_prefix,
                                qty_tolerance,
                                mark: last_mark.unwrap_or(baseline_mark),
                            },
                            &mut ledger,
                            &mut stats,
                        )
                        .await
                        {
                            Ok((observed, fills)) => {
                                last_observed = observed;
                                total_fills += fills.len() as u64;
                                for fill in &fills {
                                    emit_live_fill(fill, &symbol, cycle, output_format);
                                }
                                if (observed - ledger.expected_position).abs() <= qty_tolerance {
                                    recovered = true;
                                    break;
                                }
                            }
                            Err(error) => emit_reconciliation_snapshot_error(
                                output_format,
                                &symbol,
                                cycle,
                                &error.to_string(),
                            ),
                        }
                    }
                    if recovered {
                        if let Some(projection) = account_projection.as_mut() {
                            let generation = projection.generation();
                            projection.apply(
                                generation,
                                AccountProjectionEvent::PositionObserved {
                                    position: last_observed,
                                },
                            );
                        }
                        emit_reconciliation_state(
                            output_format,
                            &symbol,
                            cycle,
                            "recovered",
                            ledger.expected_position,
                            last_observed,
                        );
                        notifier
                            .risk(
                                RiskNotice {
                                    kind: "position_reconciliation",
                                    severity: "resolved",
                                    event: "recovered",
                                    message: "position ledger recovered within the 3-second freeze window; quoting may resume from an empty maker book",
                                    symbol: &symbol,
                                    cycle,
                                    position_before: None,
                                    position_after: None,
                                    expected: Some(ledger.expected_position),
                                    observed: Some(last_observed),
                                },
                                false,
                            )
                        .await;
                        consecutive_errors = 0;
                        runtime_state.handle(MakerEvent::RecoverySucceeded(recovery_token));
                        continue;
                    }
                    emit_reconciliation_state(
                        output_format,
                        &symbol,
                        cycle,
                        "failed",
                        ledger.expected_position,
                        last_observed,
                    );
                    notifier
                        .risk(
                            RiskNotice {
                                kind: "position_reconciliation",
                                severity: "critical",
                                event: "failed",
                                message: "position ledger remained inconsistent after the 3-second freeze window",
                                symbol: &symbol,
                                cycle,
                                position_before: None,
                                position_after: None,
                                expected: Some(ledger.expected_position),
                                observed: Some(last_observed),
                            },
                            true,
                        )
                    .await;
                    runtime_state.handle(MakerEvent::RecoveryFailed {
                        token: recovery_token,
                        reason: format!(
                            "expected position {:+.8}, venue reported {:+.8} after 3s freeze",
                            ledger.expected_position, last_observed
                        ),
                    });
                    break 'main match take_stop_effect(&mut runtime_state) {
                        Ok(exit) => exit,
                        Err(error) => MakerExit::PositionReconciliation(error.to_string()),
                    };
                }
                if exit_pending_before {
                    let message = format!("inventory exit cycle failed: {e}");
                    notifier
                        .risk(
                            RiskNotice {
                                kind: "inventory_exit",
                                severity: "warning",
                                event: "failed",
                                message: &message,
                                symbol: &symbol,
                                cycle,
                                position_before: None,
                                position_after: Some(ledger.expected_position),
                                expected: Some(ledger.expected_position),
                                observed: None,
                            },
                            false,
                        )
                        .await;
                }
                runtime_state.handle(MakerEvent::CycleFailed {
                    token: cycle_work_token,
                    reason: e.to_string(),
                });
                consecutive_errors += 1;
                eprintln!("⚠️  maker cycle failed ({}/3): {}", consecutive_errors, e);
                if matches!(runtime_state.pending_effect(), Some(MakerEffect::Stop(_))) {
                    break match take_stop_effect(&mut runtime_state) {
                        Ok(exit) => exit,
                        Err(error) => MakerExit::ConsecutiveErrors(error.to_string()),
                    };
                }
            }
        }

        cycle += 1;

        if matches!(
            runtime_state.pending_effect(),
            Some(MakerEffect::RunCycle(_))
        ) {
            continue 'main;
        }

        // Sleep until the next cycle, but wake early when the cached mark
        // has already drifted beyond refresh_bps — the quotes would be
        // re-quoted anyway, so reacting now shrinks the pick-off window
        // without adding flicker. min-gap of 1s bounds the API rate.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(args.interval);
        let min_gap = tokio::time::Instant::now() + Duration::from_secs(1);
        loop {
            let update = async {
                match updates.as_mut() {
                    Some(rx) => rx.changed().await.is_ok(),
                    None => std::future::pending().await,
                }
            };
            let account_update = async {
                match account_events.as_mut() {
                    Some(receiver) => receiver.recv().await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                _ = signal::ctrl_c() => {
                    runtime_state.handle(MakerEvent::CtrlC);
                    break 'main match take_stop_effect(&mut runtime_state) {
                        Ok(exit) => exit,
                        Err(error) => MakerExit::PositionReconciliation(error.to_string()),
                    };
                },
                _ = tokio::time::sleep_until(deadline) => break,
                event = account_update => {
                    let Some(event) = event else {
                        if let Some(health) = account_stream_health.as_ref() {
                            health.mark_unhealthy("authenticated account stream disconnected");
                        }
                        break;
                    };
                    match apply_account_event(
                        event,
                        &mut AccountEventState {
                            ledger: &mut ledger,
                            stats: &mut stats,
                            projection: account_projection
                                .as_mut()
                                .expect("live account events require initialized projection"),
                        },
                        &AccountEventContext {
                            symbol: &symbol,
                            run_order_prefix: &run_order_prefix,
                            mark: last_mark.unwrap_or(baseline_mark),
                            cycle,
                            output_format,
                        },
                    ) {
                        Ok(outcome) => {
                            total_fills += outcome.fills;
                            if outcome.exit_fill_observed {
                                inventory_exit_pending = false;
                            }
                            let position = outcome.latest_position;
                            if let Some(position) = position {
                                notifier
                                    .position_jump(
                                        &mut position_alert_anchor,
                                        PositionChange {
                                            observed: position,
                                            expected: ledger.expected_position,
                                            max_position: cfg.max_position,
                                            inventory_exit_pct: args.inventory_exit_pct,
                                            qty_tolerance: 10_f64
                                                .powi(-(cfg.qty_decimals as i32))
                                                / 2.0,
                                            symbol: &symbol,
                                            cycle,
                                        },
                                    )
                                .await;
                            }
                            if let Some(position) = position.filter(|position| {
                                (*position - ledger.expected_position).abs()
                                    > 10_f64.powi(-(cfg.qty_decimals as i32)) / 2.0
                            }) {
                                account_position_mismatch = Some(position);
                            }
                            break;
                        }
                        Err(error) => {
                            if let Some(health) = account_stream_health.as_ref() {
                                health.mark_unhealthy(error.to_string());
                            }
                            break;
                        }
                    }
                }
                ok = update => {
                    if !ok {
                        // Feed task gone: fall back to plain interval waits.
                        updates = None;
                        continue;
                    }
                    if tokio::time::Instant::now() < min_gap {
                        continue;
                    }
                    let (Some(feed), Some(prev)) = (feed.as_ref(), last_mark) else {
                        continue;
                    };
                    let drifted = {
                        let s = feed.read().await;
                        s.mark
                            .is_some_and(|m| maker::bps_diff(m, prev) > cfg.refresh_bps)
                    };
                    if drifted {
                        runtime_state.handle(MakerEvent::MarketChanged);
                        break; // early re-quote cycle
                    }
                }
            }
        }
    };

    // ---- Cleanup on ALL exit paths ----
    if let Some(handle) = feed_handle {
        handle.abort();
    }
    if let Some(handle) = account_stream_handle {
        handle.abort();
    }
    let final_position = if args.live {
        ledger.expected_position
    } else {
        stats.position()
    };
    if output_format == OutputFormat::Table {
        println!(
            "\n👋 Stopping maker (ran {} cycles: {} places, {} cancels, {} holds)",
            cycle, total_places, total_cancels, total_holds
        );
        let pnl_note = match last_mark {
            Some(m) => format!(
                " | PnL {:+.2} (mark-to-market)",
                stats.pnl(final_position, m)
            ),
            None => String::new(),
        };
        println!(
            "   {} fills | uptime {:.0}% | max pos {} | avg capture {:.1}bps{}",
            total_fills,
            stats.uptime_pct(),
            maker::format_decimals(stats.max_abs_position, cfg.qty_decimals),
            stats.avg_spread_capture_bps(),
            pnl_note
        );
        if breaker.enabled() {
            println!("   vol breaker: {} cycles halted", total_halted);
        }
        if !args.live {
            println!(
                "   paper sim: ending position {}",
                maker::format_decimals(sim_position, cfg.qty_decimals)
            );
        }
    }
    if let Some(handle) = order_response_handle {
        handle.abort();
    }
    // Do not return early on cleanup failure: operators need the stopped
    // lifecycle alert most when residual maker orders may still be live.
    let cleanup_error = if args.live {
        cancel_maker_orders_with_retry(&client, &symbol, 3, output_format)
            .await
            .err()
    } else {
        None
    };

    // Notify stop on every exit path. Await delivery so the message lands
    // before the process exits.
    let reason = exit.lifecycle_reason();
    let pnl_str = last_mark
        .map(|m| format!("{:+.2}", stats.pnl(final_position, m)))
        .unwrap_or_else(|| "n/a".to_string());
    let cleanup_note = cleanup_error.as_ref().map_or_else(String::new, |error| {
        format!(" | ⚠️ cleanup failed: {error}")
    });
    if let Some(error) = cleanup_error.as_ref() {
        let message = format!("maker cleanup failed or left residual orders: {error}");
        notifier
            .risk(
                RiskNotice {
                    kind: "maker_cleanup",
                    severity: "critical",
                    event: "residual_orders",
                    message: &message,
                    symbol: &symbol,
                    cycle,
                    position_before: None,
                    position_after: None,
                    expected: Some(ledger.expected_position),
                    observed: None,
                },
                true,
            )
            .await;
    }
    if !matches!(&exit, MakerExit::CtrlC) {
        notifier
            .risk(
                RiskNotice {
                    kind: "fail_safe",
                    severity: "critical",
                    event: "stopped",
                    message: &reason,
                    symbol: &symbol,
                    cycle,
                    position_before: None,
                    position_after: None,
                    expected: Some(ledger.expected_position),
                    observed: None,
                },
                true,
            )
            .await;
    }
    notifier
        .lifecycle(
            "stopped",
            &format!(
                "🔴 maker stopped ({}) — {} | {} cycles, {} fills, uptime {:.0}%, PnL {}{}",
                reason,
                symbol,
                cycle,
                total_fills,
                stats.uptime_pct(),
                pnl_str,
                cleanup_note,
            ),
            &symbol,
            true,
        )
        .await;

    if let Some(error) = cleanup_error {
        // A residual-order cleanup failure is an intentional fail-safe stop
        // that needs a human, not an automatic restart.
        return Err(anyhow::Error::new(FailSafeShutdown {
            message: format!(
                "maker stopped (fail-safe) but maker-owned order cleanup failed: {error}"
            ),
        }));
    }

    match exit.terminal_error() {
        Some(message) => Err(anyhow::Error::new(FailSafeShutdown { message })),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use standx_maker::{OrderObservation, ProjectionPendingCancel, ProjectionPendingPlace};
    use standx_sdk::account_stream::{OrderUpdate, PositionUpdate};

    fn pending_place(request_id: &str) -> ProjectionPendingPlace {
        ProjectionPendingPlace {
            request_id: request_id.to_string(),
            client_order_id: format!("cl-{request_id}"),
            side: OrderSide::Buy,
            price: 100.0,
            qty: 1.0,
            level: 0,
            ref_center: 100.0,
            cycle: 1,
        }
    }

    fn projection_with_pending(request_ids: &[&str]) -> MakerAccountProjection {
        let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0);
        for request_id in request_ids {
            projection.apply(
                1,
                AccountProjectionEvent::PlaceSubmitted(pending_place(request_id)),
            );
        }
        projection
    }

    fn order_response(request_id: Option<&str>, code: i64) -> OrderResponse {
        OrderResponse {
            code,
            message: String::new(),
            request_id: request_id.map(str::to_string),
        }
    }

    fn position_update(symbol: &str, side: Option<OrderSide>, qty: &str) -> PositionUpdate {
        PositionUpdate {
            seq: 0,
            id: 0,
            symbol: symbol.to_string(),
            side,
            qty: qty.to_string(),
            entry_price: String::new(),
            realized_pnl: String::new(),
            status: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn runtime_effect_executor_orders_abort_cleanup_and_recovery() {
        let mut runtime_state = MakerState::starting();
        runtime_state.handle(MakerEvent::StartupReady);
        let cycle_token = match next_runtime_effect(&mut runtime_state) {
            Some(MakerEffect::RunCycle(token)) => token,
            effect => panic!("expected cycle effect, got {effect:?}"),
        };

        runtime_state.handle(MakerEvent::PositionMismatch);
        let cleanup =
            take_cleanup_effect(&mut runtime_state, RecoveryTarget::PositionReconciliation)
                .expect("abort must be drained before cleanup");
        runtime_state.handle(MakerEvent::CycleCompleted(cycle_token));
        assert!(runtime_state.pending_effect().is_none());

        runtime_state.handle(MakerEvent::CleanupCompleted(cleanup));
        let recovery =
            take_recovery_effect(&mut runtime_state, RecoveryTarget::PositionReconciliation)
                .expect("cleanup completion must schedule recovery");
        runtime_state.handle(MakerEvent::RecoverySucceeded(recovery));
        assert!(matches!(
            next_runtime_effect(&mut runtime_state),
            Some(MakerEffect::RunCycle(_))
        ));
    }

    #[test]
    fn runtime_recovery_failure_is_the_stop_source_of_truth() {
        let mut runtime_state = MakerState::starting();
        runtime_state.handle(MakerEvent::StartupReady);
        let _ = next_runtime_effect(&mut runtime_state);
        runtime_state.handle(MakerEvent::OrderResponseDisconnected("closed".to_string()));
        let cleanup = take_cleanup_effect(&mut runtime_state, RecoveryTarget::OrderResponse)
            .expect("cleanup effect");
        runtime_state.handle(MakerEvent::CleanupCompleted(cleanup));
        let recovery = take_recovery_effect(&mut runtime_state, RecoveryTarget::OrderResponse)
            .expect("recovery effect");
        let exit = recovery_failed_exit(
            &mut runtime_state,
            recovery,
            "residual maker orders".to_string(),
        );
        assert!(
            matches!(exit, MakerExit::OrderResponse(reason) if reason == "residual maker orders")
        );
    }

    #[test]
    fn apply_order_response_keeps_accepted_placement() {
        let mut projection = projection_with_pending(&["req-1"]);
        let matched = apply_order_response(
            order_response(Some("req-1"), 0),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        );
        assert!(matched);
        assert_eq!(
            projection.pending_places().len(),
            1,
            "accepted placement stays pending"
        );
        assert_eq!(projection.pending_request_count(), 0);
    }

    #[test]
    fn apply_order_response_drops_rejected_placement() {
        let mut projection = projection_with_pending(&["req-1"]);
        let matched = apply_order_response(
            order_response(Some("req-1"), 1),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        );
        assert!(matched);
        assert!(
            projection.pending_places().is_empty(),
            "rejected placement is removed"
        );
    }

    #[test]
    fn apply_order_response_matches_cancel_acknowledgement() {
        let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0);
        projection.apply(
            1,
            AccountProjectionEvent::CancelSubmitted(ProjectionPendingCancel {
                request_id: "cancel-1".to_string(),
                order_id: 7,
                side: OrderSide::Buy,
                level: 0,
                price: 100.0,
                cycle: 1,
            }),
        );

        assert!(apply_order_response(
            order_response(Some("cancel-1"), 0),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        ));
        assert!(projection.pending_cancels().is_empty());
    }

    #[test]
    fn apply_order_response_matches_rejected_cancel_acknowledgement() {
        let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0);
        projection.apply(
            1,
            AccountProjectionEvent::CancelSubmitted(ProjectionPendingCancel {
                request_id: "cancel-1".to_string(),
                order_id: 7,
                side: OrderSide::Buy,
                level: 0,
                price: 100.0,
                cycle: 1,
            }),
        );

        assert!(apply_order_response(
            order_response(Some("cancel-1"), 400),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        ));
        assert!(projection.pending_cancels().is_empty());
        assert_eq!(projection.pending_request_count(), 0);
    }

    #[test]
    fn apply_order_response_matches_late_ack_after_terminal_account_order() {
        let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0);
        projection.apply(
            1,
            AccountProjectionEvent::PlaceSubmitted(ProjectionPendingPlace {
                request_id: "req-1".to_string(),
                client_order_id: "sxmk-test-q00000001b0".to_string(),
                side: OrderSide::Buy,
                price: 100.0,
                qty: 1.0,
                level: 0,
                ref_center: 100.0,
                cycle: 1,
            }),
        );
        projection.apply(
            1,
            AccountProjectionEvent::OrderObserved(OrderObservation {
                order_id: 7,
                client_order_id: Some("sxmk-test-q00000001b0".to_string()),
                side: OrderSide::Buy,
                price: 100.0,
                open_qty: 0.0,
                terminal: true,
            }),
        );
        assert!(projection.pending_places().is_empty());
        assert_eq!(projection.pending_request_count(), 1);

        assert!(apply_order_response(
            order_response(Some("req-1"), 0),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        ));
        assert_eq!(projection.pending_request_count(), 0);
    }

    #[test]
    fn apply_order_response_reports_unmatched_ids() {
        let mut projection = projection_with_pending(&["req-1"]);
        assert!(!apply_order_response(
            order_response(Some("other"), 0),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        ));
        assert!(!apply_order_response(
            order_response(None, 0),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        ));
        assert_eq!(projection.pending_places().len(), 1);
    }

    #[test]
    fn apply_order_responses_matched_acks_clear_request_registry() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let mut projection = projection_with_pending(&["req-1", "req-2"]);
        let mut runtime_state = MakerState::starting();
        runtime_state.handle(MakerEvent::StartupReady);
        assert!(matches!(
            next_runtime_effect(&mut runtime_state),
            Some(MakerEffect::RunCycle(_))
        ));

        // Benign matched acknowledgements for placements we are tracking.
        tx.try_send(order_response(Some("req-1"), 0)).unwrap();
        tx.try_send(order_response(Some("req-2"), 0)).unwrap();

        apply_order_responses(
            &mut rx,
            &mut projection,
            &mut runtime_state,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        )
        .expect("benign matched acks must not fail closed");

        assert!(runtime_state.pending_effect().is_none());
        // Accepted placements remain pending; the matched arm keeps them.
        assert_eq!(projection.pending_places().len(), 2);
        assert_eq!(projection.pending_request_count(), 0);
    }

    #[test]
    fn apply_order_responses_unknown_request_fails_closed() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let mut projection = projection_with_pending(&[]);
        let mut runtime_state = MakerState::starting();
        runtime_state.handle(MakerEvent::StartupReady);
        assert!(matches!(
            next_runtime_effect(&mut runtime_state),
            Some(MakerEffect::RunCycle(_))
        ));

        tx.try_send(order_response(Some("req-1"), 0)).unwrap();
        let error = apply_order_responses(
            &mut rx,
            &mut projection,
            &mut runtime_state,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        )
        .unwrap_err();
        assert!(error.to_string().contains("correlation failed closed"));
        assert!(matches!(
            runtime_state.pending_effect(),
            Some(MakerEffect::AbortInFlight(_))
        ));
    }

    #[test]
    fn plan_affecting_account_events_invalidate_cycle_work() {
        assert!(account_event_invalidates_cycle(&AccountEvent::Position(
            position_update("BTC-USD", Some(OrderSide::Buy), "0.5")
        )));
        assert!(account_event_invalidates_cycle(&AccountEvent::Error {
            reason: "bad payload".to_string(),
        }));
        assert!(!account_event_invalidates_cycle(&AccountEvent::Order(
            OrderUpdate {
                seq: 1,
                order_id: 7,
                cl_ord_id: Some("sxmk-test-q00000001b0".to_string()),
                symbol: "BTC-USD".to_string(),
                side: OrderSide::Buy,
                qty: "1".to_string(),
                fill_qty: "0".to_string(),
                fill_avg_price: "0".to_string(),
                price: "100".to_string(),
                status: standx_sdk::models::OrderStatus::Open,
                reduce_only: false,
                updated_at: String::new(),
            }
        )));
        assert!(!account_event_invalidates_cycle(&AccountEvent::Connected {
            epoch: 1,
        }));
    }

    fn drain_positions(events: Vec<AccountEvent>) -> AccountEventOutcome {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        for event in events {
            tx.try_send(event).unwrap();
        }
        let mut ledger = MakerLedger::new(0.0);
        let mut stats = MakerStats::default();
        let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0);
        let mut state = AccountEventState {
            ledger: &mut ledger,
            stats: &mut stats,
            projection: &mut projection,
        };
        let context = AccountEventContext {
            symbol: "BTC-USD",
            run_order_prefix: "sxmk-test-",
            mark: 100.0,
            cycle: 1,
            output_format: OutputFormat::Quiet,
        };
        apply_account_events(&mut rx, &mut state, &context).expect("benign events drain cleanly")
    }

    #[test]
    fn apply_account_events_records_position_mismatch_with_sign() {
        let buy = drain_positions(vec![AccountEvent::Position(position_update(
            "BTC-USD",
            Some(OrderSide::Buy),
            "0.5",
        ))]);
        assert_eq!(buy.latest_position, Some(0.5));

        let sell = drain_positions(vec![AccountEvent::Position(position_update(
            "BTC-USD",
            Some(OrderSide::Sell),
            "0.5",
        ))]);
        assert_eq!(
            sell.latest_position,
            Some(-0.5),
            "sell position is negative"
        );
    }

    #[test]
    fn apply_account_events_applies_buffered_events_in_order() {
        // The last position update in the buffer wins; benign Connected /
        // Balance events are drained without contributing fills.
        let outcome = drain_positions(vec![
            AccountEvent::Connected { epoch: 1 },
            AccountEvent::Position(position_update("BTC-USD", Some(OrderSide::Buy), "0.2")),
            AccountEvent::Balance(standx_sdk::account_stream::BalanceUpdate {
                seq: 1,
                account_type: "perps".to_string(),
                token: "DUSD".to_string(),
                free: "1".to_string(),
                total: "1".to_string(),
                locked: "0".to_string(),
                occupied: "0".to_string(),
                updated_at: "2026-07-14T00:00:00Z".to_string(),
            }),
            AccountEvent::Position(position_update("BTC-USD", Some(OrderSide::Sell), "0.9")),
        ]);
        assert_eq!(outcome.fills, 0);
        assert_eq!(
            outcome.latest_position,
            Some(-0.9),
            "latest position reflects last update"
        );
    }

    #[test]
    fn balance_event_updates_raw_projection_without_touching_fill_accounting() {
        let mut ledger = MakerLedger::new(0.0);
        let mut stats = MakerStats::default();
        let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0);
        let context = AccountEventContext {
            symbol: "BTC-USD",
            run_order_prefix: "sxmk-test-",
            mark: 100.0,
            cycle: 1,
            output_format: OutputFormat::Quiet,
        };
        let outcome = {
            let mut state = AccountEventState {
                ledger: &mut ledger,
                stats: &mut stats,
                projection: &mut projection,
            };
            apply_account_event(
                AccountEvent::Balance(standx_sdk::account_stream::BalanceUpdate {
                    seq: 1,
                    account_type: "perps".to_string(),
                    token: "DUSD".to_string(),
                    free: "90".to_string(),
                    total: "100".to_string(),
                    locked: "0".to_string(),
                    occupied: "10".to_string(),
                    updated_at: "2026-07-14T00:00:00Z".to_string(),
                }),
                &mut state,
                &context,
            )
            .unwrap()
        };
        assert_eq!(outcome.fills, 0);
        assert_eq!(projection.raw_balance("perps", "DUSD").unwrap().free, "90");
        assert_eq!(stats.fills(), 0);
    }

    #[test]
    fn typed_trade_event_is_booked_once_after_order_ownership() {
        let order = standx_sdk::account_stream::OrderUpdate {
            seq: 1,
            order_id: 7,
            cl_ord_id: Some("sxmk-test-q00000001b0".to_string()),
            symbol: "BTC-USD".to_string(),
            side: OrderSide::Buy,
            qty: "0.2".to_string(),
            fill_qty: "0.2".to_string(),
            fill_avg_price: "100".to_string(),
            price: "100".to_string(),
            status: standx_sdk::models::OrderStatus::Filled,
            reduce_only: false,
            updated_at: "2026-07-14T00:00:00Z".to_string(),
        };
        let trade = standx_sdk::account_stream::TradeUpdate {
            seq: 2,
            trade_id: 11,
            order_id: 7,
            symbol: "BTC-USD".to_string(),
            side: OrderSide::Buy,
            price: "100".to_string(),
            qty: "0.2".to_string(),
            trade_ts: "2026-07-14T00:00:00Z".to_string(),
        };

        let outcome = drain_positions(vec![
            AccountEvent::Order(order),
            AccountEvent::Trade(trade.clone()),
            AccountEvent::Trade(trade),
        ]);
        assert_eq!(outcome.fills, 1);
        assert_eq!(outcome.latest_position, None);
    }

    #[test]
    fn apply_account_events_ignores_other_symbols() {
        let outcome = drain_positions(vec![AccountEvent::Position(position_update(
            "ETH-USD",
            Some(OrderSide::Buy),
            "1.0",
        ))]);
        assert_eq!(outcome.fills, 0);
        assert_eq!(
            outcome.latest_position, None,
            "position updates for other symbols are ignored"
        );
    }

    #[test]
    fn stable_trade_reports_current_run_inventory_exit_once() {
        let mut ledger = MakerLedger::new(0.2);
        let mut stats = MakerStats::with_inventory_baseline(0.2, 100.0);
        let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.2);
        let mut state = AccountEventState {
            ledger: &mut ledger,
            stats: &mut stats,
            projection: &mut projection,
        };
        let context = AccountEventContext {
            symbol: "BTC-USD",
            run_order_prefix: "sxmk-test-",
            mark: 100.0,
            cycle: 1,
            output_format: OutputFormat::Quiet,
        };
        let update = OrderUpdate {
            seq: 1,
            order_id: 7,
            cl_ord_id: Some("sxmk-test-x00000001".to_string()),
            symbol: "BTC-USD".to_string(),
            side: OrderSide::Sell,
            qty: "0.2".to_string(),
            fill_qty: "0.2".to_string(),
            fill_avg_price: "100".to_string(),
            price: "100".to_string(),
            status: standx_sdk::models::OrderStatus::Filled,
            reduce_only: true,
            updated_at: "2026-07-14T00:00:00Z".to_string(),
        };

        let order = apply_account_event(AccountEvent::Order(update), &mut state, &context)
            .expect("exit order is valid");
        assert_eq!(order.fills, 0);
        assert!(!order.exit_fill_observed);

        let trade = standx_sdk::account_stream::TradeUpdate {
            seq: 2,
            trade_id: 11,
            order_id: 7,
            symbol: "BTC-USD".to_string(),
            side: OrderSide::Sell,
            price: "100".to_string(),
            qty: "0.2".to_string(),
            trade_ts: "2026-07-14T00:00:00Z".to_string(),
        };
        let first = apply_account_event(AccountEvent::Trade(trade.clone()), &mut state, &context)
            .expect("exit trade is valid");
        assert_eq!(first.fills, 1);
        assert!(first.exit_fill_observed);

        let duplicate = apply_account_event(AccountEvent::Trade(trade), &mut state, &context)
            .expect("duplicate exit fill is valid");
        assert_eq!(duplicate.fills, 0);
        assert!(!duplicate.exit_fill_observed);
    }

    #[test]
    fn accounting_position_mismatch_respects_half_tick_tolerance() {
        let tolerance = 0.0005;
        assert!(!accounting_position_mismatch(0.2, 0.20049, tolerance));
        assert!(accounting_position_mismatch(0.2, 0.20051, tolerance));
        assert!(!accounting_position_mismatch(-0.2, -0.20049, tolerance));
        assert!(accounting_position_mismatch(-0.2, -0.20051, tolerance));
    }

    #[test]
    fn accounting_position_mismatch_fails_closed_on_non_finite() {
        let tolerance = 0.0005;
        assert!(accounting_position_mismatch(f64::NAN, 0.2, tolerance));
        assert!(accounting_position_mismatch(0.2, f64::NAN, tolerance));
        assert!(accounting_position_mismatch(f64::INFINITY, 0.2, tolerance));
    }

    #[tokio::test]
    async fn accounting_invariant_mismatch_becomes_fail_safe_exit() {
        let notifier = MakerNotifier::new(
            OutputFormat::Quiet,
            None,
            crate::cli::AlertWebhookFormat::Raw,
        );

        assert!(
            accounting_invariant_exit(&notifier, "XAG-USD", 1396, 0.0, -0.2, 0.0005,)
                .await
                .is_some_and(|exit| matches!(exit, MakerExit::AccountingInvariant(_)))
        );
        assert!(
            accounting_invariant_exit(&notifier, "XAG-USD", 1396, 0.0, 0.00049, 0.0005,)
                .await
                .is_none()
        );
    }
}
