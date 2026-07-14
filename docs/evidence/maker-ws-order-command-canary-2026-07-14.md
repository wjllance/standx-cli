# 2026-07-14 WS order-command controlled canary

Runtime commit: `a5af9b7` (`Correlate maker cancel acknowledgements`). This
record covers the change that sends normal live maker `order:new` and
`order:cancel` commands over the authenticated order-response WebSocket while
retaining HTTP for snapshots, cleanup, and recovery.

## Authorization and boundary

- Operator authorization was explicitly confirmed in the task.
- Symbol: `BTC-USD`; pre-check returned `orders=[]` and `positions=[]`.
- Two runs used one level, `size=0.001`, `max_position=0.001`, no inventory
  exit, a Feishu risk webhook, and the supervised
  `--controlled-disconnect-after 15` fault.
- No credential or webhook value is recorded here.

## Observations

| Run | Window (UTC) | WS command evidence | Outcome |
| --- | --- | --- | --- |
| 1 | 04:35:40–04:36:01 | Two initial live maker placements; zero fills | Controlled response-stream fault froze placements, HTTP cleanup reported zero remaining maker orders twice, then fail-safe stopped. |
| 2 | 04:37:02–04:37:25 | Two initial live maker placements; zero fills | Same controlled fault and cleanup result. `--refresh-bps 0.1` was used to seek a normal cancellation. |

Both runs submitted the initial live maker placements through the authenticated
command stream. The account and order-response streams reached WebSocket-live
state before the injected fault. This run did not retain a correlated venue
acceptance response or an in-flight open-order snapshot, so it is evidence of
production command submission and cleanup, not yet of venue acceptance.
Structured warning and critical `risk_notification` events were emitted for
freeze, reconnect-unavailable, and final fail-safe.

The fault is deliberately classified as a fail-safe shutdown, so the process
exit code `75` is expected for these runs rather than a test failure.

## Deterministic protocol coverage

`order_response::tests::command_sender_writes_signed_cancel_and_delivers_correlated_response`
uses a local loopback WebSocket server to assert that `order:cancel` carries a
signed envelope, the documented `{ "order_id": 42 }` JSON params, and a
correlated accepted response. This is repeatable protocol evidence; it does
not replace the production normal-cancellation evidence below.

## Post-check and limitation

After each run, independent production queries returned `orders=[]` and
`positions=[]`. No fill, position change, or maker-order residue occurred.

The market mark did not move during the short windows, including the second
run, so neither run produced a normal re-quote. Therefore **correlated venue
acceptance and production evidence for normal WebSocket `order:cancel` remain
pending**. This record does not unlock the live gate for the changed command
path. A future canary must retain an in-flight order snapshot and correlated
acceptance response, then obtain a bounded normal cancellation (or use a
reviewed deterministic venue-safe cancellation harness) before repeating the
empty-order and empty-position post-check.

## Harness hardening after this record

The hidden `maker ws-command-canary` harness now emits a structured JSON chain
with `action=ws_command_canary` for preflight, create submission and correlated
acceptance/rejection, REST order visibility, cancel submission and correlated
acceptance/rejection, REST absence, and final flat-position verification. Each
applicable event carries the client-order ID, request ID, venue order ID,
response code, quantity, price, and preflight/post-check position. This evidence
is printed in every output mode so a supervised run can be retained without
depending on terminal prose.

If any step fails, the harness records the start and result of compensating
HTTP cleanup and exits fail-safe. A non-zero final position is recorded as a
position mismatch; the harness does not flatten it automatically.

This is an implementation and offline-test update only. No additional
production command, cancellation, disconnect, or position change was executed,
so the pending limitation and locked decision above remain unchanged.

## XAG-USD correlated command canary

A separately authorized supervised run used the hardened harness after the
offline workspace checks passed.

- Window: `2026-07-14T12:24:47.785Z`–`2026-07-14T12:24:51.898Z`.
- Symbol and bounds: `XAG-USD`, venue-minimum quantity `0.001`, post-only buy
  price `57.35` (100 bps below the observed mark calculation).
- Client-order ID: `sxmk-canary-a041fedd9a53`.
- Create request `c9d50ec7-7dc2-4147-8356-3bac8386d1bf` received code `0`
  (`success`). REST then observed venue order `11545819148` with the expected
  client-order ID.
- Cancel request `9ca9e810-214b-4c3c-aba5-6b8b032528e4` received code `0`
  (`success`). REST then verified the client order was absent.
- The harness verified final `XAG-USD` position `0.0` and exited successfully.
  Independent REST post-checks immediately afterward returned `orders=[]` and
  `positions=[]`.

This run supplies the previously missing correlated production acceptance,
normal WS cancellation, in-flight REST visibility, empty-book verification,
and flat-position evidence for the changed command path. It does not by itself
record a release-owner decision to unlock the re-locked live gate.
