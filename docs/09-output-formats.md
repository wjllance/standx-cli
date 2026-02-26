# 09 - 输出格式

本文档介绍 StandX CLI 支持的多种输出格式。

---

## 9.1 格式概述

| 格式 | 选项 | 适用场景 |
|------|------|----------|
| Table | `-o table` (默认) | 人类阅读，命令行交互 |
| JSON | `-o json` | 程序解析，AI Agent |
| CSV | `-o csv` | 数据导出，Excel 分析 |
| Quiet | `-o quiet` | 脚本使用，只输出值 |

---

## 9.2 Table 格式（默认）

### 特点
- 美观的表格布局
- 适合人类阅读
- 自动对齐列

### 示例

```bash
standx market ticker BTC-USD
```

**输出：**
```
┌─────────┬────────────┬────────────┬────────────┬─────────────┐
│ Symbol  │ Mark Price │ Index Price│ Last Price │ Funding Rate│
├─────────┼────────────┼────────────┼────────────┼─────────────┤
│ BTC-USD │ 63127.37   │ 63126.67   │ 63115.80   │ 0.00001250  │
└─────────┴────────────┴────────────┴────────────┴─────────────┘
```

---

## 9.3 JSON 格式 ⭐

### 特点
- 结构化数据
- 易于程序解析
- AI Agent 友好

### 示例

```bash
standx -o json market ticker BTC-USD
```

**输出：**
```json
{
  "symbol": "BTC-USD",
  "mark_price": "63127.37",
  "index_price": "63126.67",
  "last_price": "63115.80",
  "funding_rate": "0.00001250",
  "next_funding_time": "2024-01-01T08:00:00Z"
}
```

### 结合 jq 使用

```bash
# 提取特定字段
standx -o json market ticker BTC-USD | jq '.mark_price'
# 输出: "63127.37"

# 列表数据处理
standx -o json market symbols | jq '.[] | .symbol'
# 输出:
# "BTC-USD"
# "ETH-USD"
# "SOL-USD"
# "XRP-USD"

# 复杂查询
standx -o json account positions | jq '.[] | {symbol: .symbol, pnl: .pnl}'
```

---

## 9.4 CSV 格式

### 特点
- 逗号分隔值
- 适合导入 Excel
- 便于数据分析

### 示例

```bash
standx -o csv market symbols
```

**输出：**
```csv
symbol,base,quote,min_order,max_leverage
BTC-USD,BTC,DUSD,0.0001,40
ETH-USD,ETH,DUSD,0.001,40
SOL-USD,SOL,DUSD,0.01,40
XRP-USD,XRP,DUSD,1,40
```

### 导出到文件

```bash
# 导出交易对列表
standx -o csv market symbols > symbols.csv

# 导出订单历史
standx -o csv account history > orders.csv

# 导出成交记录
standx -o csv trade history BTC-USD --from 7d > trades.csv
```

---

## 9.5 Quiet 格式

### 特点
- 只输出值，无格式
- 适合脚本使用
- 便于管道传递

### 示例

```bash
# 获取单个值
standx -o quiet config get base_url
# 输出: https://perps.standx.com

# 获取杠杆值
standx -o quiet leverage get BTC-USD
# 输出: 10

# 脚本中使用
PRICE=$(standx -o quiet market ticker BTC-USD)
echo "Current BTC price: $PRICE"
```

---

## 9.6 全局选项

输出格式选项是全局的，可以放在命令的任何位置：

```bash
# 以下命令等价
standx -o json market ticker BTC-USD
standx market -o json ticker BTC-USD
standx market ticker -o json BTC-USD
```

---

## 9.7 格式选择建议

| 场景 | 推荐格式 | 原因 |
|------|----------|------|
| 日常交互 | Table | 直观易读 |
| 脚本自动化 | Quiet | 便于解析 |
| 数据分析 | CSV | Excel 友好 |
| AI Agent | JSON | 结构化数据 |
| API 集成 | JSON | 标准格式 |

---

## 9.8 测试检查清单

### 基础测试
- [ ] `-o table` 显示表格（默认）
- [ ] `-o json` 显示 JSON
- [ ] `-o csv` 显示 CSV
- [ ] `-o quiet` 只显示值

### 不同命令测试
- [ ] `market ticker` 支持所有格式
- [ ] `account balances` 支持所有格式
- [ ] `config get` 支持所有格式
- [ ] `leverage get` 支持所有格式

### 边界情况测试
- [ ] 空数据时 JSON 返回 `[]` 或 `{}`
- [ ] 空数据时 CSV 只有表头
- [ ] 空数据时 Quiet 无输出

---

## 下一步

- 特殊功能？阅读 [10-special-features.md](10-special-features.md)
- 故障排除？阅读 [11-troubleshooting.md](11-troubleshooting.md)

---

*文档版本: 0.3.1*  
*最后更新: 2026-02-26*
