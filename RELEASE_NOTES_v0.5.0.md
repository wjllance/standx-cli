# StandX CLI v0.5.0 发布说明

**发布日期**: 2026-03-01  
**版本**: v0.5.0  
**代号**: "Test Foundation"

---

## 🎯 发布亮点

v0.5.0 是一个专注于**测试基础设施**的版本。我们为 StandX CLI 建立了完整的测试框架，包括集成测试和端到端测试，为未来的功能开发奠定坚实基础。

---

## ✨ 新功能

### 测试框架

#### Phase 3: 集成测试框架 (#61, #62)
- **CLI 命令测试** - 使用 `assert_cmd` 测试所有 CLI 命令
- **API 流程测试** - 使用 `mockito` 模拟 API 服务器
- **输出格式测试** - 验证 JSON、Table、CSV、Quiet 格式
- **市场数据测试** - 测试 symbols、ticker、depth、funding 命令

#### Phase 4: E2E 测试框架 (#32)
- **新用户旅程测试** - 模拟从安装到首次交易的完整流程
- **交易员工作流测试** - 模拟日常交易操作
- **自动化测试框架** - 支持 CI/CD 集成

#### 配置可测试性 (#66)
- 添加 `load_from_path` 方法支持自定义配置路径
- 环境变量覆盖测试
- 配置隔离测试

---

## 🔧 修复

### E2E 测试参数格式 (380bd8c)
修复了 E2E 测试中 market ticker 命令使用错误参数格式的问题：
- 修复前: `--symbol BTC-USD`
- 修复后: `BTC-USD` (positional arg)

---

## 📊 测试覆盖

| 测试类型 | 测试数量 | 覆盖率 |
|----------|----------|--------|
| 单元测试 | 30+ | 核心模型、工具函数 |
| 集成测试 | 15+ | CLI 命令、API 流程 |
| E2E 测试 | 4 | 用户旅程、工作流 |

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

## 🧪 运行测试

```bash
# 运行所有测试
cargo test

# 运行集成测试
cargo test --test integration_tests

# 运行 E2E 测试 (需要 API 凭证)
export TEST_TOKEN="your_jwt_token"
export TEST_PRIVATE_KEY="your_private_key"
cargo test -- --ignored
```

---

## 📚 文档

- [快速开始](docs/01-quickstart.md)
- [认证指南](docs/02-authentication.md)
- [市场数据](docs/03-market-data.md)
- [订单管理](docs/04-orders.md)
- [测试文档](TESTING.md)

---

## 🔮 下一步

### v0.6.0 预览
- **Dashboard 功能** - 组合视图命令 (PR #106)
- **Portfolio 视图** - 多时间维度 PnL 分析
- **更多订单类型** - FOK/GTD/FAK/Post-only

---

## 🙏 贡献者

感谢所有为 v0.5.0 做出贡献的开发者！

---

**完整变更日志**: [CHANGELOG.md](CHANGELOG.md)  
**问题反馈**: https://github.com/wjllance/standx-cli/issues

*Happy Trading!* 🚀
