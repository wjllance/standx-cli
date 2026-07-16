use super::super::feed::FeedState;
use super::*;

pub(super) struct RuntimeDeps {
    pub(super) args: MakerRunArgs,
    pub(super) output_format: OutputFormat,
    pub(super) client: StandXClient,
    pub(super) cfg: MakerConfig,
    pub(super) symbol: String,
    pub(super) notifier: MakerNotifier,
    pub(super) qty_tolerance: f64,
    pub(super) run_order_prefix: String,
    pub(super) starting_position: f64,
    pub(super) baseline_mark: f64,
    pub(super) session_started_at: i64,
}

#[derive(Default)]
pub(super) struct RuntimeCounters {
    pub(super) cycle: u64,
    pub(super) total_places: u64,
    pub(super) total_cancels: u64,
    pub(super) total_holds: u64,
    pub(super) total_fills: u64,
    pub(super) total_halted: u64,
}

pub(super) struct RuntimeLoopState {
    pub(super) resting: Vec<RestingQuote>,
    pub(super) inventory_exit_pending: bool,
    pub(super) ledger: MakerLedger,
    pub(super) performance_started: std::time::Instant,
    pub(super) performance_epoch_ms: i64,
    pub(super) position_alert_anchor: PositionAlertAnchor,
    pub(super) counters: RuntimeCounters,
    pub(super) next_cycle_is_recovery: bool,
    pub(super) sim_position: f64,
    pub(super) stats: MakerStats,
    pub(super) breaker: VolBreaker,
    pub(super) alerts: AlertMonitor,
    pub(super) account_balance_refresh_requested: bool,
    pub(super) balance_floor_parse_warned: bool,
}

pub(super) struct RuntimeMarketState {
    pub(super) feed: Option<std::sync::Arc<tokio::sync::RwLock<FeedState>>>,
    pub(super) updates: Option<tokio::sync::watch::Receiver<u64>>,
    pub(super) market_watchdog_updates: Option<tokio::sync::watch::Receiver<u64>>,
    pub(super) feed_handle: Option<tokio::task::JoinHandle<()>>,
    pub(super) health_started: std::time::Instant,
    pub(super) health: maker::MarketDataHealth,
    pub(super) pending_degradation: Option<String>,
    pub(super) standby_started: Option<std::time::Instant>,
    pub(super) next_heartbeat: Option<std::time::Instant>,
    pub(super) last_divergence_bps: Option<f64>,
    pub(super) maker_book_verified_empty: bool,
    pub(super) last_mark: Option<f64>,
    pub(super) last_src: Option<&'static str>,
}

pub(super) struct RuntimeRecoveryState {
    pub(super) account_position_mismatch: Option<f64>,
    pub(super) pending_request_timeout: Option<TimedOutOrderRequest>,
    pub(super) account_order_reconciliation_required: bool,
    pub(super) runtime_state: MakerState,
}

pub(super) struct RuntimeLifecycleState {
    pub(super) token_expiry_alerted: TokenExpiryLevel,
    pub(super) last_token_expiry_check: Option<std::time::Instant>,
}

pub(super) struct MakerRuntime {
    pub(super) deps: RuntimeDeps,
    pub(super) loop_state: RuntimeLoopState,
    pub(super) market: RuntimeMarketState,
    pub(super) recovery: RuntimeRecoveryState,
    pub(super) lifecycle: RuntimeLifecycleState,
    pub(super) live_session: Option<LiveSession>,
    pub(super) ctrl_c_rx: tokio::sync::watch::Receiver<bool>,
}

pub(super) enum LoopDirective {
    Proceed,
    Restart,
    Exit(MakerExit),
}

impl MakerRuntime {
    pub(super) fn new(
        args: MakerRunArgs,
        output_format: OutputFormat,
        startup: MakerStartup,
    ) -> Result<Self> {
        let MakerStartup {
            client,
            cfg,
            symbol,
            notifier,
            qty_tolerance,
            run_order_prefix,
            starting_position,
            baseline_mark,
            session_started_at,
            live_session,
        } = startup;

        let (feed, updates, feed_handle) = if args.no_ws {
            (None, None, None)
        } else {
            let (state, rx, handle) = spawn_market_feed(symbol.clone(), args.verbose);
            (Some(state), Some(rx), Some(handle))
        };
        let market_watchdog_updates = updates.as_ref().cloned();

        let mut ledger = MakerLedger::new(starting_position);
        ledger.enable_performance(baseline_mark)?;
        let performance_started = std::time::Instant::now();
        let performance_epoch_ms = chrono::Utc::now().timestamp_millis();
        let position_alert_anchor =
            PositionAlertAnchor::new(starting_position, args.alert_position_change_pct);
        let stats = if args.live {
            MakerStats::with_inventory_baseline(starting_position, baseline_mark)
        } else {
            MakerStats::default()
        };
        let breaker = VolBreaker::new(args.vol_window.max(1) as usize, args.vol_pause_bps);
        let alerts =
            AlertMonitor::new(args.alert_loss, args.alert_inventory_pct, args.alert_uptime)
                .with_account_floors(args.alert_equity_below, args.alert_margin_below);
        let market_health_started = std::time::Instant::now();
        let mut runtime_state = MakerState::starting();
        runtime_state.handle(MakerEvent::StartupReady);

        // Tokio installs a process-wide SIGINT handler on the first call. Keep
        // one long-lived task and latch presses so no phase can lose Ctrl+C.
        let (ctrl_c_tx, ctrl_c_rx) = tokio::sync::watch::channel(false);
        tokio::spawn(async move {
            while tokio::signal::ctrl_c().await.is_ok() {
                let _ = ctrl_c_tx.send(true);
            }
        });

        Ok(Self {
            deps: RuntimeDeps {
                args,
                output_format,
                client,
                cfg,
                symbol,
                notifier,
                qty_tolerance,
                run_order_prefix,
                starting_position,
                baseline_mark,
                session_started_at,
            },
            loop_state: RuntimeLoopState {
                resting: Vec::new(),
                inventory_exit_pending: false,
                ledger,
                performance_started,
                performance_epoch_ms,
                position_alert_anchor,
                counters: RuntimeCounters::default(),
                next_cycle_is_recovery: false,
                sim_position: 0.0,
                stats,
                breaker,
                alerts,
                account_balance_refresh_requested: false,
                balance_floor_parse_warned: false,
            },
            market: RuntimeMarketState {
                feed,
                updates,
                market_watchdog_updates,
                feed_handle,
                health_started: market_health_started,
                health: maker::MarketDataHealth::default(),
                pending_degradation: None,
                standby_started: None,
                next_heartbeat: None,
                last_divergence_bps: None,
                maker_book_verified_empty: false,
                last_mark: None,
                last_src: None,
            },
            recovery: RuntimeRecoveryState {
                account_position_mismatch: None,
                pending_request_timeout: None,
                account_order_reconciliation_required: false,
                runtime_state,
            },
            lifecycle: RuntimeLifecycleState {
                token_expiry_alerted: TokenExpiryLevel::Ok,
                last_token_expiry_check: None,
            },
            live_session,
            ctrl_c_rx,
        })
    }
}
