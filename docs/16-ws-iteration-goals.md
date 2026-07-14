# Maker WebSocket 迭代目标

本文档记录 `standx maker` 的 WebSocket 迭代方向，作为后续设计、拆分任务和 live
验证的依据。它不改变当前 maker 策略、下单阈值或 live gate；所有网络 I/O 仍由
`standx-cli` 执行，确定性的策略、账本和状态迁移仍归属 `standx-maker`。

## 当前定位

maker 当前使用三类 WebSocket 能力：

| 通道 | 当前作用 | 当前边界 |
|---|---|---|
| 公共行情流（`ws-stream/v1`） | 订阅 `price` 与 `depth_book`，缓存 mark 与最佳 bid/ask；超 5 秒自动回退 REST | 主要用于行情输入和 mark 漂移早醒 |
| 认证账户流（`ws-stream/v1`） | 订阅并 typed 解析 `order`、`position`、`trade`、`balance`，驱动当前 run 的成交账本、仓位异常、余额风险刷新和断线冻结 | REST 负责启动/恢复对账、30 秒账户审计和权威余额刷新；启用账户 floor 时，WS balance 会让下一轮立即刷新余额 |
| 订单回报流（`ws-api/v1`） | 正常 live maker 直接发送签名 `order:new`/`order:cancel`，并按 `request_id` 关联接受/拒绝回报 | REST 继续负责快照、清理和恢复；新执行通道仍处于重新锁定的 live gate，缺少完整生产 canary 证据 |

WS 现在是账户投影、成交账本和正常 live 下单/撤单的主要事件通道，也是 maker 的安全
依赖；它不是唯一权威源。REST 快照必须保留为启动、恢复、周期性审计、余额风险计算和
fail-safe 对账的权威补偿路径。

## 迭代原则

- 先修复安全状态机与连接健康，再减少 REST 轮询或追求延迟。
- 任何 stream 丢失、序号异常、无法解释的仓位差异或未确认的 maker 订单，都必须
  冻结新 placement、失效当前 generation、清空队列并进行 maker 自有订单清理。
- 被取消的 HTTP/WS 请求仍可能已经到达交易所；清理与 REST 对账是必需的补偿动作。
- WS 与 REST 成交可以任意顺序到达；必须按照稳定 `trade_id` 与订单累计成交量精确
  去重，不把历史或非当前 run 的 activity 纳入 session ledger。
- 不在这些迭代中顺带修改报价公式、风险阈值、PnL 语义、库存退出行为或 live 默认值。

## P0：先完成安全闭环

### 1. 让 CLI 真正执行 reducer effect

`standx-maker` 已能为关键事件返回 `AbortInFlight`、`Cleanup`、`Reconnect`、`Stop`
等 effect。CLI 应成为唯一的 effect executor，而不是只读取 state 后在多个手写分支
中重复恢复逻辑。

目标：

- 账户流断开/错误、订单回报流失效、仓位不一致发生时，立即冻结 placement。
- 递增 generation，取消可取消的 in-flight 工作，并拒绝其后续结果。
- 清空待执行动作，撤销 maker 自有订单，再执行受限重连与 REST 对账。
- 不能因为中断请求而假定订单未到达交易所；清理必须始终执行。

验收：关键 WS 事件在多订单 cycle 的中途到达时，本轮后续 placement 不得继续提交；
stale generation 的结果不能恢复报价。

### 2. 加固订单回报流

订单回报流需要达到与账户流相同等级的可观测健康性：

- 主动 heartbeat、空闲超时和 24 小时上限前的主动轮换。
- 严格处理 malformed、意外 request ID 和流结束；不能静默丢弃会影响安全判断的消息。
- 为 create、cancel、库存退出维护统一、上限明确的 request registry。
- 保留每个 cancel 的 `request_id`，用 WS 回报确认其终态；REST 空簿校验继续作为最终
  确认与补偿措施。

验收：半开连接、close/error、轮换、撤单拒绝、晚到回报和未知回报都有 loopback
测试；失去关联或健康状态时 fail closed。

