//! 时间解析函数测试

use standx_cli::commands::parse_time_string;

/// 测试相对时间解析 - 小时
#[test]
fn test_parse_relative_time_hours() {
    let now = chrono::Utc::now().timestamp();

    // 测试 1 小时前
    let result = parse_time_string("1h", false).unwrap();
    assert!(result < now);
    assert!(result >= now - 3600);
    assert!(result <= now - 3590); // 允许 10 秒误差

    // 测试 1 小时后（用于 to 参数）
    let result = parse_time_string("1h", true).unwrap();
    assert!(result > now);
    assert!(result <= now + 3600);
}

/// 测试相对时间解析 - 天
#[test]
fn test_parse_relative_time_days() {
    let now = chrono::Utc::now().timestamp();

    // 测试 1 天前
    let result = parse_time_string("1d", false).unwrap();
    assert!(result < now);
    assert!(result >= now - 86400);

    // 测试 7 天前
    let result = parse_time_string("7d", false).unwrap();
    assert!(result < now);
    assert!(result >= now - 604800);
}

/// 测试相对时间解析 - 分钟和秒
#[test]
fn test_parse_relative_time_minutes_seconds() {
    let now = chrono::Utc::now().timestamp();

    // 测试 30 分钟前
    let result = parse_time_string("30m", false).unwrap();
    assert!(result < now);
    assert!(result >= now - 1800);

    // 测试 60 秒前
    let result = parse_time_string("60s", false).unwrap();
    assert!(result < now);
    assert!(result >= now - 60);
}

/// 测试相对时间解析 - 周
#[test]
fn test_parse_relative_time_weeks() {
    let now = chrono::Utc::now().timestamp();

    let result = parse_time_string("1w", false).unwrap();
    assert!(result < now);
    assert!(result >= now - 604800);
}

/// 测试 ISO 日期解析
#[test]
fn test_parse_iso_date() {
    // 标准日期格式
    let result = parse_time_string("2024-01-01", true).unwrap();
    let expected = chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp();
    assert_eq!(result, expected);

    // 另一个日期
    let result = parse_time_string("2024-12-31", true).unwrap();
    let expected = chrono::NaiveDate::from_ymd_opt(2024, 12, 31)
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
    // 标准时间戳
    let result = parse_time_string("1704067200", true).unwrap();
    assert_eq!(result, 1704067200);

    // 0 时间戳
    let result = parse_time_string("0", true).unwrap();
    assert_eq!(result, 0);

    // 较大时间戳
    let result = parse_time_string("2000000000", true).unwrap();
    assert_eq!(result, 2000000000);
}

/// 测试无效格式
#[test]
fn test_parse_invalid_time() {
    // 完全无效
    assert!(parse_time_string("invalid", true).is_err());

    // 空字符串
    assert!(parse_time_string("", true).is_err());

    // 随机字符
    assert!(parse_time_string("abc123", true).is_err());

    // 错误格式
    assert!(parse_time_string("2024/01/01", true).is_err());

    // 无效相对时间
    assert!(parse_time_string("1x", true).is_err());
}

/// 测试边界值
#[test]
fn test_parse_time_edge_cases() {
    // 最小单位 - 1 秒
    assert!(parse_time_string("1s", false).is_ok());

    // 最大单位 - 52 周（1年）
    assert!(parse_time_string("52w", false).is_ok());

    // 大数字天数
    assert!(parse_time_string("999d", false).is_ok());

    // 大数字小时
    assert!(parse_time_string("9999h", false).is_ok());
}

/// 测试大小写不敏感
#[test]
fn test_parse_time_case_insensitive() {
    let now = chrono::Utc::now().timestamp();

    // 大写应该也支持
    let result_lower = parse_time_string("1d", false).unwrap();
    let result_upper = parse_time_string("1D", false).unwrap();
    assert_eq!(result_lower, result_upper);
}
