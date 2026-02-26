# 07 - 杠杆与保证金

本文档介绍 StandX CLI 的杠杆和保证金管理功能。

---

## 前置条件

需要完成认证并配置私钥，参考 [02-authentication.md](02-authentication.md)。

---

## 7.1 查询杠杆

### 命令

```bash
standx leverage get <SYMBOL>
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
| Margin Mode | 保证金模式 | cross |

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
# 查询 BTC-USD 杠杆
standx leverage get BTC-USD

# JSON 格式
standx -o json leverage get BTC-USD

# Quiet 模式（只输出杠杆值）
standx -o quiet leverage get BTC-USD
# 输出: 10
```

---

## 7.2 设置杠杆

### 命令

```bash
standx leverage set <SYMBOL> <LEVERAGE>
```

### 参数

| 参数 | 说明 | 必需 | 示例 |
|------|------|------|------|
| SYMBOL | 交易对 | 是 | BTC-USD |
| LEVERAGE | 杠杆倍数 | 是 | 5, 10, 20 |

### 示例

```bash
# 设置 20 倍杠杆
standx leverage set BTC-USD 20
```

**预期输出（成功）：**
```
✅ Leverage for BTC-USD set to 20x
```

**预期输出（失败）：**
```
⚠️  Leverage change failed
   Symbol: BTC-USD
   Requested leverage: 100x
   Error: Leverage exceeds maximum allowed (40x)
```

### 注意事项

- 杠杆调整会影响现有持仓的保证金要求
- 有持仓时可能无法降低杠杆
- 不同交易对可能有不同的最大杠杆限制

---

## 7.3 查询保证金模式

### 命令

```bash
standx margin mode <SYMBOL>
```

### 参数

| 参数 | 说明 | 必需 | 示例 |
|------|------|------|------|
| SYMBOL | 交易对 | 是 | BTC-USD |

### 保证金模式说明

| 模式 | 说明 |
|------|------|
| cross | 全仓模式 - 所有仓位共享保证金 |
| isolated | 逐仓模式 - 每个仓位独立保证金 |

### 预期输出

```
Margin mode for BTC-USD: cross (leverage: 10x)
```

---

## 7.4 设置保证金模式

### 命令

```bash
standx margin mode <SYMBOL> --set <MODE>
```

### 参数

| 参数 | 说明 | 必需 | 示例 |
|------|------|------|------|
| SYMBOL | 交易对 | 是 | BTC-USD |
| --set | 设置模式 | 是 | cross / isolated |

### 示例

```bash
# 设置为逐仓模式
standx margin mode BTC-USD --set isolated
```

**预期输出（成功）：**
```
✅ Margin mode for BTC-USD set to isolated
```

**预期输出（失败）：**
```
⚠️  Margin mode change failed
   Symbol: BTC-USD
   Mode: isolated
   Error: Cannot change margin mode with open positions
```

### 注意事项

- 有持仓时通常无法切换保证金模式
- 切换模式前需要先平仓
- 不同模式的风险管理方式不同

---

## 7.5 保证金划转

### 命令

```bash
standx margin transfer <SYMBOL> <AMOUNT> --direction <DIRECTION>
```

### 参数

| 参数 | 说明 | 必需 | 示例 |
|------|------|------|------|
| SYMBOL | 交易对 | 是 | BTC-USD |
| AMOUNT | 划转金额 | 是 | 1000 |
| --direction | 划转方向 | 是 | deposit / withdraw |

### 示例

```bash
# 划入保证金
standx margin transfer BTC-USD 1000 --direction deposit

# 划出保证金
standx margin transfer BTC-USD 500 --direction withdraw
```

**预期输出（成功）：**
```
✅ Margin transferred for BTC-USD: 1000 (direction: deposit)
```

**预期输出（失败）：**
```
⚠️  Margin transfer failed
   Symbol: BTC-USD
   Amount: 10000
   Direction: withdraw
   Error: Insufficient available margin
```

### 注意事项

- 划出保证金不能超过可用保证金
- 划转会影响仓位的强平价格
- 全仓模式下保证金是共享的

---

## 7.6 完整流程示例

### 场景：调整 BTC-USD 杠杆并划入保证金

```bash
# 1. 查看当前杠杆
standx leverage get BTC-USD

# 2. 查看当前持仓（如果有）
standx account positions --symbol BTC-USD

# 3. 调整杠杆（Dry Run 预览）
standx --dry-run leverage set BTC-USD 20

# 4. 执行杠杆调整
standx leverage set BTC-USD 20

# 5. 划入保证金
standx margin transfer BTC-USD 5000 --direction deposit

# 6. 验证调整结果
standx leverage get BTC-USD
standx account balances
```

---

## 7.7 测试检查清单

### 查询功能测试
- [ ] `leverage get BTC-USD` 返回杠杆信息
- [ ] `margin mode BTC-USD` 返回保证金模式

### 修改功能测试
- [ ] `leverage set BTC-USD 10` 成功设置杠杆
- [ ] `margin mode BTC-USD --set isolated` 成功切换模式
- [ ] `margin transfer BTC-USD 1000 --direction deposit` 成功划入
- [ ] `margin transfer BTC-USD 500 --direction withdraw` 成功划出

### 边界情况测试
- [ ] 设置超过最大杠杆时失败
- [ ] 有持仓时切换保证金模式失败
- [ ] 划出超过可用保证金时失败
- [ ] 无持仓时切换模式成功

### 输出格式测试
- [ ] Table 格式（默认）
- [ ] JSON 格式
- [ ] Quiet 格式

---

## 下一步

- 实时数据流？阅读 [08-streaming.md](08-streaming.md)
- 了解输出格式？阅读 [09-output-formats.md](09-output-formats.md)
- 特殊功能？阅读 [10-special-features.md](10-special-features.md)

---

*文档版本: 0.3.1*  
*最后更新: 2026-02-26*
