# Phase 2 测试任务拆解

## 概述
Phase 2 专注于核心模块的单元测试：Auth、Client、Config

---

## 子任务 1: Auth 模块测试
**优先级**: ⭐⭐⭐⭐⭐
**预计时间**: 1 天
**依赖**: 无

### 测试内容
- [ ] Credentials 加密/解密测试
  - XOR 加密正确性
  - 文件保存/加载
  - 损坏文件处理
- [ ] JWT Token 解析测试
  - Token 解码
  - 过期时间计算
  - 无效 Token 处理
- [ ] Ed25519 签名测试
  - 私钥加载（Base58）
  - 签名生成
  - 签名格式验证

### 文件
- `src/auth/credentials.rs` (已有测试，需补充)
- `src/auth/mod.rs` (已有测试，需补充)

---

## 子任务 2: Client 模块测试（Mock）
**优先级**: ⭐⭐⭐⭐⭐
**预计时间**: 1-2 天
**依赖**: 无

### 测试内容
- [ ] Mock 服务器搭建
  - mockito 集成
  - 响应 fixtures
- [ ] API 请求测试
  - GET /api/query_symbol_info
  - GET /api/query_market_info
  - GET /api/query_positions (需认证)
- [ ] 错误处理测试
  - 401/403 错误
  - 500 错误
  - 网络超时
- [ ] 重试机制测试
  - 可重试错误（5xx）
  - 不可重试错误（4xx）

### 文件
- `tests/integration/client/` (新建)
- `tests/fixtures/responses/` (已有)

---

## 子任务 3: Config 模块测试
**优先级**: ⭐⭐⭐⭐
**预计时间**: 0.5 天
**依赖**: 无

### 测试内容
- [ ] 配置读写测试
  - 保存/加载配置
  - 默认值处理
- [ ] 环境变量覆盖测试
  - STANDX_JWT
  - STANDX_PRIVATE_KEY
  - STANDX_OPENCLAW_MODE
- [ ] 边界情况
  - 配置文件不存在
  - 损坏的配置文件

### 文件
- `src/config.rs` (已有测试，需补充)

---

## GitHub Issues 创建计划

### Issue #1: [Test] Auth module comprehensive tests
```
标题: [Test] Auth module comprehensive tests
标签: testing, auth, phase-2
内容:
- 补充 Credentials 加密/解密测试
- 补充 JWT Token 解析测试  
- 补充 Ed25519 签名测试

子任务:
- [ ] test_credentials_encryption_roundtrip
- [ ] test_credentials_corrupted_file
- [ ] test_jwt_expiration_calculation
- [ ] test_jwt_invalid_format
- [ ] test_ed25519_sign_request
- [ ] test_ed25519_invalid_private_key
```

### Issue #2: [Test] Client module with mock server
```
标题: [Test] Client module with mock server
标签: testing, client, mock, phase-2
内容:
- 搭建 mockito Mock 服务器
- API 请求/响应流程测试
- 错误处理和重试机制测试

子任务:
- [ ] Setup mockito mock server
- [ ] test_get_symbol_info_success
- [ ] test_get_symbol_info_404
- [ ] test_get_positions_authenticated
- [ ] test_api_error_retryable
- [ ] test_api_error_not_retryable
```

### Issue #3: [Test] Config module edge cases
```
标题: [Test] Config module edge cases
标签: testing, config, phase-2
内容:
- 配置读写边界情况
- 环境变量覆盖测试

子任务:
- [ ] test_config_save_load_roundtrip
- [ ] test_config_env_override
- [ ] test_config_missing_file
- [ ] test_config_corrupted_file
```

---

## 执行顺序

```
Week 1:
  Day 1-2: Issue #1 (Auth 模块)
  Day 3-4: Issue #2 (Client 模块)
  Day 5:   Issue #3 (Config 模块)
```

---

## 验收标准

- [ ] 每个子任务至少 80% 代码覆盖率
- [ ] 所有测试通过 CI
- [ ] 文档更新（如有新测试工具）

---

需要我创建这些 GitHub Issues 吗？
