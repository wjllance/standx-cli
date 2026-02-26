# 03 - 市场数据

本文档详细介绍 StandX CLI 的市场数据命令，包括行情、深度、K线、资金费率等。

---

## 3.1 交易对列表

### 命令

```bash
standx market symbols
```

### 功能

获取所有可交易的交易对信息。

### 输出字段

| 字段 | 说明 | 示例 |
|------|------|------|
| Symbol | 交易对名称 | BTC-USD |
| Base | 基础资产 | BTC |
| Quote | 计价资产 | DUSD |
| Min Order | 最小下单量 | 0.0001 |
| Max Leverage | 最大杠杆 | 40 |

### 预期输出

```
┌─────────┬───────────┬───────────┬─────────────┬─────────────┐
│ Symbol  │ Base      │ Quote     │ Min Order   │ Max Leverage│
├─────────┼───────────┼───────────┼─────────────┼─────────────┤
│ BTC-USD │ BTC       │ DUSD      │ 0.0001      │ 40          │
│ ETH-USD │ ETH       │ DUSD      │ 0.001       │ 40          │
│ SOL-USD │ SOL       │ DUSD      │ 0.01        │ 40          │
│ XRP-USD │ XRP       │ DUSD      │ 1           │ 40          │
└─────────┴───────────┴───────────┴─────────────┴─────────────┘
```

### 测试方法

```bash
# 基础测试
standx market symbols

# JSON 格式输出
standx -o json market symbols | jq '.[] | .symbol'

# CSV 格式导出
standx -o csv market symbols > symbols.csv
```

---

## 3.2 行情数据

### 命令

```bash
standx market ticker <SYMBOL>
```

### 参数

| 参数 | 说明 | 必需 | 示例 |
|------|------|------|------|
| SYMBOL | 交易对 | 是 | BTC-USD |

### 输出字段

| 字段 | 说明 | 示例 |
|------|------|------|
| Symbol | 交易对 | BTC-USD |
| Mark Price | 标记价格 | 63127.37 |
| Index Price | 指数价格 | 63126.67 |
| Last Price | 最新成交价 | 63115.80 |
| Funding Rate | 资金费率 | 0.00001250 |
| Next Funding Time | 下次结算时间 | 2024-01-01T08:00:00Z |

### 预期输出

```
┌─────────┬────────────┬────────────┬────────────┬─────────────┐
│ Symbol  │ Mark Price │ Index Price│ Last Price │ Funding Rate│
├─────────┼────────────┼────────────┼────────────┼─────────────┤
│ BTC-USD │ 63127.37   │ 63126.67   │ 63115.80   │ 0.00001250  │
└─────────┴────────────┴────────────┴────────────┴─────────────┘
```

### 测试方法

```bash
# 单个交易对
standx market ticker BTC-USD

# 多个交易对
for symbol in BTC-USD ETH-USD SOL-USD; do
  standx market ticker $symbol
done

# OpenClaw 模式（JSON 输出）
standx --openclaw market ticker BTC-USD
```

---

## 3.3 所有行情

### 命令

```bash
standx market tickers
```

### 功能

获取所有交易对的行情数据。

### 预期输出

```
┌─────────┬────────────┬────────────┬────────────┬─────────────┐
│ Symbol  │ Mark Price │ Index Price│ Last Price │ Funding Rate│
├─────────┼────────────┼────────────┼────────────┼─────────────┤
│ BTC-USD │ 63127.37   │ 63126.67   │ 63115.80   │ 0.00001250  │
│ ETH-USD │ 3456.78    │ 3456.12    │ 3455.90    │ 0.00001000  │
│ SOL-USD │ 98.76      │ 98.75      │ 98.74      │ 0.00001500  │
│ XRP-USD │ 0.56       │ 0.56       │ 0.56       │ 0.00000800  │
└─────────┴────────────┴────────────┴────────────┴─────────────┘
```

---

## 3.4 订单簿深度

### 命令

```bash
standx market depth <SYMBOL> [--limit <N>]
```

### 参数

| 参数 | 说明 | 必需 | 默认值 | 示例 |
|------|------|------|--------|------|
| SYMBOL | 交易对 | 是 | - | BTC-USD |
| --limit | 深度层级 | 否 | 10 | 5, 10, 20 |

### 预期输出

```
=== Order Book: BTC-USD ===

Asks:
  63130.50: 0.5000
  63129.00: 1.2000
  63128.50: 0.8000
  63128.00: 2.1000
  63127.50: 1.5000

Bids:
  63126.50: 1.8000
  63126.00: 2.2000
  63125.50: 0.9000
  63125.00: 1.6000
  63124.50: 3.2000
```

### 测试方法

```bash
# 默认 10 层深度
standx market depth BTC-USD

# 指定 5 层
standx market depth BTC-USD --limit 5

# Quiet 模式（获取最佳买卖价）
standx -o quiet market depth BTC-USD
```

---

## 3.5 最近成交

### 命令

```bash
standx market trades <SYMBOL> [--limit <N>]
```

### 参数

| 参数 | 说明 | 必需 | 默认值 | 示例 |
|------|------|------|--------|------|
| SYMBOL | 交易对 | 是 | - | BTC-USD |
| --limit | 记录数 | 否 | 20 | 10, 50, 100 |

### 输出字段

| 字段 | 说明 | 示例 |
|------|------|------|
| Time | 成交时间 | 2024-01-01 12:34:56 |
| Price | 成交价格 | 63127.50 |
| Quantity | 成交数量 | 0.5000 |
| Side | 买卖方向 | Buy / Sell |

### 测试方法

