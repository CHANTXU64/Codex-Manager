# Local Modifications

本文件只记录本地 fork / 我们自己维护的修改，避免把本地定制混入官方功能说明文档。

## Chat Completions JSON mode and SSE aggregation compatibility

Status: active

Files:

- `crates/service/src/gateway/local_validation/request.rs`
- `crates/service/src/gateway/observability/http_bridge/aggregate/sse_aggregate.rs`
- `crates/service/src/gateway/observability/http_bridge/delivery.rs`

Summary:

- 修复 OpenAI Chat Completions 兼容入口改写到 Responses API 时丢失 JSON 输出约束的问题，并避免非流式桥接 Responses SSE 时把同一份输出文本从 delta / completed response 重复聚合进 Chat Completions `message.content`。

What changed:

- `/v1/chat/completions` 请求改写到 `/v1/responses` 时，将 `response_format: {"type":"json_object"}` 映射到 Responses API 的 `text.format`；`json_schema` 格式会从 Chat Completions 的 `json_schema` 包装中展开到 Responses `text.format`。
- 同步转发常用生成控制字段：`max_completion_tokens` / `max_tokens` -> `max_output_tokens`，以及 `temperature`、`top_p`、`stop`。
- Responses SSE 非流式聚合如果 completed response 已经包含有效结构化 `output`，不再用从 SSE delta 聚合出的 `synthesis.output_text` 补写顶层 `output_text`，避免后续 Chat Completions 转换优先读取重复文本。

Merge protection:

- Preserve when: 仍需要支持 OpenAI Chat Completions 客户端经 Codex-Manager 访问 Responses API，尤其是 JSON mode / Hindsight 等依赖严格 JSON 输出的调用。
- Drop when: upstream 已提供等价的 Chat Completions -> Responses 字段映射，并已验证 Responses SSE delta/done/completed 聚合不会重复输出文本。
- Ask user when: upstream 重构请求协议适配或 SSE 聚合路径，导致 `text.format` 映射字段、`output_text` 优先级或 completed response 处理语义变化。

Verification:

```bash
cargo test -p codexmanager-service chat_completions_response_format_json_object_maps_to_responses_text_format -- --nocapture
cargo test -p codexmanager-service responses_sse_delta_done_and_completed_output_maps_to_single_chat_content -- --nocapture
cargo fmt --check
cargo test -p codexmanager-service -- --nocapture
```

Upstream status: fork-only

## Web logs cache anomaly highlighting

- 提交：`d4309812 Highlight cache miss rows in logs`
- 标题：`Highlight cache miss rows in logs`

### 摘要

这项本地修改在 Web Logs 页面高亮疑似缓存异常的长上下文请求，方便排查 active account 是否稳定复用同一账号，以及长上下文请求是否正常命中上游缓存。

核心行为：

- 当请求输入 token 数不少于 1024，且 `cachedInputTokens` 明确为 `0` 时，将该请求视为缓存异常候选。
- 只有当前列表中存在更早的可比较请求时才高亮，避免首个长上下文请求被误标为异常。
- 可比较请求必须具备相同平台 Key、相同模型和相同请求路径；缺少任一字段时不高亮，降低不同 prompt 被误判的概率。
- 命中的表格行使用淡红背景，缓存 token 文案显示为“缓存异常”并使用红色强调；hover 状态也保持红色提示。
- 表格行 key 回退为 `traceId` 或请求路径加时间，避免缺少 `id` 时 React key 不稳定。

合并上游时重点保护：`apps/src/app/logs/page.tsx` 中缓存异常判定 helper、日志行高亮 class，以及缓存 token 文案的红色强调逻辑。

## Daily quota consumption chart and usage polling

Status: active

Files:

- `crates/core/migrations/063_quota_consumption_daily.sql`
- `crates/core/src/storage/quota_consumption_daily.rs`
- `crates/core/src/storage/mod.rs`
- `crates/core/src/rpc/types.rs`
- `crates/service/src/usage/usage_snapshot_store.rs`
- `crates/service/src/usage/usage_scheduler.rs`
- `crates/service/src/usage/tests/usage_scheduler_tests.rs`
- `crates/service/src/dashboard.rs`
- `apps/src/app/page.tsx`
- `apps/src/lib/api/dashboard-client.ts`
- `apps/src/lib/api/normalize.ts`
- `apps/src/lib/store/useAppStore.ts`
- `apps/src/types/dashboard.ts`
- `docs/CHANTXU64/2026-05-17-daily-quota-consumption-design.md`

