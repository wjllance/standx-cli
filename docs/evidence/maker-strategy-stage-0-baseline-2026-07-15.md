# Maker 策略阶段 0 基线记录 — 2026-07-15

## 状态与目标

- 阶段：0 — 基线与证据校准
- 状态：`accepted`
- 目标：在不改变交易行为的前提下，冻结可追溯基线、消除参数说明冲突，并形成三类市场
  trace 清单，供阶段 1 的确定性回放和后续策略 A/B 共用。
- live 授权范围：`none`
- 策略行为变更：`none`

本记录只校准配置、文档和已有证据。它不授权 live 运行，不把历史 canary 扩展为新的
参数批准，也不把缺少原始 trace 的摘要当成可回放数据。

## 冻结基线

阶段 0 策略/运行时基线锁定为提交 `ccdcf3191f206c17dda89105de0ee9346ff563d4`。
以下哈希是阶段 0 验收时的工作树内容 SHA-256；除 conservative profile 的说明注释校准外，
数值配置与该提交一致。最终 paper run 实际使用的 `examples/maker.toml` 与基线提交逐字节一致。
比较运行如果使用 CLI 覆盖项，还必须另外记录完整的非敏感参数覆盖，不能只引用文件哈希。

| 配置 | SHA-256 | 用途 | live 验证状态 |
|---|---|---|---|
| `examples/maker.toml` | `37a63617b5438d949415eacc26487020cd0a35299ea2f8bddc7b1655ea9d62dd` | 通用 paper 基线；主动退出 `0 / 0` | 不作为 live profile |
| `examples/maker-xag-100u.toml` | `0573e99a15375e6caeb95f78a467501be41a69201e89cafbbb2e57fefdb59740` | XAG 历史参考 profile；退出 `25% / 0.2` | 仅该 exact tuple 有 2026-07-10 生产证据 |
| `examples/maker-xag-100u-conservative.toml` | `336fb9b507845ab8535291da4b14bd77eab9cc817451895f71e5342643b4b838` | XAG 保守 profile；退出 `50% / 0.2`；仅注释校准 | exact tuple 未验证 |
| `examples/maker-xag-100u-standard.toml` | `50194d1bebc0c79a424c3365754147c144173ba3442f38281b1abd6138715ea1` | XAG 标准 profile；退出 `25% / 0.3` | exact tuple 未验证 |

最终身份校准 run 使用 `ccdcf31 + examples/maker.toml + BTC-USD + paper`：

| 字段 | 记录值 |
|---|---|
| `run_id` | `stage0-btc-paper-built-20260715` |
| UTC 窗口 | `2026-07-15T06:37:12Z–06:38:10Z`（58 秒） |
| git/config | `ccdcf3191f206c17dda89105de0ee9346ff563d4` / `37a63617...d62dd` |
| program SHA-256 | `226403cab6275c20f729a2733087581a4855038c9eef5b87dd2afccaca3f4301` |
| collector SHA-256 | manifest `1098680c...8ce64`；wrapper `9a59ecd4...70a` |
| raw NDJSON SHA-256 | `fdc999d6528675f45f38cc2e99347e72d814cf13186bf4ea96599e02bb1f0a94` |
| symbol metadata | price decimals `2`；qty decimals `4`；min qty `0.0001` |
| 完整性 | 43 cycles（`0..42`）；无缺失/重复；时间单调；started/stopped；exit `0` |
| paper 结果 | 2 simulated fills；final position `+0.002`；uptime `100%`；PnL `-0.10147` |
| 数据质量 | 32/43 cycle 使用 REST fallback，其中 31 次为 `ws_server_time_skew` |

该 run 在执行前通过 `cargo build -p standx-cli --offline` 显式重建程序。策略/运行时 source
相对基线提交 clean；整仓 dirty paths 仅为本阶段文档、采集工具和 XAG 注释校准，均在
manifest 中列出，实际参与运行的 collector 文件另有内容哈希。`validation.baseline_eligible=true`，但因窗口不足
300 cycles / 600 秒，`comparison_window_eligible=false`；它只证明身份和采集闭环，不作为
长期绩效样本。

每个后续比较 run 必须继续记录：

- `run_id`、完整 40 位 `git_sha`、配置内容哈希和非敏感 CLI override；
- 整仓/策略源码 dirty paths、实际执行程序及采集脚本 SHA-256；
- symbol、`price_tick_decimals`、`qty_tick_decimals`、`min_order_qty`；
- UTC 起止时间、模式、行情来源和原始 NDJSON SHA-256；
- lifecycle 是否完整、cycle 数、缺失序号和终止原因。

