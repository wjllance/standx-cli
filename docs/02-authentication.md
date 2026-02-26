# 02 - 认证管理

本文档介绍 StandX CLI 的认证管理功能，包括登录、登出和状态检查。

---

## 2.1 认证概述

StandX CLI 使用两种认证方式：

1. **JWT Token** - 用于读取账户信息（必需）
2. **Ed25519 私钥** - 用于交易操作（可选，但推荐）

### 获取认证信息

访问 https://standx.com/user/session 生成：
- JWT Token（有效期 7 天）
- Ed25519 私钥（Base58 编码）

---

## 2.2 登录

### 交互式登录

```bash
standx auth login --interactive
```

**预期输出：**
```
Enter JWT Token:
Token: [输入你的 JWT token]

Enter Ed25519 Private Key (Base58) - optional, press Enter to skip:
Private Key: [输入你的私钥，或直接回车跳过]

✅ Login successful!
   Token expires at: 2024-02-02T09:56:07Z
   ⚠️  No private key provided - trading operations will be unavailable
   Run 'standx auth login' again to add a private key
```

### 命令行参数登录

```bash
standx auth login \
  --token "your_jwt_token" \
  --private-key "your_private_key"
```

### 从文件读取

```bash
# 将 token 保存到文件
echo "your_jwt_token" > ~/.standx_token
echo "your_private_key" > ~/.standx_key

# 使用文件登录
standx auth login \
  --token-file ~/.standx_token \
  --key-file ~/.standx_key
```

### 环境变量方式

```bash
export STANDX_JWT="your_jwt_token"
export STANDX_PRIVATE_KEY="your_private_key"
```

CLI 会自动读取这些环境变量。

---

## 2.3 检查认证状态

```bash
standx auth status
```

### 已认证（有效）

**预期输出：**
```
✅ Authenticated
   Token expires at: 2024-02-02T09:56:07Z
   Remaining: 167 hours
```

### 已认证（即将过期）

**预期输出：**
```
✅ Authenticated
   Token expires at: 2024-02-02T09:56:07Z
   ⚠️  Warning: Token expires in less than 24 hours!
   Remaining: 12 hours
```

### 已认证（已过期）

**预期输出：**
```
✅ Authenticated
   Token expires at: 2024-02-01T09:56:07Z
   ❌ Token has expired! Please login again.
```

### 未认证

**预期输出：**
```
❌ Not authenticated
   Run 'standx auth login' to authenticate
```

---

## 2.4 登出

```bash
standx auth logout
```

**预期输出：**
```
✅ Logged out successfully
```

---

## 2.5 权限说明

### 仅需 JWT Token 的操作

- `account balances` - 查看余额
- `account positions` - 查看持仓
- `account orders` - 查看订单
- `account history` - 查看历史
- `trade history` - 查看成交
- `leverage get` - 查询杠杆
- `margin mode` - 查询保证金模式
- `stream order/position/balance/fills` - 用户数据流

### 需要私钥的操作

- `order create/cancel` - 下单/撤单
- `leverage set` - 修改杠杆
- `margin transfer` - 保证金划转
- `margin mode --set` - 修改保证金模式

---

## 2.6 测试检查清单

### 基础认证测试
- [ ] `standx auth login --interactive` 成功登录
- [ ] `standx auth status` 显示已认证状态
- [ ] `standx auth logout` 成功登出
- [ ] `standx auth status` 显示未认证状态

### Token 方式测试
- [ ] 使用 `--token` 参数登录成功
- [ ] 使用 `--token-file` 方式登录成功
- [ ] 使用环境变量 `STANDX_JWT` 自动认证

### 私钥测试
- [ ] 仅使用 Token 登录，交易操作提示需要私钥
- [ ] 使用 Token + 私钥登录，交易操作正常
- [ ] 使用 `STANDX_PRIVATE_KEY` 环境变量

### 过期处理测试
- [ ] Token 过期后，命令提示重新登录
- [ ] 即将过期时，状态检查显示警告

---

## 2.7 常见问题

### Q: 登录后仍然提示未认证？

**A:** 检查以下几点：
1. 确认登录成功（看到 "Login successful"）
2. 检查 `standx auth status` 输出
3. 确认 Token 未过期
4. 检查是否有多个配置文件冲突

### Q: 交易操作提示需要私钥？

**A:** 重新登录并添加私钥：
```bash
standx auth login --interactive
# 或
standx auth login --token "xxx" --private-key "yyy"
```

### Q: Token 在哪里保存？

**A:** 
- Linux/macOS: `~/.config/standx/credentials`
- Windows: `%APPDATA%\standx\credentials`

### Q: 如何更新 Token？

**A:** 直接重新登录，新 Token 会覆盖旧 Token：
```bash
standx auth login --token "new_token"
```

---

## 下一步

- 查看市场数据？阅读 [03-market-data.md](03-market-data.md)
- 查看账户信息？阅读 [04-account.md](04-account.md)
- 开始交易？阅读 [05-orders.md](05-orders.md)

---

*文档版本: 0.3.1*  
*最后更新: 2026-02-26*
