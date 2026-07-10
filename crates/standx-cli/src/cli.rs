use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "standx")]
#[command(about = "OpenClaw-first AI Agent trading toolkit")]
#[command(version = env!("CARGO_PKG_VERSION"))]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Configuration file path
    #[arg(short, long, global = true)]
    pub config: Option<String>,

    /// Output format
    #[arg(short, long, global = true, value_enum, default_value = "table")]
    pub output: OutputFormat,

    /// Verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Quiet mode
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// OpenClaw mode - optimized for AI Agent execution
    #[arg(long, global = true, env = "STANDX_OPENCLAW_MODE")]
    pub openclaw: bool,

    /// Dry run - show what would be executed without executing
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Auto-confirm dangerous operations (skip prompts)
    #[arg(long, global = true, env = "STANDX_AUTO_CONFIRM")]
    pub yes: bool,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Configuration management
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Authentication management
    Auth {
        #[command(subcommand)]
        command: AuthCommands,
    },
    /// Market data (public)
    #[command(visible_alias = "m")]
    Market {
        #[command(subcommand)]
        command: MarketCommands,
    },
    /// Account information (authenticated)
    #[command(visible_alias = "a")]
    Account {
        #[command(subcommand)]
        command: AccountCommands,
    },
    /// Order management (authenticated)
    #[command(visible_alias = "o")]
    Order {
        #[command(subcommand)]
        command: OrderCommands,
    },
    /// Trade history (authenticated)
    #[command(visible_alias = "t")]
    Trade {
        #[command(subcommand)]
        command: TradeCommands,
    },
    /// Leverage management (authenticated)
    Leverage {
        #[command(subcommand)]
        command: LeverageCommands,
    },
    /// Margin management (authenticated)
    Margin {
        #[command(subcommand)]
        command: MarginCommands,
    },
    /// Real-time data stream
    Stream {
        #[command(subcommand)]
        command: StreamCommands,
    },
    /// Dashboard - unified view of account, positions, orders, and market data
    #[command(visible_alias = "d")]
    Dashboard {
        /// Filter by specific symbols (comma-separated)
        #[arg(short, long = "symbols", alias = "symbol")]
        symbols: Option<String>,
        /// Enable verbose output with more details
        #[arg(short, long)]
        verbose: bool,
        /// Watch mode: refresh every N seconds (default 5 when flag used without value)
        #[arg(short, long, num_args = 0..=1, default_missing_value = "5")]
        watch: Option<u64>,
        /// Compact mode: skip RECENT TRADES section
        #[arg(long)]
        compact: bool,
    },
    /// Portfolio - view portfolio summary and performance (alias for portfolio snapshot)
    #[command(visible_alias = "p")]
    Portfolio {
        /// Enable verbose output with more details
        #[arg(short, long)]
        verbose: bool,
        /// Watch mode: refresh every N seconds (default 5 when flag used without value)
        #[arg(short, long, num_args = 0..=1, default_missing_value = "5")]
        watch: Option<u64>,
    },
    /// Block trade operations (authenticated)
    #[command(visible_alias = "b")]
    Block {
        #[command(subcommand)]
        command: BlockCommands,
    },
    /// Market-maker bot (SIP-5A community maker yield)
    #[command(visible_alias = "mk")]
    Maker {
        #[command(subcommand)]
        command: MakerCommands,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommands {
    /// Initialize configuration
    Init,
    /// Set configuration value
    Set { key: String, value: String },
    /// Get configuration value
    Get { key: String },
    /// Show full configuration
    Show,
}

#[derive(Subcommand, Debug)]
pub enum AuthCommands {
    /// Login with JWT token (private key optional for read-only access)
    Login {
        /// JWT token (required)
        #[arg(short, long)]
        token: Option<String>,
        /// Token file path
        #[arg(short = 'f', long)]
        token_file: Option<String>,
        /// Private key (Base58) - optional, required for trading
        #[arg(short, long)]
        private_key: Option<String>,
        /// Private key file path
        #[arg(short = 'k', long)]
        key_file: Option<String>,
        /// Interactive mode
        #[arg(short, long)]
        interactive: bool,
    },
    /// Logout and clear credentials
    Logout,
    /// Show authentication status
    Status,
}

#[derive(Subcommand, Debug)]
pub enum MarketCommands {
    /// List all trading symbols
    Symbols,
    /// Get ticker for a symbol (includes funding rate)
    #[command(visible_alias = "t")]
    Ticker { symbol: String },
    /// Get all tickers
    #[command(visible_alias = "ts")]
    Tickers,
    /// Get recent trades
    #[command(visible_alias = "tr")]
    Trades {
        symbol: String,
        #[arg(short, long)]
        limit: Option<u32>,
    },
    /// Get order book depth
    #[command(visible_alias = "dep")]
    Depth {
        symbol: String,
        #[arg(short, long)]
        limit: Option<u32>,
    },
    /// Get kline data
    #[command(visible_alias = "k")]
    Kline {
        symbol: String,
        #[arg(
            short,
            long,
            help = "Resolution: 1, 5, 15, 30, 60, 240, 720, 1D, 1W, 1M"
        )]
        resolution: String,
        /// Start time (timestamp, ISO date YYYY-MM-DD, or relative like 1h, 1d, 7d)
        #[arg(short, long)]
        from: Option<String>,
        /// End time (timestamp, ISO date YYYY-MM-DD, or relative like 1h, 1d, 7d, defaults to now)
        #[arg(short, long)]
        to: Option<String>,
        /// Number of candles to fetch (alternative to from/to)
        #[arg(short, long, help = "Number of candles (alternative to from/to)")]
        limit: Option<u32>,
    },
    /// Get funding rate history
    #[command(visible_alias = "f")]
    Funding {
        symbol: String,
        #[arg(short, long, default_value = "7")]
        days: i64,
    },
}

