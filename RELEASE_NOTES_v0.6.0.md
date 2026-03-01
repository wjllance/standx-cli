# StandX CLI v0.6.0 发布说明

**发布日期**: 2026-03-01  
**版本**: v0.6.0  
**代号**: "Dashboard & Testing"

---

## 🎯 发布亮点

v0.6.0 是一个重要的功能版本，带来了备受期待的 **Dashboard 实时交易面板** 功能，同时建立了完整的测试基础设施。现在你可以在一个界面中实时监控所有交易数据！

---

## ✨ 新功能

### Dashboard 交易面板 (#35)

全新的 `dashboard` 命令，提供实时交易数据监控：

```bash
# 启动实时面板
standx dashboard

# 只监控特定币种
standx dashboard --symbols BTC-USD,ETH-USD

# 自动刷新模式
standx dashboard --watch
```

**功能特性**:
- 📊 实时价格、持仓、订单数据一览
- 🔄 自动刷新模式 (`--watch`)
- 🎯 币种过滤 (`--symbols`)
- 🎨 表格输出带颜色编码
- ⚡ 低延迟数据更新

### Portfolio 组合视图基础设施 (#105)

为即将推出的 Portfolio 功能奠定基础：
- Portfolio snapshot 框架
- PnL 分析数据结构
- 多时间维度支持准备

### 测试框架 (#61, #62, #32)

#### Phase 3: 集成测试
- CLI 命令测试 (`assert_cmd`)
- API 流程测试 (`mockito`)
- 输出格式验证

#### Phase 4: E2E 测试
- 新用户旅程测试
- 交易员工作流测试
- CI/CD 集成支持

---

## 🔧 修复与优化

### Dashboard 优化 (#101)
- 简化 symbol filter 逻辑
- 使用 `Ordering::Relaxed` 提升性能
- 修复并发控制问题

### E2E 测试修复 (#63)
- 修复 market ticker 参数格式

---

## 📊 测试覆盖

| 测试类型 | 测试数量 | 状态 |
|----------|----------|------|
| 单元测试 | 30+ | ✅ 通过 |
| 集成测试 | 15+ | ✅ 通过 |
| E2E 测试 | 4 | ✅ 通过 |

---

## 📦 安装

### 快速安装
```bash
# macOS / Linux
curl -sSL https://raw.githubusercontent.com/wjllance/standx-cli/main/install.sh | sh

# Homebrew (macOS)
brew tap wjllance/standx-cli
brew install standx-cli
```

### 从源码构建
```bash
git clone https://github.com/wjllance/standx-cli.git
cd standx-cli
cargo build --release
```

---

## 🚀 快速开始

### 启动 Dashboard
```bash
# 基础用法
standx dashboard

# 监控特定币种
standx dashboard --symbols BTC-USD,ETH-USD,SOL-USD

# 自动刷新 (每 5 秒)
standx dashboard --watch
```

### 运行测试
```bash
# 所有测试
cargo test

# 集成测试
cargo test --test integration_tests

# E2E 测试 (需要凭证)
export TEST_TOKEN="your_jwt_token"
cargo test -- --ignored
```

---

## 📚 文档

- [快速开始](docs/01-quickstart.md)
- [Dashboard 指南](docs/05-dashboard.md) *(新增)*
- [认证指南](docs/02-authentication.md)
- [测试文档](TESTING.md)

---

## 🔮 下一步 (v0.7.0)

- **Portfolio PnL 分析** - 多时间维度盈亏分析
- **更多订单类型** - FOK/GTD/FAK/Post-only
- **交互式 Shell** - 命令补全、历史记录

---

## 🙏 贡献者

感谢所有为 v0.6.0 做出贡献的开发者！

---

**完整变更日志**: [CHANGELOG.md](CHANGELOG.md)  
**问题反馈**: https://github.com/wjllance/standx-cli/issues

*Happy Trading!* 🚀
