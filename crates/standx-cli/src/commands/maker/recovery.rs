use super::ledger::{adopt_order, apply_rest_trade};
use super::model::{is_current_run_order, is_maker_order, position_for_symbol};
use super::pipeline::{fetch_account_audit, AccountAudit};
use crate::cli::OutputFormat;
use anyhow::Result;
use standx_maker::{MakerFill, MakerLedger, MakerStats};
use standx_sdk::client::StandXClient;
use standx_sdk::models::{Order, Position, Trade};
use standx_sdk::order_response::{
    OrderCommandSender, OrderResponse, OrderResponseHealth, OrderResponseStream,
};
use std::collections::HashSet;
use std::fmt;
use std::time::Duration;

const MAKER_CLEANUP_VERIFY_DELAY: Duration = Duration::from_millis(500);
const MAKER_CLEANUP_RETRY_DELAY: Duration = Duration::from_secs(1);

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

/// Marker error: the operator pressed Ctrl+C while a reconnect wait was in
/// progress. The caller routes this to shutdown instead of RecoveryFailed.
#[derive(Debug)]
pub(super) struct ReconnectInterrupted;

impl fmt::Display for ReconnectInterrupted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "order-response reconnect interrupted by Ctrl+C")
    }
}

impl std::error::Error for ReconnectInterrupted {}

/// Resolves once the runtime's Ctrl+C latch has been set (see runtime.rs);
/// pends forever if the listener is gone so callers' selects don't spin.
pub(super) async fn ctrl_c_latched(ctrl_c: &mut tokio::sync::watch::Receiver<bool>) {
    if ctrl_c.wait_for(|pressed| *pressed).await.is_err() {
        std::future::pending::<()>().await;
    }
}

pub(super) async fn recover_current_run_order_ids_for_reconciliation(
    client: &StandXClient,
    trades: &[Trade],
    gap: PositionGap<'_>,
    ledger: &mut MakerLedger,
) {
    const MAX_ORDER_LOOKUPS: usize = 8;
    let position_gap = gap.observed - gap.expected;
    if position_gap.abs() <= gap.qty_tolerance {
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
            || qty > position_gap.abs() + gap.qty_tolerance
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
                if let Err(error) = adopt_order(ledger, &order, gap.run_order_prefix) {
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

pub(super) struct PositionGap<'a> {
    pub(super) expected: f64,
    pub(super) observed: f64,
    pub(super) qty_tolerance: f64,
    pub(super) run_order_prefix: &'a str,
}

pub(super) async fn reconcile_ledger_snapshot(
    client: &StandXClient,
    request: ReconcileRequest<'_>,
    ledger: &mut MakerLedger,
    stats: &mut MakerStats,
) -> Result<(f64, Vec<MakerFill>)> {
    let now = chrono::Utc::now().timestamp();
    let audit =
        fetch_account_audit(client, request.symbol, request.session_started_at, now).await?;
    let AccountAudit {
        open_orders,
        positions,
        filled_orders,
        trades,
    } = audit;
    for order in open_orders.iter().chain(filled_orders.iter()) {
        adopt_order(ledger, order, request.run_order_prefix)?;
    }
    let observed = position_for_symbol(&positions, request.symbol)?;
    recover_current_run_order_ids_for_reconciliation(
        client,
        &trades,
        PositionGap {
            expected: ledger.expected_position,
            observed,
            qty_tolerance: request.qty_tolerance,
            run_order_prefix: request.run_order_prefix,
        },
        ledger,
    )
    .await;
    let mut fills = Vec::new();
    for trade in trades {
        apply_rest_trade(
            ledger,
            trade,
            request.session_started_at,
            now,
            request.mark,
            stats,
            &mut fills,
        )?;
    }
    Ok((observed, fills))
}

pub(super) struct ReconcileRequest<'a> {
    pub(super) symbol: &'a str,
    pub(super) session_started_at: i64,
    pub(super) run_order_prefix: &'a str,
    pub(super) qty_tolerance: f64,
    pub(super) mark: f64,
}

pub(super) async fn cancel_maker_orders_with_retry(
    client: &StandXClient,
    symbol: &str,
    attempts: u32,
    output_format: OutputFormat,
) -> Result<()> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=attempts {
        let result = cleanup_once(client, symbol).await;
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
            Err(error) => {
                // Precursor signal: an incomplete cleanup retry often precedes a
                // failed shutdown. Emit it on stdout (JSON mode) so the ingest
                // pipeline uploads it, instead of leaving it only in local stderr.
                if output_format == OutputFormat::Json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                            "symbol": symbol,
                            "action": "maker_cleanup",
                            "event": "retry_incomplete",
                            "severity": "warning",
                            "attempt": attempt,
                            "max_attempts": attempts,
                            "message": error.to_string(),
                        })
                    );
                } else {
                    eprintln!(
                        "⚠️  maker-order cancellation attempt {}/{} incomplete: {}",
                        attempt, attempts, error
                    );
                }
                last_err = Some(error);
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

