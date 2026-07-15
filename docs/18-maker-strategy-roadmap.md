# Maker 策略迭代路线

本文档记录 `standx maker` 在当前 anti-flicker 双边报价策略之上的迭代路线。目标是先建立
可信、可重放的绩效归因，再逐步引入自适应报价、库存控制和订单流信号；任何阶段都不能
以破坏现有 fail-closed 安全语义、订单归属、账本 exactly-once 或输出契约为代价。

路线中的数值门槛是进入下一阶段的工程验收线，不是收益承诺。每个阶段开始前必须冻结
基线 commit、配置哈希、数据集和比较口径；未达到门槛时保留旧策略作为默认行为。

## 总体原则

- `standx-maker` 只接收归一化 typed input，负责纯策略、风险、统计和状态决策。
- `standx-cli` 负责行情/账户 I/O、命令执行、回放文件读取、遥测和用户输出。
- `standx-sdk` 负责交易所 payload、协议、认证和传输健康。
- 新能力默认关闭；关闭时必须与当前策略行为等价。
- 保留已有 JSON action 名称和字段；新增指标只能增加可选字段或新 action。
- WS/REST 成交继续走同一账本入口，以稳定 `trade_id` exactly-once 去重。
- 所有策略比较先经过 deterministic replay，再进入 paper/shadow；live canary 必须单独授权。
- 策略、风险控制或交易所命令路径发生变化后，按
  [14-maker-live-gate.md](14-maker-live-gate.md) 重新锁定并补充证据。

## 阶段总览

| 阶段 | 主题 | 主要产物 | 进入下一阶段的核心条件 |
|---|---|---|---|
| 0 | 基线与证据校准 | 冻结基线、配置/文档对齐、数据集清单 | 基线可复现，配置与 live 证据无冲突 |
| 1 | 绩效账本与回放 | 净 PnL 归因、markout、订单延迟、时间加权 uptime、replay runner | 同一 trace 确定性重放，指标守恒且可查询 |
| 2 | 自适应 spread / refresh | 波动与逆向选择驱动的动态报价宽度 | 样本外风险改善，收益/uptime 不越过退化线 |
| 3 | 库存控制器 | 非线性 price/size/level skew、库存年龄 | 尾部库存显著下降，退出成本和敞口不恶化 |
| 4 | 公平价与订单流 | 深度数量、microprice、OFI、信号质量降级 | shadow/replay 中 markout 改善，坏数据安全回退 |
| 5 | 异常与退出政策 | 分级背离处理、正常/紧急退出分离 | 冻结/清理/恢复全覆盖，受监督 canary 通过 |

## 统一验收口径

阶段 1 完成后，后续策略阶段统一以至少三类互不重叠的数据窗口验收：

1. 平静/窄幅市场；
2. 单边趋势/库存持续累积市场；
3. 快速波动、盘口稀疏或行情源短暂异常市场。

参数只允许在训练窗口调整，验收必须使用冻结参数和未参与调参的样本外窗口。比较报告至少
包含：净 PnL、最大回撤、1s/5s/30s markout、下单/撤单 effective latency、时间加权双边
uptime、合格深度时间积分、成交率、撤单率、`p95 |position|`、高库存持续时间和主动退出
成本。

除各阶段的专项门槛外，所有阶段还必须满足：

- 无 `max_position`、worst-case pending exposure、账本或 generation 安全不变量违规。
- 无 wrong-run/manual fill 进入 current-run ledger。
- 无 stale generation 结果恢复报价；冻结后 maker book 必须先清空再恢复。
- 现有 JSON contract 兼容，live 默认值和 live gate 默认锁定状态不变。
- 通过仓库标准离线验证：

```bash
HOME=/tmp/standx-test-home CARGO_HOME=~/.cargo cargo test --workspace --offline
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo fmt --all -- --check
python3 -m py_compile scripts/openobserve_dashboard.py
```

