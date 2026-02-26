# StandX CLI 本地测试指南

本文档说明如何在本地测试 StandX CLI 的各个命令，以及预期的输出结果。

---

## 测试环境准备

### 1. 构建项目

```bash
# 克隆仓库
git clone https://github.com/wjllance/standx-cli.git
cd standx-cli

# 构建 Release 版本
cargo build --release

# 验证构建成功
./target/release/standx --version
# 预期输出: standx 0.3.1
```

### 2. 配置认证（可选）

部分命令需要认证，配置环境变量：

```bash
export STANDX_JWT="your_jwt_token"
export STANDX_PRIVATE_KEY="your_ed25519_private_key"
```

获取方式：访问 https://standx.com/user/session

---

## 命令测试流程

### Part 1: 基础命令

#### 1.1 版本信息
```bash
./target/release/standx --version
```
**预期输出：**
```
standx 0.3.1
```

#### 1.2 帮助信息
```bash
./target/release/standx --help
```
**预期输出：** 显示所有子命令列表

#### 1.3 配置管理
```bash
# 显示配置
./target/release/standx config show

# 获取单个配置项
./target/release/standx config get base_url

# JSON 格式输出
./target/release/standx -o json config get base_url

# Quiet 模式
./target/release/standx -o quiet config get base_url
```

---

### Part 2: 市场数据（公共接口，无需认证）

#### 2.1 交易对列表
```bash
./target/release/standx market symbols
```

#### 2.2 行情数据
```bash
./target/release/standx market ticker BTC-USD
```

#### 2.3 订单簿深度
```bash
./target/release/standx market depth BTC-USD --limit 5
```

#### 2.4 K-line 数据
```bash
# 使用相对时间
./target/release/standx market kline BTC-USD -r 60 --from 1d

# 使用 ISO 日期
./target/release/standx market kline BTC-USD -r 1D --from 2024-01-01

# 使用 limit
./target/release/standx market kline BTC-USD -r 60 -l 10
```

#### 2.5 资金费率
```bash
./target/release/standx market funding BTC-USD --days 7
```

---

### Part 3: 认证与账户（需要 JWT）

#### 3.1 认证状态
```bash
./target/release/standx auth status
```

#### 3.2 账户余额
```bash
./target/release/standx account balances
```

#### 3.3 持仓查询
```bash
./target/release/standx account positions
```

---

### Part 4: 订单与交易

#### 4.1 创建订单
```bash
./target/release/standx order create BTC-USD buy limit --qty 0.01 --price 60000
```

#### 4.2 交易历史
```bash
./target/release/standx trade history BTC-USD --from 1d
```

---

### Part 5: 杠杆与保证金

#### 5.1 查询杠杆
```bash
./target/release/standx leverage get BTC-USD
```

#### 5.2 设置杠杆
```bash
./target/release/standx leverage set BTC-USD 10
```

#### 5.3 查询保证金模式
```bash
./target/release/standx margin mode BTC-USD
```

---

### Part 6: WebSocket 流数据

#### 6.1 价格流（公共）
```bash
./target/release/standx stream price BTC-USD
```
**预期输出：**
```
Streaming price for BTC-USD
Press Ctrl+C to exit

2024-01-01T00:00:00Z | Mark: 63127.37 | Index: 63126.67 | Last: 63115.80
...
```

#### 6.2 深度流（公共）
```bash
./target/release/standx stream depth BTC-USD --levels 5
```

#### 6.3 成交流（公共）
```bash
./target/release/standx stream trade BTC-USD
```

---

## 输出格式说明

### Table 格式（默认）
以表格形式展示数据，适合人类阅读。

### JSON 格式
```bash
./target/release/standx -o json market ticker BTC-USD
```
适合程序解析和 AI Agent 处理。

### CSV 格式
```bash
./target/release/standx -o csv market symbols
```
适合导入 Excel 或其他工具。

### Quiet 格式
```bash
./target/release/standx -o quiet config get base_url
```
只输出值，适合脚本使用。

---

## 特殊功能

### OpenClaw 模式
```bash
./target/release/standx --openclaw market ticker BTC-USD
```
强制 JSON 输出，优化 AI Agent 使用。

### Dry Run 模式
```bash
./target/release/standx --dry-run order create BTC-USD buy limit --qty 0.01 --price 60000
```
显示将要执行的操作，但不实际执行。

---

## 测试检查清单

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

### Q: 构建失败怎么办？
A: 确保安装了 Rust 工具链：
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Q: 认证失败怎么办？
A: 检查 JWT token 是否过期，访问 https://standx.com/user/session 重新获取。

### Q: K-line 时间格式不支持？
A: 支持三种格式：
- 相对时间：`1h`, `1d`, `7d`
- ISO 日期：`2024-01-01`
- Unix 时间戳：`1704067200`

---

*文档版本: 0.3.1*  
*最后更新: 2026-02-26*