2026-07-15 已从 StandX 公共 symbol-info 接口只读确认 BTC-USD 当时的 metadata：
`price_tick_decimals=2`、`qty_tick_decimals=4`、`min_order_qty=0.0001`、状态 `trading`。
这些值用于本次 baseline 的启动快照；未来 run 仍须重新读取，不能永久沿用本次结果。

## 配置与语义校准

| 项目 | 当前权威语义 | 校准结果 |
|---|---|---|
| 通用主动退出默认值 | `examples/maker.toml` 为 `0 / 0`，默认关闭 | 与 `13-maker.md` 一致 |
| 已验证 XAG 退出 tuple | 仅 `max_position=0.8, pct=25, qty=0.2` | 与 2026-07-10 live-gate evidence 一致 |
| XAG conservative tuple | `0.8 / 50 / 0.2` | 参数不变；注释已改为“exact tuple 未验证” |
| XAG standard tuple | `1.2 / 25 / 0.3` | 已明确是按比例扩展、未做 exact-tuple live 验证 |
| `alert_*` | 边沿触发通知，不改变报价/退出决策 | 已在 maker 文档分开列出 |
| `stop_loss` | 会话 PnL 触线后 freeze、maker cleanup、critical webhook、停机 | 已明确不会自动平仓 |
| 正常库存退出 | live-only reduce-only chunk，先确认 maker 空簿 | 与 stop-loss/fail-safe 分开 |
| 残余仓位 | 停机后明确 handoff；当前没有默认自动 flatten | 与 live-gate/canary 证据一致 |

## Trace 分类口径

分类只组织数据集，不改变 maker 行为。窗口至少 300 个连续 cycle 且覆盖 600 秒；优先级为
快速波动/压力、趋势、平静、未分类：

- 快速波动/压力：`max_vol_bps >= 50`，或 halted cycle > 0 且 range ≥ 50bps；
- 趋势：`|net move| >= 75bps`、directionality（`|net| / range`）≥ 0.7，且无 halted cycle；
- 平静：range ≤ 10bps 且 `|net move| <= 5bps`；
- 缺字段或不满足上述条件：`unclassified`。

range 和 net move 分别按 `(max-min)/min` 与 `(end/start-1)` 计算，报告必须保存原值。

## 三类 trace 清单与字段缺口

| 市场类型 | 现有证据 | 身份信息 | 可用性与缺口 |
|---|---|---|---|
| 平静/窄幅 | `xag-live-20260711T013747Z`；XAG-USD；6120 cycles；`01:37:54–07:06:04Z` | range `6.68bps`；net `-1.67bps`；0 halted | git `957f1567a217f5dd2b189580775d82f44fbe4fea`；config `43629a9c...e84`；source `xag-live-20260711T013747Z.ndjson` |
| 单边趋势/库存累积 | `xag-live-20260713T071547Z`；XAG-USD；1266 cycles；`07:16:11–08:26:42Z` | `58.23→58.88`；net `+111.63bps`；range `137.48bps`；directionality `0.812`；0 halted | git `4ae5d91c41f377b0f384921f3a3c8421b7c33461`；config `f94a15f2...77fa`；source `xag-live-20260713T071547Z.ndjson` |
| 快速波动/压力 | `20260710T141618Z-3940793`；XAG-USD；1043 cycles；`14:16:24–15:13:14Z` | range `119.69bps`；max vol `116.81bps`；11 halted；net `-25.08bps` | git `563568cf89ab6dd54f7dbec01c4f2084e0907ba1`；config `3ec0e8a0...ce3f`；source `20260710T141618Z-3940793.ndjson` |

三份 trace 的 `config_hash` 均已与对应完整 git commit 内的
`examples/maker-xag-100u.toml` 逐字节 SHA-256 匹配。核心 cycle 字段 `mark/best_bid/
best_ask/position/pnl/uptime_pct` 缺失数均为 0，事件以 `event_id` 去重。

明确缺口：三份历史 trace 产生于 `market_source` 字段加入前，因此该字段全部缺失，不能
分析 WS/REST 来源占比；原始 NDJSON 不在当前 checkout，OpenObserve 只保留 `source_file`
和事件，无法补算原文件 SHA-256；也没有阶段 1 所需的完整 normalized market/account trace、
fee/funding、markout、时间加权 quote interval 或 place/cancel 生命周期时间点。它们可用于
阶段 0 regime 清单与阶段 1 schema 设计，不得伪装成已经可确定性回放的数据集。

