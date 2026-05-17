# Active Account Routing 维护契约

> 本文档记录 `active-account-routing` 分支的产品需求、设计边界和维护约束。后续合并官方上游代码时，必须优先保护这里定义的行为，避免被官方默认账号轮换逻辑覆盖。

## 背景

本仓库基于官方 `Codex-Manager` 分支维护，但本分支有一项本地定制需求：账号路由不能每次请求都自由轮换，也不能按 conversation、prompt cache key、model 等维度做复杂绑定。

用户的真实需求很明确：

1. **同一个 API Key 活跃时尽量固定使用同一个 ChatGPT 账号**，最大化 prompt cache / 上游缓存命中。
2. **不要浪费 weekly 额度**，尤其避免某个账号 weekly 快到期还剩很多没用，而另一个账号已经用光。
3. **外部调用失败后由外部调用者自己重试**。CodexManager 不要在同一次外部请求里静默切换到其他账号。
4. **连续真实失败达到阈值后，下一次外部请求才换账号**。
5. 用户主动中断、客户端断开、Hermes interrupt、stream interrupted 不能被当作账号失败。

这套逻辑是本地 fork 的核心行为，不是官方默认逻辑。

---

## 核心模型

本分支使用“每 API Key 一个 active account”的模型：

```text
key_id -> active_account_id
```

示例：

```text
API Key A -> account_1
API Key B -> account_2
API Key C -> account_1
```

不同 API Key 可以固定到不同账号。账号的 5h / weekly usage 仍然是账号级别共享的，但 active account 状态按 API Key 独立维护。

新增表：

```sql
CREATE TABLE IF NOT EXISTS api_key_active_accounts (
  key_id TEXT PRIMARY KEY,
  active_account_id TEXT NOT NULL,
  active_started_at INTEGER NOT NULL,
  last_used_at INTEGER NOT NULL,
  consecutive_real_errors INTEGER NOT NULL DEFAULT 0,
  last_switch_reason TEXT,
  updated_at INTEGER NOT NULL
);
```

字段含义：

- `key_id`: 内部 API Key ID，不是 raw API key。
- `active_account_id`: 当前 API Key 固定使用的账号。
- `active_started_at`: 本轮固定账号开始时间。
- `last_used_at`: 最近一次成功或正常使用时间。
- `consecutive_real_errors`: 当前 active account 的连续真实错误次数。
- `last_switch_reason`: 最近选择、清除或错误原因，用于排查。
- `updated_at`: 记录更新时间。

---

## 必须保持的行为

### 1. 单次外部请求只尝试当前 active account

这是最重要的维护约束。

当某个 `key_id` 已选出 `active_account_id = A` 后，一次外部请求只能尝试 A。

正确行为：

```text
请求 1 -> 只打 A -> 失败 -> 返回失败给外部调用者
请求 2 -> 只打 A -> 失败 -> 返回失败给外部调用者
请求 3 -> 只打 A -> 失败 -> 达到阈值，清除 active_account_id，仍返回失败
请求 4 -> active_account_id 为空 -> 重新选择 B -> 打 B
```

错误行为：

```text
请求 1 -> 打 A -> A 失败 -> 同一次请求内静默切到 B
```

这个错误行为会破坏缓存最大化，也会让外部调用者以为“同一次请求重试成功”，但实际已经换了账号。

因此，active account 选中后，应将候选列表固定为单账号。例如：

```rust
rotate_to_account(candidates, selected_account_id);
candidates.truncate(1);
```

后续合并官方代码时，不能把这里改回“保留完整候选列表并 failover 到下一个账号”。

### 2. 不做内部静默 retry 三次

“重试三次”的含义不是单次请求内部 retry。

正确含义是：

```text
外部请求失败一次 -> consecutive_real_errors += 1
外部调用者再次请求 -> 仍用同一个 active account
连续真实失败达到 3 次 -> 清 active account
下一次外部请求 -> 重新选择账号
```

本分支应保留：

```text
MAX_CONSECUTIVE_REAL_ERRORS = 3
```

不要重新引入类似：

```text
MAX_SAME_ACCOUNT_TRANSIENT_ATTEMPTS = 3
```

这类名字容易误导 AI 以为要在同一次请求里静默 retry。

### 3. Active account 优先于普通 route strategy

普通 route strategy、balanced round-robin、health P2C、candidate sort 等只能作为 active account 选择前的候选来源或兜底。

请求路径应保持：

```text
collect candidates
-> get_or_select_active_account(key_id, candidates)
-> rotate selected account to front
-> truncate candidates to 1
-> execute request
```

active account 已经存在且可用时，不能因为 model、conversation_id、previous_response_id、prompt_cache_key、route_strategy 等因素切换账号。

### 4. 不按 conversation / prompt cache key / model 绑定账号

