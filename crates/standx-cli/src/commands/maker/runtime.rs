use super::*;
use standx_sdk::order_response::OrderResponse;

pub(super) fn apply_order_responses(
    receiver: &mut tokio::sync::mpsc::Receiver<OrderResponse>,
    pending: &mut Vec<PendingPlace>,
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
            pending,
            output_format,
            symbol,
            cycle,
            price_decimals,
        );
        if let Some(request_id) = request_id {
            runtime_state.reduce(if matched {
                MakerEvent::OrderResponseMatched(request_id)
            } else {
                MakerEvent::OrderResponseUnmatched { request_id, cycle }
            });
        }
        if runtime_state.is_frozen() {
            return Err(anyhow::anyhow!("order-response correlation failed closed"));
        }
    }
}

fn apply_order_response(
    response: OrderResponse,
    pending: &mut Vec<PendingPlace>,
    output_format: OutputFormat,
    symbol: &str,
    cycle: u64,
    price_decimals: u32,
) -> bool {
    let Some(request_id) = response.request_id.as_deref() else {
        return false;
    };
    let Some(index) = pending
        .iter()
        .position(|place| place.request_id == request_id)
    else {
        return false;
    };
    if response.accepted() {
        return true;
    }
    let rejected = pending.remove(index);
    output::log_maker_event(output::MakerLogEvent {
        output_format,
        symbol,
        cycle,
        action: "place_rejected_async",
        side: rejected.side,
        level: rejected.level,
        price: rejected.price,
        price_decimals,
        detail: &response.message,
    });
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
}

fn apply_account_events(
    receiver: &mut tokio::sync::mpsc::Receiver<AccountEvent>,
    state: &mut AccountEventState<'_>,
    context: &AccountEventContext<'_>,
) -> Result<(u64, Option<f64>)> {
    let mut fills_total = 0;
    let mut latest_position = None;
    loop {
        let event = match receiver.try_recv() {
            Ok(event) => event,
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                return Ok((fills_total, latest_position));
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                return Err(anyhow::anyhow!("authenticated account stream disconnected"));
            }
        };
        let (fills, position) = apply_account_event(event, state, context)?;
        fills_total += fills;
        if position.is_some() {
            latest_position = position;
        }
    }
}

