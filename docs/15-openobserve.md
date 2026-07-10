# OpenObserve maker 日志

这是面向单机、小规模 maker 的最小日志方案。maker 的 stdout 先完整写入本地
NDJSON，stderr 单独保存；启用自动上传后，运行期间按 checkpoint 增量上传，退出时
再做一次最终补传。远端不可用时，原始日志仍保留，上传错误不会改变 maker 的退出
状态，也不会进入报价循环。

## 1. 启动本地 OpenObserve

```bash
cp deploy/openobserve/.env.example deploy/openobserve/.env
# 修改 OPENOBSERVE_PASSWORD，保留大小写字母、数字和特殊字符
make openobserve-up
```

管理界面：<http://127.0.0.1:5080>。Compose 只绑定 loopback，数据保存在
`deploy/openobserve/data/`，该目录和 `.env` 均不进入 Git。

停止或查看服务日志：

```bash
make openobserve-logs
make openobserve-down
```

## 2. 运行并采集 maker

先把 OpenObserve 环境变量导入当前 shell：

```bash
set -a
source deploy/openobserve/.env
set +a
export OPENOBSERVE_AUTO_UPLOAD=1
# 可选，默认每 2 秒上传一次新增事件
export OPENOBSERVE_UPLOAD_INTERVAL=2
```

纸面示例：

```bash
scripts/run_maker_observed.sh \
  target/debug/standx --output json maker run XAG-USD \
  --maker-config examples/maker-xag-100u.toml
```

包装器会：

1. 为每次运行生成唯一 `run_id`。
2. 将 stdout 写入 `var/standx/<run_id>.ndjson`。
3. 将 stderr 写入 `var/standx/<run_id>.stderr.log`，同时保留终端显示。
4. 转发 Ctrl+C/TERM 给 maker，并等待 lifecycle/cleanup 日志写完。
5. 若 `OPENOBSERVE_AUTO_UPLOAD=1`，启动时验证 OpenObserve 连接，运行期间持续增量上传。
6. maker 退出后等待 lifecycle/cleanup 落盘，再做最终补传。

终端看到以下信息表示启动检查和实时上传正常：

```text
OpenObserve live uploader starting: run_id=... interval=2s
OpenObserve preflight ok: org=default stream=standx_maker
OpenObserve live upload: run_id=... uploaded=... checkpoint=...
```

上传失败只会输出 warning；checkpoint 不前移，网络恢复后自动补传，本地 maker 不停机。
Dashboard 页面需要选择包含当前时间的范围，并把自动刷新设为 `5s`（或手动点击刷新），
即可在 maker 运行期间看到新增周期、成交和告警。

实盘仍需显式 `--live --yes`，采集脚本不会自行开启实盘，也不会读取或上传
`STANDX_JWT`、`STANDX_PRIVATE_KEY`。OpenObserve 密码通过环境变量传入，不出现在
命令参数或日志中。

## 3. 导入已有日志

先离线验证；非 JSON 行会被统计并跳过：

```bash
python3 scripts/openobserve_ingest.py --dry-run \
  docs/evidence/logs/maker-paper-20260710T0705Z.log
```

确认后上传：

```bash
python3 scripts/openobserve_ingest.py \
  docs/evidence/logs/maker-paper-20260710T0705Z.log
```

上传器按 500 条批量写入，失败自动重试，并在
`var/standx/openobserve-uploaded.json` 保存逐行 checkpoint。每条记录会补充：

- `schema_version=maker_event_v1`
- `run_id`
- `event_id`（文件哈希与行号生成）
- `source_file`
- `_timestamp`
- `git_sha`、`config_hash`（包装器可获取时）

实时模式使用稳定的 `run_id + 行号` 生成 `event_id`，文件增长不会改变已经上传事件的
身份，正常反复扫描不会重复发送。若 HTTP 已写入但响应丢失，at-least-once 重试仍可能
产生相同 `event_id` 的重复行，分析查询应继续使用 `count(DISTINCT event_id)`。手工导入
已结束的不可变日志仍保持原有的文件哈希 checkpoint 语义。

JWT、private key、token、password、authorization、webhook 等字段在上传前会递归
替换为 `[REDACTED]`。原始本地文件不被修改。

## 4. 第一组分析查询

在 OpenObserve Logs 中选择 `standx_maker` stream：

```sql
SELECT
  run_id,
  symbol,
  max(fills_total) AS fills,
  min(pnl) AS min_pnl,
  max(pnl) AS max_pnl,
  avg(uptime_pct) AS avg_uptime,
  sum(CASE WHEN halted THEN 1 ELSE 0 END) AS halted_cycles
FROM "standx_maker"
WHERE action = 'cycle_summary'
GROUP BY run_id, symbol
ORDER BY run_id DESC;
```

撤单原因分布：

```sql
SELECT run_id, reason, count(DISTINCT event_id) AS events
FROM "standx_maker"
WHERE action = 'cancel'
GROUP BY run_id, reason
ORDER BY events DESC;
```

分析时以 `event_id` 去重；外部预置仓位不属于 maker-correlated fill ledger，不能把
对应会话的 maker PnL 当成账户真实盈亏。

同一查询也可以从终端只读执行：

```bash
python3 scripts/openobserve_query.py --hours 24
```

## 5. 创建或刷新 maker Dashboard

创建原生 `StandX Maker Overview` Dashboard；如果同名 Dashboard 已存在，则更新它：

```bash
set -a
source deploy/openobserve/.env
set +a
python3 scripts/openobserve_dashboard.py
```

Dashboard 默认选择最新 maker run，包含 `Overview` 和 `Runs & Events` 两个页签。
PnL 面板展示 maker session 自身的 PnL；reduce-only exit 演练所用的外部预置库存，
通过 Inventory 和 Events 面板单独判断。

## 6. 安全边界

- OpenObserve 仅监听 `127.0.0.1:5080`，不要直接暴露公网。
- `deploy/openobserve/.env` 使用独立密码，不复用交易所凭证。
- stdout 才进入分析 stream；stderr 只本地保存，避免诊断噪声污染事件表。
- verbose WebSocket 日志不得包含认证载荷；SDK 只记录认证动作和 stream 数量。
- 先落盘、后上传是有意设计：日志后端绝不能给 maker 施加背压。
