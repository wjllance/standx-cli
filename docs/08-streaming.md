# 08 - 实时数据流

本文档介绍 StandX CLI 的 WebSocket 实时数据流功能。

---

## 前置条件

- **公共频道**: 无需认证
- **用户频道**: 需要 JWT Token，参考 [02-authentication.md](02-authentication.md)

---

## 8.1 公共频道（无需认证）

### 价格流

```bash
standx stream price <SYMBOL>
```

**预期输出：**
```
Streaming price for BTC-USD
Press Ctrl+C to exit

2024-01-01T12:34:56Z | Mark: 63127.37 | Index: 63126.67 | Last: 63115.80
2024-01-01T12:34:57Z | Mark: 63128.50 | Index: 63127.80 | Last: 63117.20
2024-01-01T12:34:58Z | Mark: 63126.00 | Index: 63125.30 | Last: 63114.50
...
```

### 深度流

```bash
standx stream depth <SYMBOL> [--levels <N>]
```

**预期输出：**
```
Streaming depth for BTC-USD (top 10 levels)
Press Ctrl+C to exit

=== Order Book: BTC-USD ===
Asks:
  63130.50: 0.5000
  63129.00: 1.2000
  ...
Bids:
  63126.50: 1.8000
  63125.00: 2.1000
  ...

=== Order Book: BTC-USD ===
Asks:
  63131.00: 0.3000
  63129.50: 1.5000
  ...
Bids:
  63127.00: 2.2000
  63126.00: 1.9000
  ...
```

### 成交流

```bash
standx stream trade <SYMBOL>
```

**预期输出：**
```
Streaming trades for BTC-USD
Press Ctrl+C to exit

2024-01-01T12:34:56Z | Buy  | 63127.50 | 0.5000
2024-01-01T12:34:57Z | Sell | 63126.00 | 1.2000
2024-01-01T12:34:58Z | Buy  | 63128.00 | 0.3000
...
```

---

## 8.2 用户频道（需要认证）

⚠️ **注意**: 当前存在 [ISSUE-5.1](https://github.com/wjllance/standx-cli/issues/3)，用户频道可能返回 `invalid token` 错误。

### 订单流

```bash
standx stream order
```

**预期输出（正常）：**
```
Streaming order updates
Press Ctrl+C to exit

Order Update:
  ID: 123456
  Symbol: BTC-USD
  Status: PartiallyFilled
  Filled: 0.0500 / 0.1000
...
```

### 持仓流

```bash
standx stream position
```

### 余额流

```bash
standx stream balance
```

### 成交流

```bash
standx stream fills
```

---

## 8.3 调试模式

使用 `-v` 参数启用调试输出：

```bash
standx -v stream price BTC-USD
```

**预期输出：**
```
[WebSocket Debug] Connecting to: wss://perps.standx.com/ws-stream/v1
[WebSocket Debug] Connected successfully
[WebSocket Debug] Skipping auth (public channel)
[WebSocket Debug] Subscribing to 1 topics
[WebSocket Debug] Sending subscribe: {"subscribe":{"channel":"price","symbol":"BTC-USD"}}
Streaming price for BTC-USD
Press Ctrl+C to exit
...
```

---

## 8.4 使用示例

### 监控多个数据流

打开多个终端窗口：

```bash
# 终端 1: 监控价格
standx stream price BTC-USD

# 终端 2: 监控深度
standx stream depth BTC-USD --levels 5

# 终端 3: 监控成交
standx stream trade BTC-USD
```

### 脚本中使用

```bash
# 获取最新价格（Quiet 模式不支持流，这里用超时方式）
timeout 5 standx stream price BTC-USD 2>/dev/null | tail -1
```

---

## 8.5 测试检查清单

### 公共频道测试
- [ ] `stream price BTC-USD` 正常输出价格
- [ ] `stream depth BTC-USD` 正常输出深度
- [ ] `stream trade BTC-USD` 正常输出成交
- [ ] `--levels` 参数控制深度层级
- [ ] `-v` 调试模式显示连接信息

### 用户频道测试
- [ ] `stream order` 需要认证
- [ ] `stream position` 需要认证
- [ ] `stream balance` 需要认证
- [ ] `stream fills` 需要认证
- [ ] 认证失败时显示错误信息

### 边界情况测试
- [ ] 无效交易对时显示错误
- [ ] 网络断开时自动重连
- [ ] Ctrl+C 正常退出

---

## 8.6 常见问题

### Q: 用户频道返回 "invalid token"？

**A:** 这是已知问题 [ISSUE-5.1](https://github.com/wjllance/standx-cli/issues/3)，正在调查中。临时的解决方法：
1. 确认 Token 未过期
2. 尝试重新登录
3. 使用公共频道替代

### Q: 连接断开怎么办？

**A:** CLI 会自动重连，无需手动操作。

### Q: 如何同时监控多个交易对？

**A:** 打开多个终端窗口，每个窗口监控一个交易对。

---

## 下一步

- 了解输出格式？阅读 [09-output-formats.md](09-output-formats.md)
- 特殊功能？阅读 [10-special-features.md](10-special-features.md)
- 故障排除？阅读 [11-troubleshooting.md](11-troubleshooting.md)

---

*文档版本: 0.3.1*  
*最后更新: 2026-02-26*
