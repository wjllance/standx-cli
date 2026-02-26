# StandX CLI 测试体系设计方案

## 一、测试分层架构

```
┌─────────────────────────────────────────────────────────────┐
│                    E2E 测试 (端到端)                         │
│         完整用户场景，模拟真实使用流程                         │
├─────────────────────────────────────────────────────────────┤
│                   集成测试 (Integration)                     │
│         模块间交互，API 调用，数据库操作                       │
├─────────────────────────────────────────────────────────────┤
│                    单元测试 (Unit)                           │
│         函数/方法级别，独立测试，Mock 依赖                     │
└─────────────────────────────────────────────────────────────┘
```

---

## 二、测试目录结构

```
tests/
├── unit/                           # 单元测试
│   ├── models/                     # 模型序列化测试
│   │   ├── symbol_info_test.rs
│   │   ├── market_data_test.rs
│   │   ├── position_test.rs
│   │   ├── order_test.rs
│   │   └── funding_rate_test.rs
│   ├── auth/                       # 认证模块测试
│   │   ├── credentials_test.rs
│   │   ├── signer_test.rs
│   │   └── jwt_test.rs
│   ├── client/                     # API 客户端测试
│   │   ├── market_test.rs
│   │   ├── account_test.rs
│   │   ├── order_test.rs
│   │   └── error_handling_test.rs
│   ├── utils/                      # 工具函数测试
│   │   ├── time_parser_test.rs
│   │   ├── output_formatter_test.rs
│   │   └── validation_test.rs
│   └── config/                     # 配置管理测试
│       ├── config_test.rs
│       └── env_test.rs
├── integration/                    # 集成测试
│   ├── api_flows/                  # API 流程测试
│   │   ├── market_data_flow_test.rs
│   │   ├── authentication_flow_test.rs
│   │   └── trading_flow_test.rs
│   ├── cli/                        # CLI 命令测试
│   │   ├── commands_test.rs
│   │   ├── output_formats_test.rs
│   │   └── special_modes_test.rs
│   └── websocket/                  # WebSocket 测试
│       ├── public_stream_test.rs
│       └── user_stream_test.rs
├── e2e/                            # 端到端测试
│   ├── scenarios/                  # 用户场景
│   │   ├── new_user_journey_test.rs
│   │   ├── trader_daily_workflow_test.rs
│   │   └── api_integrator_test.rs
│   └── regression/                 # 回归测试
│       ├── critical_path_test.rs
│       └── smoke_test.rs
├── fixtures/                       # 测试数据
│   ├── responses/                  # API 响应样本
│   │   ├── symbol_info.json
│   │   ├── market_data.json
│   │   ├── position.json
│   │   └── order.json
│   ├── mocks/                      # Mock 定义
│   │   ├── server.rs
│   │   └── websocket.rs
│   └── helpers/                    # 测试辅助函数
│       ├── mod.rs
│       └── assertions.rs
└── README.md                       # 测试文档
```

---

## 三、单元测试详细设计

### 3.1 模型层测试 (tests/unit/models/)

#### symbol_info_test.rs
```rust
//! SymbolInfo 模型测试

use standx_cli::models::SymbolInfo;

/// 测试正常 JSON 反序列化
#[test]
fn test_symbol_info_deserialization() {
    let json = r#"{
        "symbol": "BTC-USD",
        "base_asset": "BTC",
        "quote_asset": "DUSD",
        "base_decimals": 8,
        "price_tick_decimals": 2,
        "qty_tick_decimals": 4,
        "min_order_qty": "0.0001",
        "def_leverage": "10",
        "max_leverage": "40",
        "maker_fee": "0.0002",
        "taker_fee": "0.0005",
        "status": "active"
    }"#;
    
    let info: SymbolInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.symbol, "BTC-USD");
    assert_eq!(info.base_asset, "BTC");
    assert_eq!(info.max_leverage, "40");
}

/// 测试数字字符串兼容
#[test]
fn test_symbol_info_with_number_fields() {
    let json = r#"{
        "symbol": "ETH-USD",
        "base_asset": "ETH",
        "quote_asset": "DUSD",
        "base_decimals": 18,
        "price_tick_decimals": 2,
        "qty_tick_decimals": 3,
        "min_order_qty": 0.001,
        "def_leverage": 10,
        "max_leverage": 40,
        "maker_fee": 0.0002,
        "taker_fee": 0.0005,
        "status": "active"
    }"#;
    
    let info: SymbolInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.min_order_qty, "0.001");
    assert_eq!(info.max_leverage, "40");
}

/// 测试缺失字段处理
#[test]
fn test_symbol_info_missing_optional_fields() {
    // 测试必填字段缺失时应该失败
    let json = r#"{"symbol": "BTC-USD"}"#;
    let result: Result<SymbolInfo, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

/// 测试空字符串处理
#[test]
fn test_symbol_info_empty_strings() {
    let json = r#"{
        "symbol": "",
        "base_asset": "BTC",
        "quote_asset": "DUSD",
        "base_decimals": 8,
        "price_tick_decimals": 2,
        "qty_tick_decimals": 4,
        "min_order_qty": "0.0001",
        "def_leverage": "10",
        "max_leverage": "40",
        "maker_fee": "0.0002",
        "taker_fee": "0.0005",
        "status": ""
    }"#;
    
    let info: SymbolInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.symbol, "");
}
```

