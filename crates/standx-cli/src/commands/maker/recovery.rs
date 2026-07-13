use super::ledger::{adopt_order, apply_rest_trade};
use super::model::{is_current_run_order, is_maker_order, position_for_symbol};
use crate::cli::OutputFormat;
use anyhow::Result;
use standx_maker::{MakerFill, MakerLedger, MakerStats};
use standx_sdk::client::StandXClient;
use standx_sdk::models::{Order, OrderSide, Position, Trade};
use standx_sdk::order_response::{OrderResponse, OrderResponseHealth, OrderResponseStream};
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
    let (orders, filled_orders, trades, positions) = tokio::join!(
        client.get_open_orders(Some(request.symbol)),
        client.get_order_history(Some(request.symbol), Some(100)),
        client.get_user_trades(request.symbol, request.session_started_at, now, Some(500)),
        client.get_positions(Some(request.symbol)),
    );
    let orders = orders?;
    let filled_orders = filled_orders?;
    for order in orders.iter().chain(filled_orders.iter()) {
        adopt_order(ledger, order, request.run_order_prefix)?;
    }
    let trades = trades?;
    let observed = position_for_symbol(&positions?, request.symbol)?;
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
                eprintln!(
                    "⚠️  maker-order cancellation attempt {}/{} incomplete: {}",
                    attempt, attempts, error
                );
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

    let mut position = 0.0;
    for item in positions
        .iter()
        .filter(|position| position.symbol.eq_ignore_ascii_case(symbol))
    {
        let qty = item.qty.parse::<f64>().map_err(|_| {
            anyhow::anyhow!(
                "reconnect reconciliation found invalid position qty '{}' on {symbol}",
                item.qty
            )
        })?;
        if !qty.is_finite() {
            return Err(anyhow::anyhow!(
                "reconnect reconciliation found non-finite position qty on {symbol}"
            ));
        }
        position += match item.side {
            Some(OrderSide::Sell) => -qty,
            _ => qty,
        };
    }

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
    let (open_orders, positions, filled_orders, trades) = tokio::join!(
        client.get_open_orders(Some(symbol)),
        client.get_positions(Some(symbol)),
        client.get_order_history(Some(symbol), Some(100)),
        client.get_user_trades(symbol, session_started_at, now, Some(500)),
    );
    validate_reconnect_snapshot(
        symbol,
        run_order_prefix,
        &open_orders?,
        &positions?,
        &filled_orders?,
        &trades?,
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
    } = request;
    let mut cleanup_client = cleanup_client;
    let first_attempt = attempts_used.saturating_add(1);
    let mut last_error = None;

    for attempt in first_attempt..=max_attempts {
        emit_order_response_reconnect(
            output_format,
            symbol,
            "starting",
            attempt,
            max_attempts,
            original_failure,
        );

        if let Err(error) =
            cancel_maker_orders_with_retry(&cleanup_client, symbol, 3, output_format).await
        {
            last_error = Some(anyhow::anyhow!("pre-reconnect cleanup failed: {error}"));
        } else {
            // Give just-submitted HTTP orders time to become visible, then
            // require a second authoritative snapshot after authentication.
            tokio::time::sleep(Duration::from_secs(1)).await;
            let session_id = uuid::Uuid::new_v4().to_string();
            let candidate_client = StandXClient::new()?.with_session_id(&session_id);
            let stream = OrderResponseStream::new(&session_id)?;
            match tokio::time::timeout(Duration::from_secs(15), stream.connect()).await {
                Ok(Ok((responses, health, handle))) => {
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
            tokio::time::sleep(base_backoff.saturating_mul(multiplier)).await;
        }
    }

    Err(anyhow::anyhow!(
        "safe order-response reconnect exhausted: {}",
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "no attempts available".to_string())
    ))
}