async fn cleanup_once(client: &StandXClient, symbol: &str) -> Result<()> {
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
        return Ok(());
    }
    client.cancel_orders(&order_ids).await?;
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

#[derive(Debug, PartialEq)]
pub(super) struct ReconnectSnapshot {
    pub(super) position: f64,
    pub(super) maker_filled_orders: usize,
    pub(super) maker_trades: usize,
}

pub(super) struct ReconnectedOrderResponse {
    pub(super) client: StandXClient,
    pub(super) commands: OrderCommandSender,
    pub(super) responses: tokio::sync::mpsc::Receiver<OrderResponse>,
    pub(super) health: OrderResponseHealth,
    pub(super) handle: tokio::task::JoinHandle<()>,
}

fn emit_order_response_reconnect(
    output_format: OutputFormat,
    symbol: &str,
    event: &str,
    attempt: u32,
    max_attempts: u32,
    message: &str,
) {
    if output_format == OutputFormat::Json {
        println!(
            "{}",
            serde_json::json!({
                "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "symbol": symbol,
                "action": "order_response_reconnect",
                "event": event,
                "attempt": attempt,
                "max_attempts": max_attempts,
                "message": message,
            })
        );
    } else {
        eprintln!(
            "⚠️  order-response reconnect {event} ({attempt}/{max_attempts}) on {symbol}: {message}"
        );
    }
}

pub(super) fn order_response_reconnect_available(
    failure: &str,
    attempts_used: u32,
    max: u32,
) -> bool {
    !failure.starts_with("controlled fault injection") && attempts_used < max
}

pub(super) fn validate_reconnect_snapshot(
    symbol: &str,
    run_order_prefix: &str,
    open_orders: &[Order],
    positions: &[Position],
    filled_orders: &[Order],
    trades: &[Trade],
) -> Result<ReconnectSnapshot> {
    let residual_ids = open_orders
        .iter()
        .filter(|order| is_maker_order(order))
        .map(|order| order.id.as_str())
        .collect::<Vec<_>>();
    if !residual_ids.is_empty() {
        return Err(anyhow::anyhow!(
            "maker orders appeared after cleanup on {symbol}: [{}]",
            residual_ids.join(", ")
        ));
    }

    let position = position_for_symbol(positions, symbol).map_err(|error| {
        anyhow::anyhow!("reconnect reconciliation found invalid position on {symbol}: {error}")
    })?;

    let maker_filled_order_ids = filled_orders
        .iter()
        .filter(|order| is_current_run_order(order, run_order_prefix))
        .map(|order| {
            order.id.parse::<u64>().map_err(|_| {
                anyhow::anyhow!(
                    "reconnect reconciliation found non-integer maker order ID '{}'",
                    order.id
                )
            })
        })
        .collect::<Result<HashSet<_>>>()?;
    let maker_trades = trades
        .iter()
        .filter(|trade| {
            trade
                .order_id
                .is_some_and(|order_id| maker_filled_order_ids.contains(&order_id))
        })
        .map(|trade| {
            if trade.id == 0 {
                Err(anyhow::anyhow!(
                    "reconnect reconciliation found maker trade without a stable trade ID"
                ))
            } else {
                Ok(())
            }
        })
        .collect::<Result<Vec<_>>>()?
        .len();

    Ok(ReconnectSnapshot {
        position,
        maker_filled_orders: maker_filled_order_ids.len(),
        maker_trades,
    })
}

async fn query_reconnect_snapshot(
    client: &StandXClient,
    symbol: &str,
    session_started_at: i64,
    run_order_prefix: &str,
) -> Result<ReconnectSnapshot> {
    let now = chrono::Utc::now().timestamp();
    let audit = fetch_account_audit(client, symbol, session_started_at, now).await?;
    validate_reconnect_snapshot(
        symbol,
        run_order_prefix,
        &audit.open_orders,
        &audit.positions,
        &audit.filled_orders,
        &audit.trades,
    )
}

pub(super) struct ReconnectRequest<'a> {
    pub(super) cleanup_client: StandXClient,
    pub(super) symbol: &'a str,
    pub(super) session_started_at: i64,
    pub(super) run_order_prefix: &'a str,
    pub(super) expected_position: f64,
    pub(super) qty_tolerance: f64,
    pub(super) output_format: OutputFormat,
    pub(super) attempts_used: u32,
    pub(super) max_attempts: u32,
    pub(super) base_backoff: Duration,
    pub(super) original_failure: &'a str,
    pub(super) ctrl_c: tokio::sync::watch::Receiver<bool>,
}

