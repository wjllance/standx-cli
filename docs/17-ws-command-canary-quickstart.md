# WS 生产 Canary 快速启动

用于验证认证 WebSocket 的 `order:new` / `order:cancel` 生产链路。命令会真实提交一笔
最小数量的 post-only 买单，确认交易所接受并可通过 REST 查询后立即撤单。

> 这是生产交易操作。仅在操作员在线、具备手工紧急撤单能力并明确授权后执行。

## 1. 配置通知

在仓库根目录创建 `.env.local`：

```bash
STANDX_SUPERVISOR_WEBHOOK=https://your-webhook-url
STANDX_SUPERVISOR_WEBHOOK_FORMAT=feishu
```

格式可选 `slack`、`feishu`、`telegram` 或 `raw`。保护文件并确认它不会进入 Git：

```bash
chmod 600 .env.local
git check-ignore .env.local
```

CLI 仅在运行 `ws-command-canary` 时自动加载这两个字段。优先级为：命令行参数 >
进程环境变量 > `.env.local`。

## 2. 构建与认证

```bash
cargo build -p standx-cli --offline
target/debug/standx auth status
```

凭证必须有效并包含签名私钥。

## 3. 执行

以 `XAG-USD` 为例：

```bash
STANDX_ENABLE_LIVE_MAKER=1 \
  target/debug/standx --output json maker ws-command-canary XAG-USD
```

默认行为：

- 使用交易所最小下单数量；
- 买价放在 mark 下方 100 bps；
- 每个 WS 回报和 REST 校验最多等待 10 秒；
- 启动前要求该交易对无挂单且仓位为零。

可通过 `--size`、`--price-offset-bps` 和 `--timeout-secs` 显式调整边界。

## 4. 成功判据

输出应依次包含：

```text
preflight_verified
create_submitted
create_accepted
order_visible
cancel_submitted
cancel_accepted
absence_verified
position_verified
```

最后应出现 `lifecycle event=completed`，且 `position_verified` 的 `position` 为 `0.0`。
建议再做一次独立后检：

```bash
target/debug/standx --output json account orders --symbol XAG-USD
target/debug/standx --output json account positions --symbol XAG-USD
```

两条命令都应返回 `[]`。

## 失败处理

任一步骤失败都会停止后续动作并尝试通过 REST 清理当前 canary 订单。若最终仓位非零，
命令会 fail-safe 退出，但不会自动平仓；此时必须由在线操作员按应急流程处理。

完整门槛与证据要求见 [14-maker-live-gate.md](14-maker-live-gate.md)。
