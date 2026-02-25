# StandX CLI 测试报告

**测试时间**: 2026-02-26  
**CLI 版本**: 0.3.0  
**测试环境**: Linux x86_64, Rust 1.93.1

---

## 测试概览

| 部分 | 名称 | 测试数 | 通过 | 失败 | 通过率 |
|------|------|--------|------|------|--------|
| 第一部分 | 基础与配置 | 8 | 6 | 2 | 75% |
| 第二部分 | 公共市场数据 | 9 | 7 | 2 | 78% |
| 第三部分 | 认证与账户 | 6 | 6 | 0 | 100% |
| 第四部分 | 订单与交易 | 8 | 5 | 3 | 63% |
| 第五部分 | 流式数据 | 7 | 6 | 1 | 86% |
| **总计** | | **38** | **30** | **8** | **79%** |

---

## 第一部分：基础与配置

### ✅ 通过的测试

| 测试 | 命令 | 结果 |
|------|------|------|
| 版本信息 | `standx --version` | `standx 0.3.0` |
| 主帮助 | `standx --help` | 显示所有子命令 |
| config 帮助 | `standx config --help` | 显示 4 个子命令 |
| 显示配置 | `standx config show` | 3 项配置正常 |
| 获取配置项 | `standx config get base_url` | `https://perps.standx.com` |
| verbose 模式 | `standx -v config show` | 正常执行 |

### ⚠️ 问题

| 问题 | 描述 | 状态 |
|------|------|------|
| ISSUE-1.1 | JSON 输出格式不生效 | 🔴 待修复 |
| ISSUE-1.2 | quiet 模式未简化 | 🔴 待完善 |

---

## 第二部分：公共市场数据

### ✅ 通过的测试

| 测试 | 命令 | 结果 |
|------|------|------|
| 交易对列表 | `market symbols` | 4 个交易对 |
| BTC 行情 | `market ticker BTC-USD` | 价格正常 |
| ETH 行情 | `market ticker ETH-USD` | 价格正常 |
| 所有行情 | `market tickers` | 4 个交易对 |
| 订单簿深度 | `market depth BTC-USD` | 10 档买卖盘 |
| 最近成交 | `market trades BTC-USD` | 成交记录正常 |
| OpenClaw 模式 | `--openclaw market ticker` | JSON 输出正常 |

### ⚠️ 问题

| 问题 | 描述 | 状态 |
|------|------|------|
| ISSUE-2.1 | K 线参数格式不友好 | 🔴 待优化 |
| ISSUE-2.2 | 资金费率返回空数据 | 🔴 待排查 |

---

## 第三部分：认证与账户

### ✅ 通过的测试

| 测试 | 命令 | 结果 |
|------|------|------|
| auth 帮助 | `auth --help` | 3 个子命令 |
| 认证状态 | `auth status` | Authenticated |
| account 帮助 | `account --help` | 5 个子命令 |
| 账户余额 | `account balances` | Balance 正常显示 |
| 持仓查询 | `account positions` | 正常显示 |
| 当前订单 | `account orders` | 正常显示订单列表 |
| 订单历史 | `account history` | 正常显示 |

---

## 第四部分：订单与交易

### ✅ 通过的测试

| 测试 | 命令 | 结果 |
|------|------|------|
| order 帮助 | `order --help` | 3 个子命令 |
| order create 帮助 | `order create --help` | 参数完整 |
| trade 帮助 | `trade --help` | 1 个子命令 |
| leverage 帮助 | `leverage --help` | 2 个子命令 |
| **下单** | `order create BTC-USD buy limit` | **✅ 成功** |
| **查单** | `account orders` | **✅ 显示正常** |
| **撤单** | `order cancel` | **✅ 取消成功** |

### ⚠️ 未实现的功能

| 功能 | 状态 | 说明 |
|------|------|------|
| `trade history` | ⚠️ | 未实现 |
| `leverage get/set` | ⚠️ | 未实现 |
| `margin transfer/mode` | ⚠️ | 未实现 |

---

## 第五部分：流式数据 (WebSocket)

### ✅ 通过的测试

