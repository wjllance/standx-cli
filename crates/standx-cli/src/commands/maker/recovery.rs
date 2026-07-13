use super::ledger::MakerLedger;
use super::model::{is_maker_order, position_for_symbol, MakerFill};
use crate::cli::OutputFormat;
use anyhow::Result;
use standx_maker::MakerStats;
use standx_sdk::client::StandXClient;
use standx_sdk::models::Trade;
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
                if let Err(error) = ledger.adopt_order(&order, gap.run_order_prefix) {
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
        ledger.adopt_order(order, request.run_order_prefix)?;
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
        ledger.apply_rest_trade(
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
