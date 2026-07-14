# Maker WebSocket 迭代目标

本文档记录 `standx maker` 的 WebSocket 迭代方向，作为后续设计、拆分任务和 live
验证的依据。它不改变当前 maker 策略、下单阈值或 live gate；所有网络 I/O 仍由
`standx-cli` 执行，确定性的策略、账本和状态迁移仍归属 `standx-maker`。

## 当前定位

maker 当前使用三类 WebSocket 能力：

| 通道 | 当前作用 | 当前边界 |
|---|---|---|
| 公共行情流（`ws-stream/v1`） | 订阅 `price` 与 `depth_book`，缓存 mark 与最佳 bid/ask；超 5 秒自动回退 REST | 主要用于行情输入和 mark 漂移早醒 |
| 认证账户流（`ws-stream/v1`） | 订阅 `order`、`position`、`trade`，驱动当前 run 的成交账本、仓位异常和断线冻结 | REST 仍负责每轮完整账户快照与恢复对账 |
| 订单回报流（`ws-api/v1`） | 将带 `x-session-id` 的 HTTP 下单请求与异步接受/拒绝回报关联 | 当前不经 WS 直接发送 `order:new`/`order:cancel` |

这使 WS 已经是 live maker 的安全依赖，但尚未成为唯一或主要的账户状态源。REST
快照必须保留为启动、恢复、周期性校验和 fail-safe 对账的权威补偿路径。

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
是受支持且更成熟的路径，因此只应先做 paper/loopback spike：比较签名、request/session
生命周期、背压、断线后请求不确定性、幂等性与端到端延迟。

默认 live 执行通道不应因该 spike 自动切换。任何采用决定都需要单独的受监管 canary，
并保留 REST 的恢复与对账能力。

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
