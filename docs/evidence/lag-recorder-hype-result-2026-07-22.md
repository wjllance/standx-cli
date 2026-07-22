# StandX↔Hyperliquid lag-recorder HYPE 实测结果 — 2026-07-22

## Decision

- Status: `measurement_complete_conditional_window_found_low_coverage`
- 44.5 小时实测（2026-07-20T05:09Z ~ 2026-07-22T00:33Z，重叠跨度
  160,284s，录制主机零重连）。
- 条件性 lag（StandX 发生跟随时）≈ **1.2s**（中位数 1209ms，p25=777ms，
  p75=2380ms，均值 1750ms），落入工具交付文档定义的 1–3s 窗口下沿；
  **不是** `< ~0.3s 路线终止` 的情形。
- 但信号覆盖率仅 ~19%（154 次跳动中仅 29 次产生可测量跟随）：对大多数
  行情，外部价格并无领先性。原假设"StandX mark 系统性滞后、存在可利用
  窗口"只得到弱支持——**窗口存在但稀疏**。
- Stage 4（fair-price / order-flow）决策须将此覆盖率约束与 widen-spread
  A/B 结论、SIP-5A $/Maker-Hour 联合评估；本测量只提供 lag 数字，
  不改变任何报价行为。

## Setup

- Tool: `standx lag-recorder`（PR #319，`lag-recorder-standx-hyperliquid`），
  read-only，无认证、无订单。
- Command: `target/release/standx lag-recorder --symbol HYPE-USD --out
  var/standx/lag-rec-20260720T050910Z.ndjson --status-secs 300`
- Data: `var/standx/lag-rec-20260720T050910Z.ndjson`（665,407 行，
  StandX 508,953 / Hyperliquid 156,201，SIGTERM 优雅 flush 退出，
  末行 JSON 校验通过）；stderr 心跳日志同 basename `.stderr.log`。
- Analyzer: `python3 scripts/lag_analysis.py`（stdlib only）。
- 行情覆盖：HYPE mark 60.0–62.8，含多段平静期与一轮 ~4% 的下行波动
  （62.7 → 60.1），跳动事件主要在波动期产生。

## Results

### Event response（主估计）

154 次 Hyperliquid ≥8bps 跳动（2s 窗口内）：

| 类别 | 次数 | 占比 |
| --- | --- | --- |
| StandX 已在前侧（同步或领先） | 100 | 65% |
| 完全不跟随（窗口内未覆盖 50%） | 25 | 16% |
| 可测量跟随 | 29 | 19% |

可测量跟随事件（n=29）的 follow-time（覆盖 50% 跳动幅度）：

- median=1209ms, p25=777ms, p75=2380ms, mean=1750ms
- 收敛性：最近三次 4h 间隔检查中位数分别为 1271 / 1209 / 1209ms，
  估计已稳定。

### Cross-correlation（辅助估计）

- 全程噪声水平（峰值 r ≤ 0.06），峰值位置在 -500ms ~ -3750ms 间漂移，
  无证据价值，不予采信。

## Interpretation

- 按交付文档阈值框架：条件性 lag ≈ 1.2s 位于 1–3s "可防守窗口"下沿，
  外部价格引导报价路线**未被证据否定**。
- 但 65% 的跳动 StandX 并不落后、16% 完全不跟随：可利用跟随只占 ~19%。
  即使 Stage 4 采用外部 mark 作为领先信号，其预期价值受覆盖率制约，
  需在候选人机台/PnL 模型中以 ~19% 触发率折算。
- 与 HYPE jump-kill 毒性诊断的关联：74% toxic fills 无本地盘口前兆，
  本测量确认外部领先市场确实存在 StandX 跟随后 ~1.2s 才到位的行情片段，
  与 jump-kill 假设方向一致，但只覆盖少数跳动事件。

## Honest limitations

- 绝对 lag 带固定差分网络延迟偏移（录制主机→StandX 与→Hyperliquid 的
  RTT 差）。本机有 maker live 运行记录（`var/standx/` 下 stage2/xag-live
  日志），推定为 maker 同主机，但未单独验证区域一致性；变量部分（跟随
  分布形态）稳健，绝对值 ±RTT 差。
- 分辨率下限 ~0.5s（HL activeAssetCtx 出块节奏）；p25=777ms 接近但仍
  高于下限。
- 单窗口、单品种：结果 HYPE 专属，其他 symbol 须重新录制。
- 未覆盖极端行情（急跌/急涨 >10%）；尾部行为未知。

## Data integrity

- 录制进程 uptime 44.5h，双源零重连；NDJSON 665,407 行末行校验通过。
- 录制期间每 4 小时增量分析一次（共 11 次），跟随事件 n 从 8 增至 29，
  中位数收敛过程：838 → 865 → 945 → 985 → 1097 → 1240 → 1271 → 1209ms。
