# 04 - 账户信息

本文档介绍 StandX CLI 的账户信息查询功能，包括余额、持仓、订单等。

---

## 前置条件

需要完成认证，参考 [02-authentication.md](02-authentication.md)。

---

## 4.1 账户余额

### 命令

```bash
standx account balances
```

### 功能

查询账户各资产的余额信息。

### 输出字段

| 字段 | 说明 | 示例 |
|------|------|------|
| Asset | 资产名称 | DUSD |
| Available | 可用余额 | 10000.00 |
| Total | 总余额 | 10000.00 |

### 预期输出

```
┌─────────────┬─────────────┬─────────────┐
│ Asset       │ Available   │ Total       │
├─────────────┼─────────────┼─────────────┤
│ DUSD        │ 10000.00    │ 10000.00    │
└─────────────┴─────────────┴─────────────┘
```

### 使用示例

```bash
# 基础查询
standx account balances

# JSON 格式
standx -o json account balances

# Quiet 模式（脚本使用）
standx -o quiet account balances
```

---

## 4.2 持仓查询

### 命令

```bash
standx account positions [--symbol <SYMBOL>]
```

### 参数

| 参数 | 说明 | 必需 | 示例 |
|------|------|------|------|
| --symbol | 指定交易对 | 否 | BTC-USD |

### 输出字段

| 字段 | 说明 | 示例 |
|------|------|------|
| Symbol | 交易对 | BTC-USD |
| Side | 持仓方向 | Long / Short |
| Quantity | 持仓数量 | 0.5000 |
| Entry Price | 开仓均价 | 62000.00 |
| Mark Price | 标记价格 | 63127.37 |
| PnL | 未实现盈亏 | +563.69 |
| Leverage | 杠杆倍数 | 10 |

### 预期输出（有持仓）

```
┌─────────┬────────┬──────────┬─────────────┬────────────┬─────────┬──────────┐
│ Symbol  │ Side   │ Quantity │ Entry Price │ Mark Price │ PnL     │ Leverage │
├─────────┼────────┼──────────┼─────────────┼────────────┼─────────┼──────────┤
│ BTC-USD │ Long   │ 0.5000   │ 62000.00    │ 63127.37   │ +563.69 │ 10       │
│ ETH-USD │ Short  │ 2.0000   │ 3500.00     │ 3456.78    │ +86.44  │ 5        │
└─────────┴────────┴──────────┴─────────────┴────────────┴─────────┴──────────┘
```

### 预期输出（无持仓）

```
No positions found
```

### 使用示例

```bash
# 查询所有持仓
standx account positions

# 查询指定交易对
standx account positions --symbol BTC-USD

# JSON 格式
standx -o json account positions
```

---

## 4.3 当前订单

### 命令

```bash
standx account orders [--symbol <SYMBOL>]
```

### 参数

| 参数 | 说明 | 必需 | 示例 |
|------|------|------|------|
| --symbol | 指定交易对 | 否 | BTC-USD |

### 输出字段

| 字段 | 说明 | 示例 |
|------|------|------|
| ID | 订单ID | 123456 |
| Symbol | 交易对 | BTC-USD |
| Side | 买卖方向 | Buy / Sell |
| Type | 订单类型 | Limit / Market |
| Price | 订单价格 | 60000.00 |
| Quantity | 订单数量 | 0.1000 |
| Filled | 已成交数量 | 0.0500 |
| Status | 订单状态 | Open / PartiallyFilled |
| Time | 下单时间 | 2024-01-01 12:34:56 |

### 预期输出

```
┌────────┬─────────┬───────┬────────┬──────────┬──────────┬─────────┬────────┬─────────────────────┐
│ ID     │ Symbol  │ Side  │ Type   │ Price    │ Quantity │ Filled  │ Status │ Time                │
├────────┼─────────┼───────┼────────┼──────────┼──────────┼─────────┼────────┼─────────────────────┤
│ 123456 │ BTC-USD │ Buy   │ Limit  │ 60000.00 │ 0.1000   │ 0.0500  │ Partial│ 2024-01-01 12:34:56 │
│ 123457 │ ETH-USD │ Sell  │ Limit  │ 3600.00  │ 1.0000   │ 0.0000  │ Open   │ 2024-01-01 12:30:00 │
└────────┴─────────┴───────┴────────┴──────────┴──────────┴─────────┴────────┴─────────────────────┘
```