#### position_test.rs
```rust
//! Position 模型测试

use standx_cli::models::Position;

/// 测试完整 Position 反序列化
#[test]
fn test_position_full_deserialization() {
    let json = r#"[{
        "id": 80374,
        "symbol": "BTC-USD",
        "qty": "0.5",
        "entry_price": "62000",
        "entry_value": "31000",
        "holding_margin": "1550",
        "initial_margin": "1550",
        "leverage": "20",
        "mark_price": "67972.53",
        "margin_asset": "DUSD",
        "margin_mode": "isolated",
        "position_value": "33986.27",
        "realized_pnl": "0.062040",
        "required_margin": "1699.31",
        "status": "open",
        "upnl": "2986.27",
        "time": "2026-02-26T07:45:48.770053Z",
        "created_at": "2026-02-25T14:07:08.498140Z",
        "updated_at": "2026-02-25T17:31:29.932389Z",
        "liq_price": "60000",
        "mmr": "0.05",
        "user": "bsc_0x7ccEA090C8BCE0038c9407c9341baF3f6c714Fe2"
    }]"#;
    
    let positions: Vec<Position> = serde_json::from_str(json).unwrap();
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].symbol, "BTC-USD");
    assert_eq!(positions[0].qty, "0.5");
}

/// 测试可选字段为 null
#[test]
fn test_position_optional_null_fields() {
    let json = r#"[{
        "id": 1,
        "symbol": "ETH-USD",
        "qty": "0",
        "entry_price": "0",
        "entry_value": "0",
        "holding_margin": "0",
        "initial_margin": "0",
        "leverage": "20",
        "mark_price": "3456.78",
        "margin_asset": "DUSD",
        "margin_mode": "isolated",
        "position_value": "0",
        "realized_pnl": "0",
        "required_margin": "0",
        "status": "open",
        "upnl": "0",
        "time": "2026-02-26T07:45:48Z",
        "created_at": "2026-02-25T14:07:08Z",
        "updated_at": "2026-02-25T17:31:29Z",
        "liq_price": null,
        "mmr": null,
        "user": "test_user"
    }]"#;
    
    let positions: Vec<Position> = serde_json::from_str(json).unwrap();
    assert!(positions[0].liq_price.is_none());
    assert!(positions[0].mmr.is_none());
}

/// 测试空持仓列表
#[test]
fn test_position_empty_list() {
    let json = r#"[]"#;
    let positions: Vec<Position> = serde_json::from_str(json).unwrap();
    assert!(positions.is_empty());
}
```

### 3.2 工具函数测试 (tests/unit/utils/)