```bash
# 最近 20 条
standx market trades BTC-USD

# 最近 50 条
standx market trades BTC-USD --limit 50

# JSON 格式
standx -o json market trades BTC-USD | jq '.[] | {price: .price, side: .side}'
```

---

## 3.6 K-line 数据 ⭐

### 命令

```bash
standx market kline <SYMBOL> \
  --resolution <RES> \
  [--from <TIME>] \
  [--to <TIME>] \
  [--limit <N>]
```

### 参数

| 参数 | 说明 | 必需 | 示例 |
|------|------|------|------|
| SYMBOL | 交易对 | 是 | BTC-USD |
| -r, --resolution | 时间周期 | 是 | 1, 5, 15, 30, 60, 240, 720, 1D, 1W, 1M |
| -f, --from | 开始时间 | 否* | 1d, 2024-01-01, 1704067200 |
| -t, --to | 结束时间 | 否 | 1h, 2024-01-07, 1706659200 |
| -l, --limit | 返回条数 | 否* | 10, 50, 100, 500 |

*from/to 和 limit 至少提供一个

### 时间格式支持

| 格式 | 示例 | 说明 |
|------|------|------|
| 相对时间 | `1h`, `1d`, `7d` | 相对于现在 |
| ISO 日期 | `2024-01-01` | 年月日 |
| Unix 时间戳 | `1704067200` | 秒级时间戳 |

### 预期输出

```
Kline data for BTC-USD (60):
  2024-01-01 00:00:00: O:42000.00 H:42100.00 L:41900.00 C:42050.00 V:100.50
  2024-01-01 01:00:00: O:42050.00 H:42200.00 L:42000.00 C:42150.00 V:85.30
  2024-01-01 02:00:00: O:42150.00 H:42300.00 L:42100.00 C:42250.00 V:120.80
  ...
```

### 字段说明

| 字段 | 说明 |
|------|------|
| O | 开盘价 (Open) |
| H | 最高价 (High) |
| L | 最低价 (Low) |
| C | 收盘价 (Close) |
| V | 成交量 (Volume) |

### 使用示例

```bash
# 获取最近 1 天的 1 小时 K 线
standx market kline BTC-USD -r 60 --from 1d

# 获取最近 7 天的日线数据
standx market kline BTC-USD -r 1D --from 7d

# 获取指定日期范围的 15 分钟线
standx market kline BTC-USD -r 15 --from 2024-01-01 --to 2024-01-02

# 获取最近 100 条 5 分钟线
standx market kline BTC-USD -r 5 -l 100

# 使用 Unix 时间戳
standx market kline BTC-USD -r 60 --from 1704067200 --to 1706659200

# JSON 格式输出
standx -o json market kline BTC-USD -r 1D --from 7d | jq '.[] | {time: .time, close: .close}'
```

---

## 3.7 资金费率 ⭐

### 命令

```bash
standx market funding <SYMBOL> [--days <N>]
```

### 参数

| 参数 | 说明 | 必需 | 默认值 | 示例 |
|------|------|------|--------|------|
| SYMBOL | 交易对 | 是 | - | BTC-USD |
| -d, --days | 查询天数 | 否 | 7 | 1, 7, 30 |

### 预期输出（有数据）

```
┌─────────────┬─────────────┬─────────────────────┐
│ Symbol      │ Funding Rate│ Next Funding Time   │
├─────────────┼─────────────┼─────────────────────┤
│ BTC-USD     │ 0.00001250  │ 2024-01-01T08:00:00Z│
│ BTC-USD     │ 0.00001100  │ 2024-01-01T16:00:00Z│
│ BTC-USD     │ 0.00001300  │ 2024-01-02T00:00:00Z│
└─────────────┴─────────────┴─────────────────────┘
```

### 预期输出（无数据）⭐

```
ℹ️  No funding rate data available for BTC-USD in the last 7 days
   This may be because:
   - The symbol is not actively trading
   - Funding rates are only recorded at specific intervals
   - Try checking the current funding rate with: standx market ticker BTC-USD
```

### 测试方法

```bash
# 查询最近 7 天
standx market funding BTC-USD

# 查询最近 30 天
standx market funding BTC-USD --days 30

# 查看当前资金费率（通过 ticker）
standx market ticker BTC-USD
```

---

## 3.8 测试检查清单

### 基础功能测试
- [ ] `market symbols` 返回交易对列表
- [ ] `market ticker BTC-USD` 返回行情数据
- [ ] `market tickers` 返回所有交易对行情
- [ ] `market depth BTC-USD` 返回订单簿
- [ ] `market trades BTC-USD` 返回成交记录

### K-line 功能测试 ⭐
- [ ] 相对时间格式：`--from 1d`, `--from 7d`
- [ ] ISO 日期格式：`--from 2024-01-01`
- [ ] Unix 时间戳：`--from 1704067200`
- [ ] limit 参数：`-l 10`, `-l 100`
- [ ] 不同时间周期：`-r 1`, `-r 60`, `-r 1D`
- [ ] 组合使用：`--from 1d -l 10`

### 资金费率测试 ⭐
- [ ] 默认 7 天查询
- [ ] 指定天数：`--days 30`
- [ ] 空数据提示信息

### 输出格式测试
- [ ] Table 格式（默认）
- [ ] JSON 格式：`-o json`
- [ ] CSV 格式：`-o csv`
- [ ] Quiet 格式：`-o quiet`

---

## 下一步

- 查看账户信息？阅读 [04-account.md](04-account.md)
- 了解输出格式？阅读 [09-output-formats.md](09-output-formats.md)
- 实时数据流？阅读 [08-streaming.md](08-streaming.md)

---

*文档版本: 0.3.1*  
*最后更新: 2026-02-26*
