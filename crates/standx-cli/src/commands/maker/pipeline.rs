use crate::cli::OutputFormat;
use anyhow::Result;
use standx_maker::{
    MakerAccountProjection, MakerConfig, MakerLedger, MakerStats, RestingQuote, VolBreaker,
};
use standx_sdk::account_stream::AccountStreamHealth;
use standx_sdk::client::StandXClient;
use standx_sdk::models::{Balance, Order, Position, Trade};
use standx_sdk::order_response::{OrderCommandSender, OrderResponseHealth};
use std::time::{Duration, Instant};

const ACCOUNT_AUDIT_INTERVAL: Duration = Duration::from_secs(30);
const BALANCE_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const BALANCE_MAX_STALE: Duration = Duration::from_secs(60);
const BALANCE_REFRESH_RETRY: Duration = Duration::from_secs(5);

pub(super) struct CycleRequest<'a> {
    pub(super) client: &'a StandXClient,
    pub(super) symbol: &'a str,
    pub(super) cfg: &'a MakerConfig,
    pub(super) live: bool,
    pub(super) cycle: u64,
    pub(super) mark: f64,
    pub(super) best_bid: Option<f64>,
    pub(super) best_ask: Option<f64>,
    pub(super) market_source: &'static str,
    pub(super) market_fallback_reason: Option<&'static str>,
    pub(super) max_divergence_bps: f64,
    pub(super) inventory_exit_pct: f64,
    pub(super) inventory_exit_qty: f64,
    pub(super) session_started_at: i64,
    pub(super) run_order_prefix: &'a str,
    pub(super) starting_position: f64,
    pub(super) output_format: OutputFormat,
    pub(super) order_commands: Option<&'a OrderCommandSender>,
    pub(super) order_response_health: Option<&'a OrderResponseHealth>,
    pub(super) account_stream_health: Option<&'a AccountStreamHealth>,
}

pub(super) struct CycleState<'a> {
    pub(super) resting: &'a mut Vec<RestingQuote>,
    pub(super) account_projection: Option<&'a mut MakerAccountProjection>,
    pub(super) inventory_exit_pending: &'a mut bool,
    pub(super) ledger: &'a mut MakerLedger,
    pub(super) sim_position: &'a mut f64,
    pub(super) stats: &'a mut MakerStats,
    pub(super) breaker: &'a mut VolBreaker,
    /// Live-only REST polling state. It is deliberately a CLI concern: it
    /// controls I/O cadence and cached account presentation, not strategy.
    pub(super) live_account_poll: Option<&'a mut LiveAccountPollState>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct CycleResult {
    pub(super) places: u64,
    pub(super) cancels: u64,
    pub(super) holds: u64,
    pub(super) fills: u64,
    /// Latest account snapshot (live mode only; `None` in paper mode).
    pub(super) balance: Option<Balance>,
}

pub(super) struct AccountAudit {
    pub(super) open_orders: Vec<Order>,
    pub(super) positions: Vec<Position>,
    pub(super) filled_orders: Vec<Order>,
    pub(super) trades: Vec<Trade>,
}

/// Cached, REST-derived account presentation plus the low-frequency full
/// account audit cadence. Healthy maker cycles use the authenticated account
/// stream projection and perform no account REST reads.
pub(super) struct LiveAccountPollState {
    balance: Balance,
    balance_updated_at: Instant,
    next_balance_refresh_at: Instant,
    next_account_audit_at: Instant,
}

impl LiveAccountPollState {
    pub(super) fn new(balance: Balance, now: Instant) -> Self {
        Self {
            balance,
            balance_updated_at: now,
            next_balance_refresh_at: now + BALANCE_REFRESH_INTERVAL,
            next_account_audit_at: now + ACCOUNT_AUDIT_INTERVAL,
        }
    }

    pub(super) fn balance(&self) -> &Balance {
        &self.balance
    }

    pub(super) fn balance_refresh_due(&self, now: Instant) -> bool {
        now >= self.next_balance_refresh_at
    }

