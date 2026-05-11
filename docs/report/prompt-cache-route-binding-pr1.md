# PR #1 Prompt Cache Route Binding 修改记录

> 目的：记录本分支 `fix/prompt-cache-route-binding` 在 PR #1 中对 Codex-Manager 网关账号路由、prompt cache、previous response 处理的本地改动。后续如果继续修改本 PR，请继续追加到本文档，合并时不要遗漏或覆盖这些行为约束。

- PR: https://github.com/CHANTXU64/Codex-Manager/pull/1
- 分支：`fix/prompt-cache-route-binding`
- 基线提交：`b1604e72 fix: 让路由优先按缓存线程固定账号`
- 当前最新提交：`1f6091a9 docs: record prompt cache route binding changes`
- 记录时间：2026-05-11 20:40:27 CST
- 总入口文档：`docs/LOCAL_MODIFICATIONS.md`（后续所有本地修改优先追加到该文件）

## 合并时必须保留的原则

1. **同一 `prompt_cache_key` 要尽量固定同一账号**，目标是最大化上游缓存输入命中。
2. **内部路由 key 不能污染上游请求**：`pck:v1:...` 只能用于本地账号路由，不能写入上游 `conversation_id`、`session_id`、`thread_anchor`、body `prompt_cache_key`。
3. **原生会话信号优先**：`conversation_id` / `x-codex-turn-state` / `previous_response_id` 不能被普通 prompt-cache route 反客为主。
4. **临时 failover 不应漂移绑定**：绑定账号本轮已被选中，但执行阶段临时切到其他账号成功时，不应把 pck binding 改到新账号。
5. **stale binding 要能自愈**：绑定账号已经不在候选池 / 未被选中时，其他账号成功后允许 rebind。
6. **`previous_response_id + prompt_cache_key` 只能走 existing-only pck routing**：可以使用已有 pck binding 保持同账号，但不能因为 previous-response 请求新建 pck binding。
7. **所有后续改动必须追加到本文档“后续修改记录”章节**，避免合并/重构时丢掉上下文。

## 已修改文件

- `crates/service/src/gateway/local_validation/request.rs`
- `crates/service/src/gateway/local_validation/tests/request_tests.rs`
- `crates/service/src/gateway/request/request_helpers.rs`
- `crates/service/src/gateway/request/tests/request_helpers_tests.rs`
- `crates/service/src/gateway/routing/conversation_binding.rs`

## 提交记录

### `ded0a060 fix: harden prompt-cache route binding`

主要修复第一轮发现的问题：

- 为 OpenAI-compatible `/v1/responses` 请求增加基于 `prompt_cache_key` 的 route-only conversation binding。
- route id 格式：`pck:v1:<hash>`。
- route id hash 输入去掉 model，只使用：
  - `platform_key_hash`
  - `protocol_type`
  - `prompt_cache_key`
- 原因：validation 阶段看到的 model 可能和执行阶段 per-account override 后的实际 model 不一致，带 model 会把同一缓存线程拆成多个 route key。
- 路径判断从 `starts_with("/v1/responses")` 改为 `official_responses_http::is_responses_path(...)`，只影响 prompt-cache route binding 入口。
- `ParsedRequestMetadata` 新增 `has_previous_response_id`。
- 初版处理：遇到 `previous_response_id` 时不使用 pck route。后来已被 `4c8d5564` 修正为 existing-only routing。
- PromptCacheKey source 下 `resolve_attempt_thread()` 返回 `None`，避免生成上游 thread/session 相关锚点。
- PromptCacheKey binding 初版不自动 rebind，后来已被 `4c8d5564` 修正为“选中绑定账号时不漂移，未选中时允许自愈”。

新增/调整测试：

- `route_conversation_id_uses_prompt_cache_key_without_native_anchor`
- `route_conversation_id_does_not_use_prompt_cache_key_when_turn_state_exists`
- `route_conversation_id_prefers_native_conversation_over_prompt_cache_key`
- `route_conversation_id_does_not_use_prompt_cache_key_for_non_responses_path_prefix`
- `parse_request_metadata_detects_previous_response_id`
- `parse_request_metadata_ignores_blank_previous_response_id`
- `prompt_cache_route_binding_records_without_attempt_thread`
- `prompt_cache_route_binding_rotates_bound_account_first`