## 阶段 0：基线与证据校准

当前执行记录：
[maker-strategy-stage-0-baseline-2026-07-15.md](evidence/maker-strategy-stage-0-baseline-2026-07-15.md)。

### 目标

在不改变交易行为的前提下，建立后续 A/B 比较的唯一基线，消除配置、注释和 live 证据之间
的歧义。

### 范围

- 记录基线 commit、maker 配置文件哈希、symbol/tick 元数据和数据时间窗。
- 对齐 `examples/maker*.toml`、`13-maker.md` 与 live-gate evidence 中的阈值说明。
- 明确区分：告警阈值、fail-safe stop、正常库存退出和是否平仓。
- 盘点已有 paper/live 日志，登记可用于回放的数据字段和缺口。
- 保持通用 `examples/maker.toml` 的主动库存退出默认关闭。

### Trace 分类口径

阶段 0 的市场分类只用于组织数据集，不改变策略阈值。可比较窗口至少包含 300 个连续
`cycle_summary` 且覆盖 600 秒；不足时只能作为身份/采集校准 run。分类按以下优先级执行：

1. 快速波动/压力：`max_vol_bps >= 50`，或存在 halted cycle 且窗口 range ≥ 50bps；
2. 趋势：`|net_move_bps| >= 75`、`|net_move_bps| / range_bps >= 0.7` 且无 halted cycle；
3. 平静：range ≤ 10bps 且 `|net_move_bps| <= 5`；
4. 其余或必需字段缺失：`unclassified`。

其中 range 使用 `(max_mark - min_mark) / min_mark`，net move 使用
`(end_mark / start_mark - 1)`。分类结果必须连同原始值记录，不能只保存标签。

### 验收标准

- [x] 每个用于比较的基线 run 都能追溯到 `git_sha + config_hash + symbol + time range`。
- [x] 同一配置值在 TOML 注释、maker 文档和 live-gate evidence 中不存在冲突。
- [x] 已生产验证与未验证的 inventory-exit tuple 分开标注；未验证 tuple 不宣称可用于 live。
- [x] 文档明确说明 `alert_*` 只通知，`stop_loss` 会 fail-safe 停机但不会自动平仓。
- [x] 形成至少一份平静、一份趋势和一份快速波动 trace 清单；缺失字段有显式记录。
- [x] 不改变报价、退出、PnL、风控和 JSON 行为；标准离线验证全部通过。

## 阶段 1：绩效账本与确定性回放

### 目标

把“策略是否更好”变成可复现、可归因的判断，而不是只看 session 末尾 PnL。此阶段仍不
改变 live 下单决策。

### 范围

- 分离 passive maker fills、reduce-only inventory exits 和外部/错误 run 事件。
- 增加 gross spread、fee/rebate、funding、exit slippage 和 net PnL 归因。
- 增加数量加权 capture，以及成交后 1s/5s/30s markout。
- 分阶段记录 place/cancel 从 intent、socket write、command ack 到 account-order 生效的延迟。
- 将 uptime 从 cycle 占比升级为时间加权双边 uptime；增加合格报价深度 × 时间。
- 新增 deterministic replay runner，读取归一化 market/account trace 驱动纯 planner/ledger。
- 将新增指标接入 JSON、OpenObserve 查询和 dashboard。

### 下单与撤单延迟观测

不能把“本地 socket 写入成功”“交易所接受命令”和“订单已经在账户/盘口状态中生效”合并成
一个延迟。阶段 1 至少记录以下区间：

