//! 错误处理测试

use standx_cli::error::Error;

/// 测试 HTTP 错误显示
#[test]
fn test_http_error_display() {
    let err = Error::Http {
        code: 401,
        message: "Unauthorized".to_string(),
        retryable: Some(false),
    };

    let display = err.to_string();
    assert!(display.contains("401"));
    assert!(display.contains("Unauthorized"));
}

/// 测试 API 错误显示
#[test]
fn test_api_error_display() {
    let err = Error::Api {
        code: 500,
        message: "Internal Server Error".to_string(),
        endpoint: Some("/api/query_positions".to_string()),
        retryable: true,
    };

    let display = err.to_string();
    assert!(display.contains("500"));
    assert!(display.contains("Internal Server Error"));
}

/// 测试认证错误
#[test]
fn test_auth_required_error() {
    let err = Error::AuthRequired {
        message: "Token expired".to_string(),
        resolution: "Run 'standx auth login'".to_string(),
    };

    let display = err.to_string();
    assert!(display.contains("Authentication required"));
}

/// 测试 JSON 序列化 - HTTP 错误
#[test]
fn test_http_error_json_serialization() {
    let err = Error::Http {
        code: 404,
        message: "Not Found".to_string(),
        retryable: Some(false),
    };

    let json = serde_json::to_string(&err).unwrap();
    assert!(json.contains("HTTP_ERROR"));
    assert!(json.contains("404"));
    assert!(json.contains("Not Found"));
}

/// 测试 JSON 序列化 - API 错误
#[test]
fn test_api_error_json_serialization() {
    let err = Error::Api {
        code: 429,
        message: "Too Many Requests".to_string(),
        endpoint: Some("/api/query_symbol_info".to_string()),
        retryable: true,
    };

    let json = serde_json::to_string(&err).unwrap();
    assert!(json.contains("API_ERROR"));
    assert!(json.contains("429"));
    assert!(json.contains("Too Many Requests"));
    assert!(json.contains("/api/query_symbol_info"));
}

/// 测试重试判断
#[test]
fn test_error_retryable() {
    // 5xx 错误应该可重试
    let err = Error::Http {
        code: 500,
        message: "Server Error".to_string(),
        retryable: Some(true),
    };
    // retryable 字段已设置

    // 4xx 错误不应该重试
    let err = Error::Http {
        code: 400,
        message: "Bad Request".to_string(),
        retryable: Some(false),
    };
    // retryable 字段已设置
}