pub(super) async fn reconnect_order_response(
    request: ReconnectRequest<'_>,
) -> Result<(ReconnectedOrderResponse, u32)> {
    let ReconnectRequest {
        cleanup_client,
        symbol,
        session_started_at,
        run_order_prefix,
        expected_position,
        qty_tolerance,
        output_format,
        attempts_used,
        max_attempts,
        base_backoff,
        original_failure,
        ctrl_c,
    } = request;
    let mut cleanup_client = cleanup_client;
    let mut ctrl_c = ctrl_c;
    let first_attempt = attempts_used.saturating_add(1);
    let mut last_error = None;
    // The runtime Cleanup effect has already emptied and verified the maker
    // book before it emits Recover. Only repeat cleanup between failed
    // reconnect attempts, when a late venue-side request may have surfaced.
    let mut cleanup_needed = false;

    for attempt in first_attempt..=max_attempts {
        emit_order_response_reconnect(
            output_format,
            symbol,
            "starting",
            attempt,
            max_attempts,
            original_failure,
        );

        let cleanup_ok = if cleanup_needed {
            match cancel_maker_orders_with_retry(&cleanup_client, symbol, 3, output_format).await {
                Ok(()) => true,
                Err(error) => {
                    last_error = Some(anyhow::anyhow!("retry cleanup failed: {error}"));
                    false
                }
            }
        } else {
            true
        };
        if cleanup_ok {
            // Give just-submitted HTTP orders time to become visible, then
            // require a second authoritative snapshot after authentication.
            // The maker book is verified empty at this point, so aborting the
            // reconnect waits on Ctrl+C is safe.
            tokio::select! {
                biased;
                _ = ctrl_c_latched(&mut ctrl_c) => {
                    return Err(anyhow::Error::new(ReconnectInterrupted));
                }
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
            let session_id = uuid::Uuid::new_v4().to_string();
            let candidate_client = StandXClient::new()?.with_session_id(&session_id);
            let stream = OrderResponseStream::new(&session_id)?;
            let connect_attempt = tokio::select! {
                biased;
                _ = ctrl_c_latched(&mut ctrl_c) => {
                    return Err(anyhow::Error::new(ReconnectInterrupted));
                }
                result = tokio::time::timeout(Duration::from_secs(15), stream.connect()) => result,
            };
            match connect_attempt {
                Ok(Ok((commands, responses, health, handle))) => {
                    match query_reconnect_snapshot(
                        &candidate_client,
                        symbol,
                        session_started_at,
                        run_order_prefix,
                    )
                    .await
                    {
                        Ok(snapshot) => {
                            if (snapshot.position - expected_position).abs() > qty_tolerance {
                                handle.abort();
                                return Err(anyhow::Error::new(PositionReconciliationError {
                                    expected: expected_position,
                                    observed: snapshot.position,
                                }));
                            }
                            if !health.is_healthy() {
                                let reason = health.failure_reason().unwrap_or_else(|| {
                                    "new order-response session became unhealthy during reconciliation without a recorded reason".to_string()
                                });
                                handle.abort();
                                cleanup_client = candidate_client;
                                last_error = Some(anyhow::anyhow!(
                                    "new order-response session failed during reconciliation: {reason}"
                                ));
                            } else {
                                let message = format!(
                                    "authenticated new session {}; maker book empty; position={:+.8}; maker filled orders={}; maker trades={}",
                                    session_id,
                                    snapshot.position,
                                    snapshot.maker_filled_orders,
                                    snapshot.maker_trades,
                                );
                                emit_order_response_reconnect(
                                    output_format,
                                    symbol,
                                    "complete",
                                    attempt,
                                    max_attempts,
                                    &message,
                                );
                                return Ok((
                                    ReconnectedOrderResponse {
                                        client: candidate_client,
                                        commands,
                                        responses,
                                        health,
                                        handle,
                                    },
                                    attempt,
                                ));
                            }
                        }
                        Err(error) => {
                            handle.abort();
                            cleanup_client = candidate_client;
                            last_error =
                                Some(anyhow::anyhow!("post-auth reconciliation failed: {error}"));
                        }
                    }
                }
                Ok(Err(error)) => {
                    cleanup_client = candidate_client;
                    last_error = Some(anyhow::anyhow!(
                        "order-response authentication failed: {error}"
                    ));
                }
                Err(_) => {
                    cleanup_client = candidate_client;
                    last_error = Some(anyhow::anyhow!(
                        "order-response reconnect timed out after 15 seconds"
                    ));
                }
            }
        }
        cleanup_needed = true;

        let error_text = last_error
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "unknown reconnect failure".to_string());
        emit_order_response_reconnect(
            output_format,
            symbol,
            "attempt_failed",
            attempt,
            max_attempts,
            &error_text,
        );
        if attempt < max_attempts {
            let local_attempt = attempt.saturating_sub(first_attempt).min(4);
            let multiplier = 1_u32 << local_attempt;
            tokio::select! {
                biased;
                _ = ctrl_c_latched(&mut ctrl_c) => {
                    return Err(anyhow::Error::new(ReconnectInterrupted));
                }
                _ = tokio::time::sleep(base_backoff.saturating_mul(multiplier)) => {}
            }
        }
    }

    Err(anyhow::anyhow!(
        "safe order-response reconnect exhausted: {}",
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "no attempts available".to_string())
    ))
}