| 指标 | 起点 | 终点 | 含义 |
|---|---|---|---|
| `place_write_ms` | place intent 生成 | 完整 command frame 写入本地 WS sink | 本地调度、签名、锁和 socket backlog |
| `place_ack_ms` | frame written | 关联 `request_id` 的 accepted/rejected response | 网络与交易所命令处理 |
| `place_effective_ms` | place intent 生成 | account stream 首次观察到该 current-run order | 订单真正进入 live account projection 前的总延迟 |
| `cancel_write_ms` | cancel intent 生成 | 完整 cancel frame 写入本地 WS sink | 本地撤单发送延迟 |
| `cancel_ack_ms` | frame written | 关联 `request_id` 的 cancel accepted/rejected response | 撤单命令确认延迟 |
| `cancel_effective_ms` | cancel intent 生成 | account stream terminal/零 open qty，或审计确认 absent | 真正解除挂单敞口的总延迟 |
| `account_event_lag_ms` | 交易所 `updated_at` / `trade_ts` | 本地收到对应 typed event | 账户流传输和消费延迟 |
| `fill_after_cancel_ms` | cancel intent 生成 | 撤单窗口内到达的关联成交 | 撤单在途期间的实际成交风险 |

观测实现遵守以下边界：

- `standx-cli` 使用单调时钟计算进程内 duration，并同时记录 UTC 时间供跨系统对齐。
- `standx-maker` 的 projection/reducer 不读取时钟、不保存 `Instant`；以后若策略需要延迟状态，
  只接收 CLI 归一化后的 typed latency summary。
- 使用 `request_id + client_order_id/order_id + generation + cycle` 关联生命周期，允许
  account-order 先于 command ack 到达。
- 超时、拒绝、generation invalidation、断流和进程结束都必须产生 terminal/censored outcome；
  不能只统计成功请求，否则延迟分位数会产生 survivor bias。
- 通过新增可选字段或独立 `action="order_latency"` 输出，不修改现有 JSON action/字段语义。
- 阶段 1 只观测和回放延迟，不因延迟阈值改变报价、冻结或恢复行为。

### 验收标准

- [ ] 同一 trace、配置和 seed 连续重放三次，结构化 summary 完全一致。
- [ ] WS→REST、REST→WS、重复成交、部分成交后撤单均只记账一次。
- [ ] passive fill 和 active exit 的数量、现金流、capture/exit cost 可独立汇总。
- [ ] `gross spread + inventory MTM change + rebate - fee + signed funding cashflow - exit cost` 与 net PnL 的差异不超过一个计价币最小精度单位；无法换算的费用单列且不静默忽略。
- [ ] 1s/5s/30s markout 使用成交时点后的行情；缺失窗口标为 unavailable，不使用回补时的当前 mark 伪造。
- [ ] 合成 trace 中时间加权 uptime、深度时间积分和库存持有时间与手工计算一致。
- [ ] 100% 已注册 place/cancel request 最终归入 accepted、rejected、effective、timeout、
  invalidated 或 process_ended 之一；没有无解释的 pending request。
- [ ] 同一请求的生命周期时间单调、duration 非负，并正确覆盖 account-order 先于 ack、晚到
  ack、拒单、超时、重连和 stale generation。
- [ ] place/cancel 分别输出 p50、p95、p99、timeout rate 和 reject rate；超时样本作为
  censored/timeout 保留，不从分布中静默删除。
- [ ] `cancel_effective_ms` 能与 `fill_after_cancel_ms`、负向 markout 和仓位跳变按
  `run_id/config_hash/symbol` 关联查询。
- [ ] 延迟指标可按正常运行/recovery、symbol、side、level 和 market source 分组，且不会把
  socket write success 误标为 venue accepted。
- [ ] replay runner 不访问网络、环境变量、终端或实时时钟，且不会执行任何订单 I/O。
- [ ] 旧 JSON 字段语义不变；新增字段在旧消费者缺失时仍可正常工作。
- [ ] dashboard 能按 `run_id/config_hash` 对比上述指标，区分 passive fill 与 inventory exit，
  并展示 place/cancel latency 分位数、超时率和撤单后晚到成交。

## 阶段 2：自适应 Spread 与 Refresh

### 目标

