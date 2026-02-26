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
    
    let pos = &positions[0];
    assert_eq!(pos.id, 80374);
    assert_eq!(pos.symbol, "BTC-USD");
    assert_eq!(pos.qty, "0.5");
    assert_eq!(pos.entry_price, "62000");
    assert_eq!(pos.entry_value, "31000");
    assert_eq!(pos.holding_margin, "1550");
    assert_eq!(pos.initial_margin, "1550");
    assert_eq!(pos.leverage, "20");
    assert_eq!(pos.mark_price, "67972.53");
    assert_eq!(pos.margin_asset, "DUSD");
    assert_eq!(pos.margin_mode, "isolated");
    assert_eq!(pos.position_value, "33986.27");
    assert_eq!(pos.realized_pnl, "0.062040");
    assert_eq!(pos.required_margin, "1699.31");
    assert_eq!(pos.status, "open");
    assert_eq!(pos.upnl, "2986.27");
    assert_eq!(pos.time, "2026-02-26T07:45:48.770053Z");
    assert_eq!(pos.created_at, "2026-02-25T14:07:08.498140Z");
    assert_eq!(pos.updated_at, "2026-02-25T17:31:29.932389Z");
    assert_eq!(pos.liq_price, Some("60000".to_string()));
    assert_eq!(pos.mmr, Some("0.05".to_string()));
    assert_eq!(pos.user, "bsc_0x7ccEA090C8BCE0038c9407c9341baF3f6c714Fe2");
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

/// 测试可选字段缺失
#[test]
fn test_position_optional_missing_fields() {
    let json = r#"[{
        "id": 1,
        "symbol": "BTC-USD",
        "qty": "0.1",
        "entry_price": "60000",
        "entry_value": "6000",
        "holding_margin": "300",
        "initial_margin": "300",
        "leverage": "20",
        "mark_price": "65000",
        "margin_asset": "DUSD",
        "margin_mode": "cross",
        "position_value": "6500",
        "realized_pnl": "0",
        "required_margin": "325",
        "status": "open",
        "upnl": "500",
        "time": "2026-02-26T07:45:48Z",
        "created_at": "2026-02-25T14:07:08Z",
        "updated_at": "2026-02-25T17:31:29Z",
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

/// 测试多持仓
#[test]
fn test_position_multiple_positions() {
    let json = r#"[
        {"id": 1, "symbol": "BTC-USD", "qty": "0.5", "entry_price": "60000", "entry_value": "30000", "holding_margin": "1500", "initial_margin": "1500", "leverage": "20", "mark_price": "65000", "margin_asset": "DUSD", "margin_mode": "isolated", "position_value": "32500", "realized_pnl": "0", "required_margin": "1625", "status": "open", "upnl": "2500", "time": "2026-02-26T07:45:48Z", "created_at": "2026-02-25T14:07:08Z", "updated_at": "2026-02-25T17:31:29Z", "user": "user1"},
        {"id": 2, "symbol": "ETH-USD", "qty": "5", "entry_price": "3000", "entry_value": "15000", "holding_margin": "750", "initial_margin": "750", "leverage": "20", "mark_price": "3500", "margin_asset": "DUSD", "margin_mode": "cross", "position_value": "17500", "realized_pnl": "0", "required_margin": "875", "status": "open", "upnl": "2500", "time": "2026-02-26T07:45:48Z", "created_at": "2026-02-25T14:07:08Z", "updated_at": "2026-02-25T17:31:29Z", "user": "user1"}
    ]"#;

    let positions: Vec<Position> = serde_json::from_str(json).unwrap();
    assert_eq!(positions.len(), 2);
    assert_eq!(positions[0].symbol, "BTC-USD");
    assert_eq!(positions[1].symbol, "ETH-USD");
}
