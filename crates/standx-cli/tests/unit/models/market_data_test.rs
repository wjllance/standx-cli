//! MarketData 和 FundingRate 模型测试

use standx_cli::models::{FundingRate, MarketData};

// ==================== MarketData 测试 ====================

#[test]
fn test_market_data_deserialization() {
    let json = r#"{
        "symbol": "BTC-USD",
        "mark_price": "63127.37",
        "index_price": "63126.67",
        "last_price": "63115.80",
        "volume_24h": "1234567.89",
        "high_price_24h": "64000.00",
        "low_price_24h": "62000.00",
        "funding_rate": "0.00001250",
        "next_funding_time": "2024-01-01T08:00:00Z"
    }"#;

    let data: MarketData = serde_json::from_str(json).unwrap();
    assert_eq!(data.symbol, "BTC-USD");
    assert_eq!(data.mark_price, "63127.37");
    assert_eq!(data.index_price, "63126.67");
    assert_eq!(data.last_price, "63115.80");
    assert_eq!(data.volume_24h, "1234567.89");
    assert_eq!(data.high_24h, "64000.00");
    assert_eq!(data.low_24h, "62000.00");
    assert_eq!(data.funding_rate, "0.00001250");
    assert_eq!(data.next_funding_time, "2024-01-01T08:00:00Z");
}

#[test]
fn test_market_data_with_numbers() {
    let json = r#"{
        "symbol": "ETH-USD",
        "mark_price": 3456.78,
        "index_price": 3456.12,
        "last_price": 3455.90,
        "volume_24h": 987654.32,
        "high_price_24h": 3500.00,
        "low_price_24h": 3400.00,
        "funding_rate": 0.00001000,
        "next_funding_time": "2024-01-01T08:00:00Z"
    }"#;

    let data: MarketData = serde_json::from_str(json).unwrap();
    assert_eq!(data.mark_price, "3456.78");
    assert_eq!(data.index_price, "3456.12");
}

// ==================== FundingRate 测试 ====================

#[test]
fn test_funding_rate_deserialization() {
    let json = r#"{
        "id": 12345,
        "symbol": "BTC-USD",
        "funding_rate": "0.00001250",
        "mark_price": "63127.37",
        "index_price": "63126.67",
        "premium": "0.00000100",
        "time": "2024-01-01T08:00:00Z",
        "created_at": "2024-01-01T07:59:59Z",
        "updated_at": "2024-01-01T08:00:00Z"
    }"#;

    let rate: FundingRate = serde_json::from_str(json).unwrap();
    assert_eq!(rate.id, 12345);
    assert_eq!(rate.symbol, "BTC-USD");
    assert_eq!(rate.funding_rate, "0.00001250");
    assert_eq!(rate.mark_price, "63127.37");
    assert_eq!(rate.index_price, "63126.67");
    assert_eq!(rate.premium, "0.00000100");
    assert_eq!(rate.time, "2024-01-01T08:00:00Z");
    assert_eq!(rate.created_at, "2024-01-01T07:59:59Z");
    assert_eq!(rate.updated_at, "2024-01-01T08:00:00Z");
}

#[test]
fn test_funding_rate_with_numbers() {
    let json = r#"{
        "id": 12346,
        "symbol": "ETH-USD",
        "funding_rate": 0.00001000,
        "mark_price": 3456.78,
        "index_price": 3456.12,
        "premium": 0.00000050,
        "time": "2024-01-01T08:00:00Z",
        "created_at": "2024-01-01T07:59:59Z",
        "updated_at": "2024-01-01T08:00:00Z"
    }"#;

    let rate: FundingRate = serde_json::from_str(json).unwrap();
    assert_eq!(rate.funding_rate, "0.00001");
    assert_eq!(rate.mark_price, "3456.78");
}

#[test]
fn test_funding_rate_list() {
    let json = r#"[
        {"id": 1, "symbol": "BTC-USD", "funding_rate": "0.00001250", "mark_price": "63127.37", "index_price": "63126.67", "premium": "0.00000100", "time": "2024-01-01T08:00:00Z", "created_at": "2024-01-01T07:59:59Z", "updated_at": "2024-01-01T08:00:00Z"},
        {"id": 2, "symbol": "ETH-USD", "funding_rate": "0.00001000", "mark_price": "3456.78", "index_price": "3456.12", "premium": "0.00000050", "time": "2024-01-01T08:00:00Z", "created_at": "2024-01-01T07:59:59Z", "updated_at": "2024-01-01T08:00:00Z"}
    ]"#;

    let rates: Vec<FundingRate> = serde_json::from_str(json).unwrap();
    assert_eq!(rates.len(), 2);
    assert_eq!(rates[0].symbol, "BTC-USD");
    assert_eq!(rates[1].symbol, "ETH-USD");
}