本分支的账号选择维度只有：

```text
key_id -> active_account_id
```

不要引入或恢复以下行为：

```text
conversation_id -> account_id
previous_response_id -> account_id
prompt_cache_key -> account_id
model -> active_account_id
thread_epoch/thread_anchor -> account_id
```

如果官方代码里仍有 conversation binding、thread anchor、session affinity 相关逻辑，可以为了兼容协议保留，但它们不得参与账号候选排序，不得覆盖 active account 的选择结果。

一句话：

```text
conversation binding 可以存在，但不能决定选哪个 ChatGPT 账号。
```

### 5. 客户端断开不惩罚账号

以下情况不能增加 `consecutive_real_errors`，不能清除 active account，不能 mark cooldown，不能降低 route quality：

```text
client disconnected
user cancelled
Hermes interrupt
Codex interrupt
stream_interrupted
broken pipe
downstream disconnected
connection reset by peer，但确认是下游/客户端断开
```

尤其注意 stream 场景：如果 upstream stream incomplete 是由客户端断开导致的，不能对账号做 network cooldown。

正确逻辑类似：

```rust
if upstream_stream_failed && !client_delivery_failed {
    mark_account_cooldown(...);
    record_route_quality(...);
}
```

如果是 `client_delivery_failed`，应视作本次 active account 正常使用过，最多 touch `last_used_at`，不要记错。

### 6. 真实错误才累计 consecutive_real_errors

以下可以计为真实错误：

```text
upstream timeout
network error
temporary upstream failure
EOF before response
connection reset by upstream
HTTP 500 / 502 / 503 / 504
```

这些错误发生后：

```text
consecutive_real_errors += 1
本次请求返回失败给外部调用者
不在本次请求内换账号
```

达到阈值：

```text
consecutive_real_errors >= 3
-> clear active_account_id
-> 可短暂 cooldown 该账号
-> 本次请求仍返回失败
-> 下一次外部请求重新选择账号
```

### 7. 明确账号状态问题直接清 active account

以下错误说明当前账号不应继续使用：

```text
usage limit
account exhausted
rate limited / 429
unauthorized / 401
forbidden / 403
challenge
invalid token
account not found
workspace/account unavailable
```

这些错误可以直接清除当前 `key_id` 的 active account。

但仍应保持原则：

```text
不要在同一次外部请求内静默切到 B。
下一次外部请求才重新选择。
```

### 8. Exhausted / Failover 路径也必须记账

因为 active account 模式下候选列表被 truncate 到 1，`CandidateUpstreamDecision::Failover` 之后可能进入 exhausted / final 503 路径。

这条路径也必须记录 active account outcome。

如果实际尝试过账号：

```text
attempted_account_ids 非空
```

则在返回最终 503 前，应对最后尝试的账号调用 active account 结果记录逻辑，例如：

```rust
if let Some(account_id) = attempted_account_ids.last() {
    let now = now_ts();
    let _ = active_account::record_active_account_terminal_outcome(
        &storage,
        key_id.as_str(),
        account_id.as_str(),
        503,
        last_attempt_error.as_deref().or(Some(final_error.as_str())),
        now,
    );
}
```

如果没有实际尝试账号，只是 cooldown / inflight skip，则不能增加真实错误。

这一点非常重要，否则某些 failover-style transient error 永远不会累计到 3 次，active account 就永远不会切换。

---

## Active account 继续使用条件

已有 `key_id -> active_account_id` 时，只有满足以下条件才复用：

```text
账号存在
账号 status = active
token 存在且 access_token 非空
账号不在 cooldown
账号仍在当前 candidates 中
5h usage 未达到 low quota 阈值
weekly usage 未明确耗尽
idle 未超过 ACTIVE_ACCOUNT_IDLE_TTL_SECS
sticky 未超过 ACTIVE_ACCOUNT_MAX_STICKY_SECS
```

默认参数：

```text
ACTIVE_ACCOUNT_IDLE_TTL_SECS = 3600      # 空闲 1 小时后重新评估
ACTIVE_ACCOUNT_MAX_STICKY_SECS = 57600   # 连续固定 16 小时后重新评估
MAX_CONSECUTIVE_REAL_ERRORS = 3
```

idle / sticky 过期不是账号坏了，只是触发重新评估。如果重新评估后原账号仍然最合适，可以继续选回原账号。

---

## 重新选择账号：临期 weekly 优先

重新选择只发生在：

```text
没有 active account
active account stale
active account 不可用
idle 过期
sticky 过期
连续真实错误达到阈值后已清空
```

不要每个请求都重新排序切换。

重新选择时先过滤候选：

```text
账号 active
有 token
不在 cooldown
5h usage 未达到 low quota 阈值
weekly 未明确耗尽
```