2026-07-10/14 的生产 canary 用于验证认证、命令关联、清理和 fail-closed 行为，不作为策略
绩效基线。它们的市场窗口太短，且实验目标不是采集可回放的市场/account trace。

现有 `cycle_summary` 可提供 cycle、symbol、mark/touch、position、PnL、fills、uptime、
capture、halt/vol 和动作计数；阶段 1 所需而当前缺失的主要字段包括：完整 normalized market
trace、有效配置快照、费用/funding、成交后 1s/5s/30s mark、时间加权 quote intervals，
以及 place/cancel 生命周期时间点。

## 阶段 0 验收进度

- [x] 每个比较 run 可追溯到 `git_sha + config_hash + symbol + time range`；新 paper 校准 run
  的 manifest 与日志哈希复验通过。
- [x] TOML 注释、maker 文档和 live-gate evidence 的已知 tuple 冲突已消除。
- [x] 已生产验证与未验证的 inventory-exit tuple 已分开标注。
- [x] 已明确 `alert_*` 只通知，`stop_loss` fail-safe 停机但不自动平仓。
- [x] 已登记互不重叠的平静、趋势和快速波动窗口，并显式记录 schema/artifact 缺口。
- [x] 未改变报价、退出、PnL、风控或 maker JSON；标准离线验证全部通过。

## 本批验证结果

- `python3 scripts/test_maker_run_manifest.py`：4 个用例通过，覆盖敏感参数排除、完整 trace
  晋级、日志篡改拒绝、缺 cycle/lifecycle/metadata 拒绝和三类 regime 口径。
- 包装器合成日志联调：manifest 正确记录 config/override/program/collector/log 哈希和
  生命周期。该合成日志只验证工具链，不列入三类市场 trace。
- `HOME=/tmp/standx-test-home CARGO_HOME=~/.cargo cargo test --workspace --offline`：通过。
  受限沙箱内 mockito 无法绑定 loopback，允许本机回环后同一离线命令全部通过；没有访问
  生产接口或执行订单。
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过。
- `cargo fmt --all -- --check`：通过。
- `python3 -m py_compile scripts/openobserve_dashboard.py scripts/maker_run_manifest.py
  scripts/test_maker_run_manifest.py`：通过。
- `bash -n scripts/run_maker_observed.sh` 与 `git diff --check`：通过。

阶段 0 已满足以下条件：

1. 生成至少一份完整的新 paper baseline manifest，证明身份字段可复查；
2. 登记三类互不重叠的历史 trace，并把缺失的原始 artifact/schema 字段显式标为不可用；
3. 对新 baseline 计算文件哈希并验证 lifecycle/序号完整性；历史 trace 使用唯一
   `run_id + source_file + event_id` 集合，不虚构当前已不存在的原文件哈希；
4. 通过仓库标准离线验证。

## 阶段 1 交接

1. 回放 schema 必须补齐 normalized market/account events，不能直接把旧 `cycle_summary`
   当成完整输入 trace。
2. 新 trace 继续使用 sidecar manifest；没有可证明 regime 的数据保持 `unclassified`。
3. 先解决/解释当前 paper 校准 run 中高比例 `ws_server_time_skew` fallback，再把新的长窗口
   用作策略绩效比较；这不会阻止阶段 0 身份校准验收。

## 阶段状态模板

```text
阶段：0
状态：accepted
baseline_git_sha：ccdcf3191f206c17dda89105de0ee9346ff563d4
candidate_git_sha：worktree over ccdcf3191f206c17dda89105de0ee9346ff563d4 (strategy source clean)
baseline_config_hash：37a63617b5438d949415eacc26487020cd0a35299ea2f8bddc7b1655ea9d62dd
candidate_config_hash：same (no strategy config change)
训练数据窗口：none（阶段 0 不调参）
样本外验收窗口：N/A（阶段 0 不调参）；已登记 calm / trend / fast_or_stressed 三个历史窗口
专项指标结果：baseline manifest PASS；三类 trace 清单完成；缺口已登记
统一安全门槛结果：no strategy/runtime behavior change
离线验证结果：PASS（workspace test / clippy / fmt / Python compile / shell syntax）
live 授权范围（如无则写 none）：none
release owner 决定：N/A；阶段 0 工程验收通过，不改变 live gate
```