让报价宽度和重报阈值响应市场风险，同时保持 SIP-5A band、post-only 和 anti-flicker 约束。

建议的纯策略输入包括短窗 realized volatility、当前 touch spread、近期 markout/toxicity、
阶段 1 产出的滚动 latency summary 和可换算的手续费下限。输出仍只是目标 spread/refresh
或 typed quote intent。

### 范围

- 新增可关闭的 adaptive quote policy；关闭时使用现有静态 `spread_bps/refresh_bps`。
- 建立 spread 下限、上限、变化速率和 hysteresis，避免逐 tick 撤挂。
- 保持 band/no-cross/tick rounding/exposure cap 作为最终不可绕过的约束。
- 将策略使用的 volatility 改为明确的时间窗口，避免 cycle 频率改变统计周期。

### 验收标准

- [ ] adaptive policy 对同一 typed input 始终返回相同结果；关闭时与旧 planner action 等价。
- [ ] 有效 spread 不低于配置的费用/风险下限，不高于 band 可容纳的安全上限。
- [ ] 任意输入下都不会生成穿 touch、出 band、低于最小数量或突破敞口预算的报价。
- [ ] 波动或负向 markout 单调恶化时 spread 不收窄；恢复时通过 hysteresis 平滑回落。
- [ ] 三类样本外窗口合计 net PnL 不低于静态基线的 95%。
- [ ] 最大回撤绝对值不得大于基线；5s 负向 markout 绝对值至少改善 10%，否则不晋级。
- [ ] 时间加权双边 uptime 相对基线下降不超过 3 个百分点。
- [ ] 每 quote-hour 撤单数相对基线增加不超过 20%，且不出现阈值附近振荡。
- [ ] 先完成 replay 和 paper/shadow 报告；未单独授权前不进行 live canary。

## 阶段 3：非线性库存控制

### 目标

把库存治理从“线性移动整个报价中心 + 达阈值市价退出”升级为价格、数量和档位共同作用的
控制器，减少高库存持续时间和被动积累后的退出成本。

### 范围

- price skew：保留方向正确性，但允许靠近上限时非线性增强。
- size skew：缩小加仓侧数量，必要时增加减仓侧的安全 maker 数量。
- level skew：高库存时减少加仓侧档数，优先保留减仓侧流动性。
- inventory age：库存长时间未回中时逐步提高减仓强度。
- 主动退出继续是独立策略；不在本阶段隐式改变市价退出或波动熔断语义。

### 验收标准

- [ ] 控制器关闭时，价格、数量、档位和 action 顺序与旧策略等价。
- [ ] long/short 完全对称；零库存不产生 skew；满仓及越界输入正确饱和。
- [ ] 当前仓位、所有 resting quote 和 pending place 全部同侧成交后仍不突破 `max_position`（半个 qty tick 容差）。
- [ ] 数量始终 tick-aligned 且不低于 venue minimum；无法安全缩量时丢弃该档而不是提交非法数量。
- [ ] 样本外 `p95 |position|` 至少下降 15%，或处于 `|position| >= 70% max_position` 的时间至少下降 25%。
- [ ] 主动退出次数和总 taker exit cost 均不得高于基线；若其中一项增加，阶段不晋级。
- [ ] net PnL 不低于基线的 95%，时间加权双边 uptime 下降不超过 3 个百分点。
- [ ] 覆盖数量 tick 边界、方向翻转、阈值跨越、部分成交、pending reservation 和 wrong-run 事件测试。

## 阶段 4：公平价与订单流信号

### 目标

在不脱离 mark eligibility band 的前提下，用盘口深度和短周期订单流改善公平价，降低被
有毒流量成交后的负向 markout。

### 范围

- SDK/CLI adapter 解析前若干档价格和数量，归一化为 maker domain depth input。
- 在 `standx-maker` 中纯计算 microprice、depth imbalance、OFI 和信号新鲜度。
- fair-price adjustment 必须有边界、过期时间和质量分；数据不足时回退到当前 mark 策略。
- 第一轮只做 shadow：同时计算 legacy/fair-price plan，但执行 legacy plan。