Summary:

- 首页管理员用量分析新增每日账号 5h 额度消耗百分比图表，并把默认用量轮询间隔从 10 分钟缩短到 2 分钟。

What changed:

- 新增 `quota_consumption_daily` 存储表，按账号和本地日期累计 5h 额度消耗百分比。
- `store_usage_snapshot` 在写入新快照前读取同账号上一条快照，只累计 `used_percent` 正向增长；检测到 5h 重置时不补记上周期剩余额度，例如 `87% -> 0%` 不会把剩余 `13%` 加到首页消耗统计。
- `read_admin_usage_summary` 返回 `dailyQuotaConsumption`，前端首页用堆叠柱状图展示每日总消耗及账号拆分。
- 后端、前端默认配置中的 `usagePollIntervalSecs` 从 `600` 改为 `120`，仍可通过设置项或 `CODEXMANAGER_USAGE_POLL_INTERVAL_SECS` 覆盖。

Why it matters:

- Token 用量不能直接反映账号池 5h 额度消耗情况；该图表用于观察账号池当天额度消耗节奏。
- 重置刷新不代表用户把重置前剩余额度用完，补记剩余额度会制造虚假的消耗尖峰。
- 2 分钟轮询降低快照间隔导致的漏记概率。

Merge protection:

- Preserve when: 首页仍需要展示账号池 5h 额度消耗趋势，或仍需要更高频自动刷新账号额度。
- Drop when: upstream 已提供等价且验证过的每日额度消耗统计，并且确认重置刷新不会补记剩余额度。
- Ask user when: upstream 改动了 usage snapshot 结构、dashboard summary schema、后台轮询配置，或出现与 `quota_consumption_daily` 相关的迁移冲突。

Verification:

```bash
cargo fmt --check
cargo test -p codexmanager-service default_usage_poll_interval_is_two_minutes -- --nocapture
cargo test -p codexmanager-service reset_refresh_does_not_count_previous_remaining_quota_as_consumed -- --nocapture
cargo test -p codexmanager-service normal_usage_increase_counts_delta -- --nocapture
pnpm run build:desktop
```

Feature docs: `docs/CHANTXU64/2026-05-17-daily-quota-consumption-design.md`

Upstream status: fork-only

## Local props probe handling

- 提交：`3d05626 Intercept local props probes`
- 标题：`Intercept local props probes`

### 摘要

这项本地修改在网关入口拦截兼容客户端的属性探测请求，避免把非 Codex/OpenAI 推理请求转发到 `chatgpt.com/backend-api/codex`。

核心行为：

- 对 `GET /v1/props` 和 `GET /props` 直接返回本地 JSON `{}`。
- 拦截发生在本地平台 Key 鉴权、账号候选选择和上游代理之前，因此无 Authorization 的兼容客户端探测也会返回 `{}`，不会访问 ChatGPT 上游，不会触发 Cloudflare challenge，也不会影响 active account。
- 只匹配精确 props 路径及其 query string，不匹配 `/v1/props-extra`、`/v1/props/extra` 等其它路径。

本次加固：

- 补充真实 gateway pipeline 回归测试，覆盖 `/v1/props`、`/props`、`/v1/props?xxx=yyy` 的本地响应行为，确认返回 `{}`、`application/json`、不访问上游、不触发账号选择、不改写 active account，并写入 request log。
- 明确无 Authorization 的 `/v1/props` 兼容客户端探测也必须在本地鉴权前返回 `{}`。

合并上游时重点保护：`request/local_props.rs` 的本地短路处理，以及 `request_entry.rs` 中该处理必须位于 `prepare_local_request` 和 `proxy_validated_request` 之前。

## Web log key names and scheduled warmup

- 提交：
  - `a516f43 Add scheduled account warmup and log key names`
  - `85bbbaf Support multiple warmup cron schedules`