#### time_parser_test.rs
```rust
//! 时间解析函数测试

use standx_cli::commands::parse_time_string;

/// 测试相对时间解析
#[test]
fn test_parse_relative_time() {
    let now = chrono::Utc::now().timestamp();
    
    // 测试 1 小时前
    let result = parse_time_string("1h", false).unwrap();
    assert!(result < now);
    assert!(result > now - 7200); // 不超过 2 小时
    
    // 测试 1 天后
    let result = parse_time_string("1d", true).unwrap();
    assert!(result > now);
    assert!(result < now + 172800); // 不超过 2 天
    
    // 测试 7 天前
    let result = parse_time_string("7d", false).unwrap();
    assert!(result < now);
}

/// 测试 ISO 日期解析
#[test]
fn test_parse_iso_date() {
    let result = parse_time_string("2024-01-01", true).unwrap();
    let expected = chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp();
    assert_eq!(result, expected);
}

/// 测试 Unix 时间戳
#[test]
fn test_parse_unix_timestamp() {
    let result = parse_time_string("1704067200", true).unwrap();
    assert_eq!(result, 1704067200);
}

/// 测试无效格式
#[test]
fn test_parse_invalid_time() {
    assert!(parse_time_string("invalid", true).is_err());
    assert!(parse_time_string("", true).is_err());
    assert!(parse_time_string("abc123", true).is_err());
}

/// 测试边界值
#[test]
fn test_parse_time_edge_cases() {
    // 最小单位
    assert!(parse_time_string("1s", false).is_ok());
    
    // 最大单位
    assert!(parse_time_string("52w", false).is_ok());
    
    // 大数字
    assert!(parse_time_string("999d", false).is_ok());
}
```

#### output_formatter_test.rs
```rust
//! 输出格式化测试

use standx_cli::output::{format_table, format_json, format_csv};
use standx_cli::models::SymbolInfo;

/// 测试表格格式化
#[test]
fn test_format_table() {
    let symbols = vec![
        SymbolInfo {
            symbol: "BTC-USD".to_string(),
            base_asset: "BTC".to_string(),
            quote_asset: "DUSD".to_string(),
            base_decimals: 8,
            price_tick_decimals: 2,
            qty_tick_decimals: 4,
            min_order_qty: "0.0001".to_string(),
            def_leverage: "10".to_string(),
            max_leverage: "40".to_string(),
            maker_fee: "0.0002".to_string(),
            taker_fee: "0.0005".to_string(),
            status: "active".to_string(),
        },
    ];
    
    let output = format_table(symbols);
    assert!(output.contains("BTC-USD"));
    assert!(output.contains("Symbol"));
}

/// 测试 JSON 格式化
#[test]
fn test_format_json() {
    let symbols = vec![
        SymbolInfo {
            symbol: "BTC-USD".to_string(),
            base_asset: "BTC".to_string(),
            quote_asset: "DUSD".to_string(),
            base_decimals: 8,
            price_tick_decimals: 2,
            qty_tick_decimals: 4,
            min_order_qty: "0.0001".to_string(),
            def_leverage: "10".to_string(),
            max_leverage: "40".to_string(),
            maker_fee: "0.0002".to_string(),
            taker_fee: "0.0005".to_string(),
            status: "active".to_string(),
        },
    ];
    
    let output = format_json(&symbols).unwrap();
    assert!(output.contains("BTC-USD"));
    assert!(output.contains("["));
    assert!(output.contains("]"));
}

/// 测试 CSV 格式化
#[test]
fn test_format_csv() {
    let symbols = vec![
        SymbolInfo {
            symbol: "BTC-USD".to_string(),
            base_asset: "BTC".to_string(),
            quote_asset: "DUSD".to_string(),
            base_decimals: 8,
            price_tick_decimals: 2,
            qty_tick_decimals: 4,
            min_order_qty: "0.0001".to_string(),
            def_leverage: "10".to_string(),
            max_leverage: "40".to_string(),
            maker_fee: "0.0002".to_string(),
            taker_fee: "0.0005".to_string(),
            status: "active".to_string(),
        },
    ];
    
    let output = format_csv(&symbols);
    assert!(output.contains("BTC-USD"));
    assert!(output.contains(","));
}

/// 测试空数据格式化
#[test]
fn test_format_empty_data() {
    let symbols: Vec<SymbolInfo> = vec![];
    
    let table = format_table(symbols.clone());
    assert!(table.contains("No data") || table.is_empty());
    
    let json = format_json(&symbols).unwrap();
    assert_eq!(json, "[]");
    
    let csv = format_csv(&symbols);
    assert!(csv.contains("symbol")); // 应该有表头
}
```

---

## 四、集成测试设计

### 4.1 API 流程测试 (tests/integration/api_flows/)