### `4c8d5564 fix: handle previous response cache routing`

修正第二轮 review 发现的问题：

- 新增 `RouteConversationSource::PromptCacheKeyExistingOnly`。
- `previous_response_id + prompt_cache_key` 不再直接 `return None`，而是生成同一个 pck route id，但 source 标记为 `PromptCacheKeyExistingOnly`。
- `PromptCacheKeyExistingOnly` 语义：
  - 如果已有 pck binding，可以用它把请求路由回同一账号。
  - 如果没有已有 pck binding，不允许创建新的 pck binding。
- `RouteConversationSource` 新增 helper：
  - `is_prompt_cache_key()`：匹配 `PromptCacheKey` 和 `PromptCacheKeyExistingOnly`。
  - `allows_initial_binding_create()`：`PromptCacheKeyExistingOnly` 返回 false。
- rebind 逻辑改为：
  - `binding.account_id == account.id`：touch。
  - `source.is_prompt_cache_key() && routing.bound_account_selectable`：不 rebind，用于防止临时 failover 或 manual preferred account 造成缓存线程漂移。
  - `source.is_prompt_cache_key() && !routing.bound_account_selectable && status_code < 400`：允许 rebind，用于 stale binding 自愈。
  - `None && source.allows_initial_binding_create() && status_code < 400`：才允许创建新 binding。

新增/调整测试：

- `route_conversation_id_uses_existing_only_prompt_cache_key_when_previous_response_id_exists`
- `prompt_cache_route_binding_does_not_rebind_after_selected_binding_failover_success`
- `prompt_cache_route_binding_rebinds_when_bound_account_is_not_selected`
- `prompt_cache_existing_only_route_binding_does_not_create_initial_binding`

### `1f4445af fix: exclude existing-only cache binding from thread anchor`

修正第三轮 review 发现的问题：

- local validation 计算 fallback thread anchor 时，原本只排除了 `PromptCacheKey`，漏了 `PromptCacheKeyExistingOnly`。
- 这会让 `previous_response_id + 已有 pck binding` 场景下，existing-only 的 pck binding 仍可能被当作 `fallback_thread_anchor`。
- 修复为：`PromptCacheKey` 和 `PromptCacheKeyExistingOnly` 都不能作为 fallback thread anchor 来源。

### `1afc72fd test: cover existing-only cache thread anchor`

补测试和抽 helper：

- 将 thread-anchor binding 过滤逻辑抽成：
  - `conversation_binding_for_thread_anchor(...)`
- 将 `RouteConversationSource::is_prompt_cache_key()` 改为 `pub(crate)`，local validation 复用该 helper，避免重复手写 matches。
- 新增测试：
  - `existing_only_prompt_cache_binding_is_not_used_as_fallback_thread_anchor`
- 该测试验证：
  - `PromptCacheKeyExistingOnly + 已存在 pck binding` 时，`conversation_binding_for_thread_anchor(...)` 返回 `None`。

## 当前行为说明

### 普通无原生锚点请求

条件：

- OpenAI-compatible protocol
- path 是官方 responses path
- 无 `conversation_id`
- 无 `x-codex-turn-state`
- 有长度至少 8 的 `prompt_cache_key`
- 无 `previous_response_id`

行为：

1. 生成 `pck:v1:<hash>` route id。
2. source = `PromptCacheKey`。
3. 查已有 conversation binding。
4. 如果已有 binding，优先把绑定账号排到候选第一位。
5. 不生成上游 thread attempt。
6. 不把 `pck:v1:<hash>` 写进上游请求。
7. 成功后：
   - 无 binding：创建 pck binding。
   - 绑定账号成功：touch。
   - 绑定账号本轮仍可选但其他账号因 failover 或 manual preferred 成功：不 rebind。
   - 绑定账号本轮不可选且其他账号成功：允许 rebind。

### 有 `previous_response_id + prompt_cache_key` 的请求

