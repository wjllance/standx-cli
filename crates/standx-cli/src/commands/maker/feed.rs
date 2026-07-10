use anyhow::Result;
use standx_sdk::client::StandXClient;
use standx_sdk::websocket::{StandXWebSocket, WsMessage};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, RwLock};

/// Latest market data from the WebSocket feed. Values are pre-parsed on
/// receipt so cycle reads are lock-and-go.
#[derive(Default)]
pub(super) struct FeedState {
    pub(super) mark: Option<f64>,
    pub(super) mark_at: Option<std::time::Instant>,
    pub(super) best_bid: Option<f64>,
    pub(super) best_ask: Option<f64>,
    pub(super) book_at: Option<std::time::Instant>,
}

/// WS cache entries older than this fall back to REST for the cycle. REST
/// polling refreshed data once per interval, so 5s keeps freshness at least
/// as good as the old behavior while tolerating slow feed ticks.
const WS_STALE_AFTER: Duration = Duration::from_secs(5);

/// Spawn the resident market-feed task: one public WS connection carrying
/// `price` + `depth_book`, written into a shared cache. The outer loop wraps
/// the SDK's internal 5-attempt reconnect — when the stream ends (attempts
/// exhausted or clean close), it rebuilds the connection from scratch, since
/// subscriptions only take effect when registered before `connect()`.
pub(super) fn spawn_market_feed(
    symbol: String,
    verbose: bool,
) -> (
    Arc<RwLock<FeedState>>,
    watch::Receiver<u64>,
    tokio::task::JoinHandle<()>,
) {
    let state = Arc::new(RwLock::new(FeedState::default()));
    let (tx, rx) = watch::channel(0u64);
    let state_task = state.clone();

    let handle = tokio::spawn(async move {
        let mut seq = 0u64;
        loop {
            let ws = match StandXWebSocket::without_auth_with_verbose(verbose) {
                Ok(ws) => ws,
                Err(e) => {
                    eprintln!("⚠️  market feed setup failed: {e}; retrying in 10s");
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    continue;
                }
            };
            let _ = ws.subscribe("price", Some(&symbol)).await;
            let _ = ws.subscribe("depth_book", Some(&symbol)).await;
            let mut events = match ws.connect().await {
                Ok(rx) => rx,
                Err(e) => {
                    eprintln!("⚠️  market feed connect failed: {e}; retrying in 10s");
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    continue;
                }
            };
            while let Some(msg) = events.recv().await {
                let now = std::time::Instant::now();
                match msg {
                    WsMessage::Price(p) if p.symbol.eq_ignore_ascii_case(&symbol) => {
                        if let Ok(mark) = p.mark_price.parse::<f64>() {
                            let mut s = state_task.write().await;
                            s.mark = Some(mark);
                            s.mark_at = Some(now);
                        }
                    }
                    WsMessage::Depth(d) if d.symbol.eq_ignore_ascii_case(&symbol) => {
                        let mut s = state_task.write().await;
                        s.best_bid = d.best_bid().and_then(|v| v.parse().ok());
                        s.best_ask = d.best_ask().and_then(|v| v.parse().ok());
                        s.book_at = Some(now);
                    }
                    _ => continue,
                }
                seq += 1;
                let _ = tx.send(seq);
            }
            // Stream ended: SDK reconnects exhausted or server closed.
            eprintln!("⚠️  market feed stream ended; rebuilding connection in 10s");
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });

    (state, rx, handle)
}

/// One market snapshot: WS cache when fresh, REST fallback otherwise
/// (startup warm-up, feed outage, or --no-ws).
pub(super) async fn market_snapshot(
    client: &StandXClient,
    symbol: &str,
    feed: Option<&Arc<RwLock<FeedState>>>,
) -> Result<(f64, Option<f64>, Option<f64>, &'static str)> {
    if let Some(feed) = feed {
        let s = feed.read().await;
        let fresh =
            |at: Option<std::time::Instant>| at.is_some_and(|t| t.elapsed() < WS_STALE_AFTER);
        if fresh(s.mark_at) && fresh(s.book_at) {
            if let Some(mark) = s.mark {
                if let Ok(snapshot) = validated_snapshot(mark, s.best_bid, s.best_ask, "ws") {
                    return Ok(snapshot);
                }
            }
        }
    }

    let (price, depth) = tokio::join!(
        client.get_symbol_price(symbol),
        client.get_depth(symbol, Some(5))
    );
    let price = price?;
    let depth = depth?;
    let mark: f64 = price
        .mark_price
        .parse()
        .map_err(|_| anyhow::anyhow!("unparseable mark price: {}", price.mark_price))?;
    let best_bid: Option<f64> = depth.best_bid().and_then(|s| s.parse().ok());
    let best_ask: Option<f64> = depth.best_ask().and_then(|s| s.parse().ok());
    validated_snapshot(mark, best_bid, best_ask, "rest")
}

fn validated_snapshot(
    mark: f64,
    best_bid: Option<f64>,
    best_ask: Option<f64>,
    source: &'static str,
) -> Result<(f64, Option<f64>, Option<f64>, &'static str)> {
    if !mark.is_finite() || mark <= 0.0 {
        return Err(anyhow::anyhow!("invalid mark price from {source}: {mark}"));
    }
    if best_bid.is_some_and(|price| !price.is_finite() || price <= 0.0) {
        return Err(anyhow::anyhow!("invalid best bid from {source}"));
    }
    if best_ask.is_some_and(|price| !price.is_finite() || price <= 0.0) {
        return Err(anyhow::anyhow!("invalid best ask from {source}"));
    }
    if let (Some(bid), Some(ask)) = (best_bid, best_ask) {
        if bid >= ask {
            return Err(anyhow::anyhow!(
                "crossed order book from {source}: bid {bid} >= ask {ask}"
            ));
        }
    }

    Ok((mark, best_bid, best_ask, source))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_validation_accepts_valid_and_one_sided_books() {
        assert!(validated_snapshot(100.0, Some(99.9), Some(100.1), "test").is_ok());
        assert!(validated_snapshot(100.0, Some(99.9), None, "test").is_ok());
    }

    #[test]
    fn snapshot_validation_rejects_non_finite_and_crossed_books() {
        assert!(validated_snapshot(f64::NAN, Some(99.9), Some(100.1), "test").is_err());
        assert!(validated_snapshot(100.0, Some(f64::INFINITY), Some(100.1), "test").is_err());
        assert!(validated_snapshot(100.0, Some(100.1), Some(100.1), "test").is_err());
    }
}