    /// Make the next maker cycle refresh the authoritative unified balance.
    ///
    /// The account stream's `balance` payload is a wallet-level view and does
    /// not expose the derived `equity` / `cross_available` values used by the
    /// configured account-risk floors. A stream update therefore acts as an
    /// immediate refresh trigger rather than being reinterpreted as those REST
    /// fields.
    pub(super) fn request_balance_refresh(&mut self, now: Instant) {
        self.next_balance_refresh_at = self.next_balance_refresh_at.min(now);
    }

    pub(super) fn account_audit_due(&self, now: Instant) -> bool {
        now >= self.next_account_audit_at
    }

    pub(super) fn balance_is_within_stale_limit(&self, now: Instant) -> bool {
        now.duration_since(self.balance_updated_at) <= BALANCE_MAX_STALE
    }

    pub(super) fn record_balance_refresh(&mut self, balance: Balance, now: Instant) {
        self.balance = balance;
        self.balance_updated_at = now;
        self.next_balance_refresh_at = now + BALANCE_REFRESH_INTERVAL;
    }

    pub(super) fn record_balance_refresh_failure(&mut self, now: Instant) {
        self.next_balance_refresh_at = now + BALANCE_REFRESH_RETRY;
    }

    pub(super) fn record_account_audit(&mut self, now: Instant) {
        self.next_account_audit_at = now + ACCOUNT_AUDIT_INTERVAL;
    }
}