#[derive(Subcommand, Debug)]
pub enum AccountCommands {
    /// Get account balances
    #[command(visible_alias = "b")]
    Balances,
    /// Get positions
    #[command(visible_alias = "p")]
    Positions {
        #[arg(short, long)]
        symbol: Option<String>,
    },
    /// Get open orders
    #[command(visible_alias = "o")]
    Orders {
        #[arg(short, long)]
        symbol: Option<String>,
    },
    /// Get order history
    #[command(visible_alias = "h")]
    History {
        #[arg(short, long)]
        symbol: Option<String>,
        #[arg(short, long, default_value = "50")]
        limit: u32,
    },
    /// Get position config
    #[command(visible_alias = "cfg")]
    Config { symbol: String },
}

#[derive(Subcommand, Debug)]
pub enum OrderCommands {
    /// Create a new order
    #[command(visible_alias = "c")]
    Create {
        symbol: String,
        side: String,
        order_type: String,
        // No short: `-q` is taken by the global `--quiet` flag (clap
        // debug-asserts on the collision in debug builds).
        #[arg(long)]
        qty: String,
        #[arg(short, long)]
        price: Option<String>,
        #[arg(short, long)]
        tif: Option<String>,
        #[arg(long)]
        reduce_only: bool,
        #[arg(long)]
        sl_price: Option<String>,
        #[arg(long)]
        tp_price: Option<String>,
    },
    /// Cancel an order by ID
    #[command(visible_alias = "cxl")]
    Cancel {
        symbol: String,
        #[arg(short, long)]
        order_id: String,
    },
    /// Cancel all orders for a symbol
    #[command(visible_alias = "cxa")]
    CancelAll { symbol: String },
}

#[derive(Subcommand, Debug)]
pub enum TradeCommands {
    /// Get trade history
    History {
        symbol: String,
        /// Start time (timestamp, ISO date YYYY-MM-DD, or relative like 1h, 1d, 7d)
        #[arg(short, long)]
        from: Option<String>,
        /// End time (timestamp, ISO date YYYY-MM-DD, or relative like 1h, 1d, 7d, defaults to now)
        #[arg(short, long)]
        to: Option<String>,
        /// Number of trades to fetch
        #[arg(short, long)]
        limit: Option<u32>,
    },
}

#[derive(Subcommand, Debug)]
pub enum LeverageCommands {
    /// Get leverage for a symbol
    Get { symbol: String },
    /// Set leverage for a symbol
    Set { symbol: String, leverage: String },
}