### 使用示例

```bash
# 查询所有未成交订单
standx account orders

# 查询指定交易对
standx account orders --symbol BTC-USD

# JSON 格式
standx -o json account orders | jq '.[] | {id: .id, symbol: .symbol, status: .status}'
```

---

## 4.4 订单历史

### 命令

```bash
standx account history [--symbol <SYMBOL>] [--limit <N>]
```

### 参数

| 参数 | 说明 | 必需 | 默认值 | 示例 |
|------|------|------|--------|------|
| --symbol | 指定交易对 | 否 | - | BTC-USD |
| --limit | 返回条数 | 否 | 50 | 10, 50, 100 |

### 输出字段

与当前订单相同，但只显示已完成的订单（Filled, Cancelled）。

### 预期输出

```
┌────────┬─────────┬───────┬────────┬──────────┬──────────┬─────────┬──────────┬─────────────────────┐
│ ID     │ Symbol  │ Side  │ Type   │ Price    │ Quantity │ Filled  │ Status   │ Time                │
├────────┼─────────┼───────┼────────┼──────────┼──────────┼─────────┼──────────┼─────────────────────┤
│ 123450 │ BTC-USD │ Buy   │ Limit  │ 61000.00 │ 0.2000   │ 0.2000  │ Filled   │ 2024-01-01 10:00:00 │
│ 123451 │ BTC-USD │ Sell  │ Market │ 63000.00 │ 0.1000   │ 0.1000  │ Filled   │ 2024-01-01 11:00:00 │
│ 123452 │ ETH-USD │ Buy   │ Limit  │ 3400.00  │ 1.0000   │ 0.0000  │ Cancelled│ 2024-01-01 09:00:00 │
└────────┴─────────┴───────┴────────┴──────────┴──────────┴─────────┴──────────┴─────────────────────┘
```

### 使用示例

```bash
# 查询最近 50 条历史
standx account history

# 查询指定交易对
standx account history --symbol BTC-USD

# 查询最近 100 条
standx account history --limit 100

# CSV 导出
standx -o csv account history > order_history.csv
```

---

## 4.5 仓位配置

### 命令

```bash
standx account config <SYMBOL>
```

### 参数

| 参数 | 说明 | 必需 | 示例 |
|------|------|------|------|
| SYMBOL | 交易对 | 是 | BTC-USD |

### 输出字段

| 字段 | 说明 | 示例 |
|------|------|------|
| Symbol | 交易对 | BTC-USD |
| Leverage | 当前杠杆 | 10 |
| Max Leverage | 最大杠杆 | 40 |
| Default Leverage | 默认杠杆 | 10 |
| Margin Mode | 保证金模式 | cross / isolated |

### 预期输出

```
┌─────────┬──────────┬─────────────┬─────────────────┬─────────────┐
│ Symbol  │ Leverage │ Max Leverage│ Default Leverage│ Margin Mode │
├─────────┼──────────┼─────────────┼─────────────────┼─────────────┤
│ BTC-USD │ 10       │ 40          │ 10              │ cross       │
└─────────┴──────────┴─────────────┴─────────────────┴─────────────┘
```

### 使用示例

```bash
# 查询 BTC-USD 配置
standx account config BTC-USD

# JSON 格式
standx -o json account config BTC-USD
```

---

## 4.6 测试检查清单

### 基础功能测试
- [ ] `account balances` 返回余额信息
- [ ] `account positions` 返回持仓信息
- [ ] `account orders` 返回当前订单
- [ ] `account history` 返回订单历史
- [ ] `account config BTC-USD` 返回配置信息

### 参数测试
- [ ] `--symbol` 过滤指定交易对
- [ ] `--limit` 限制返回条数

### 输出格式测试
- [ ] Table 格式（默认）
- [ ] JSON 格式
- [ ] CSV 格式
- [ ] Quiet 格式

### 边界情况测试
- [ ] 无持仓时显示空列表
- [ ] 无订单时显示空列表
- [ ] Token 过期时提示重新登录

---

## 下一步

- 下单交易？阅读 [05-orders.md](05-orders.md)
- 查看成交历史？阅读 [06-trading.md](06-trading.md)
- 调整杠杆？阅读 [07-leverage-margin.md](07-leverage-margin.md)

---

*文档版本: 0.3.1*  
*最后更新: 2026-02-26*
