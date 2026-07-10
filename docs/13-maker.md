# 13 - 做市机器人（Maker Bot）

本文档介绍 `standx maker` —— 面向 [SIP-5A 社区做市收益](https://docs.standx.com/sip/sip-5a-community-maker-yield)的双边报价机器人。

SIP-5A 按**在线时长（uptime）**、**报价贴近 mark 价（须在合格带内）**和**深度**奖励做市商，并惩罚闪撤（flicker-cancel）与短命报价。因此本机器人的核心是 **anti-flicker 循环**：只要报价还在合格带内就一直挂着，仅当 mark 价相对下单时漂移超过阈值才重新报价。

---

## 前置条件

- **Paper（模拟）模式**：无需认证，只读公共行情，不下任何真实单。默认即此模式。
- **Live（实盘）模式**：需要 JWT + Ed25519 私钥（参考 [02-authentication.md](02-authentication.md)），且当前锁定在环境变量 `STANDX_ENABLE_LIVE_MAKER=1` 之后（见 [13.5](#135-live-实盘模式)）。

---

## 13.1 快速开始

```bash
# Paper 模式（默认）：跑完整循环、打印将要执行的动作，但不下真实单。
# 成交会被模拟（touch 穿过报价即视为成交），所以仓位与库存 skew 可观测。
standx maker run BTC-USD --size 0.001 --interval 3
```

**预期输出（表格模式）：**
```
┌──────────────────────────────────────────────────────────┐
│ standx maker — PAPER mode on BTC-USD
│ spread 5bps | band 20bps | refresh 3bps | 1 level(s)
│ size 0.001 | max-position 0.05 | interval 3s
│ ticks: price 2dp, qty 4dp | min qty 0.0001
│ paper mode: no real orders; fills are simulated when the
│ touch crosses a quote, so position & skew move. --live for real.
│ feed: websocket (REST fallback) | divergence guard 25bps
│ Ctrl+C to stop (cancels all resting orders on exit)
└──────────────────────────────────────────────────────────┘
[12:00:00] #0 mark=62000.00 bid=61999.00 ask=62001.00 pos=0.0000 pnl=0.00 | hold=0 place=2 cancel=0
    PLACE  buy  L0 @ 61969.00 x 0.0010
    PLACE  sell L0 @ 62031.00 x 0.0010
[12:00:03] #1 mark=62001.00 bid=62000.00 ask=62002.00 pos=0.0000 pnl=0.00 | hold=2 place=0 cancel=0
    HOLD   buy  L0 @ 61969.00 (age 1 cycles, drift 0.2bps)
    HOLD   sell L0 @ 62031.00 (age 1 cycles, drift 0.2bps)
```

按 `Ctrl+C` 退出，机器人会撤掉所有挂单并打印本次会话的统计。

---

## 13.2 策略参数

```bash
standx maker run <SYMBOL> [OPTIONS]
```

| 参数 | 默认 | 说明 |
|------|------|------|
| `<SYMBOL>` | — | 交易对，如 `BTC-USD`（必填） |
| `--spread-bps` | `5` | 距 mark 价的半价差（bps）：L0 买 = mark×(1−spread)，卖 = mark×(1+spread) |
| `--band-bps` | `20` | 合格带守卫：绝不报到 mark ± band 之外（须 > spread） |
| `--size` | `0.01` | 每侧每档数量（取整后须 ≥ 交易对最小下单量） |
| `--levels` | `1` | 每侧报价档数 |
| `--level-step-bps` | `2` | 档间距（bps，`--levels > 1` 时生效） |
| `--refresh-bps` | `3` | anti-flicker：报价中心相对下单时漂移超过此值才重报 |
| `-i, --interval` | `5` | 循环间隔（秒） |
| `--max-position` | `0.05` | 最大绝对持仓；会把继续加仓的一侧压制掉 |
| `--skew-bps` | `0` | 库存 skew：满仓时把报价中心向减仓侧偏移的最大幅度（bps），0 关闭。见 [13.3](#库存-skew) |
| `--inventory-exit-pct` | `0` | 主动减仓触发线：仓位达到 `--max-position` 的此百分比时启动退出流程；需同时设置 `--inventory-exit-qty`，0 关闭 |
| `--inventory-exit-qty` | `0` | 单次 reduce-only 主动退出的最大数量；先确认 maker 空簿再下市价单，提交后未确认会 fail-safe，0 关闭 |
| `--max-divergence-bps` | `25` | 当 mark 价与盘口中价背离超过此值时跳过该轮（不动挂单） |
| `--vol-pause-bps` | `0` | 波动率熔断：mark 在 `--vol-window` 轮内的极差达到此值（bps）即撤掉全部报价暂停，回落到一半以下才恢复。0 关闭。见 [13.3](#波动率熔断) |
| `--vol-window` | `12` | 波动率熔断测量极差的窗口（最近 N 轮） |
| `--alert-loss` | `0` | 风险告警：mark-to-market PnL 跌到 −此值（计价单位）时告警。0 关闭 |
| `--alert-inventory-pct` | `0` | 风险告警：\|仓位\| 达到 `--max-position` 的此百分比时告警。0 关闭 |
| `--alert-uptime` | `0` | 风险告警：双边 uptime 跌破此百分比时告警（过预热期后）。0 关闭 |
| `--alert-webhook` | 无 | 除 stderr/JSON 外，把告警 POST 到此 URL |
| `--alert-webhook-format` | `slack` | webhook 报文格式：`slack` / `feishu` / `telegram` / `raw` |
| `--no-ws` | 关 | 禁用 WebSocket 行情，改为每轮 REST 轮询 |
| `--live` | 关 | 下真实单（不带此标志即 paper 模式） |

启动时会做快速校验（fail fast）：交易对存在且在交易中、`spread-bps > 0`、`band-bps > spread-bps`、`size` 取整后 ≥ 最小下单量、`skew-bps ≥ 0`；主动退出必须同时设置百分比与数量。

### 专用配置文件

默认会读取 `~/.config/standx/maker.toml`（macOS 为 `~/Library/Application Support/standx/maker.toml`）；也可用 `--maker-config <PATH>` 指定文件。文件只保存非敏感策略参数，命令行显式参数优先于文件，文件未设置的字段继续使用内置默认值。

完整可复制模板见 [`examples/maker.toml`](../examples/maker.toml)。

```toml
# maker.toml — 不放 JWT、私钥、--live 或 webhook URL
spread_bps = 8.0
band_bps = 30.0
size = 0.001
levels = 2
level_step_bps = 2.0
refresh_bps = 4.0
interval = 5
max_position = 0.01
skew_bps = 6.0
max_divergence_bps = 25.0
vol_pause_bps = 40.0
vol_window = 12
alert_inventory_pct = 80.0
no_ws = false
```

```bash
# 文件值生效
standx maker run BTC-USD

# 本次运行覆盖文件中的 size
standx maker run BTC-USD --size 0.002

# 使用另一套策略文件
standx maker run ETH-USD --maker-config ./configs/eth-maker.toml
```

---

## 13.3 工作原理

### Anti-flicker reconcile

每一轮，机器人对比"期望报价"与"当前挂单"，按以下决策表逐条处理每个挂单（顺序即优先级）：

| # | 条件 | 动作 |
|---|------|------|
| 1 | 该侧被 max-position 压制 | 撤单（side_suppressed） |
| 2 | 该 (side, level) 已无期望报价 | 撤单（stale） |
| 3 | 挂单价出了当前合格带 | 撤单（outside_band） |
| 4 | 挂单价穿过当前 touch | 撤单（would_cross） |
| 5 | 报价中心相对下单时漂移 > refresh-bps | 撤单（mark_moved） |
| 6 | 以上都不满足 | **保持（HOLD）** |

保持是关键：只要还在带内、未穿价、漂移未超阈值，就不动它 —— 这正是 SIP-5A 奖励的 uptime。

### 库存 skew

被动做市有天然逆向选择：买单只在下跌时成交、卖单只在上涨时成交，库存会往亏损方向累积。`--skew-bps` 把报价中心按当前仓位偏移：

```
center = mark × (1 − skew_bps × clamp(position / max_position, ±1) / 1e4)
```

持多头时中心下移 → 减仓侧（卖）更贴近 mark（更易成交）、加仓侧（买）更远（更难成交）；持空头相反。这把 `max-position` 从"急刹车"变成"渐进回中"。anti-flicker 的锚点也是这个中心，所以同一条重报规则同时响应 mark 漂移与库存 skew。

> **注意**：paper 模式下 skew 只有在模拟成交累积出仓位后才生效；`--skew-bps 0`（默认）时行为与不带 skew 完全一致。

### 主动库存退出

`--inventory-exit-pct 80 --inventory-exit-qty 0.01` 表示仓位达到上限的 80% 后，先撤销 maker 自有报价；在下一轮确认 maker 空簿且没有待确认下单时，再提交一笔最大 `0.01` 的 reduce-only 市价单。多头只卖出、空头只买入，数量不会超过当前仓位。该功能只在 live 模式生效，默认关闭。波动熔断期间只撤单、不发送主动市价退出；每笔退出必须由 `sxmk-exit-` 关联成交进入账本后才能允许下一笔 chunk，未确认时策略 fail-safe，不会重复追单。

### 行情来源与守卫

- **WebSocket feed**：价格与深度走同一条公共连接；缓存超过 5 秒未更新时自动回退到 REST（覆盖预热、断线、`--no-ws`）。
- **早醒重报**：循环在 sleep 期间若发现 mark 已漂过 `--refresh-bps`，会提前进入下一轮，缩短暴露窗口而不增加闪撤（仅在本来就要重报时才早醒）。
- **mark/mid 背离守卫**：mark 价与盘口中价背离超过 `--max-divergence-bps` 时，本轮不做任何动作（不撤不挂），避免在数据源打架时误动作。

### 波动率熔断

快速行情里被动做市最容易被"扫单"（逆向选择）。`--vol-pause-bps` 开启后，机器人跟踪 mark 在最近 `--vol-window` 轮内的极差（(max−min)/min，bps）：

- 极差 **达到 `--vol-pause-bps`** → **熔断**：撤掉全部挂单、暂停报价（`⚡HALT`）。
- 极差 **回落到阈值一半以下** → 恢复报价。

采用滞回（hysteresis）：大幅波动必须先滚出窗口、极差降到一半以下才恢复，避免在阈值附近反复开关。熔断期间不报价 = 主动放弃这段 uptime，换取不被扫单——这是有意的取舍。`--vol-pause-bps 0`（默认）关闭。

> 权衡：熔断会牺牲 SIP-5A 的 uptime 得分。阈值要结合 [13.4](#134-输出与遥测) 的遥测来定——熔断太频繁会拉低 uptime，太松则起不到保护。mark 价通常比盘口 touch 更"黏"，据此设阈值。

---

## 13.4 输出与遥测

三种输出格式：

- **表格（默认）**：每轮一行 `[时间] #轮次 mark= bid= ask= pos= pnl= | hold= place= cancel=`，其下缩进列出 PLACE / CANCEL / HOLD / FILL 明细。
- **JSON（`--output json` 或 `--openclaw`）**：每个动作一行 JSON；每轮末尾一条 `cycle_summary`，含 `position`、`pnl`、`fills_total`、`uptime_pct`、`avg_capture_bps`、`halted`、`vol_bps`。
- **Quiet（`--quiet`）**：只打印成交与增删挂单。

退出时打印本次会话统计：

```
👋 Stopping maker (ran 120 cycles: 40 places, 38 cancels, 210 holds)
   6 fills | uptime 92% | max pos 0.0030 | avg capture 1.7bps | PnL +0.42 (mark-to-market)
   paper sim: ending position 0.0010
```

**遥测指标含义**：

| 指标 | 含义 |
|------|------|
| PnL（mark-to-market） | 已实现现金 + 持仓按 mark 计价，一个数同时体现点差捕获与库存盈亏 |
| avg capture (bps) | 每笔成交在赚钱方向上离 mark 的平均距离 —— 做市 edge |
| uptime % | 同时挂着买卖单的周期占比 —— SIP-5A 真正奖励的东西 |
| fills / max pos | 成交笔数、会话内最大绝对持仓 |

> paper 用精确模拟成交计算；live 从认证成交历史读取本次会话内、已关联到 `sxmk-` maker 订单的成交，并按成交 ID 去重。不会把手工/API 订单或单纯仓位变化当作 maker 成交。当前 mark-to-market 仍基于成交现金流与仓位估值；费用币种和交易所已实现盈亏会单列核对。**调参在 paper 里做**：观察 avg capture 与 PnL 的关系来调 `--spread-bps` / `--refresh-bps` / `--skew-bps`。

### 风险告警

遥测默认只**展示**指标；风险告警把它变成**主动通知**。三个阈值各自 opt-in（0 关闭）：

- `--alert-loss <X>`：mark-to-market PnL 跌到 −X 时告警（回升到 −X/2 以上解除）。
- `--alert-inventory-pct <P>`：\|仓位\| 达到 `--max-position` 的 P% 时告警（跌回 0.9×P% 以下解除）——趋势市里最先亮的灯。
- `--alert-uptime <U>`：双边 uptime 跌破 U% 时告警（前 20 轮预热期不判）。

告警是**边沿触发**的：进入异常态时响一次、恢复时响一次，不会每轮刷屏。输出到 stderr（`🚨 ALERT` / `✅ RESOLVED`）与 JSON（`action:"alert"`）；若设了 `--alert-webhook <url>`，还会 POST 一条 JSON（fire-and-forget，慢/坏的端点不会拖住报价循环）。

`--alert-webhook-format` 按目标平台组织报文:

| 格式 | 报文 | 用法 |
|------|------|------|
| `slack`(默认) | `{"text":"..."}` | Slack incoming webhook |
| `feishu` | `{"msg_type":"text","content":{"text":"..."}}` | 飞书/Lark 自定义机器人 |
| `telegram` | `{"text":"..."}` | Telegram sendMessage,token 和 chat_id 放 URL 里:`https://api.telegram.org/bot<TOKEN>/sendMessage?chat_id=<ID>` |
| `raw` | 完整结构化对象(text + ts/symbol/kind/firing/message) | 自定义消费端 |

```bash
# 飞书
standx maker run BTC-USD --alert-loss 50 --alert-inventory-pct 80 \
  --alert-webhook <飞书机器人地址> --alert-webhook-format feishu

# Telegram
standx maker run BTC-USD --alert-inventory-pct 80 \
  --alert-webhook "https://api.telegram.org/bot<TOKEN>/sendMessage?chat_id=<ID>" \
  --alert-webhook-format telegram
```

> 注意:这些覆盖的是**金融风险**;基础设施类告警(feed 断线、连续错误 fail-safe、退出残留单)是内置的,始终开启。

**启动/停止通知**:只要设了 `--alert-webhook`,机器人还会在启动时推一条 🟢(含模式与关键参数)、退出时推一条 🔴(含停止原因——Ctrl+C 或 fail-safe——及本次会话汇总)。**不需要**配任何风险阈值即可生效;停止消息会等待投递完成再退出进程,确保送达。JSON 模式下同时输出 `action:"lifecycle"` 行。

---

## 13.5 Live（实盘）模式

> ⚠️ **风险提示**：live 模式会下真实的 post-only（ALO）订单。它已实现但**尚未经过生产环境的监督测试**，因此锁定在环境变量之后。

```bash
export STANDX_ENABLE_LIVE_MAKER=1        # 解锁（自行承担风险）
standx maker run BTC-USD --size 0.0001 --max-position 0.001 --live
```

live 模式的安全栏：

- **启动即 cancel-all**：从干净的盘口开始，避免上一轮残留干扰对账。
- **机器人接管该交易对的全部挂单**：手动挂的单会被当作 stale 撤掉。
- **拒单容错**：post-only 穿价拒单、撤已成交单等属正常事件（记录后下轮重报），不计入 fail-safe；只有网络/5xx 等瞬时故障才计数。
- **部分成交容忍**:部分成交的挂单保留剩余部分继续挂,不会被误撤。
- **fail-safe 停机**:连续 3 次瞬时错误即停机并清理。
- **退出必清理**:所有退出路径都会 cancel-all(3 次重试 + 校验),有残留会大字告警并给出手动撤单命令。

---

## 13.6 使用示例

```bash
# 多档报价(每侧 3 档,档间距 1bps)
standx maker run BTC-USD --levels 3 --level-step-bps 1 --size 0.001

# 开启库存 skew,并用 JSON 输出喂给 agent / 日志管道
standx maker run ETH-USD --skew-bps 5 --output json

# 强制 REST 轮询(不用 WebSocket)
standx maker run BTC-USD --no-ws --interval 5

# 全局 --dry-run:只打印说明,不进入循环
standx --dry-run maker run BTC-USD
```

---

## 13.7 测试检查清单

### Paper 模式(零风险,只读公共 API)

- [ ] `standx maker run BTC-USD --size 0.001 --interval 3` 首轮两侧各下一单
- [ ] 平静期出现 HOLD,age 递增
- [ ] mark 漂过 refresh-bps 时出现 CANCEL(mark_moved)+PLACE
- [ ] 盘口穿过某侧挂单时,只撤违规一侧(would_cross)
- [ ] `--output json` 每行都是合法 JSON,含 cycle_summary
- [ ] touch 穿过报价时出现 FILL,pos 与 pnl 变化
- [ ] `--vol-pause-bps` 设很小(如 2)时,波动中出现 `⚡HALT`、全部挂单被撤、平静后恢复
- [ ] `--alert-inventory-pct 50`(配小 `--max-position`)时,仓位过半出现 `🚨 ALERT`,只响一次;`--alert-webhook` 指向本地监听器能收到 POST
- [ ] `Ctrl+C` 退出时打印统计并(live)清理挂单

### 参数校验

- [ ] `--band-bps` ≤ `--spread-bps` 时报错
- [ ] `--size` 小于最小下单量时报错
- [ ] `--skew-bps` 为负时报错
- [ ] 未知交易对时报错并列出可用交易对

### Live 模式(需 `STANDX_ENABLE_LIVE_MAKER=1` + 认证)

- [ ] 不设环境变量时 `--live` 报 "live mode not yet enabled"
- [ ] 解锁后小额观测:ALO 拒单行为、启动 cancel-all、退出清理

---

## 13.8 常见问题

### Q: paper 模式为什么 PnL / skew 一直是 0?

paper 只有在模拟成交累积出仓位后才有非零仓位与 skew。若市场平静、touch 从未穿过你的报价,就不会有成交。用更紧的 `--spread-bps` 或更长运行时间即可观察到。

### Q: `--live` 报 "live mode not yet enabled"?

live 模式当前锁定,需要 `export STANDX_ENABLE_LIVE_MAKER=1` 解锁,并确保已 `standx auth login`(含私钥)。

### Q: 如何选择 `--spread-bps` / `--refresh-bps` / `--skew-bps`?

在 paper 里跑,观察退出统计的 **avg capture(点差捕获)** 与 **PnL**:capture 为正说明在赚点差,PnL 为负说明库存风险压过了点差 —— 此时应收紧仓位或调大 skew。

---

## 下一步

- 认证配置:[02-authentication.md](02-authentication.md)
- 输出格式:[09-output-formats.md](09-output-formats.md)
- 下单/撤单基础:[05-orders.md](05-orders.md)