行为：

1. 生成同样的 `pck:v1:<hash>` route id。
2. source = `PromptCacheKeyExistingOnly`。
3. 可以使用已有 pck binding 固定账号。
4. 不允许新建 pck binding。
5. 不生成上游 thread attempt。
6. 不把 pck binding 用作 fallback thread anchor。

### 有原生 `conversation_id` 的请求

行为：

- source = `NativeConversation`。
- 优先使用原生 conversation id。
- prompt_cache_key 不反客为主。

### 有 `x-codex-turn-state` 的请求

行为：

- 不启用 prompt-cache route binding。
- 避免和 Codex 原生 turn state 冲突。

## 验证记录

已执行并通过：

```bash
cargo test -p codexmanager-service route_conversation_id -- --nocapture
cargo test -p codexmanager-service prompt_cache_route_binding -- --nocapture
cargo test -p codexmanager-service parse_request_metadata -- --nocapture
cargo test -p codexmanager-service existing_only_prompt_cache_binding_is_not_used_as_fallback_thread_anchor -- --nocapture
cargo check -p codexmanager-service
git diff --check
```

当前本地验证结果摘要：

- `route_conversation_id`: 5 passed
- `prompt_cache_route_binding`: 5 passed
- `parse_request_metadata`: 2 passed
- `existing_only_prompt_cache_binding_is_not_used_as_fallback_thread_anchor`: 1 passed
- `cargo check`: passed
- `git diff --check`: passed

## 合并注意事项

合并或 rebase 时，重点检查以下点不要被覆盖：

1. `RouteConversationSource` 必须保留 `PromptCacheKeyExistingOnly`。
2. `RouteConversationSource::is_prompt_cache_key()` 必须同时匹配：
   - `PromptCacheKey`
   - `PromptCacheKeyExistingOnly`
3. `PromptCacheKeyExistingOnly` 必须禁止创建初始 binding。
4. `record_conversation_binding_terminal_response(...)` 里必须保留：
   - 绑定账号仍可选时，manual preferred / failover 到其他账号成功不 rebind。
   - 绑定账号不可选时允许 stale binding rebind。
5. `conversation_binding_for_thread_anchor(...)` 必须排除所有 prompt-cache source。
6. `resolve_attempt_thread(...)` 必须对所有 prompt-cache source 返回 `None`。
7. `previous_response_id + prompt_cache_key` 不能直接走普通 route strategy，也不能创建新 pck binding。
8. prompt-cache route key 不能包含 model。
9. prompt-cache route binding 入口要继续用精确 responses path 判断，不要退回 `starts_with("/v1/responses")`。
10. 后续如改动 account selection / failover / binding 记录逻辑，必须重新跑本文档“验证记录”中的测试。

## 后续修改记录

> 后续如果继续修改 PR #1，请按下面格式追加，不要覆盖前文。

### 2026-05-11 20:56 CST - this commit

- 修改文件：
  - `crates/service/src/gateway/routing/conversation_binding.rs`
  - `docs/LOCAL_MODIFICATIONS.md`
  - `docs/report/prompt-cache-route-binding-pr1.md`
- 修改内容：
  - 新增 `bound_account_selectable`，把“binding 没被选中”和“binding 账号不可选”分开。
  - pck rebind 判断改为：只要原绑定账号仍可选，manual preferred account 成功也不迁移 binding；只有原绑定账号不可选时才允许 stale binding rebind。
  - 补充 manual preferred no-rebind / stale rebind 测试。
- 原因：
  - 手动指定账号只能影响本次尝试顺序，不能静默破坏已有缓存线程亲和。
- 验证：
  - `cargo test -p codexmanager-service prompt_cache_manual_preferred -- --nocapture`
- 合并注意：
  - 不要再只用 `binding_selected` 判断 pck binding 是否 stale；必须看 `bound_account_selectable`。

### YYYY-MM-DD HH:mm CST - <commit 或未提交>

- 修改文件：
  - `path/to/file.rs`
- 修改内容：
  - ...
- 原因：
  - ...
- 验证：
  - `cargo test ...`
- 合并注意：
  - ...