#### market_data_flow_test.rs
```rust
//! 市场数据 API 流程测试

use mockito::{mock, server_url};
use standx_cli::client::StandXClient;

/// 测试获取交易对列表完整流程
#[tokio::test]
async fn test_get_symbols_flow() {
    let mock_server = mock("GET", "/api/query_symbol_info")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"[{"symbol":"BTC-USD","base_asset":"BTC","quote_asset":"DUSD","base_decimals":8,"price_tick_decimals":2,"qty_tick_decimals":4,"min_order_qty":"0.0001","def_leverage":"10","max_leverage":"40","maker_fee":"0.0002","taker_fee":"0.0005","status":"active"}]"#)
        .create();
    
    let client = StandXClient::with_base_url(&server_url());
    let symbols = client.get_symbol_info().await.unwrap();
    
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].symbol, "BTC-USD");
    mock_server.assert();
}

/// 测试行情数据获取流程
#[tokio::test]
async fn test_get_ticker_flow() {
    let mock_server = mock("GET", "/api/query_market_info")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"[{"symbol":"BTC-USD","mark_price":"63127.37","index_price":"63126.67","last_price":"63115.80","funding_rate":"0.00001250","next_funding_time":"2024-01-01T08:00:00Z"}]"#)
        .create();
    
    let client = StandXClient::with_base_url(&server_url());
    let markets = client.get_symbol_market().await.unwrap();
    
    assert!(!markets.is_empty());
    mock_server.assert();
}

/// 测试错误处理流程
#[tokio::test]
async fn test_api_error_handling() {
    let mock_server = mock("GET", "/api/query_symbol_info")
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":"Internal Server Error"}"#)
        .create();
    
    let client = StandXClient::with_base_url(&server_url());
    let result = client.get_symbol_info().await;
    
    assert!(result.is_err());
    mock_server.assert();
}
```

### 4.2 CLI 命令测试 (tests/integration/cli/)

#### commands_test.rs
```rust
//! CLI 命令集成测试

use assert_cmd::Command;
use predicates::prelude::*;

/// 测试版本命令
#[test]
fn test_cli_version() {
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.arg("--version");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("standx"));
}

/// 测试帮助命令
#[test]
fn test_cli_help() {
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Commands:"));
}

/// 测试市场数据命令（无需认证）
#[test]
fn test_market_symbols_command() {
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.args(&["market", "symbols"]);
    // 由于需要网络，这里只测试命令解析成功
    // 实际测试应该使用 mock
}

/// 测试 JSON 输出格式
#[test]
fn test_json_output_flag() {
    let mut cmd = Command::cargo_bin("standx").unwrap();
    cmd.args(&["-o", "json", "--help"]);
    cmd.assert().success();
}
```

---

## 五、E2E 测试设计

### 5.1 用户场景测试 (tests/e2e/scenarios/)

#### new_user_journey_test.rs
```rust
//! 新用户完整旅程测试

use std::process::Command;
use tempfile::TempDir;

/// 场景：新用户从安装到第一次查询
/// 
/// 步骤：
/// 1. 查看版本
/// 2. 查看帮助
/// 3. 初始化配置
/// 4. 查询公共市场数据
/// 5. 尝试查询需要认证的数据（应该失败）
#[test]
fn test_new_user_journey() {
    let temp_dir = TempDir::new().unwrap();
    
    // 步骤 1: 查看版本
    let output = Command::cargo_bin("standx")
        .unwrap()
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    
    // 步骤 2: 查看帮助
    let output = Command::cargo_bin("standx")
        .unwrap()
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Commands:"));
    
    // 步骤 3: 查询市场数据（无需认证）
    // 注意：这里需要网络连接，实际测试可能需要 mock
}
```

#### trader_daily_workflow_test.rs
```rust
//! 交易员日常工作流测试

/// 场景：交易员每日工作流程
///
/// 步骤：
/// 1. 登录认证
/// 2. 查看账户余额
/// 3. 查看持仓
/// 4. 查看市场行情
/// 5. 创建订单（Dry Run）
/// 6. 创建真实订单
/// 7. 查看订单状态
/// 8. 查看成交历史
#[tokio::test]
#[ignore] // 需要真实 API，默认跳过
async fn test_trader_daily_workflow() {
    // 这是一个完整的 E2E 测试，需要真实环境
    // 使用 #[ignore] 标记，只在特定环境运行
}
```

---

## 六、测试数据管理

### 6.1 Fixtures 结构

