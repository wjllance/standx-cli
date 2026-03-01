# StandX CLI 测试指南

本文档说明如何运行 StandX CLI 的自动化测试和手动测试。

---

## 测试架构

StandX CLI 采用三层测试架构：

```
┌─────────────────────────────────────────────────────────┐
│  E2E Tests (tests/e2e/)                                │
│  - 端到端测试，模拟真实用户场景                          │
│  - 需要真实 API 凭证                                     │
│  - 手动运行                                              │
├─────────────────────────────────────────────────────────┤
│  Integration Tests (tests/integration/)                │
│  - 集成测试，测试 CLI 命令和 API 流程                    │
│  - 使用 mock 服务器                                      │
│  - CI 自动运行                                           │
├─────────────────────────────────────────────────────────┤
│  Unit Tests (src/*/tests.rs, tests/unit/)              │
│  - 单元测试，测试独立函数和模块                          │
│  - 无外部依赖                                            │
│  - CI 自动运行                                           │
└─────────────────────────────────────────────────────────┘
```

---

## 快速开始

### 运行所有测试

```bash
cargo test
```

### 运行特定测试

```bash
# 仅单元测试
cargo test --lib

# 仅集成测试
cargo test --test integration_tests

# 仅 E2E 测试 (需要凭证)
cargo test -- --ignored
```

---

## 单元测试

### 位置
- `src/config.rs` (内联测试)
- `tests/unit/models/`
- `tests/unit/utils/`

### 运行
```bash
cargo test --lib
```

### 覆盖范围
| 模块 | 测试内容 |
|------|----------|
| `config` | 配置加载、保存、环境变量覆盖 |
| `models/market_data` | MarketData/FundingRate 序列化 |
| `models/position` | Position 模型测试 |
| `models/symbol_info` | SymbolInfo 模型测试 |
| `utils/time_parser` | `parse_time_string()` 函数 |
| `utils/error` | Error 处理 |

---

## 集成测试

### 位置
- `tests/integration/`

### 运行
```bash
cargo test --test integration_tests
```

### 测试内容

#### CLI 命令测试 (`cli_commands.rs`)
```bash
cargo test test_cli_version
cargo test test_cli_help
cargo test test_cli_market_help
```

#### 市场命令测试 (`cli_market_commands.rs`)
```bash
cargo test test_market_symbols_command
cargo test test_market_ticker_command
cargo test test_market_depth_command
cargo test test_market_funding_command
```

#### 输出格式测试 (`cli_output_formats.rs`)
```bash
cargo test test_market_symbols_json_output
cargo test test_market_symbols_table_output
cargo test test_market_symbols_csv_output
cargo test test_output_format_quiet
```

#### API 流程测试 (`api_flows.rs`)
使用 `mockito` 模拟 API 服务器：
```bash
cargo test test_market_data_flow
cargo test test_api_error_handling_flow
```

---

## E2E 测试

### 位置
- `tests/e2e/`

### 前置条件
需要设置环境变量：
```bash
export TEST_TOKEN="your_jwt_token"
export TEST_PRIVATE_KEY="your_ed25519_private_key"
```

### 运行
```bash
cargo test -- --ignored
```

### 测试内容

#### 新用户旅程 (`new_user_journey.rs`)
模拟新用户从安装到首次交易的完整流程：
1. 检查 CLI 版本
2. 查看帮助信息
3. 查看市场数据 (无需认证)
4. 设置认证
5. 执行交易操作

#### 交易员工作流 (`trader_workflow.rs`)
模拟交易员日常工作：
1. 检查账户余额
2. 查看持仓
3. 市场分析 (订单簿、资金费率)
4. 执行交易

---

## 手动测试

### 构建 Release 版本

```bash
cargo build --release
./target/release/standx --version
```

### 基础命令测试

```bash
# 版本信息
./target/release/standx --version

# 帮助信息
./target/release/standx --help

# 配置管理
./target/release/standx config show
./target/release/standx config get base_url
```

### 市场数据测试 (无需认证)

