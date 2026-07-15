# Maker 策略阶段 1 实施记录 — 2026-07-15

## 状态与边界

- 阶段：1 — 绩效账本与确定性回放
- 状态：`implementing`
- baseline git SHA：`ccdcf3191f206c17dda89105de0ee9346ff563d4`
- baseline config SHA-256：`37a63617b5438d949415eacc26487020cd0a35299ea2f8bddc7b1655ea9d62dd`
- live 授权范围：`none`
- 策略、报价、退出和安全决策变更：`none`

本阶段只增加观测、归因与纯回放能力。新增状态不参与 planner、freeze、cleanup、recovery、
stop-loss 或 inventory-exit 决策；旧 JSON action/字段语义不变。

## 已实现

- `standx-maker::performance`
  - passive maker fill 与 reduce-only inventory exit 分开归因；
  - gross spread、quote fee/rebate、signed funding、exit slippage、inventory MTM residual 与
    net PnL 使用可复算守恒式；
  - 成交后 1s/5s/30s markout 使用目标时间之后第一条 mark，缺失窗口保持
    `pending/unavailable`，不使用当前 mark 伪造；
  - 时间加权双边 uptime 和 bid/ask/total eligible quantity-ms；
  - WS fill 先到、REST 费用后到时按 `trade_id` 幂等补齐；不可换算费用显式计入
    `execution_costs_unavailable`。
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
- JSON / OpenObserve
  - `cycle_summary.performance`、`performance_summary`；
  - `order_latency`、`order_latency_summary`；
  - `account_event_lag`；
  - dashboard 新增 `Performance & Latency` tab。

## 当前验证

- `HOME=/tmp/standx-test-home CARGO_HOME=~/.cargo cargo test --workspace --offline`：通过；
  `standx-cli` 130、CLI main 2、integration 13、unit 31、`standx-maker` 116、`standx-sdk` 74，
  credential-dependent e2e 2 个按既有标记 ignored。
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过。
- `cargo fmt --all -- --check`：通过。
- `python3 -m py_compile scripts/openobserve_dashboard.py scripts/maker_run_manifest.py
  scripts/test_maker_run_manifest.py`：通过。
- `python3 scripts/test_maker_run_manifest.py`：4 个测试通过。
- `examples/maker-replay-trace.ndjson` 使用 candidate CLI 连续回放三次，完整 JSONL 输出
  SHA-256 均为 `7dcde76d4650b932d40590ecee763533f7f28a8d9dda5c0d8f698c0f30c5ecf2`。
- 示例回放结果：4 cycles、1 passive fill、1 inventory exit、费用可换算率 100%、
  `net_pnl_quote=0.15`、时间加权双边 uptime 100%；30s markout 的不足窗口明确记为
  `unavailable=1`。

## 尚未达到 accepted 的项目

- 采集平静、趋势、快速波动三类新的 normalized trace；阶段 0 历史日志缺少 phase-1 字段，
  不能伪装为完整回放输入。
- 使用冻结样本核对净 PnL 最小计价精度、手工 uptime/depth-time、markout unavailable 和
  place/cancel terminal coverage。
- OpenObserve 实际字段映射与 dashboard 查询需要用阶段 1 新日志做一次离线/本地导入验证。

## 阶段状态模板

```text
阶段：1
状态：implementing
baseline_git_sha：ccdcf3191f206c17dda89105de0ee9346ff563d4
candidate_git_sha：working tree
baseline_config_hash：37a63617b5438d949415eacc26487020cd0a35299ea2f8bddc7b1655ea9d62dd
candidate_config_hash：same (no strategy config change)
训练数据窗口：none（当前只实现测量与回放基础设施）
样本外验收窗口：阶段 0 三类窗口需重新采集 normalized phase-1 trace
专项指标结果：synthetic replay deterministic PASS; frozen market-window acceptance pending
统一安全门槛结果：no strategy or live-gate change
离线验证结果：full standard bundle PASS
live 授权范围（如无则写 none）：none
release owner 决定：pending
```