fn apply_account_event(
    event: AccountEvent,
    state: &mut AccountEventState<'_>,
    context: &AccountEventContext<'_>,
) -> Result<(u64, Option<f64>)> {
    match event {
        AccountEvent::Connected { .. } => Ok((0, None)),
        AccountEvent::Order(update) => {
            let mut fills = Vec::new();
            ledger::apply_order_update(
                state.ledger,
                &update,
                context.symbol,
                context.run_order_prefix,
                context.mark,
                state.stats,
                &mut fills,
            )?;
            for fill in &fills {
                emit_live_fill(fill, context.symbol, context.cycle, context.output_format);
            }
            Ok((fills.len() as u64, None))
        }
        AccountEvent::Position(update) => {
            if !update.symbol.eq_ignore_ascii_case(context.symbol) {
                return Ok((0, None));
            }
            let qty =
                model::signed_position_quantity(&update.qty, update.side).map_err(|error| {
                    anyhow::anyhow!("account position update has invalid qty: {error}")
                })?;
            Ok((0, Some(qty)))
        }
        AccountEvent::TradeShadow { seq, data } => {
            if context.output_format == OutputFormat::Json {
                println!(
                    "{}",
                    serde_json::json!({
                        "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                        "symbol": context.symbol,
                        "cycle": context.cycle,
                        "action": "account_trade_shadow",
                        "seq": seq,
                        "data": data,
                    })
                );
            }
            Ok((0, None))
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
    let mut order_response_health = None;
    let mut order_response_handle = None;
    let mut account_events = None;
    let mut account_stream_health: Option<AccountStreamHealth> = None;
    let mut account_stream_handle = None;
    let mut account_stream_epoch = 1_u64;
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
        let (positions, startup_market, filled_orders, historical_trades) = tokio::join!(
            client.get_positions(Some(&symbol)),
            market_snapshot(&client, &symbol, None),
            client.get_order_history(Some(&symbol), Some(100)),
            client.get_user_trades(&symbol, history_from, history_to, Some(500)),
        );
        let positions = positions?;
        let (mark, _, _, _) = startup_market?;
        let filled_orders = filled_orders?;
        let historical_trades = historical_trades?;
        starting_position = position_for_symbol(&positions, &symbol)?;
        baseline_mark = mark;

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
        let (responses, health, handle) = stream.connect().await?;
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
    let mut adopted: HashMap<String, (u32, f64, u64)> = HashMap::new(); // id -> (level, ref_mark, cycle)
    let mut pending: Vec<PendingPlace> = Vec::new();
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
    let mut breaker = VolBreaker::new(args.vol_window.max(1) as usize, args.vol_pause_bps);
    let mut alerts =
        AlertMonitor::new(args.alert_loss, args.alert_inventory_pct, args.alert_uptime)
            .with_account_floors(args.alert_equity_below, args.alert_margin_below);
    let mut last_mark: Option<f64> = None;
    let mut last_src: Option<&'static str> = None;
    let mut order_response_reconnect_attempts_used = 0_u32;
    let mut account_stream_reconnect_attempts_used = 0_u32;
    let mut account_position_mismatch: Option<f64> = None;
    let mut runtime_state = MakerState::starting();
    runtime_state.reduce(MakerEvent::StartupReady);

    let exit = 'main: loop {
        if args.live {
            if let Some(health) = account_stream_health
                .as_ref()
                .filter(|health| !health.is_healthy())
            {
                let detail = health.failure_reason().unwrap_or_else(|| {
                    "account stream became unhealthy without a recorded reason".to_string()
                });
                runtime_state.reduce(MakerEvent::AccountStreamDisconnected(detail.clone()));
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
                    break MakerExit::PositionReconciliation(format!(
                        "account stream disconnected ({detail}); freeze cleanup failed: {cleanup_error}"
                    ));
                }
                resting.clear();
                adopted.clear();
                pending.clear();
                inventory_exit_pending = false;
                if let Some(handle) = account_stream_handle.take() {
                    handle.abort();
                }
                account_events.take();

                let qty_tolerance = 10_f64.powi(-(cfg.qty_decimals as i32)) / 2.0;
                if account_stream_reconnect_attempts_used >= args.account_stream_reconnect_attempts
                {
                    break MakerExit::PositionReconciliation(format!(
                        "account stream disconnected ({detail}); reconnect disabled or budget exhausted ({}/{})",
                        account_stream_reconnect_attempts_used, args.account_stream_reconnect_attempts
                    ));
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
                    runtime_state.reduce(MakerEvent::RecoveryFailed(format!(
                        "account stream reconnect exhausted: {}",
                        last_connect_error
                            .clone()
                            .unwrap_or_else(|| "no attempts available".to_string())
                    )));
                    break MakerExit::PositionReconciliation(format!(
                        "account stream disconnected ({detail}); reconnect exhausted: {}",
                        last_connect_error.unwrap_or_else(|| "no attempts available".to_string())
                    ));
                };

                let mut reconnect_fills = match apply_account_events(
                    &mut events,
                    &mut AccountEventState {
                        ledger: &mut ledger,
                        stats: &mut stats,
                    },
                    &AccountEventContext {
                        symbol: &symbol,
                        run_order_prefix: &run_order_prefix,
                        mark: last_mark.unwrap_or(baseline_mark),
                        cycle,
                        output_format,
                    },
                ) {
                    Ok((fills, _)) => fills,
                    Err(error) => {
                        handle.abort();
                        break MakerExit::PositionReconciliation(format!(
                            "account stream reconnect event validation failed: {error}"
                        ));
                    }
                };
                let positions = match client.get_positions(Some(&symbol)).await {
                    Ok(positions) => positions,
                    Err(error) => {
                        handle.abort();
                        break MakerExit::PositionReconciliation(format!(
                            "account stream reconnect snapshot failed: {error}"
                        ));
                    }
                };
                let mut observed = match position_for_symbol(&positions, &symbol) {
                    Ok(position) => position,
                    Err(error) => {
                        handle.abort();
                        break MakerExit::PositionReconciliation(error.to_string());
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
                            },
                            &AccountEventContext {
                                symbol: &symbol,
                                run_order_prefix: &run_order_prefix,
                                mark: last_mark.unwrap_or(baseline_mark),
                                cycle,
                                output_format,
                            },
                        ) {
                            Ok((fills, _)) => reconnect_fills += fills,
                            Err(error) => {
                                handle.abort();
                                break 'main MakerExit::PositionReconciliation(format!(
                                    "account stream reconnect event validation failed during REST backfill: {error}"
                                ));
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
                        break MakerExit::PositionReconciliation(format!(
                            "account stream reconnect snapshot expected {:+.8}, observed {:+.8} (REST trade backfill did not close the gap)",
                            ledger.expected_position, observed
                        ));
                    }
                }

                account_events = Some(events);
                account_stream_health = Some(health);
                account_stream_handle = Some(handle);
                total_fills += reconnect_fills;
                runtime_state.reduce(MakerEvent::CleanupComplete);
                runtime_state.reduce(MakerEvent::RecoverySucceeded);
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
                runtime_state.reduce(MakerEvent::OrderResponseDisconnected(detail.clone()));
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
                if reconnect_available {
                    if let Some(handle) = order_response_handle.take() {
                        handle.abort();
                    }
                    order_responses.take();
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
                            order_responses = Some(reconnected.responses);
                            order_response_health = Some(reconnected.health);
                            order_response_handle = Some(reconnected.handle);
                            order_response_reconnect_attempts_used = attempts_used;
                            // Cleanup verified an empty maker book. The next
                            // cycle rebuilds exchange state before it may place.
                            resting.clear();
                            adopted.clear();
                            pending.clear();
                            consecutive_errors = 0;
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
                                break MakerExit::PositionReconciliation(error.to_string());
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
                            break MakerExit::OrderResponse(reconnect_failed_message);
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
                break MakerExit::OrderResponse(refuse_message);
            }
        }
        if let Some(receiver) = order_responses.as_mut() {
            if let Err(error) = apply_order_responses(
                receiver,
                &mut pending,
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
                break MakerExit::OrderResponse(error.to_string());
            }
        }
        if let Some(receiver) = account_events.as_mut() {
            match apply_account_events(
                receiver,
                &mut AccountEventState {
                    ledger: &mut ledger,
                    stats: &mut stats,
                },
                &AccountEventContext {
                    symbol: &symbol,
                    run_order_prefix: &run_order_prefix,
                    mark: last_mark.unwrap_or(baseline_mark),
                    cycle,
                    output_format,
                },
            ) {
                Ok((fills, position)) => {
                    total_fills += fills;
                    if let Some(position) = position {
                        notifier
                            .position_jump(
                                &mut position_alert_anchor,
                                PositionChange {
                                    observed: position,
                                    expected: ledger.expected_position,
                                    max_position: cfg.max_position,
                                    inventory_exit_pct: args.inventory_exit_pct,
                                    qty_tolerance: 10_f64.powi(-(cfg.qty_decimals as i32)) / 2.0,
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
                }
                Err(error) => {
                    if let Some(health) = account_stream_health.as_ref() {
                        health.mark_unhealthy(error.to_string());
                        continue;
                    }
                    break MakerExit::PositionReconciliation(error.to_string());
                }
            }
        }
        if account_position_mismatch.is_some_and(|position| {
            (position - ledger.expected_position).abs()
                <= 10_f64.powi(-(cfg.qty_decimals as i32)) / 2.0
        }) {
            account_position_mismatch = None;
        }

        // Work phase raced against Ctrl+C so a slow API call can be
        // interrupted (mirrors run_watch_loop).
        let mismatch = account_position_mismatch.take();
        let exit_pending_before = inventory_exit_pending;
        let breaker_halted_before = breaker.halted();
        runtime_state.reduce(MakerEvent::Timer);
        let cycle_work_token = runtime_state.in_flight();
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
                    order_response_health: order_response_health.as_ref(),
                },
                CycleState {
                    resting: &mut resting,
                    adopted: &mut adopted,
                    pending: &mut pending,
                    inventory_exit_pending: &mut inventory_exit_pending,
                    ledger: &mut ledger,
                    sim_position: &mut sim_position,
                    stats: &mut stats,
                    breaker: &mut breaker,
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
        // Stream events that arrive while `work` is placing/cancelling orders
        // are buffered rather than acted on immediately: interrupting the work
        // future would drop an in-flight create/cancel HTTP call, and a live
        // cycle's own placements produce account events almost instantly, so
        // acting mid-flight tore a multi-order cycle apart. We still keep the
        // channels drained so the WS reader never backpressures and so a
        // disconnect is noticed promptly. Buffered events are applied once the
        // work future completes and releases its ledger/pending borrows.
        let mut buffered_account: Vec<AccountEvent> = Vec::new();
        let mut buffered_orders: Vec<OrderResponse> = Vec::new();
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
                    _ = signal::ctrl_c() => {
                        runtime_state.reduce(MakerEvent::CtrlC);
                        break 'main MakerExit::CtrlC;
                    },
                    event = account_during_work => {
                        let Some(event) = event else {
                            let reason = "authenticated account stream disconnected during cycle".to_string();
                            runtime_state.reduce(MakerEvent::AccountStreamDisconnected(reason.clone()));
                            if let Some(health) = account_stream_health.as_ref() {
                                health.mark_unhealthy(reason);
                            }
                            continue 'main;
                        };
                        buffered_account.push(event);
                    },
                    response = order_during_work => {
                        let Some(response) = response else {
                            let reason = "order-response stream disconnected during cycle".to_string();
                            runtime_state.reduce(MakerEvent::OrderResponseDisconnected(reason.clone()));
                            if let Some(health) = order_response_health.as_ref() {
                                health.mark_unhealthy(reason);
                            }
                            continue 'main;
                        };
                        buffered_orders.push(response);
                    },
                    result = &mut work => break result,
                }
            }
        };
        if let Some(token) = cycle_work_token {
            runtime_state.reduce(MakerEvent::WorkFinished(token));
        }

        // Apply the events buffered during work, ordering order-responses
        // before account events to mirror the top-of-loop drain.
        for response in buffered_orders {
            let request_id = response.request_id.clone();
            let matched = apply_order_response(
                response,
                &mut pending,
                output_format,
                &symbol,
                cycle,
                cfg.price_decimals,
            );
            if let Some(request_id) = request_id {
                runtime_state.reduce(if matched {
                    MakerEvent::OrderResponseMatched(request_id)
                } else {
                    MakerEvent::OrderResponseUnmatched { request_id, cycle }
                });
            }
            if runtime_state.is_frozen() {
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
                },
                &AccountEventContext {
                    symbol: &symbol,
                    run_order_prefix: &run_order_prefix,
                    mark: last_mark.unwrap_or(baseline_mark),
                    cycle,
                    output_format,
                },
            ) {
                Ok((fills, position)) => {
                    total_fills += fills;
                    if let Some(position) = position {
                        notifier
                            .position_jump(
                                &mut position_alert_anchor,
                                PositionChange {
                                    observed: position,
                                    expected: ledger.expected_position,
                                    max_position: cfg.max_position,
                                    inventory_exit_pct: args.inventory_exit_pct,
                                    qty_tolerance: 10_f64.powi(-(cfg.qty_decimals as i32)) / 2.0,
                                    symbol: &symbol,
                                    cycle,
                                },
                            )
                            .await;
                        if (position - ledger.expected_position).abs()
                            > 10_f64.powi(-(cfg.qty_decimals as i32)) / 2.0
                        {
                            runtime_state.reduce(MakerEvent::PositionMismatch);
                            account_position_mismatch = Some(position);
                        }
                    }
                }
                Err(error) => {
                    runtime_state.reduce(MakerEvent::AccountStreamDisconnected(error.to_string()));
                    if let Some(health) = account_stream_health.as_ref() {
                        health.mark_unhealthy(error.to_string());
                    }
                }
            }
        }

        match cycle_result {
            Ok((places, cancels, holds, fills, mark, src, halted, exit_pending_after, balance)) => {
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
                if alerts.enabled() {
                    let fired =
                        alerts.evaluate(&stats, stats.position(), mark, cfg.max_position, cycle);
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
                                notifier.alert(&alert, &symbol);
                            }
                        }
                    }
                }
                // Financial brake: a session loss breaching --stop-loss routes
                // through the fail-safe shutdown (freeze, cancel the maker
                // book, await the critical webhook, exit) — the same path the
                // other MakerExit variants use.
                if args.stop_loss > 0.0 {
                    let pnl = stats.pnl(stats.position(), mark);
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
                        break 'main MakerExit::StopLoss(format!(
                            "session PnL {pnl:+.2} <= -{:.2}",
                            args.stop_loss
                        ));
                    }
                }
            }
            Err(e) => {
                if let Some(mismatch) = e.downcast_ref::<PositionReconciliationError>() {
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
                        break 'main MakerExit::PositionReconciliation(format!(
                            "freeze cleanup failed: {cleanup_error}"
                        ));
                    }
                    resting.clear();
                    adopted.clear();
                    pending.clear();
                    inventory_exit_pending = false;
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
                                },
                                &AccountEventContext {
                                    symbol: &symbol,
                                    run_order_prefix: &run_order_prefix,
                                    mark: last_mark.unwrap_or(baseline_mark),
                                    cycle,
                                    output_format,
                                },
                            ) {
                                Ok((fills, position)) => {
                                    total_fills += fills;
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
                            Err(error) => eprintln!(
                                "⚠️  bounded position reconciliation snapshot failed: {error}"
                            ),
                        }
                    }
                    if recovered {
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
                    break 'main MakerExit::PositionReconciliation(format!(
                        "expected position {:+.8}, venue reported {:+.8} after 3s freeze",
                        ledger.expected_position, last_observed
                    ));
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
                consecutive_errors += 1;
                eprintln!("⚠️  maker cycle failed ({}/3): {}", consecutive_errors, e);
                if consecutive_errors >= 3 {
                    break MakerExit::ConsecutiveErrors(e.to_string());
                }
            }
        }

        cycle += 1;

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
                _ = signal::ctrl_c() => break 'main MakerExit::CtrlC,
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
                        },
                        &AccountEventContext {
                            symbol: &symbol,
                            run_order_prefix: &run_order_prefix,
                            mark: last_mark.unwrap_or(baseline_mark),
                            cycle,
                            output_format,
                        },
                    ) {
                        Ok((fills, position)) => {
                            total_fills += fills;
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
                        runtime_state.reduce(MakerEvent::MarketChanged);
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
    if output_format == OutputFormat::Table {
        println!(
            "\n👋 Stopping maker (ran {} cycles: {} places, {} cancels, {} holds)",
            cycle, total_places, total_cancels, total_holds
        );
        let pnl_note = match last_mark {
            Some(m) => format!(" | PnL {:+.2} (mark-to-market)", stats.mark_to_market(m)),
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
        .map(|m| format!("{:+.2}", stats.mark_to_market(m)))
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
        return Err(anyhow::anyhow!(
            "maker stopped but maker-owned order cleanup failed: {}",
            error
        ));
    }

    match exit.terminal_error() {
        Some(message) => Err(anyhow::anyhow!(message)),
        None => Ok(()),
    }
}
