#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[tokio::test]
    async fn test_get_symbol_info() {
        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/api/query_symbol_info")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"[{"symbol":"BTC-USD","base_asset":"BTC","quote_asset":"DUSD","base_decimals":9,"price_tick_decimals":2,"qty_tick_decimals":4,"min_order_qty":"0.0001","def_leverage":"10","max_leverage":"40","maker_fee":"0.0001","taker_fee":"0.0004","status":"trading"}]"#)
            .create();

        let client = StandXClient::with_base_url(server.url()).unwrap();
        let symbols = client.get_symbol_info().await.unwrap();
        
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].symbol, "BTC-USD");
    }

    #[tokio::test]
    async fn test_get_symbol_market() {
        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/api/query_symbol_market")
            .match_query(mockito::Matcher::UrlEncoded("symbol".into(), "BTC-USD".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"symbol":"BTC-USD","mark_price":"68000.00","index_price":"68001.50","last_price":"68000.00","volume_24h":"1234567.89","high_24h":"69000.00","low_24h":"67000.00","funding_rate":"0.0001","next_funding_time":"2026-02-24T16:00:00Z"}"#)
            .create();

        let client = StandXClient::with_base_url(server.url()).unwrap();
        let market = client.get_symbol_market("BTC-USD").await.unwrap();
        
        assert_eq!(market.symbol, "BTC-USD");
        assert_eq!(market.mark_price, "68000.00");
    }

    #[tokio::test]
    async fn test_health_check() {
        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/api/health")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"ok","version":"1.0.0"}"#)
            .create();

        let client = StandXClient::with_base_url(server.url()).unwrap();
        let healthy = client.health_check().await.unwrap();
        
        assert!(healthy);
    }

    #[tokio::test]
    async fn test_api_error() {
        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/api/query_symbol_info")
            .with_status(400)
            .with_body("Invalid request")
            .create();

        let client = StandXClient::with_base_url(server.url()).unwrap();
        let result = client.get_symbol_info().await;
        
        assert!(matches!(result, Err(Error::Api { code: 400, .. })));
    }
}
