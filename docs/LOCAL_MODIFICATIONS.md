# Local Modifications

本文件只记录本地 fork / 我们自己维护的修改，避免把本地定制混入官方功能说明文档。

## Web log key names and scheduled warmup

- 提交：`待提交`
- 标题：`Support multiple scheduled account warmup cron schedules and log key names`

### 摘要

这项本地修改补强了网关日志可读性，并加入可配置的账号定时预热能力。目标是减少冷启动请求的失败概率，同时让 Web Log 里“账号 / 密钥”列直接展示平台 Key 名称，而不是内部 Key ID。Docker Compose 默认时区固定为 `Asia/Shanghai`，避免容器 UTC 时区导致定时预热时间偏移。

核心行为：

- Web Log 的“账号 / 密钥”列优先显示平台 Key 名称；没有名称时才回退为压缩后的 Key ID。tooltip 中保留完整 Key ID，便于排障。
- 设置页“后台任务线程”新增“定时账号预热”开关，支持 Cron 表达式配置，默认 `0 */4 * * *`。
- 定时预热支持用 `|` 分隔多个 Cron 表达式，例如 `0 7 * * *|10 12 * * *|20 17 * * *`，调度器会取最近一次触发时间。
- 设置页会展示下一次预热时间与剩余倒计时，便于确认调度器是否已经进入等待状态。
- 定时预热复用现有账号预热逻辑，会对当前所有可用网关账号发送轻量预热请求。
- Cron 支持 5 段格式，也支持带秒的 6 段格式；表达式非法时只记录 warning，不让服务崩溃。
- Docker Compose 的开发版、release 版和 all-in-one 部署均默认注入 `TZ=Asia/Shanghai`。
- 预热模型选择优先使用 catalog 中第一个可 API 使用且 slug 包含 `mini` 的模型，例如 `gpt-5.4-mini`；没有 mini 时再回退到第一个可 API 使用模型。
- catalog 缺失时的默认预热模型为 `gpt-5.4-mini`，尽量降低预热成本。

合并上游时重点保护：`account_warmup.rs` 的 mini 模型优先策略、`usage/refresh/runner.rs` 的 warmup cron 调度和 `|` 多表达式解析、`BackgroundTaskSettings` 新增字段的前后端序列化、Docker Compose 的 `TZ=Asia/Shanghai`、以及日志页密钥名称优先显示逻辑。

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
