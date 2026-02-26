# 10 - 特殊功能

本文档介绍 StandX CLI 的特殊功能，包括 OpenClaw 模式和 Dry Run 模式。

---

## 10.1 OpenClaw 模式 ⭐

### 概述

OpenClaw 模式是专为 AI Agent 优化的输出模式，强制使用 JSON 格式，便于程序解析。

### 特点

- 强制 JSON 输出（忽略 `-o` 设置）
- 结构化数据，便于 AI 处理
- 包含完整的元数据

### 使用方式

```bash
standx --openclaw <command>
```

或设置环境变量：

```bash
export STANDX_OPENCLAW_MODE=true
```

### 示例

```bash
standx --openclaw market ticker BTC-USD
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

### 与普通 JSON 的区别

| 特性 | OpenClaw 模式 | 普通 JSON 模式 |
|------|---------------|----------------|
| 强制 JSON | ✅ 总是 JSON | 需要 `-o json` |
| 元数据 | 更完整 | 标准字段 |
| AI 优化 | ✅ 是 | 否 |

---

## 10.2 Dry Run 模式 ⭐

### 概述

Dry Run 模式用于预览命令的执行效果，不实际执行操作。适合测试和验证命令。

### 特点

- 显示将要执行的操作
- 不实际调用 API
- 对财务操作显示警告

### 使用方式

```bash
standx --dry-run <command>
```

### 安全操作示例

```bash
# 市场数据（只读，安全）
standx --dry-run market ticker BTC-USD
```

**输出：**
```
🔍 DRY RUN - No actual execution
Command: market ticker BTC-USD
✅ This is a read-only operation - safe to execute
```

### 财务操作示例

```bash
# 下单（财务操作，显示警告）
standx --dry-run order create BTC-USD buy limit \
  --qty 0.01 \
  --price 60000
```

**输出：**
```
🔍 DRY RUN - No actual execution
Command: order create BTC-USD buy limit
Parameters:
  Symbol: BTC-USD
  Side: Buy
  Type: Limit
  Quantity: 0.01
  Price: 60000
⚠️  This is a financial operation - use with caution in production
```

### 支持的命令

| 命令类型 | Dry Run 支持 | 警告级别 |
|----------|-------------|----------|
| market | ✅ 显示预览 | 无（只读） |
| account | ✅ 显示预览 | 无（只读） |
| order create | ✅ 显示预览 | ⚠️ 财务警告 |
| order cancel | ✅ 显示预览 | ⚠️ 财务警告 |
| leverage set | ✅ 显示预览 | ⚠️ 财务警告 |
| margin transfer | ✅ 显示预览 | ⚠️ 财务警告 |
| stream | ❌ 不支持 | - |

---

## 10.3 组合使用

### OpenClaw + Dry Run

```bash
standx --openclaw --dry-run order create BTC-USD buy limit \
  --qty 0.01 \
  --price 60000
```

**输出：**
```json
{
  "dry_run": true,
  "command": "order create",
  "symbol": "BTC-USD",
  "side": "Buy",
  "type": "Limit",
  "quantity": "0.01",
  "price": "60000",
  "warning": "This is a financial operation"
}
```

---

## 10.4 Auto-Confirm 标志

### 概述

`--yes` 标志用于自动确认危险操作，跳过交互式提示。

### 当前状态

⚠️ **注意**: 当前 CLI 没有交互式提示，所有命令都是非交互式的。`--yes` 标志已预留，待将来添加确认提示后生效。

相关 Issue: [#4](https://github.com/wjllance/standx-cli/issues/4)

### 使用方式

```bash
# 当前：标志存在但无效果
standx --yes order create BTC-USD buy limit --qty 0.01 --price 60000

# 将来：会跳过确认提示
```

### 环境变量

```bash
export STANDX_AUTO_CONFIRM=true
```

---

## 10.5 完整示例

### AI Agent 使用场景

```bash
# 1. 获取行情（OpenClaw 模式）
standx --openclaw market ticker BTC-USD

# 2. 预览下单（Dry Run + OpenClaw）
standx --openclaw --dry-run order create BTC-USD buy limit \
  --qty 0.01 \
  --price 60000

# 3. 确认无误后执行
standx --openclaw order create BTC-USD buy limit \
  --qty 0.01 \
  --price 60000
```

---

## 10.6 测试检查清单

### OpenClaw 模式测试
- [ ] `--openclaw` 强制 JSON 输出
- [ ] 忽略 `-o table` 等格式设置
- [ ] 环境变量 `STANDX_OPENCLAW_MODE` 生效

### Dry Run 模式测试
- [ ] `--dry-run` 不实际执行
- [ ] 只读操作显示安全提示
- [ ] 财务操作显示警告
- [ ] 显示完整的参数预览

### 组合测试
- [ ] `--openclaw --dry-run` 同时生效
- [ ] JSON 格式的 Dry Run 输出

### Auto-Confirm 测试
- [ ] `--yes` 标志被接受
- [ ] 环境变量 `STANDX_AUTO_CONFIRM` 被识别

---

## 下一步

- 故障排除？阅读 [11-troubleshooting.md](11-troubleshooting.md)
- 查看所有文档？返回 [docs/README.md](README.md)

---

*文档版本: 0.3.1*  
*最后更新: 2026-02-26*
