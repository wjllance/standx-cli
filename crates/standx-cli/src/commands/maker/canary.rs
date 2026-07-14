//! Bounded live-gate verification for the authenticated order command socket.
//!
//! This intentionally lives in the CLI: it executes network I/O and does not
//! alter the maker planner or its normal quote decisions.

use super::model::position_for_symbol;
use super::notify::MakerNotifier;
use super::{FailSafeShutdown, LIVE_MAKER_ENV};
use crate::cli::{AlertWebhookFormat, OutputFormat};
use anyhow::Result;
use standx_maker::round_to_decimals;
use standx_sdk::auth::Credentials;
use standx_sdk::client::order::CreateOrderParams;
use standx_sdk::client::StandXClient;
use standx_sdk::models::{Order, OrderSide, OrderType, TimeInForce};
use standx_sdk::order_response::{OrderCommandSender, OrderResponse, OrderResponseStream};
use std::time::Duration;
use tokio::sync::mpsc;

const CANARY_PREFIX: &str = "sxmk-canary-";

fn canary_price(mark: f64, offset_bps: f64, decimals: u32) -> Result<f64> {
    if !mark.is_finite() || mark <= 0.0 {
        return Err(anyhow::anyhow!("venue returned an invalid mark price"));
    }
    if !(1.0..=1_000.0).contains(&offset_bps) {
        return Err(anyhow::anyhow!(
            "--price-offset-bps must be between 1 and 1000 for a bounded post-only canary"
        ));
    }
    let price = round_to_decimals(mark * (1.0 - offset_bps / 10_000.0), decimals);
    if price <= 0.0 {
        return Err(anyhow::anyhow!(
            "canary price rounded to a non-positive value"
        ));
    }
    Ok(price)
}

async fn await_response(
    responses: &mut mpsc::Receiver<OrderResponse>,
    request_id: &str,
    timeout: Duration,
) -> Result<OrderResponse> {
    let response = tokio::time::timeout(timeout, responses.recv())
        .await
        .map_err(|_| anyhow::anyhow!("timed out waiting for order-response acknowledgement"))?
        .ok_or_else(|| {
            anyhow::anyhow!("order-response stream closed before the expected acknowledgement")
        })?;
    if response.request_id.as_deref() == Some(request_id) {
        Ok(response)
    } else {
        Err(anyhow::anyhow!(
            "received an uncorrelated order-response acknowledgement during canary"
        ))
    }
}

