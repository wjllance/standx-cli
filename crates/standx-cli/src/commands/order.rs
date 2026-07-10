use crate::cli::*;
use anyhow::Result;
use standx_sdk::client::order::CreateOrderParams;
use standx_sdk::client::StandXClient;
use standx_sdk::models::{OrderSide, OrderType, TimeInForce};

/// Handle order commands
pub async fn handle_order(command: OrderCommands) -> Result<()> {
    let client = StandXClient::new()?;

    match command {
        OrderCommands::Create {
            symbol,
            side,
            order_type,
            qty,
            price,
            tif,
            reduce_only,
            sl_price,
            tp_price,
        } => {
            // Parse side
            let side = match side.to_lowercase().as_str() {
                "buy" => OrderSide::Buy,
                "sell" => OrderSide::Sell,
                _ => return Err(anyhow::anyhow!("Invalid side: {}", side)),
            };

            // Parse order type
            let order_type = match order_type.to_lowercase().as_str() {
                "limit" => OrderType::Limit,
                "market" => OrderType::Market,
                _ => return Err(anyhow::anyhow!("Invalid order type: {}", order_type)),
            };

            // Parse time in force
            let time_in_force = tif.map(|t| match t.to_uppercase().as_str() {
                "GTC" => TimeInForce::Gtc,
                "IOC" => TimeInForce::Ioc,
                "FOK" => TimeInForce::Fok,
                "ALO" => TimeInForce::Alo,
                _ => TimeInForce::Gtc,
            });

            let params = CreateOrderParams {
                symbol,
                cl_ord_id: None,
                side,
                order_type,
                quantity: qty,
                price,
                time_in_force,
                reduce_only,
                stop_price: None,
                sl_price,
                tp_price,
            };

            let order = client.create_order(params).await?;
            println!("✅ Order created successfully!");
            println!("   Order ID: {}", order.id);
            println!("   Symbol: {}", order.symbol);
            println!("   Side: {:?}", order.side);
            println!("   Type: {:?}", order.order_type);
            println!("   Quantity: {}", order.qty);
            if !order.price.is_empty() && order.price != "0" {
                println!("   Price: {}", order.price);
            }
        }
        OrderCommands::Cancel { symbol, order_id } => {
            client.cancel_order(&symbol, &order_id).await?;
            println!("✅ Order {} cancelled successfully", order_id);
        }
        OrderCommands::CancelAll { symbol } => {
            client.cancel_all_orders(&symbol).await?;
            println!("✅ All orders for {} cancelled successfully", symbol);
        }
    }
    Ok(())
}
