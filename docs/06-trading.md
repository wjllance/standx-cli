# 06 - 交易历史

本文档介绍 StandX CLI 的交易历史查询功能。

---

## 前置条件

需要完成认证，参考 [02-authentication.md](02-authentication.md)。

---

## 6.1 查询成交历史 ⭐

### 命令

```bash
standx trade history <SYMBOL> \
  [--from <TIME>] \
  [--to <TIME>] \
  [--limit <N>]
```

### 参数

| 参数 | 说明 | 必需 | 默认值 | 示例 |
|------|------|------|--------|------|
| SYMBOL | 交易对 | 是 | - | BTC-USD |
| --from | 开始时间 | 否 | 1天前 | 1d, 2024-01-01, 1704067200 |
| --to | 结束时间 | 否 | 现在 | 1h, 2024-01-07, 1706659200 |
| --limit | 返回条数 | 否 | 无限制 | 10, 50, 100 |

### 时间格式支持

| 格式 | 示例 | 说明 |
|------|------|------|
| 相对时间 | `1h`, `1d`, `7d` | 相对于现在 |
| ISO 日期 | `2024-01-01` | 年月日 |
| Unix 时间戳 | `1704067200` | 秒级时间戳 |

### 输出字段

| 字段 | 说明 | 示例 |
|------|------|------|
| ID | 成交ID | 789012 |
| Order ID | 订单ID | 123456 |
| Symbol | 交易对 | BTC-USD |
| Side | 买卖方向 | Buy / Sell |
| Price | 成交价格 | 60100.00 |
| Quantity | 成交数量 | 0.0100 |
| Fee | 手续费 | 0.60 |
| Time | 成交时间 | 2024-01-01 12:35:00 |

### 预期输出（有数据）

```
┌────────┬──────────┬─────────┬───────┬──────────┬──────────┬──────┬─────────────────────┐
│ ID     │ Order ID │ Symbol  │ Side  │ Price    │ Quantity │ Fee  │ Time                │
├────────┼──────────┼─────────┼───────┼──────────┼──────────┼──────┼─────────────────────┤
│ 789012 │ 123456   │ BTC-USD │ Buy   │ 60100.00 │ 0.0100   │ 0.60 │ 2024-01-01 12:35:00 │
│ 789013 │ 123456   │ BTC-USD │ Buy   │ 60150.00 │ 0.0100   │ 0.60 │ 2024-01-01 12:36:00 │
│ 789014 │ 123457   │ BTC-USD │ Sell  │ 63100.00 │ 0.0050   │ 0.32 │ 2024-01-01 15:00:00 │
└────────┴──────────┴─────────┴───────┴──────────┴──────────┴──────┴─────────────────────┘
```

### 预期输出（无数据）⭐

```
ℹ️  No trades found for BTC-USD in the specified time range
```

---

## 6.2 使用示例

### 查询最近 1 天的成交

```bash
standx trade history BTC-USD --from 1d
```

### 查询最近 7 天的成交

```bash
standx trade history BTC-USD --from 7d
```

### 查询指定日期范围

```bash
standx trade history BTC-USD \
  --from 2024-01-01 \
  --to 2024-01-07
```

### 查询最近 50 条成交

```bash
standx trade history BTC-USD --limit 50
```

### 使用 Unix 时间戳

```bash
standx trade history BTC-USD \
  --from 1704067200 \
  --to 1706659200
```

### JSON 格式输出

```bash
standx -o json trade history BTC-USD --from 1d | jq '.[] | {price: .price, qty: .quantity}'
```

### CSV 导出

```bash
standx -o csv trade history BTC-USD --from 7d > trades.csv
```

---

## 6.3 与订单历史的区别

| 对比项 | 订单历史 (account history) | 成交历史 (trade history) |
|--------|---------------------------|-------------------------|
| 数据内容 | 订单的创建、修改、取消 | 实际的成交记录 |
| 一条订单 | 一条记录 | 可能有多条成交 |
| 状态 | Open, Filled, Cancelled | 只有成交记录 |
| 用途 | 查看下单操作 | 查看实际盈亏 |

### 示例场景

一个限价单分 3 次成交：
- **订单历史**: 1 条记录（订单创建，状态 Filled）
- **成交历史**: 3 条记录（3 次实际成交）

---

## 6.4 测试检查清单

### 基础功能测试
- [ ] `trade history BTC-USD` 返回最近 1 天数据
- [ ] 相对时间格式：`--from 1d`, `--from 7d`
- [ ] ISO 日期格式：`--from 2024-01-01`
- [ ] Unix 时间戳：`--from 1704067200`
- [ ] limit 参数：`--limit 50`

### 边界情况测试
- [ ] 无成交记录时显示友好提示
- [ ] 时间范围过大时正常处理
- [ ] Token 过期时提示重新登录

### 输出格式测试
- [ ] Table 格式（默认）
- [ ] JSON 格式
- [ ] CSV 格式

---

## 下一步

- 调整杠杆？阅读 [07-leverage-margin.md](07-leverage-margin.md)
- 实时数据流？阅读 [08-streaming.md](08-streaming.md)
- 了解输出格式？阅读 [09-output-formats.md](09-output-formats.md)

---

*文档版本: 0.3.1*  
*最后更新: 2026-02-26*