5h 低额度判断必须复用官方低额度阈值，默认约 95%。不要只在 `used_percent >= 100` 时才排除，因为 ChatGPT OAuth 账号经常在未到 100% 时就触发 usage limit。

weekly 临期优先算法：

```text
weekly_remaining = 100 - secondary_used_percent
time_until_reset = secondary_resets_at - now
effective_time = clamp(time_until_reset, 3600, 7 * 86400)
urgency_score = weekly_remaining * (6 * 86400) / effective_time
```

含义：

```text
剩得多 + 快过期 = 优先使用
```

示例：

```text
还剩 7 天重置：score = weekly_remaining * 6 / 7
还剩 6 天重置：score = weekly_remaining
还剩 3 天重置：score = weekly_remaining * 2
还剩 1 天重置：score = weekly_remaining * 6
还剩 1 小时或更少：score = weekly_remaining * 144
```

如果 `secondary_resets_at` 缺失，退化为 `weekly_remaining` 排序。

如果 `secondary_used_percent` 缺失，不要给异常高分。可以用中性值，例如 50，或放在已知 usage 的账号之后。关键是稳定、可解释。

---

## 与官方上游合并时的注意事项

合并官方代码后，请重点检查以下文件和行为：

```text
crates/service/src/gateway/routing/active_account.rs
crates/service/src/gateway/routing/selection.rs
crates/service/src/gateway/upstream/proxy.rs
crates/service/src/gateway/upstream/proxy_pipeline/candidate_executor.rs
crates/service/src/gateway/upstream/proxy_pipeline/response_finalize.rs
crates/service/src/gateway/upstream/proxy_pipeline/request_setup.rs
crates/service/src/gateway/routing/conversation_binding.rs
crates/core/src/storage/api_key_active_accounts.rs
crates/core/src/storage/mod.rs
crates/core/migrations/057_api_key_active_accounts.sql
```

必须复核：

1. `apply_active_account_to_candidates` 是否仍然 `truncate(1)`。
2. `CandidateUpstreamDecision::Failover` / exhausted / final 503 路径是否仍会记录 active account outcome。
3. client disconnect / broken pipe / stream_interrupted 是否不会 cooldown / 不会记 active account 错误。
4. 5h low quota 是否仍按阈值过滤，不能被 weekly urgency 覆盖。
5. `conversation_binding` 是否仍不参与候选排序。
6. active account 是否仍按 `key_id` 隔离，不要改成全局一个账号，也不要按 model 分裂。
7. 429 / 401 / 403 / usage limit 是否仍能清 active account。
8. 5xx / timeout 是否累计真实错误，而不是同次请求内切号。

---

## 推荐测试用例

维护或合并后至少保留这些测试覆盖：

```text
1. 同一 key_id 连续请求复用同一 active_account。
2. 不同 key_id 可以拥有不同 active_account。
3. active account 选中后 candidates 只剩 1 个账号。
4. transient 失败低于 3 次时继续保留 active_account。
5. transient 连续第 3 次失败后清 active_account。
6. 第 3 次失败当次仍返回失败，下一次请求才重新选择账号。
7. CandidateUpstreamDecision::Failover -> Exhausted -> 503 路径会累计真实错误。
8. client disconnect / broken pipe / stream_interrupted 不增加错误、不 cooldown、不清 active_account。
9. 429 / 401 / 403 / usage limit 清 active_account。
10. 5h used_percent >= low quota threshold 时，不会因为 weekly 临期被选中。
11. weekly 临期账号在 5h 可用时优先。
12. conversation binding 不改变 candidates 顺序。
```

---

## 非目标

以下不是本分支目标，后续不要让 AI 自动扩展：

```text
不做复杂调度系统
不做每 model active account
不做 prompt_cache_key -> account binding
不做 previous_response_id -> account binding
不做 conversation_id -> account binding
不做单次请求内部静默 retry 三次
不做单次请求内部自动切账号兜底
不为了“看起来成功率高”牺牲账号固定性
```

本分支宁可让一次请求明确失败，由外部调用者重试，也不要在内部偷偷换账号。

---

## 一句话总结

本分支的核心契约是：

```text
每个 API Key 独立维护 active_account_id；
活跃时固定一个账号以最大化缓存；
空闲 1 小时或连续固定 16 小时后重新评估；
重新评估时优先使用 weekly 快到期且剩余额度多、同时 5h 仍可用的账号；
一次外部请求只尝试一个 active account；
真实失败累计 3 次后清 active account；
下一次外部请求才切换账号；
客户端主动断开不惩罚账号；
conversation / prompt_cache_key / previous_response_id / model 都不能决定账号路由。
```

如果官方上游后续修改了 gateway routing、candidate failover、stream finalize 或 conversation binding 相关代码，合并时必须以这份契约为准重新校验。
