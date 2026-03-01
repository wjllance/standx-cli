use clap::{Parser, Subcommand};

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
    Market {
        #[command(subcommand)]
        command: MarketCommands,
    },
    /// Account information (authenticated)
    Account {
        #[command(subcommand)]
        command: AccountCommands,
    },
    /// Order management (authenticated)
    Order {
        #[command(subcommand)]
        command: OrderCommands,
    },
    /// Trade history (authenticated)
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
    Dashboard {
        #[command(subcommand)]
        command: DashboardCommands,
    },
    /// Portfolio - view portfolio summary and performance
    Portfolio {
        #[command(subcommand)]
        command: PortfolioCommands,
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
    Ticker { symbol: String },
    /// Get all tickers
    Tickers,
    /// Get recent trades
    Trades {
        symbol: String,
        #[arg(short, long)]
        limit: Option<u32>,
    },
    /// Get order book depth
    Depth {
        symbol: String,
        #[arg(short, long)]
        limit: Option<u32>,
    },
    /// Get kline data
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
    Funding {
        symbol: String,
        #[arg(short, long, default_value = "7")]
        days: i64,
    },
}

#[derive(Subcommand, Debug)]
pub enum AccountCommands {
    /// Get account balances
    Balances,
    /// Get positions
    Positions {
        #[arg(short, long)]
        symbol: Option<String>,
    },
    /// Get open orders
    Orders {
        #[arg(short, long)]
        symbol: Option<String>,
    },
    /// Get order history
    History {
        #[arg(short, long)]
        symbol: Option<String>,
        #[arg(short, long, default_value = "50")]
        limit: u32,
    },
    /// Get position config
    Config { symbol: String },
}

#[derive(Subcommand, Debug)]
pub enum OrderCommands {
    /// Create a new order
    Create {
        symbol: String,
        side: String,
        order_type: String,
        #[arg(short, long)]
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
    Cancel {
        symbol: String,
        #[arg(short, long)]
        order_id: String,
    },
    /// Cancel all orders for a symbol
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
pub enum DashboardCommands {
    /// Get a comprehensive dashboard view
    Snapshot {
        /// Filter by specific symbols (comma-separated)
        #[arg(short, long)]
        symbols: Option<String>,
        /// Enable verbose output with more details
        #[arg(short, long)]
        verbose: bool,
        /// Watch mode: refresh every N seconds
        #[arg(short, long)]
        watch: Option<u64>,
    },
}

#[derive(Subcommand, Debug)]
pub enum PortfolioCommands {
    /// Get portfolio summary and performance
    Snapshot {
        /// Enable verbose output with more details
        #[arg(short, long)]
        verbose: bool,
        /// Watch mode: refresh every N seconds
        #[arg(short, long)]
        watch: Option<u64>,
    },
}

#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
    Csv,
    Quiet,
}
