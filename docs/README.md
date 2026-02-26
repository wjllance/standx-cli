# StandX CLI 使用说明文档

本文档目录包含 StandX CLI 的详细使用说明，按功能模块分类，每个文档包含命令用法、参数说明、示例输出和测试方法。

---

## 文档结构

```
docs/
├── README.md                 # 文档目录说明
├── 01-getting-started.md     # 快速开始
├── 02-authentication.md      # 认证管理
├── 03-market-data.md         # 市场数据
├── 04-account.md             # 账户信息
├── 05-orders.md              # 订单管理
├── 06-trading.md             # 交易历史
├── 07-leverage-margin.md     # 杠杆与保证金
├── 08-streaming.md           # 实时数据流
├── 09-output-formats.md      # 输出格式
├── 10-special-features.md    # 特殊功能
└── 11-troubleshooting.md     # 故障排除
```

---

## 快速导航

| 文档 | 内容 | 阅读顺序 |
|------|------|----------|
| [01-getting-started.md](01-getting-started.md) | 安装、配置、第一个命令 | 1 |
| [02-authentication.md](02-authentication.md) | 登录、登出、状态检查 | 2 |
| [03-market-data.md](03-market-data.md) | 行情、深度、K线等 | 3 |
| [04-account.md](04-account.md) | 余额、持仓、订单 | 4 |
| [05-orders.md](05-orders.md) | 下单、撤单、查询 | 5 |
| [06-trading.md](06-trading.md) | 成交历史 | 6 |
| [07-leverage-margin.md](07-leverage-margin.md) | 杠杆、保证金 | 7 |
| [08-streaming.md](08-streaming.md) | WebSocket 实时数据 | 8 |
| [09-output-formats.md](09-output-formats.md) | 表格、JSON、CSV | 9 |
| [10-special-features.md](10-special-features.md) | OpenClaw、Dry Run | 10 |
| [11-troubleshooting.md](11-troubleshooting.md) | 常见问题解决 | 参考 |

---

## 测试环境要求

- **Rust**: 1.75+ (用于构建)
- **操作系统**: Linux, macOS, Windows
- **网络**: 可访问 https://perps.standx.com
- **认证** (可选): JWT Token 和 Ed25519 私钥

---

## 阅读建议

1. **新用户**: 按顺序阅读 01 → 02 → 03 → 04
2. **开发者**: 重点关注 03, 05, 08, 09, 10
3. **测试人员**: 每个文档末尾的「测试检查清单」
4. **故障排查**: 直接查阅 11-troubleshooting.md

---

*文档版本: 0.3.1*  
*最后更新: 2026-02-26*
