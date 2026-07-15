# Maker 策略阶段 1 实施记录 — 2026-07-15

## 状态与边界

- 阶段：1 — 绩效账本与确定性回放
- 状态：`accepted_offline`
- baseline git SHA：`ccdcf3191f206c17dda89105de0ee9346ff563d4`
- baseline config SHA-256：`37a63617b5438d949415eacc26487020cd0a35299ea2f8bddc7b1655ea9d62dd`
- live 授权范围：`none`
- 策略、报价、退出和安全决策变更：`none`

本阶段只增加观测、归因与纯回放能力。新增状态不参与 planner、freeze、cleanup、recovery、
stop-loss 或 inventory-exit 决策；旧 JSON action/字段语义不变。

## 已实现

- `standx-maker::performance`
  - passive maker fill 与 reduce-only inventory exit 分开归因；
  - 分角色数量、signed cashflow、passive quantity-weighted capture 和 exit cost 可独立复算；
  - gross spread、quote fee/rebate、signed funding、exit slippage、inventory MTM residual 与
    net PnL 使用可复算守恒式；
  - 成交后 1s/5s/30s markout 使用目标时间之后第一条 mark，缺失窗口保持
    `pending/unavailable`，不使用当前 mark 伪造；
  - 时间加权双边 uptime、bid/ask/total eligible quantity-ms 和库存持有时间积分；
  - WS fill 先到、REST 费用后到时按 `trade_id` 幂等补齐；不可换算费用显式计入
    `execution_costs_unavailable`；缺少 funding 事件时 `funding_available=false` 且
    `net_pnl_complete=false`，不会把未知 funding 静默当成已确认零值。
- `standx-maker::replay`
  - typed cycle/fill/funding event 驱动与 live 相同的 `preflight_cycle` / `plan_cycle` 和绩效账本；
  - 不读取网络、环境、文件、终端或实时时钟；
  - 同一 typed trace 三次结果完全一致测试。
- `standx maker replay <TRACE>`
  - CLI 只负责严格解析 schema v1 NDJSON 和渲染结果；
  - header 冻结 symbol、git SHA、config hash、seed、完整 config/settings；
  - 未知字段、错误顺序、缺 header/finish 均 fail fast；支持 `-` stdin。
- `standx-maker::latency`
  - place/cancel intent、socket write、ack、account effective 独立计时；
  - account-order 先于 ack、晚到 ack、reject、timeout、invalidated、process-ended；
  - p50/p95/p99、reject/timeout rate、cancel 后关联 fill；
  - account projection 返回生效 request ID 和超时 request ID，不改变投影决策。
  - generation 失效会把未完成请求归入 `invalidated`；迟到 ACK/effective 仍可附着但不会改写
    terminal outcome；生命周期时间回退会被 typed error 拒绝。
- JSON / OpenObserve
  - `cycle_summary.performance`、`performance_summary`；
  - `order_latency`、`order_latency_summary`；
  - `account_event_lag`；
  - request-level latency 保留 symbol/side/level/source/recovery；自动恢复后的首个成功周期标为
    recovery，分类只进入观测上下文；
  - dashboard v9 新增 `Performance & Latency` tab，以及按 `run_id/config_hash` 的绩效和延迟
    对比表；采集器与 dashboard payload 均有离线契约测试。

## 当前验证

- `HOME=/tmp/standx-test-home CARGO_HOME=~/.cargo cargo test --workspace --offline`：通过；
  `standx-cli` 133、CLI main 2、integration 13、unit 31、`standx-maker` 121、`standx-sdk` 74，
  credential-dependent e2e 2 个按既有标记 ignored。
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过。
- `cargo fmt --all -- --check`：通过。
- `python3 -m py_compile` 覆盖 dashboard、ingest、manifest 及阶段 1 新增 verifier/test 脚本：通过。
- `python3 scripts/test_maker_run_manifest.py`：4 个测试通过。
- `python3 scripts/test_openobserve_dashboard.py`：4 个测试通过。
- `python3 scripts/test_openobserve_ingest.py`：8 个测试通过。
- `bash -n scripts/run_maker_observed.sh` 与 `git diff --check`：通过。
- `examples/maker-replay-trace.ndjson` 使用 candidate CLI 连续回放三次，完整 JSONL 输出
  SHA-256 均为 `30b791153f785f3cc8e5b62d9f3f40e54f0ab4fa49aa8b50613e7d9a3c0c8406`。
- 示例回放结果：4 cycles、1 passive fill、1 inventory exit、费用可换算率 100%、
  funding 显式可用、PnL 守恒误差 0、时间加权双边 uptime 100%、库存绝对数量时间积分
  20,000 qty-ms；30s markout 的不足窗口明确记为 `unavailable=1`。

## 阶段 1 验收映射

- 确定性与纯度：`scripts/verify_maker_stage1.py` 连续三次执行本地 replay，完整 stdout
  byte-identical，并静态拒绝 replay core 的网络、环境、终端、实时时钟和订单 I/O 依赖。
- 账本与归因：maker 单元测试覆盖 WS→REST、REST→WS、重复、partial-then-cancel、wrong-run、
  passive/exit 分流、费用回补和 funding 完整性；合成验收的 PnL 守恒误差为 0，小于 0.01
  quote tick。
- markout 与时间积分：单元测试和合成 verifier 共同核对 1s/5s/30s、unavailable、手工
  uptime/depth-time 和基于 normalized event time 的库存持有时间。
- 生命周期：单元测试覆盖 account-before-ack、late ack、reject、timeout、process end、
  invalidation/reconnect/stale generation、单调时间和 terminal coverage；JSON 同时保留逐请求
  关联字段与 place/cancel p50/p95/p99、reject/timeout rate。
- 查询链路：ingest 契约证明 `run_id/config_hash` 补充且阶段 1 字段不丢失；dashboard 契约
  证明选中 run 查询、跨 run/config 对比和 request-level 分组字段均存在。
- JSON 兼容：只增加可选字段和新 action，不删除、改名或重解释旧字段。

生产 OpenObserve 部署、credential-dependent 验证和 live canary 不属于本次离线授权，均未执行。
“平静/趋势/快速波动”三类冻结样本是本文路线中阶段 1 完成后、阶段 2 策略比较的统一数据门槛；
不得用阶段 0 缺字段的历史日志伪装成 normalized phase-1 trace，进入阶段 2 前仍需采集。

## 阶段状态模板

```text
阶段：1
状态：accepted_offline
baseline_git_sha：ccdcf3191f206c17dda89105de0ee9346ff563d4
candidate_git_sha：bb927198818f97cc5ceece5018c149ab854cfb0f + working tree
baseline_config_hash：37a63617b5438d949415eacc26487020cd0a35299ea2f8bddc7b1655ea9d62dd
candidate_config_hash：same (no strategy config change)
训练数据窗口：none（当前只实现测量与回放基础设施）
样本外验收窗口：not applicable to stage-1 measurement infrastructure; stage-2 gate pending collection
专项指标结果：stage-1 roadmap checklist PASS; synthetic replay deterministic PASS
统一安全门槛结果：no strategy or live-gate change
离线验证结果：full standard bundle PASS
live 授权范围（如无则写 none）：none
release owner 决定：stage-1 offline engineering accepted; production deployment pending separate authorization
```