async fn wait_for_order(
    client: &StandXClient,
    symbol: &str,
    client_order_id: &str,
    timeout: Duration,
) -> Result<Order> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Some(order) = client
            .get_open_orders(Some(symbol))
            .await?
            .into_iter()
            .find(|order| order.cl_ord_id.as_deref() == Some(client_order_id))
        {
            return Ok(order);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow::anyhow!(
                "canary order was not visible through the HTTP open-order snapshot"
            ));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn wait_until_absent(
    client: &StandXClient,
    symbol: &str,
    client_order_id: &str,
    timeout: Duration,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let present = client
            .get_open_orders(Some(symbol))
            .await?
            .into_iter()
            .any(|order| order.cl_ord_id.as_deref() == Some(client_order_id));
        if !present {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow::anyhow!(
                "canary order remained visible after its cancellation acknowledgement"
            ));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn cleanup_current_order(
    client: &StandXClient,
    symbol: &str,
    client_order_id: &str,
    timeout: Duration,
) -> Result<()> {
    let orders = client.get_open_orders(Some(symbol)).await?;
    if let Some(order) = orders
        .into_iter()
        .find(|order| order.cl_ord_id.as_deref() == Some(client_order_id))
    {
        client.cancel_order(symbol, &order.id).await?;
        wait_until_absent(client, symbol, client_order_id, timeout).await?;
    }
    Ok(())
}

pub(super) async fn run_ws_command_canary(
    symbol: String,
    size: Option<f64>,
    price_offset_bps: f64,
    timeout_secs: u64,
    alert_webhook: Option<String>,
    alert_webhook_format: AlertWebhookFormat,
    output_format: OutputFormat,
) -> Result<()> {
    if std::env::var(LIVE_MAKER_ENV).ok().as_deref() != Some("1") {
        return Err(anyhow::anyhow!(
            "{}=1 is required for the WS command canary",
            LIVE_MAKER_ENV
        ));
    }
    if !(1..=30).contains(&timeout_secs) {
        return Err(anyhow::anyhow!("--timeout-secs must be between 1 and 30"));
    }
    let webhook = alert_webhook.ok_or_else(|| {
        anyhow::anyhow!("WS command canary requires --alert-webhook for fail-safe delivery")
    })?;
    let credentials = Credentials::load()?;
    if credentials.is_expired() || credentials.private_key.is_empty() {
        return Err(anyhow::anyhow!(
            "WS command canary requires current signing credentials"
        ));
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    let client = StandXClient::new()?.with_session_id(session_id.clone());
    let notifier = MakerNotifier::new(output_format, Some(webhook), alert_webhook_format);
    let timeout = Duration::from_secs(timeout_secs);
    let infos = client.get_symbol_info().await?;
    let info = infos
        .iter()
        .find(|info| info.symbol.eq_ignore_ascii_case(&symbol))
        .ok_or_else(|| anyhow::anyhow!("unknown symbol '{symbol}'"))?;
    if info.status != "trading" {
        return Err(anyhow::anyhow!("symbol {} is not trading", info.symbol));
    }
    let symbol = info.symbol.clone();
    if !client.get_open_orders(Some(&symbol)).await?.is_empty() {
        return Err(anyhow::anyhow!(
            "WS command canary requires an empty {} order book to avoid touching unrelated orders",
            symbol
        ));
    }
    if position_for_symbol(&client.get_positions(Some(&symbol)).await?, &symbol)?.abs() > 0.0 {
        return Err(anyhow::anyhow!(
            "WS command canary requires a flat {} position",
            symbol
        ));
    }
    let min_size: f64 = info.min_order_qty.parse().map_err(|_| {
        anyhow::anyhow!("venue returned an invalid minimum quantity for {}", symbol)
    })?;
    let quantity = round_to_decimals(size.unwrap_or(min_size), info.qty_tick_decimals);
    if quantity < min_size || quantity <= 0.0 {
        return Err(anyhow::anyhow!(
            "--size is below the venue minimum for {}",
            symbol
        ));
    }
    let mark: f64 = client
        .get_symbol_market(&symbol)
        .await?
        .mark_price
        .parse()
        .map_err(|_| anyhow::anyhow!("venue returned an invalid mark price for {}", symbol))?;
    let price = canary_price(mark, price_offset_bps, info.price_tick_decimals)?;
    let run_id = uuid::Uuid::new_v4().simple().to_string();
    let client_order_id = format!("{}{}", CANARY_PREFIX, &run_id[..12]);
    let stream = OrderResponseStream::new(session_id)?;
    let (commands, mut responses, _health, handle) = stream.connect().await?;
    notifier
        .lifecycle(
            "started",
            "WS command canary started: submitting one bounded post-only order",
            &symbol,
            false,
        )
        .await;

    let result = run_commands(
        &client,
        &commands,
        &mut responses,
        &symbol,
        &client_order_id,
        quantity,
        price,
        info.qty_tick_decimals,
        info.price_tick_decimals,
        timeout,
    )
    .await;
    handle.abort();
    match result {
        Ok(()) => {
            notifier
                .lifecycle(
                    "completed",
                    "WS command canary completed: acknowledged create/cancel and HTTP absence verified",
                    &symbol,
                    true,
                )
                .await;
            Ok(())
        }
        Err(error) => {
            let message = match cleanup_current_order(&client, &symbol, &client_order_id, timeout).await {
                Ok(()) => format!("WS command canary failed safe: {error}"),
                Err(cleanup_error) => format!(
                    "WS command canary failed safe: {error}; HTTP cleanup could not verify absence: {cleanup_error}"
                ),
            };
            notifier.lifecycle("failed", &message, &symbol, true).await;
            Err(anyhow::Error::new(FailSafeShutdown { message }))
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_commands(
    client: &StandXClient,
    commands: &OrderCommandSender,
    responses: &mut mpsc::Receiver<OrderResponse>,
    symbol: &str,
    client_order_id: &str,
    quantity: f64,
    price: f64,
    qty_decimals: u32,
    price_decimals: u32,
    timeout: Duration,
) -> Result<()> {
    let create_request_id = commands
        .create_order(&CreateOrderParams {
            symbol: symbol.to_string(),
            cl_ord_id: Some(client_order_id.to_string()),
            side: OrderSide::Buy,
            order_type: OrderType::Limit,
            quantity: format!("{quantity:.precision$}", precision = qty_decimals as usize),
            price: Some(format!(
                "{price:.precision$}",
                precision = price_decimals as usize
            )),
            time_in_force: Some(TimeInForce::Alo),
            reduce_only: false,
            stop_price: None,
            sl_price: None,
            tp_price: None,
        })
        .await?;
    let create_response = await_response(responses, &create_request_id, timeout).await?;
    if !create_response.accepted() {
        return Err(anyhow::anyhow!(
            "venue rejected canary order:new: {}",
            create_response.message
        ));
    }
    let order = wait_for_order(client, symbol, client_order_id, timeout).await?;
    let cancel_request_id = commands.cancel_order(&order.id).await?;
    let cancel_response = await_response(responses, &cancel_request_id, timeout).await?;
    if !cancel_response.accepted() {
        return Err(anyhow::anyhow!(
            "venue rejected canary order:cancel: {}",
            cancel_response.message
        ));
    }
    wait_until_absent(client, symbol, client_order_id, timeout).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_is_bounded_below_mark_and_rounded_to_tick() {
        assert_eq!(canary_price(100.0, 100.0, 2).unwrap(), 99.0);
        assert!(canary_price(100.0, 0.0, 2).is_err());
        assert!(canary_price(0.0, 100.0, 2).is_err());
    }
}