```bash
# 交易对列表
./target/release/standx market symbols

# 行情数据
./target/release/standx market ticker BTC-USD

# 订单簿深度
./target/release/standx market depth BTC-USD --limit 5

# K-line 数据
./target/release/standx market kline BTC-USD -r 60 --from 1d

# 资金费率
./target/release/standx market funding BTC-USD --days 7
```

### 输出格式测试

```bash
# JSON 格式
./target/release/standx -o json market ticker BTC-USD

# CSV 格式
./target/release/standx -o csv market symbols

# Quiet 模式
./target/release/standx -o quiet config get base_url

# OpenClaw 模式
./target/release/standx --openclaw market ticker BTC-USD
```

### 认证命令测试

```bash
# 认证状态
./target/release/standx auth status

# 交互式登录
./target/release/standx auth login --interactive

# 登出
./target/release/standx auth logout
```

### 账户命令测试 (需要认证)

```bash
export STANDX_JWT="your_jwt_token"

# 账户余额
./target/release/standx account balances

# 持仓查询
./target/release/standx account positions

# 当前订单
./target/release/standx account orders

# 订单历史
./target/release/standx account history
```

### WebSocket 测试

```bash
# 价格流 (公共)
./target/release/standx stream price BTC-USD

# 深度流 (公共)
./target/release/standx stream depth BTC-USD --levels 5

# 成交流 (公共)
./target/release/standx stream trade BTC-USD

# 用户流 (需要认证)
./target/release/standx stream order
./target/release/standx stream position
./target/release/standx stream balance
./target/release/standx stream fills
```

---

## CI/CD 集成

### GitHub Actions 配置

```yaml
name: Test

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      
      - name: Install Rust
        uses: dtolnay/rust-action@stable
      
      - name: Run tests
        run: cargo test
      
      - name: Check formatting
        run: cargo fmt -- --check
      
      - name: Run clippy
        run: cargo clippy -- -D warnings
```

---

## 测试检查清单

### 发布前检查

- [ ] `cargo test` 全部通过
- [ ] `cargo fmt -- --check` 无警告
- [ ] `cargo clippy -- -D warnings` 无警告
- [ ] `cargo build --release` 成功
- [ ] 版本号正确 (`standx --version`)
- [ ] 手动测试通过

### 功能检查

- [ ] 基础命令正常（version, help, config）
- [ ] 市场数据命令正常（symbols, ticker, depth, kline, funding）
- [ ] K-line 时间格式支持（相对时间、ISO 日期、时间戳）
- [ ] 认证流程正常（auth status, login）
- [ ] 账户查询正常（balances, positions, orders）
- [ ] 订单管理正常（create, cancel）
- [ ] 交易历史正常（trade history）
- [ ] 杠杆管理正常（leverage get/set）
- [ ] 保证金管理正常（margin mode）
- [ ] WebSocket 公共流正常（price, depth, trade）
- [ ] 输出格式切换正常（table, json, csv, quiet）
- [ ] OpenClaw 模式正常
- [ ] Dry Run 模式正常

---

## 常见问题

### Q: 测试失败怎么办？

**检查步骤：**
1. 确保 Rust 版本 >= 1.75
2. 运行 `cargo clean` 清理构建缓存
3. 更新依赖 `cargo update`
4. 检查是否有未提交的代码变更

### Q: E2E 测试需要哪些凭证？

需要：
- `TEST_TOKEN`: JWT Token (从 https://standx.com/user/session 获取)
- `TEST_PRIVATE_KEY`: Ed25519 私钥 (Base58 编码)

### Q: 如何跳过 E2E 测试？

```bash
# 默认运行会跳过 E2E 测试
cargo test

# 显式跳过被忽略的测试
cargo test -- --skip ignored
```

### Q: 如何调试测试？

```bash
# 显示测试输出
cargo test -- --nocapture

# 运行特定测试并显示输出
cargo test test_cli_version -- --nocapture

# 使用 dbg! 宏调试
# 在测试代码中添加: dbg!(&variable);
```

---

## 相关文档

- [发布说明](RELEASE_NOTES_v0.5.0.md)
- [CHANGELOG](CHANGELOG.md)
- [开发计划](DEVELOPMENT_PLAN.md)

---

*文档版本: 0.5.0*  
*最后更新: 2026-03-01*