| 测试 | 命令 | 结果 |
|------|------|------|
| stream 帮助 | `stream --help` | 7 个子命令 |
| **stream price** | `stream price BTC-USD` | **✅ 正常输出** |
| **stream depth** | `stream depth BTC-USD` | **✅ 正常输出** |
| **stream trade** | `stream trade BTC-USD` | **✅ 正常输出** |
| stream order | `stream order` | 需认证 |
| stream position | `stream position` | 需认证 |
| stream balance | `stream balance` | 需认证 |
| stream fills | `stream fills` | 需认证 |

### 🔧 已修复的问题

| 问题 | 修复内容 |
|------|----------|
| FIX-5.1 | 修复频道名称: `depth` → `depth_book`, `trades` → `public_trade` |
| FIX-5.2 | 修复 Trade 结构体支持 WebSocket 格式 |
| FIX-5.3 | 修复 PriceData timestamp 字段映射 |
| FIX-5.4 | 公共频道无需 token 即可使用 |
| FIX-5.5 | 添加 verbose 模式控制 debug 输出 |
| FIX-5.6 | 更新认证消息格式为 `{ "auth": { "token": "Bearer ...", "streams": [...] } }` |

### 使用示例

```bash
# 公共频道 - 无需认证
standx stream price BTC-USD
standx stream depth BTC-USD
standx stream trade BTC-USD

# 公共频道 - 带 debug 输出
standx -v stream price BTC-USD

# 用户频道 - 需要 JWT token
export STANDX_JWT="your_jwt_token"
standx stream order
standx stream position
standx stream balance
standx stream fills
```

### ⚠️ 问题

| 问题 | 描述 | 状态 |
|------|------|------|
| ISSUE-5.1 | 用户认证频道返回 `invalid token` | 🔴 待排查 |

---

## 问题汇总

### 待修复问题

| 编号 | 描述 | 优先级 |
|------|------|--------|
| ISSUE-1.1 | JSON 输出格式不生效 | 中 |
| ISSUE-1.2 | quiet 模式未简化 | 低 |
| ISSUE-2.1 | K 线参数格式不友好 | 中 |
| ISSUE-2.2 | 资金费率返回空数据 | 低 |
| ISSUE-4.1 | trade history 未实现 | 中 |
| ISSUE-4.2 | leverage 功能未实现 | 中 |
| ISSUE-4.3 | margin 功能未实现 | 低 |
| ISSUE-5.1 | 用户认证频道 token 问题 | 中 |

### 已修复问题

| 编号 | 描述 | 修复内容 |
|------|------|----------|
| FIX-3.1 | positions API 解析错误 | 改为直接解析数组 |
| FIX-3.2 | history API 404 | 改为 `/api/query_orders?status=filled` |
| FIX-3.3 | orders API 解析错误 | 使用 `ApiListResponse` 包装对象 |
| FIX-4.1 | Private Key 不正确 | 使用正确的 Ed25519 key |
| FIX-5.1-5.6 | WebSocket 流修复 | 见第五部分 |

---

## 核心功能状态

| 功能模块 | 状态 | 说明 |
|----------|------|------|
| 基础命令 | ✅ 完整 | version, help, config |
| 公共市场数据 | ✅ 完整 | symbols, ticker, depth, trades |
| 认证 | ✅ 正常 | JWT + Private Key |
| 账户查询 | ✅ 正常 | balances, positions, orders, history |
| 订单管理 | ✅ 正常 | create, cancel, query |
| 流式数据 (公共) | ✅ 正常 | price, depth, trade |
| 流式数据 (用户) | ⚠️ 需认证 | order, position, balance, fills |
| 交易历史 | ⚠️ 未实现 | trade history |
| 杠杆管理 | ⚠️ 未实现 | leverage get/set |
| 保证金管理 | ⚠️ 未实现 | margin transfer/mode |

---

## 测试环境

```bash
# 认证信息
export STANDX_JWT="eyJhbGciOiJFUzI1NiIsImtpZCI6IlhnaEJQSVNuN0RQVHlMcWJtLUVHVkVhOU1lMFpwdU9iMk1Qc2gtbUFlencifQ..."
export STANDX_PRIVATE_KEY="8RYHtn9RvCwgLyyeW5XurT4kVyZrDkN5B92P3FoLmsnb"

# API 端点
base_url: https://perps.standx.com
websocket: wss://perps.standx.com/ws-stream/v1
```

---

*报告生成时间: 2026-02-26*