## P1：事件驱动化，但保留 REST 对账

### 3. 账户流成为 typed primary feed

- 将 `trade` 从原始 shadow 日志升级为带 `trade_id`、`order_id`、成交价格和数量的
  typed domain event。
- 订阅并解析 `balance`，让 equity/available-margin 风险告警及时响应。
- 用账户流维护 maker 自有订单、仓位和余额的内存视图；订单、position 与 trade 都走
  同一条当前 run ledger ingestion path。
- REST 由每轮五路完整快照转为：启动 bootstrap、固定周期 checkpoint、重连 backfill、
  mismatch/fail-safe reconciliation。

验收：WS-first、REST-first、重复、部分成交、重连重放都只影响账本、预期仓位和
session PnL 一次；无法解释的差异仍在限定窗口内恢复或停止。

进展：`order`、`position`、`trade`、`balance` 已进入 typed account projection；完整账户
审计与余额刷新已降为 30 秒 checkpoint。启用 equity/available-margin floor 时，任一 WS
balance 更新会请求下一轮立即取得权威 REST balance 并重新计算告警。余额仍未作为
交易所可直接提供 floor 指标使用，因此风险判断保留 REST 权威计算和 60 秒 stale 上限。

### 4. 行情流提供可验证的一致快照

- 传递并检查交易所 `seq`、服务端时间与本地接收时间。
- 明确 mark 与 touch 的一致快照策略；任一边过期、回退或盘口交叉时切回 REST 或 skip。
- 除 mark 漂移外，best bid/ask 改变导致 no-cross、背离守卫或目标报价失效时，也应在
  一秒最小间隔之后提前 replan。
- 继续支持未排序的 depth，按价格确定 best bid/ask。

验收：覆盖 stale mark/book、seq regression、crossed book、未排序 depth 与只改变 touch
的行情更新；不调整策略阈值与公式。

## P2：延迟与扩展能力

### 5. 评估直接使用 WS API 下单

StandX 的 `ws-api/v1` 支持 `order:new` 与 `order:cancel`。当前 HTTP 请求加 WS 异步回报
已被签名 WS 命令替代为正常 live maker 执行路径；SDK loopback 已覆盖签名 envelope、
request/session 关联和 create/cancel 回报，CLI 保留 REST 清理、恢复与最终空簿校验。

该传输变更重新锁定 live gate。2026-07-14 的受控生产记录证明了 WS 命令提交和故障后
清理，但没有保留关联的 venue acceptance 与正常 cancel 完整证据。隐藏 canary 已加固为
输出 create/cancel request ID、venue order ID、REST 可见/消失和最终零仓位的结构化证据；
在新的明确授权与监督执行完成前，不能把本项视为已通过生产采用门槛。

### 6. 为多交易对共享连接做准备

单个 live maker 当前会使用公共行情、认证账户和订单回报等连接。多进程/多 symbol
扩展前，应设计按账户共享的账户流与订单回报流、按连接复用的公共订阅，以及清晰的
symbol 路由与背压隔离，避免接近交易所连接上限时才被动改造。

## 测试与发布门槛

- SDK loopback 测试：认证、订阅、ping/pong、idle、轮换、seq、断线、坏载荷。
- maker deterministic/replay 测试：WS/REST 到达顺序、重复/部分成交、错误 run、generation
  invalidation、冻结/清理/恢复和数量 tick 边界。
- 实盘验证与离线测试分离。没有针对该改动的明确授权，不得下真实订单、断开生产流或
  执行库存退出。
- 任何 stream、风控或账户语义改动都会重新触发 live-gate 审核和 canary 要求；现有记录
  不能自动覆盖新的传输行为。

## 参考

- [StandX Perps WebSocket API](https://docs.standx.com/standx-api/perps-ws)
- [StandX API rate limits](https://docs.standx.com/standx-api/rate-limits)
- [Maker runtime and safety contract](13-maker.md)
- [Maker live gate](14-maker-live-gate.md)