- 标题：`Support multiple scheduled account warmup cron schedules and log key names`

### 摘要

这项本地修改补强了网关日志可读性，并加入可配置的账号定时预热能力。目标是减少冷启动请求的失败概率，同时让 Web Log 里“账号 / 密钥”列直接展示平台 Key 名称，而不是内部 Key ID。Docker Compose 默认时区固定为 `Asia/Shanghai`，避免容器 UTC 时区导致定时预热时间偏移。

核心行为：

- Web Log 的“账号 / 密钥”列优先显示平台 Key 名称；没有名称时才回退为压缩后的 Key ID。tooltip 中保留完整 Key ID，便于排障。
- 设置页“后台任务线程”新增“定时账号预热”开关，支持 Cron 表达式配置，默认 `0 */4 * * *`。
- 定时预热支持用 `|` 分隔多个 Cron 表达式，例如 `0 7 * * *|10 12 * * *|20 17 * * *`，调度器会取最近一次触发时间。
- 设置页会展示下一次预热时间与剩余倒计时，便于确认调度器是否已经进入等待状态。
- 定时预热复用现有账号预热逻辑，会对当前所有可用网关账号发送轻量预热请求。
- Cron 支持 5 段格式，也支持带秒的 6 段格式；表达式保存采用严格校验，任一 `|` 分段非法或无法计算下一次触发时间都会拒绝保存，并提示具体第几项有问题。
- Docker Compose 的开发版、release 版和 all-in-one 部署均默认注入 `TZ=Asia/Shanghai`。
- 账号预热的模型选择保持上游官方行为：使用 catalog 中第一个可 API 使用模型；catalog 缺失时默认 `gpt-5.3-codex`。

本次加固：

- 前端设置页剩余时间逻辑区分未来、已到期和缺失 `warmupCronNextRunAt`，避免过期时间被强行显示成 `0s`。
- 补充 helper 测试覆盖多 Cron 表达式的前端格式校验，以及剩余时间的未来、过期、缺失三种显示分支。
- 补充 Rust 回归测试覆盖任一 `|` 分段非法时整体拒绝、无法计算下一次触发时间时拒绝，以及 settings snapshot 中 `warmupCronNextRunAt` 的 camelCase 序列化。
- 已移除 fork 中的 mini 模型优先预热策略，`account_warmup.rs` 回到上游官方模型选择逻辑。
- 账号预热流式响应采用 upstream 实现：读到 `response.completed`、`response.done` 或 `[DONE]` 才记录成功；`response.failed`、`response.incomplete` 和错误 SSE 帧会保留失败原因，避免只收到 2xx 响应头就提前关闭连接。
- 账号预热真正完成后会为该账号入队 usage refresh，让账号页更快刷新额度倒计时；该行为已由 upstream/main 提供，fork 不再维护自定义实现。

合并上游时重点保护：`usage/refresh/runner.rs` 的 warmup cron 调度和 `|` 多表达式解析、`BackgroundTaskSettings` 新增字段的前后端序列化、Docker Compose 的 `TZ=Asia/Shanghai`、日志页密钥名称优先显示逻辑；`account_warmup.rs` 的流式 drain 逻辑优先采用 upstream/main 实现。

## Active account routing

- 提交：`28ec11f6e63040b78ce00a8eca6551eb6b2d23bb`
- 标题：`Active account routing (#4)`
- 详细设计与维护契约：[`docs/active-account-routing.md`](./active-account-routing.md)

### 摘要

这项本地修改把网关账号路由从“每次请求可按候选排序/会话绑定/失败兜底切换账号”收敛为“每个 API Key 维护一个 active account”。目标是让同一个 API Key 在活跃周期内尽量固定使用同一个 ChatGPT 账号，从而提高 prompt cache / 上游缓存命中，并避免 weekly 额度快到期但未消耗的账号被浪费。

核心行为：