#[derive(Subcommand, Debug)]
pub enum MarginCommands {
    /// Transfer margin
    Transfer {
        symbol: String,
        amount: String,
        #[arg(short, long)]
        direction: String,
    },
    /// Get/Set margin mode
    Mode {
        symbol: String,
        #[arg(short, long)]
        set: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum StreamCommands {
    /// Stream price ticker (public channel)
    Price { symbol: String },
    /// Stream order book depth (public channel)
    Depth {
        symbol: String,
        #[arg(short, long, default_value = "10")]
        levels: usize,
    },
    /// Stream public trades (public channel)
    Trade { symbol: String },
    /// Stream candlestick/kline data (public channel)
    Kline {
        symbol: String,
        /// Interval: 3S, 1, 5, 15, 60 (minutes), 1D (day)
        #[arg(short, long, default_value = "3S")]
        interval: String,
    },
    /// Stream order updates (authenticated)
    Order,
    /// Stream position updates (authenticated)
    Position,
    /// Stream balance updates (authenticated)
    Balance,
    /// Stream fill/trade updates (authenticated)
    Fills,
}

#[derive(Subcommand, Debug)]
pub enum BlockCommands {
    /// List block trades
    List {
        /// Filter by symbol (e.g., BTC-USD)
        #[arg(short, long)]
        symbol: Option<String>,
        /// Limit number of results
        #[arg(short, long, default_value = "30")]
        limit: u32,
        /// Filter by status: completed, pending, all
        #[arg(short, long, default_value = "all")]
        status: String,
    },
    /// Watch block trades (polling mode)
    Watch {
        /// Filter by symbol
        #[arg(short, long)]
        symbol: Option<String>,
        /// Poll interval in seconds
        #[arg(short, long, default_value = "10")]
        interval: u64,
    },
}

#[derive(Subcommand, Debug)]
pub enum MakerCommands {
    /// Run the maker quoting loop (paper mode by default; --live to place orders)
    Run {
        /// Symbol to quote (e.g., BTC-USD)
        symbol: String,
        /// Maker strategy TOML file. If omitted, loads maker.toml from the
        /// StandX config directory when it exists.
        #[arg(long)]
        maker_config: Option<PathBuf>,
        /// Half-spread from mark price in basis points
        #[arg(long)]
        spread_bps: Option<f64>,
        /// Eligibility band guard in bps: never quote outside mark ± band
        #[arg(long)]
        band_bps: Option<f64>,
        /// Per-side, per-level order quantity
        #[arg(long)]
        size: Option<f64>,
        /// Number of quote levels per side
        #[arg(long)]
        levels: Option<u32>,
        /// Spacing between levels in bps (when levels > 1)
        #[arg(long)]
        level_step_bps: Option<f64>,
        /// Anti-flicker: re-quote only when mark moved more than this (bps)
        /// since the order was placed
        #[arg(long)]
        refresh_bps: Option<f64>,
        /// Loop interval in seconds
        #[arg(short, long)]
        interval: Option<u64>,
        /// Max absolute position; suppress the side that would exceed it
        #[arg(long)]
        max_position: Option<f64>,
        /// Inventory skew: at full inventory, shift the quote center this many
        /// bps away from mark to favor the reducing side (0 disables). Only
        /// takes effect in live mode; paper holds no position. Suggested
        /// starting point: roughly your --spread-bps
        #[arg(long)]
        skew_bps: Option<f64>,
        /// Actively exit inventory once |position| reaches this percent of
        /// --max-position. 0 disables; requires --inventory-exit-qty.
        #[arg(long)]
        inventory_exit_pct: Option<f64>,
        /// Maximum base quantity for one reduce-only inventory exit. 0
        /// disables; requires --inventory-exit-pct.
        #[arg(long)]
        inventory_exit_qty: Option<f64>,
        /// Sanity guard: skip the cycle (no places/cancels) when mark price
        /// and book mid diverge by more than this (bps) — the data sources
        /// disagree and acting on them would be unsafe
        #[arg(long)]
        max_divergence_bps: Option<f64>,
        /// Volatility circuit breaker: halt quoting (pull all quotes) when the
        /// mark's range over --vol-window cycles reaches this many bps; resume
        /// once it falls below half that. 0 disables the breaker
        #[arg(long)]
        vol_pause_bps: Option<f64>,
        /// Number of recent cycles the volatility breaker measures range over
        #[arg(long)]
        vol_window: Option<u32>,
        /// Risk alert: fire when mark-to-market PnL drops to -this (quote
        /// units). 0 disables
        #[arg(long)]
        alert_loss: Option<f64>,
        /// Risk alert: fire when |position| reaches this percent of
        /// --max-position. 0 disables
        #[arg(long)]
        alert_inventory_pct: Option<f64>,
        /// Risk alert: fire when two-sided uptime drops below this percent
        /// (after warmup). 0 disables
        #[arg(long)]
        alert_uptime: Option<f64>,
        /// Also POST risk alerts to this URL. stderr/JSON always get them
        /// regardless. Payload shape is set by --alert-webhook-format
        #[arg(long)]
        alert_webhook: Option<String>,
        /// Webhook payload format for the target chat platform
        #[arg(long, value_enum, default_value = "slack")]
        alert_webhook_format: AlertWebhookFormat,
        /// Disable the WebSocket market feed and poll REST every cycle
        #[arg(long)]
        no_ws: bool,
        /// Place real orders (without this flag the bot runs in paper mode:
        /// full loop, prints intended actions, no orders placed)
        #[arg(long)]
        live: bool,
        /// Maximum authenticated order-response reconnect attempts across one
        /// live maker run. Each attempt first cleans maker orders and must
        /// reconcile an empty maker book before quoting resumes. 0 disables.
        #[arg(long)]
        order_response_reconnect_attempts: Option<u32>,
        /// Base delay in seconds between order-response reconnect attempts.
        /// Later attempts use bounded exponential backoff.
        #[arg(long)]
        order_response_reconnect_backoff: Option<u64>,
        /// Supervised fault injection: close the local order-response stream
        /// after this many seconds. Hidden because it is only for live-gate
        /// validation and is limited by the maker command to 60 seconds.
        #[arg(long, hide = true)]
        controlled_disconnect_after: Option<u64>,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
    Csv,
    Quiet,
}

/// Payload shape for `--alert-webhook`, per target chat platform.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum AlertWebhookFormat {
    /// Slack incoming webhook: `{"text": "..."}`.
    #[default]
    Slack,
    /// Feishu/Lark custom bot: `{"msg_type":"text","content":{"text":"..."}}`.
    Feishu,
    /// Telegram sendMessage: `{"text":"..."}` (put the bot token and
    /// `chat_id` in the URL, e.g. `.../bot<TOKEN>/sendMessage?chat_id=<ID>`).
    Telegram,
    /// Generic: the full structured object (text + ts/symbol/kind/firing).
    Raw,
}