### 验收标准

- [ ] maker core 不接收松散 JSON 或 SDK payload，只接收已校验 typed depth/order-flow input。
- [ ] 乱序、重复、缺档、负数量、非有限值和 stale depth 均被拒绝或安全降级。
- [ ] 信号关闭、低质量或过期时，最终 plan 与 legacy mark-centered plan 等价。
- [ ] fair price 及最终报价始终受 mark band、no-cross、tick 和 exposure cap 约束。
- [ ] 至少一轮完整 paper/shadow 只记录 plan 差异，确认不会触发真实订单差异。
- [ ] 冻结参数的样本外窗口中，passive fills 的 5s 负向 markout 绝对值至少改善 10%。
- [ ] net PnL 不低于 legacy 基线的 95%，最大回撤和高库存时间均不得恶化。
- [ ] 改善不能只来自大幅减少成交：成交数量下降超过 20% 时必须单独评审 SIP-5A 收益/uptime 影响。

## 阶段 5：分级异常与退出政策

### 目标

明确区分短暂数据不一致、持续行情异常、正常库存修剪和紧急风险处置，避免“坏数据时永久
保留挂单”或“波动最大时只能停机持仓”的隐含政策。

### 范围

- mark/mid 背离分级：短暂 hold、持续 freeze/cleanup、恢复后空簿对账。
- 正常 inventory trim 与 emergency risk exit 使用不同 typed policy/effect。
- 明确 volatility halt 期间是否允许紧急退出；默认不得自动继承正常退出行为。
- stop-loss 后残余仓位输出明确 handoff；自动 flatten 必须是默认关闭、单独授权的 live policy。
- equity/margin 的 alert 与 hard floor 使用不同配置名和不同 typed outcome。

### 验收标准

- [ ] 短暂背离在配置宽限期内保留当前兼容行为，不盲目 cancel/re-place。
- [ ] 背离超过持续时间或严重度阈值后，generation 失效、placement 冻结、queued action 清空并安排 maker cleanup。
- [ ] cleanup 未确认空簿、required stream 不健康或仓位未与 ledger 对齐时绝不恢复报价。
- [ ] 正常 trim、emergency exit、hard stop 和 residual-position handoff 在类型、日志和 JSON action 上可区分。
- [ ] vol halt + 高库存、stop-loss + 残余仓位、退出部分成交、退出未确认、退出拒单和 cleanup residual 均有确定性测试。
- [ ] emergency flatten 默认关闭；未显式授权时，测试和运行路径只能告警、清理 maker 单并交接残余仓位。
- [ ] live canary 使用一个 symbol、最小有效数量、一个 level 和明确的 emergency cancel 操作人。
- [ ] canary 证据包含 create/cancel/exit request correlation、trade IDs、空 maker book、最终仓位和 webhook 送达结果。
- [ ] 只有 release owner 审核证据后才能解锁；一次 canary 不授权连续 live maker 或其他退出 tuple。

## 阶段状态记录模板

每个阶段进入实施时，在对应 PR/issue 或 evidence 文档中填写：

```text
阶段：
状态：planned | implementing | replay | paper/shadow | canary | accepted | rejected
baseline_git_sha：
candidate_git_sha：
baseline_config_hash：
candidate_config_hash：
训练数据窗口：
样本外验收窗口：
专项指标结果：
统一安全门槛结果：
离线验证结果：
live 授权范围（如无则写 none）：
release owner 决定：
```

只有所有必选验收项都有可复查证据时，阶段状态才能改为 `accepted`。收益指标改善但安全门槛
失败时必须判定为 `rejected`；安全通过但收益/风险没有达到专项门槛时继续保持 shadow，不能
仅凭主观观察晋级。
