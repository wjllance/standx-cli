# StandX CLI 测试补充计划

## 概述

基于对当前测试覆盖情况的分析，本文档列出需要补充的测试项，以完善测试体系。

**创建时间**: 2026-02-28  
**当前状态**: 61个单元测试通过，覆盖率约 70%  
**目标**: 补充 23-37 个新测试，提升至 85%+ 覆盖率

---

## 测试覆盖缺口分析

### 高优先级 - 核心模块无测试

| 模块 | 文件 | 当前测试数 | 风险 |
|------|------|-----------|------|
| Telemetry | `src/telemetry.rs` | 0 | 数据收集功能无验证 |
| Error | `src/error.rs` | 0 | 错误处理无验证 |
| WebSocket | `src/websocket.rs` | 1 | 实时数据功能测试不足 |

### 中优先级 - 测试不足

| 模块 | 文件 | 当前测试数 | 缺口 |
|------|------|-----------|------|
| Client/Account | `src/client/account.rs` | 1 | 仅1个错误测试 |
| Client/Order | `src/client/order.rs` | 1 | 仅1个成功测试 |
| CLI | `src/cli.rs` | 0 | 参数解析无测试 |

### 低优先级 - 可选补充

| 模块 | 文件 | 说明 |
|------|------|------|
| Commands | `src/commands.rs` | 时间解析已在 unit 测试覆盖 |

---

## 详细测试计划

### Phase 3: 核心模块测试补充

#### 3.1 Telemetry 模块测试

**目标**: 验证遥测数据收集功能

**测试文件**: `src/telemetry.rs` (内联测试)

**测试清单**:
- test_telemetry_disabled_via_env - 环境变量禁用遥测
- test_telemetry_enabled_by_default - 默认启用
- test_telemetry_event_creation - 事件创建
- test_telemetry_command_started_event - CommandStarted事件
- test_telemetry_command_completed_event - CommandCompleted事件
- test_telemetry_error_event - Error事件
- test_telemetry_file_write - 文件写入
- test_telemetry_session_id_persistence - 会话ID持久化

**依赖**: tempfile crate, EnvGuard

---

#### 3.2 Error 模块测试

**目标**: 验证错误类型和消息

**测试文件**: `src/error.rs` (内联测试)

**测试清单**:
- test_error_display_api - API错误显示
- test_error_display_auth_required - 认证错误显示
- test_error_display_config - 配置错误显示
- test_error_from_serde_json - JSON错误转换
- test_error_from_reqwest - HTTP错误转换
- test_result_type - Result类型别名

---

#### 3.3 WebSocket 模块测试

**目标**: 验证 WebSocket 连接和消息处理

**测试清单**:
- test_ws_state_transitions - 状态转换
- test_ws_message_parse_price - Price消息解析
- test_ws_message_parse_depth - Depth消息解析
- test_ws_message_parse_trade - Trade消息解析
- test_ws_subscription_add - 添加订阅
- test_ws_reconnect_attempts_increment - 重连计数

---

### Phase 4: 客户端模块测试补充

#### 4.1 Client/Account 测试补充

**补充测试**:
- test_get_balance_success - 余额查询成功
- test_get_positions_success - 持仓查询成功
- test_get_positions_empty - 空持仓
- test_get_orders_success - 订单列表
- test_get_order_history_success - 历史订单

#### 4.2 Client/Order 测试补充

**补充测试**:
- test_create_order_limit - 限价单
- test_create_order_market - 市价单
- test_create_order_with_stop_loss - 带止损
- test_cancel_order_success - 取消订单
- test_cancel_all_orders - 取消全部

---

## GitHub Issues 清单

### Issue #67: [Test] Telemetry module tests
- 标签: testing, telemetry, phase-3
- 内容: 补充 Telemetry 模块单元测试
- 预计: 4-6个测试，0.5天

### Issue #68: [Test] Error module tests  
- 标签: testing, error, phase-3
- 内容: 补充 Error 模块单元测试
- 预计: 3-5个测试，0.5天

### Issue #69: [Test] WebSocket module tests
- 标签: testing, websocket, phase-3
- 内容: 补充 WebSocket 模块单元测试
- 预计: 5-8个测试，1天

### Issue #70: [Test] Client/Account tests
- 标签: testing, client, phase-4
- 内容: 补充 Account API 测试
- 预计: 4-5个测试，0.5天

### Issue #71: [Test] Client/Order tests
- 标签: testing, client, phase-4
- 内容: 补充 Order API 测试
- 预计: 4-5个测试，0.5天

### Issue #72: [Test] CLI argument tests
- 标签: testing, cli, phase-4
- 内容: 补充 CLI 参数解析测试
- 预计: 3-5个测试，0.5天

---

## 执行计划

| 周 | 任务 | Issues | 预计测试数 |
|----|------|--------|-----------|
| Week 1 | Phase 3 - 核心模块 | #67, #68, #69 | 12-19 |
| Week 2 | Phase 4 - 客户端模块 | #70, #71, #72 | 11-15 |

---

*文档创建: 2026-02-28*