- 新增 `api_key_active_accounts` 存储表，按 `key_id -> active_account_id` 记录当前活跃账号、开始时间、最近使用时间、连续真实错误次数和切换原因。
- 请求进入网关后先收集候选账号，再为当前 API Key 获取或选择 active account；一旦选中，只保留该账号作为本次请求候选，避免同一次外部请求内静默 failover 到其他账号。
- active account 会在空闲超时、固定时间过长、账号不可用、额度不足、cooldown、连续真实错误达到阈值等情况下清除或重新评估。
- 重新选择账号时优先考虑 weekly 快到期且剩余额度较多、同时 5h 额度仍可用的账号，减少 weekly 额度浪费。
- transient / 5xx / timeout 等真实上游错误按外部请求次数累计；连续 3 次后清除 active account，但当前请求仍返回失败，下一次外部请求才重新选账号。
- 401 / 403 / 429 / usage limit / challenge / token 无效等明确账号状态问题会清除 active account。
- client disconnect、用户中断、broken pipe、stream interrupted 等下游断开不惩罚账号，不增加连续真实错误，不触发 cooldown。
- conversation binding、prompt cache key、previous response、model 等信息可以作为协议兼容存在，但不得覆盖 active account 的账号选择结果。

本次加固：

- active account 连续固定重新评估时间从 4 小时改为 16 小时，即 `ACTIVE_ACCOUNT_MAX_STICKY_SECS = 57600`。
- HTTP `502` 与 `500` 一样，会在进入最终 failover/返回前先对原账号静默重试一次。
- 将 `error sending request`、`error decoding response body`、`read upstream body failed`、`connection interrupted`、`连接中断`、`网络波动` 等 transport / stream transient 错误纳入 active account 真实错误分类。
- transport / stream transient 错误不会立刻给账号打 network cooldown，也不会因此在下一次请求直接跳到其他账号；仍由 active account 的连续真实错误计数控制，达到阈值后才清除 active account。
- 客户端主动断开仍按下游断开处理，不计入账号失败，不触发 cooldown。

合并上游时重点保护：`active_account.rs` 的选择/记录逻辑、候选列表 `truncate(1)` 行为、exhausted/final 503 路径的结果记账、客户端断开不惩罚账号、以及 conversation binding 不参与账号排序。

### LOC 统计

统计口径：

- 提交增删行：`git show --numstat 28ec11f6e63040b78ce00a8eca6551eb6b2d23bb`
- 当前物理行数：当前工作树中对应文件的行数

| 类别 | 文件数 | 本提交新增 | 本提交删除 | 当前文件物理行数 |
| --- | ---: | ---: | ---: | ---: |
| Source / migration | 13 | 1421 | 34 | 7338 |
| Tests | 1 | 115 | 2 | 1742 |
| Docs | 1 | 458 | 0 | 458 |
| **合计** | **15** | **1994** | **36** | **9538** |

### 文件级明细

| 文件 | 新增 | 删除 | 当前物理行数 |
| --- | ---: | ---: | ---: |
| `crates/core/migrations/057_api_key_active_accounts.sql` | 9 | 0 | 9 |
| `crates/core/src/storage/api_key_active_accounts.rs` | 180 | 0 | 180 |
| `crates/core/src/storage/mod.rs` | 18 | 0 | 1186 |
| `crates/core/tests/storage.rs` | 115 | 2 | 1742 |
| `crates/service/src/gateway/mod.rs` | 2 | 0 | 1127 |
| `crates/service/src/gateway/routing/active_account.rs` | 1036 | 0 | 1036 |
| `crates/service/src/gateway/routing/conversation_binding.rs` | 74 | 22 | 761 |
| `crates/service/src/gateway/routing/cooldown.rs` | 5 | 0 | 349 |
| `crates/service/src/gateway/routing/selection.rs` | 2 | 2 | 406 |
| `crates/service/src/gateway/upstream/proxy.rs` | 25 | 0 | 914 |
| `crates/service/src/gateway/upstream/proxy_pipeline/candidate_executor.rs` | 9 | 0 | 511 |
| `crates/service/src/gateway/upstream/proxy_pipeline/execution_context.rs` | 8 | 0 | 339 |
| `crates/service/src/gateway/upstream/proxy_pipeline/request_setup.rs` | 9 | 8 | 100 |
| `crates/service/src/gateway/upstream/proxy_pipeline/response_finalize.rs` | 44 | 2 | 420 |
| `docs/active-account-routing.md` | 458 | 0 | 458 |
