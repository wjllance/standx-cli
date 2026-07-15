# ADR 0001：Maker 事故恢复的监督架构

- 状态：已采纳（2026-07-15）
- 相关：[13-maker.md](../13-maker.md)、[14-maker-live-gate.md](../14-maker-live-gate.md)、[18-maker-strategy-roadmap.md](../18-maker-strategy-roadmap.md)

## 背景

`standx maker` 的 `run_maker` 主循环有三条事故恢复流程：account-stream 断连、
order-response 断连、position reconciliation。它们曾是彼此的近似拷贝，逐渐语义漂移
——order-response 恢复一度缺少 account-stream 恢复已有的 REST trade backfill
（已由 commit `4a49104` 修复）。

漂移的根因是：**恢复步骤的顺序属于安全策略，却由三份平行的 CLI 过程代码各自维护**。
`standx-maker` 的纯状态机已经保证外层顺序（`Abort → Cleanup → Recover → Resume`），
但每条流程的执行体（冻结清理、REST 收敛、恢复恢复报价）是手写的，没有单一事实源。

已经完成的收口（方案 F + B）：

- core 状态机保证外层次序，并有全故障类型的 conformance 测试钉住；
- CLI 三条流程共用 `freeze_and_cleanup_for_recovery`、`probe_position_convergence`、
  `resume_quoting_after_recovery`；
- freeze 后的 pending 处理差异被建模为
  `OrderResponseContinuity { Preserved, Replaced }`，经
  `MakerAccountProjection::finish_verified_cleanup` 单一入口执行；
- fault matrix 与 fail-closed 不变量已有测试覆盖。

在此基础上评估了两个更进一步的架构方向。

### 方案 C：把恢复次序下沉进 core 状态机

让 core 把恢复拆成细粒度转移（`CleanupPending → TransportRecoveryPending →
BackfillPending → VerificationPending → Ready / Stop`），CLI 退化为薄执行器。
`Verify` 是 core 内部纯判断，`Resume` 是状态转移，不必膨胀成独立 I/O effect。

- 优点：恢复次序本身成为 core 的纯转移逻辑，可直接单测，杜绝过程代码漂移。
- 代价：一次性触碰 core、CLI、effect/result 关联和大量测试；在 F+B 已显著降低漂移
  风险之后，完整迁移的边际收益变小。

### 方案 D：恢复 = 整包重建 order-response（crash-only 式）

任何失信故障都整包替换 order-response session，使 pending 处理表面统一。

- 否决理由：会无差别丢弃仍可送达的 ACK（`Preserved` 语义的存在正说明通道常常还活着）；
  失去「旧 order-response 是否还能交付响应」的健康探针；更依赖 REST 回补；健康流也被
  拆除；position mismatch 的原因在账本、重连传输并不能解决；并且增加双流联合连接与原子
  session 替换的失败面。当前代码也没有任何「双流可信度耦合失效」的检测信号，D 连触发器
  都不存在。

## 决策

1. **保留当前架构**：core 状态机保证外层次序（外框）+ CLI 薄共享执行器执行 I/O。
   不实施完整方案 C，不采用方案 D 作为默认恢复模型。

2. **把 pending 差异建模为 order-response continuity**：`Preserved`（通道存活，保留
   ACK 关联与 deadline）与 `Replaced`（通道被替换，结束旧 ACK 义务）。两者都必须关闭
   已验证为空的 venue slot——该不变量集中在 `finish_verified_cleanup` 一处。

3. **方案 C 的触发规则（本 ADR 的核心约定）**：出现下列**任一**情形时，直接迁移到
   C-lite，不再往 CLI 增加第四条恢复过程分支：

   - 需要新增一个恢复**阶段**（例如在 cleanup 与 recover 之间插入新的中间状态）；
   - 需要新增一条恢复**重试分支**或新的恢复**目标**（第四种 `RecoveryTarget`）；
   - 发生**任何一次新的次序漂移事故**（两条流程在某个共享步骤上再次分叉）。

   在触发之前，恢复架构**冻结**：只做 bug 修复和等价重构，不做结构扩张。

4. **方案 D 的触发条件**：仅当有证据表明两个 stream 的可信度已经**耦合失效**
   （而非单流断开或单纯仓位失配）时，才将 D 作为升级恢复手段引入；且需先补上检测该
   耦合失效的信号。在此之前 D 不预留结构。

## 后果

- **正面**：三条流程只剩一份执行体；恢复决策（continuity）显式且可单测；次序不变量由
  core + conformance 测试守护；后续复杂度增长有明确的、写入代码库的迁移触发点，不再依赖
  口头共识。
- **负面 / 需承担**：外框在 core、执行体在 CLI 的分工仍然存在——次序保证与执行分处两个
  crate。这是刻意的取舍：换取不必一次性重写一个已在跑生产的资金系统。触发规则把这笔债的
  偿还时机绑定到「真正需要扩张时」。
- **落实**：本规则应在 review 中据以拦截「第四条过程分支」类改动；相关 PR 若命中上述任一
  触发条件，应引用本 ADR 并转向 C-lite 设计。
