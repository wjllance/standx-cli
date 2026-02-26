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
    assert_eq!(info.quote_asset, "DUSD");
    assert_eq!(info.base_decimals, 8);
    assert_eq!(info.price_tick_decimals, 2);
    assert_eq!(info.qty_tick_decimals, 4);
    assert_eq!(info.min_order_qty, "0.0001");
    assert_eq!(info.def_leverage, "10");
    assert_eq!(info.max_leverage, "40");
    assert_eq!(info.maker_fee, "0.0002");
    assert_eq!(info.taker_fee, "0.0005");
    assert_eq!(info.status, "active");
}

/// 测试数字字符串兼容（API 返回数字而非字符串）
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
    assert_eq!(info.symbol, "ETH-USD");
    assert_eq!(info.min_order_qty, "0.001");
    assert_eq!(info.def_leverage, "10");
    assert_eq!(info.max_leverage, "40");
    assert_eq!(info.maker_fee, "0.0002");
    assert_eq!(info.taker_fee, "0.0005");
}

/// 测试必填字段缺失时应该失败
#[test]
fn test_symbol_info_missing_required_fields() {
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
    assert_eq!(info.status, "");
}

/// 测试列表反序列化
#[test]
fn test_symbol_info_list_deserialization() {
    let json = r#"[
        {"symbol": "BTC-USD", "base_asset": "BTC", "quote_asset": "DUSD", "base_decimals": 8, "price_tick_decimals": 2, "qty_tick_decimals": 4, "min_order_qty": "0.0001", "def_leverage": "10", "max_leverage": "40", "maker_fee": "0.0002", "taker_fee": "0.0005", "status": "active"},
        {"symbol": "ETH-USD", "base_asset": "ETH", "quote_asset": "DUSD", "base_decimals": 18, "price_tick_decimals": 2, "qty_tick_decimals": 3, "min_order_qty": "0.001", "def_leverage": "10", "max_leverage": "40", "maker_fee": "0.0002", "taker_fee": "0.0005", "status": "active"}
    ]"#;

    let symbols: Vec<SymbolInfo> = serde_json::from_str(json).unwrap();
    assert_eq!(symbols.len(), 2);
    assert_eq!(symbols[0].symbol, "BTC-USD");
    assert_eq!(symbols[1].symbol, "ETH-USD");
}

/// 测试空列表
#[test]
fn test_symbol_info_empty_list() {
    let json = r#"[]"#;
    let symbols: Vec<SymbolInfo> = serde_json::from_str(json).unwrap();
    assert!(symbols.is_empty());
}