pub(super) async fn fetch_account_audit(
    client: &StandXClient,
    symbol: &str,
    session_started_at: i64,
    now: i64,
) -> Result<AccountAudit> {
    let (open_orders, positions, filled_orders, trades) = tokio::join!(
        client.get_open_orders(Some(symbol)),
        client.get_positions(Some(symbol)),
        client.get_order_history(Some(symbol), Some(100)),
        client.get_user_trades(symbol, session_started_at, now, Some(500)),
    );
    Ok(AccountAudit {
        open_orders: open_orders?,
        positions: positions?,
        filled_orders: filled_orders?,
        trades: trades?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Matcher, Server};

    struct JwtGuard {
        original: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl JwtGuard {
        fn set() -> Self {
            // Share the crate-wide env lock so this STANDX_JWT mutation cannot
            // run concurrently with env reads in other modules' tests (e.g. the
            // maker cleanup test's Credentials::load). A per-module lock would
            // not exclude those cross-module races. See crate::TEST_ENV_LOCK.
            let lock = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let original = std::env::var("STANDX_JWT").ok();
            std::env::set_var("STANDX_JWT", "pipeline-test-jwt");
            Self {
                original,
                _lock: lock,
            }
        }
    }

    impl Drop for JwtGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(value) => std::env::set_var("STANDX_JWT", value),
                None => std::env::remove_var("STANDX_JWT"),
            }
        }
    }

    fn balance() -> Balance {
        Balance {
            balance: "100".to_string(),
            cross_available: "90".to_string(),
            cross_balance: "100".to_string(),
            cross_margin: "0".to_string(),
            cross_upnl: "0".to_string(),
            equity: "100".to_string(),
            isolated_balance: "0".to_string(),
            isolated_upnl: "0".to_string(),
            locked: "0".to_string(),
            pnl_24h: "0".to_string(),
            pnl_freeze: "0".to_string(),
            upnl: "0".to_string(),
        }
    }

    #[test]
    fn live_account_poll_uses_success_based_audit_and_balance_schedules() {
        let now = Instant::now();
        let mut state = LiveAccountPollState::new(balance(), now);
        let due = now + Duration::from_secs(30);

        assert!(!state.account_audit_due(due - Duration::from_millis(1)));
        assert!(!state.balance_refresh_due(due - Duration::from_millis(1)));
        assert!(state.account_audit_due(due));
        assert!(state.balance_refresh_due(due));

        state.record_account_audit(due);
        state.record_balance_refresh(balance(), due);
        assert!(!state.account_audit_due(due + Duration::from_secs(29)));
        assert!(!state.balance_refresh_due(due + Duration::from_secs(29)));
    }

    #[test]
    fn balance_failure_retries_quickly_and_expires_after_stale_limit() {
        let now = Instant::now();
        let mut state = LiveAccountPollState::new(balance(), now);
        let refresh_due = now + Duration::from_secs(30);

        state.record_balance_refresh_failure(refresh_due);
        assert!(!state.balance_refresh_due(refresh_due + Duration::from_secs(4)));
        assert!(state.balance_refresh_due(refresh_due + Duration::from_secs(5)));
        assert!(state.balance_is_within_stale_limit(now + Duration::from_secs(60)));
        assert!(!state.balance_is_within_stale_limit(now + Duration::from_secs(61)));
    }

    #[test]
    fn account_stream_balance_update_makes_authoritative_refresh_due_immediately() {
        let now = Instant::now();
        let mut state = LiveAccountPollState::new(balance(), now);

        assert!(!state.balance_refresh_due(now));
        state.request_balance_refresh(now);
        assert!(state.balance_refresh_due(now));

        state.record_balance_refresh(balance(), now);
        assert!(!state.balance_refresh_due(now + Duration::from_secs(29)));
        assert!(state.balance_refresh_due(now + Duration::from_secs(30)));
    }

    #[test]
    fn failed_account_audit_stays_due_until_a_successful_commit() {
        let now = Instant::now();
        let state = LiveAccountPollState::new(balance(), now);
        let due = now + Duration::from_secs(30);

        assert!(state.account_audit_due(due));
        // An audit failure deliberately does not call
        // `record_account_audit`, so the next cycle must retry it.
        assert!(state.account_audit_due(due + Duration::from_secs(1)));
    }

    #[tokio::test]
    async fn normal_cycles_make_no_account_rest_reads_until_audit_is_due() {
        let _jwt = JwtGuard::set();
        let mut server = Server::new_async().await;
        let open_orders = server
            .mock("GET", "/api/query_open_orders")
            .match_query(Matcher::UrlEncoded("symbol".into(), "BTC-USD".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"code":0,"message":"ok","result":[]}"#)
            .expect(1)
            .create_async()
            .await;
        let positions = server
            .mock("GET", "/api/query_positions")
            .match_query(Matcher::UrlEncoded("symbol".into(), "BTC-USD".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("[]")
            .expect(1)
            .create_async()
            .await;
        let filled_orders = server
            .mock("GET", "/api/query_orders")
            .match_query(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"code":0,"message":"ok","result":[]}"#)
            .expect(1)
            .create_async()
            .await;
        let trades = server
            .mock("GET", "/api/query_trades")
            .match_query(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"code":0,"message":"ok","result":[]}"#)
            .expect(1)
            .create_async()
            .await;
        let balance_mock = server
            .mock("GET", "/api/query_balance")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&balance()).unwrap())
            .expect(1)
            .create_async()
            .await;
        let client = StandXClient::with_base_url(server.url()).unwrap();
        let now = Instant::now();
        let mut poll = LiveAccountPollState::new(balance(), now);

        // A healthy cycle before the deadline does not invoke any of the
        // mocks above; it reads the local projection instead.
        assert!(!poll.account_audit_due(now));
        assert!(!poll.balance_refresh_due(now));

        let due = now + Duration::from_secs(30);
        assert!(poll.account_audit_due(due));
        assert!(poll.balance_refresh_due(due));
        let (audit, refreshed_balance) = tokio::join!(
            fetch_account_audit(&client, "BTC-USD", 1_784_304_000, 1_784_304_060),
            client.get_balance(),
        );
        let audit = audit.unwrap();
        assert!(audit.open_orders.is_empty());
        assert!(audit.positions.is_empty());
        assert!(audit.trades.is_empty());
        poll.record_account_audit(due);
        poll.record_balance_refresh(refreshed_balance.unwrap(), due);

        open_orders.assert_async().await;
        positions.assert_async().await;
        filled_orders.assert_async().await;
        trades.assert_async().await;
        balance_mock.assert_async().await;
    }
}
