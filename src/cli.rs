use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "standx")]
#[command(about = "A CLI tool for StandX perpetual DEX")]
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
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommands {
    /// Initialize configuration
    Init,
    /// Set configuration value
    Set {
        key: String,
        value: String,
    },
    /// Get configuration value
    Get {
        key: String,
    },
    /// Show full configuration
    Show,
}

#[derive(Subcommand, Debug)]
pub enum AuthCommands {
    /// Login with JWT token and private key
    Login {
        /// JWT token
        #[arg(short, long)]
        token: Option<String>,
        /// Token file path
        #[arg(short = 'f', long)]
        token_file: Option<String>,
        /// Private key (Base58)
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
    /// Get ticker for a symbol
    Ticker {
        symbol: String,
    },
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
        #[arg(short, long)]
        resolution: String,
        #[arg(short, long)]
        from: i64,
        #[arg(short, long)]
        to: i64,
    },
    /// Get funding rate
    Funding {
        symbol: String,
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
    Config {
        symbol: String,
    },
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
        #[arg(short, long)]
        client_id: Option<String>,
    },
    /// Cancel an order
    Cancel {
        symbol: String,
        #[arg(short, long)]
        order_id: Option<String>,
        #[arg(short, long)]
        client_id: Option<String>,
    },
    /// Cancel all orders for a symbol
    CancelAll {
        symbol: String,
    },
    /// List orders
    List {
        symbol: String,
        #[arg(short, long)]
        status: Option<String>,
        #[arg(short, long)]
        limit: Option<u32>,
    },
    /// Get open orders
    Open {
        symbol: String,
        #[arg(short, long)]
        limit: Option<u32>,
    },
    /// Get order details
    Get {
        symbol: String,
        #[arg(short, long)]
        order_id: Option<String>,
        #[arg(short, long)]
        client_id: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum TradeCommands {
    /// Get trade history
    History {
        symbol: String,
        #[arg(short, long)]
        from: i64,
        #[arg(short, long)]
        to: i64,
        #[arg(short, long)]
        limit: Option<u32>,
    },
}

#[derive(Subcommand, Debug)]
pub enum LeverageCommands {
    /// Get leverage for a symbol
    Get {
        symbol: String,
    },
    /// Set leverage for a symbol
    Set {
        symbol: String,
        leverage: String,
    },
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
    /// Stream trades
    Trades {
        symbol: String,
    },
    /// Stream order book
    Book {
        symbol: String,
        #[arg(short, long)]
        depth: Option<u32>,
    },
    /// Stream ticker
    Ticker {
        symbol: String,
    },
    /// Stream orders (authenticated)
    Orders,
}

#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
    Csv,
    Quiet,
}