```
tests/fixtures/
├── responses/
│   ├── symbol_info.json          # 交易对信息响应
│   ├── market_data.json          # 行情数据响应
│   ├── position.json             # 持仓数据响应
│   ├── order.json                # 订单数据响应
│   ├── balance.json              # 余额数据响应
│   ├── funding_rate.json         # 资金费率响应
│   ├── kline.json                # K线数据响应
│   ├── error_401.json            # 认证错误响应
│   ├── error_404.json            # 不存在错误响应
│   └── error_500.json            # 服务器错误响应
├── mocks/
│   ├── mod.rs                    # Mock 服务器模块
│   ├── server.rs                 # HTTP Mock 服务器
│   └── websocket.rs              # WebSocket Mock 服务器
└── helpers/
    ├── mod.rs                    # 测试辅助模块
    ├── assertions.rs             # 自定义断言
    └── factories.rs              # 测试数据工厂
```

### 6.2 测试辅助函数示例

```rust
// tests/fixtures/helpers/factories.rs

use standx_cli::models::*;

pub struct SymbolInfoFactory;

impl SymbolInfoFactory {
    pub fn btc_usd() -> SymbolInfo {
        SymbolInfo {
            symbol: "BTC-USD".to_string(),
            base_asset: "BTC".to_string(),
            quote_asset: "DUSD".to_string(),
            base_decimals: 8,
            price_tick_decimals: 2,
            qty_tick_decimals: 4,
            min_order_qty: "0.0001".to_string(),
            def_leverage: "10".to_string(),
            max_leverage: "40".to_string(),
            maker_fee: "0.0002".to_string(),
            taker_fee: "0.0005".to_string(),
            status: "active".to_string(),
        }
    }
    
    pub fn eth_usd() -> SymbolInfo {
        SymbolInfo {
            symbol: "ETH-USD".to_string(),
            base_asset: "ETH".to_string(),
            quote_asset: "DUSD".to_string(),
            base_decimals: 18,
            price_tick_decimals: 2,
            qty_tick_decimals: 3,
            min_order_qty: "0.001".to_string(),
            def_leverage: "10".to_string(),
            max_leverage: "40".to_string(),
            maker_fee: "0.0002".to_string(),
            taker_fee: "0.0005".to_string(),
            status: "active".to_string(),
        }
    }
}
```

---

## 七、测试执行策略

### 7.1 测试分类标签

```rust
// 使用 cargo test 的过滤功能

// 单元测试 - 快速，无外部依赖
// 运行: cargo test --lib
#[cfg(test)]
mod unit_tests {
    // 这些测试在 --lib 时自动运行
}

// 集成测试 - 需要 mock 服务器
// 运行: cargo test --test integration
#[cfg(test)]
#[cfg(feature = "integration-tests")]
mod integration_tests {
    // 标记需要 mock 服务器的测试
}

// E2E 测试 - 需要真实环境
// 运行: cargo test --test e2e -- --ignored
#[test]
#[ignore]
fn test_with_real_api() {
    // 需要真实 API 的测试
}
```

### 7.2 CI/CD 集成

```yaml
# .github/workflows/test.yml
name: Test

on: [push, pull_request]

jobs:
  unit-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Run unit tests
        run: cargo test --lib

  integration-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Run integration tests
        run: cargo test --test integration

  e2e-tests:
    runs-on: ubuntu-latest
    if: github.ref == 'refs/heads/main'
    steps:
      - uses: actions/checkout@v3
      - name: Run E2E tests
        run: cargo test --test e2e -- --ignored
        env:
          STANDX_TEST_TOKEN: ${{ secrets.TEST_TOKEN }}
```

---

## 八、测试覆盖率目标

| 模块 | 目标覆盖率 | 优先级 |
|------|-----------|--------|
| models | 95% | ⭐⭐⭐⭐⭐ |
| auth | 90% | ⭐⭐⭐⭐⭐ |
| client | 85% | ⭐⭐⭐⭐ |
| commands | 80% | ⭐⭐⭐⭐ |
| output | 90% | ⭐⭐⭐⭐ |
| config | 85% | ⭐⭐⭐ |
| websocket | 70% | ⭐⭐⭐ |

---

## 九、实施计划

### Phase 1: 基础单元测试（1-2 天）
- [ ] 模型序列化测试
- [ ] 工具函数测试
- [ ] 错误处理测试

### Phase 2: 核心模块测试（2-3 天）
- [ ] 认证模块测试
- [ ] API 客户端测试
- [ ] 配置管理测试

### Phase 3: 集成测试（2-3 天）
- [ ] Mock 服务器搭建
- [ ] API 流程测试
- [ ] CLI 命令测试

### Phase 4: E2E 测试（1-2 天）
- [ ] 用户场景测试
- [ ] 回归测试套件

---

需要我开始实施 Phase 1 的单元测试吗？
