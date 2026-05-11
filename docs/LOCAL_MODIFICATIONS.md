# Local Modifications

> 这是 Codex-Manager fork/本地分支的本地修改保护文档。后续任何本地改动都必须写到这里，供 AI Agent 在 rebase、merge、冲突处理、PR 合并前确认，不要因为不了解背景误删功能。

- Repository: `CHANTXU64/Codex-Manager`
- Current branch: `fix/prompt-cache-route-binding`
- PR: https://github.com/CHANTXU64/Codex-Manager/pull/1
- Current local base for this branch: `b1604e72 fix: 让路由优先按缓存线程固定账号`
- Latest documented head: `1f6091a9 docs: record prompt cache route binding changes`
- Last updated: 2026-05-11 20:44:26 CST

## Merge rules for AI agents

1. **Do not blindly use `-X ours` or `-X theirs`.** Read conflicted files and preserve the verified local behavior below while incorporating upstream changes.
2. **This file is the canonical local-modification record.** If another document disagrees, prefer this file and verify with `git diff`, `git blame`, and tests.
3. **Historical notes are not automatic keep-rules.** Preserve only entries under “Active modifications” unless the user explicitly asks to restore an old behavior.
4. **Do not remove tests that protect local behavior.** If upstream refactors paths or APIs, port the tests instead of deleting them.
5. **Before merging this PR or rebasing onto upstream, check every item in “Merge protection checklist”.**
6. **Any future local change must be appended to “Change log”.** Do not overwrite the prior history.
7. **If unsure whether a local behavior is still wanted, stop and ask.** Do not guess.

## Active modifications

### 1. Prompt-cache route-only account binding for `/v1/responses`

- Status: active
- Commits:
  - `ded0a060 fix: harden prompt-cache route binding`
  - `4c8d5564 fix: handle previous response cache routing`
  - `1f4445af fix: exclude existing-only cache binding from thread anchor`
  - `1afc72fd test: cover existing-only cache thread anchor`
- Files:
  - `crates/service/src/gateway/local_validation/request.rs`
  - `crates/service/src/gateway/local_validation/tests/request_tests.rs`
  - `crates/service/src/gateway/request/request_helpers.rs`
  - `crates/service/src/gateway/request/tests/request_helpers_tests.rs`
  - `crates/service/src/gateway/routing/conversation_binding.rs`

#### What changed

A route-only binding was added for OpenAI-compatible official responses requests with `prompt_cache_key`.

The route key format is:

```text
pck:v1:<hash>
```

The hash input intentionally uses only:

- `platform_key_hash`
- `protocol_type`
- `prompt_cache_key`

It intentionally **does not include model**. Validation-time model can differ from the final upstream model after per-account overrides, so including model would split one cache thread across multiple local route keys.

#### Why it matters

The production symptom was: same logical session / same cache thread switched accounts midway. That reduces cache reuse and can break `previous_response_id` chains. The local route binding keeps the same `prompt_cache_key` on the same account whenever possible, maximizing cached input reuse.

#### Merge protection

Preserve these behaviors:

- `prompt_cache_key` can create a local route id for account selection.
- The local route id must not be sent upstream.
- `pck:v1:<hash>` must not become upstream `conversation_id`, `session_id`, `thread_anchor`, or body `prompt_cache_key`.
- The prompt-cache route key must not include model.
- The prompt-cache route binding entry point must use precise official responses path detection, not broad `starts_with("/v1/responses")`.

### 2. `RouteConversationSource::PromptCacheKeyExistingOnly`

- Status: active
- Commit: `4c8d5564 fix: handle previous response cache routing`
- Files:
  - `crates/service/src/gateway/routing/conversation_binding.rs`
  - `crates/service/src/gateway/local_validation/request.rs`
  - `crates/service/src/gateway/local_validation/tests/request_tests.rs`

#### What changed

Added:

```rust
RouteConversationSource::PromptCacheKeyExistingOnly
```

This source is used when a request contains both:

- `previous_response_id`
- `prompt_cache_key`

The resolver still computes the same pck route id, but marks it existing-only.

#### Why it matters

A request with `previous_response_id` must usually hit the same upstream account that produced the prior response id. If the gateway simply returns `None` for route binding, normal route strategy may pick another account and break the chain. If the gateway creates a brand-new pck binding for a previous-response request, it may bind the chain to the wrong account.

Existing-only is the safer middle ground:

- Existing pck binding can keep the request on the same account.
- No existing pck binding means do not create a new one from this request.

#### Merge protection

Preserve these behaviors:

- `previous_response_id + prompt_cache_key` must produce `PromptCacheKeyExistingOnly`, not plain `PromptCacheKey` and not `None`.
- `PromptCacheKeyExistingOnly` must be treated as a prompt-cache source for “do not generate upstream thread/session anchor” rules.
- `PromptCacheKeyExistingOnly` must not create an initial binding.

### 3. Prompt-cache failover and stale-binding behavior

- Status: active
- Commit: `4c8d5564 fix: handle previous response cache routing`
- File: `crates/service/src/gateway/routing/conversation_binding.rs`

#### What changed

`record_conversation_binding_terminal_response(...)` was changed so pck bindings behave differently depending on whether the bound account was selected this round.

Current intended behavior:

- Existing binding account succeeds: touch the binding.
- Existing binding account remains selectable, but execution uses another account because of manual preferred account or temporary failover: do **not** rebind.
- Existing binding account is stale / disabled / absent from candidates, and another account succeeds: allow rebind.
- No binding + `PromptCacheKey`: allow initial binding create.
- No binding + `PromptCacheKeyExistingOnly`: do not create initial binding.

#### Why it matters

There are two different scenarios that must not be confused:

1. Temporary failover: preserving the original account maximizes cache continuity.
2. Manual preferred account: it may affect the current attempt order, but must not silently migrate a hot pck binding while the bound account remains selectable.
3. Stale binding: if the old account is no longer selectable, refusing to rebind would make the binding permanently stale.

#### Merge protection

Do not simplify this back to either of these incorrect forms:

```rust
Some(_) if routing.source == RouteConversationSource::PromptCacheKey => Ok(())
```

or:

```rust
Some(binding) if status_code < 400 => always_rebind(...)
```

Both lose important behavior.

### 4. Prompt-cache sources excluded from upstream thread/session anchor

- Status: active
- Commits:
  - `ded0a060 fix: harden prompt-cache route binding`
  - `1f4445af fix: exclude existing-only cache binding from thread anchor`
  - `1afc72fd test: cover existing-only cache thread anchor`
- Files:
  - `crates/service/src/gateway/routing/conversation_binding.rs`
  - `crates/service/src/gateway/local_validation/request.rs`
  - `crates/service/src/gateway/local_validation/tests/request_tests.rs`

#### What changed

`RouteConversationSource::is_prompt_cache_key()` now matches both:

- `PromptCacheKey`
- `PromptCacheKeyExistingOnly`

All prompt-cache sources are excluded from:

- `resolve_attempt_thread(...)`
- fallback thread-anchor binding selection via `conversation_binding_for_thread_anchor(...)`

#### Why it matters

The local route id is an internal account-selection key. If it becomes a fallback thread anchor, upstream request rewriting may accidentally treat `pck:v1:<hash>` as a real conversation/thread value.

#### Merge protection

Preserve:

```rust
source.is_prompt_cache_key()
```

for both route sources. Do not replace it with a single equality check against `PromptCacheKey`.

### 5. Request metadata parsing for `previous_response_id`

- Status: active
- Commit: `ded0a060 fix: harden prompt-cache route binding`
- Files:
  - `crates/service/src/gateway/request/request_helpers.rs`
  - `crates/service/src/gateway/request/tests/request_helpers_tests.rs`

#### What changed

`ParsedRequestMetadata` now has:

```rust
has_previous_response_id: bool
```

The parser treats a non-empty string `previous_response_id` as present. Blank strings are ignored.

#### Why it matters

Route selection needs to know whether the request is part of an upstream response chain. This changes how `prompt_cache_key` routing is allowed.

#### Merge protection

Preserve both tests:

- `parse_request_metadata_detects_previous_response_id`
- `parse_request_metadata_ignores_blank_previous_response_id`

### 6. Local modification documentation

- Status: active
- Commits:
  - `1f6091a9 docs: record prompt cache route binding changes`
  - current document update
- Files:
  - `docs/report/prompt-cache-route-binding-pr1.md`
  - `docs/LOCAL_MODIFICATIONS.md`

#### What changed

Added documentation so future AI agents can understand which local changes are intentional and must be protected during merge/rebase conflict resolution.

`docs/LOCAL_MODIFICATIONS.md` is the canonical summary. The PR-specific report remains useful as an expanded history for PR #1.

#### Merge protection

Do not delete this file during conflict resolution. If code changes alter the behavior above, append a dated note to “Change log”.

## Current behavior summary

### Plain `prompt_cache_key` request

Conditions:

- OpenAI-compatible protocol
- official responses path
- no `conversation_id`
- no `x-codex-turn-state`
- has `prompt_cache_key` length at least 8
- no `previous_response_id`

Behavior:

1. Compute local route id `pck:v1:<hash>`.
2. Source is `PromptCacheKey`.
3. Existing pck binding rotates the bound account first.
4. The pck route id is not sent upstream.
5. A successful first request may create a pck binding.

### `previous_response_id + prompt_cache_key` request

Behavior:

1. Compute the same local route id `pck:v1:<hash>`.
2. Source is `PromptCacheKeyExistingOnly`.
3. Existing pck binding may route the request to the same account.
4. No new pck binding is created if none exists.
5. No upstream thread/session/fallback anchor is derived from the pck binding.

### Native conversation request

If `conversation_id` is present, source is `NativeConversation`. Native conversation id wins over prompt-cache routing.

### Native turn-state request

If `x-codex-turn-state` is present, prompt-cache route binding is disabled to avoid conflicting with Codex-native session semantics.

## Verification commands

Run these before merge/rebase completion:

```bash
cargo test -p codexmanager-service route_conversation_id -- --nocapture
cargo test -p codexmanager-service prompt_cache_route_binding -- --nocapture
cargo test -p codexmanager-service parse_request_metadata -- --nocapture
cargo test -p codexmanager-service existing_only_prompt_cache_binding_is_not_used_as_fallback_thread_anchor -- --nocapture
cargo check -p codexmanager-service
git diff --check
```

Latest known local result:

- `route_conversation_id`: 5 passed
- `prompt_cache_route_binding`: 5 passed
- `parse_request_metadata`: 2 passed
- `existing_only_prompt_cache_binding_is_not_used_as_fallback_thread_anchor`: 1 passed
- `cargo check`: passed
- `git diff --check`: passed

## Merge protection checklist

Before merging this branch or resolving conflicts, verify these exact items:

- [ ] `RouteConversationSource::PromptCacheKeyExistingOnly` still exists.
- [ ] `RouteConversationSource::is_prompt_cache_key()` matches both prompt-cache variants.
- [ ] `PromptCacheKeyExistingOnly` cannot create initial bindings.
- [ ] `resolve_attempt_thread(...)` returns `None` for all prompt-cache sources.
- [ ] `conversation_binding_for_thread_anchor(...)` excludes all prompt-cache sources.
- [ ] `previous_response_id + prompt_cache_key` uses `PromptCacheKeyExistingOnly`.
- [ ] `record_conversation_binding_terminal_response(...)` preserves temporary-failover/manual-preferred no-rebind while the bound account remains selectable, and stale-binding rebind when it is not selectable.
- [ ] The pck route id hash does not include model.
- [ ] The pck route id is never written into upstream headers/body as a real conversation/thread/session id.
- [ ] Prompt-cache route binding still uses precise official responses path detection.
- [ ] The tests listed in “Verification commands” pass or failures are proven unrelated/upstream.

## Change log

### 2026-05-11 20:44 CST - `1f6091a9` and earlier PR #1 commits

- Modified files:
  - `crates/service/src/gateway/local_validation/request.rs`
  - `crates/service/src/gateway/local_validation/tests/request_tests.rs`
  - `crates/service/src/gateway/request/request_helpers.rs`
  - `crates/service/src/gateway/request/tests/request_helpers_tests.rs`
  - `crates/service/src/gateway/routing/conversation_binding.rs`
  - `docs/report/prompt-cache-route-binding-pr1.md`
- Summary:
  - Added route-only prompt-cache account binding.
  - Added existing-only routing for `previous_response_id + prompt_cache_key`.
  - Prevented temporary failover from drifting pck binding while allowing stale binding self-healing.
  - Excluded all prompt-cache sources from upstream thread/session/fallback-anchor derivation.
  - Added request metadata parsing for `previous_response_id`.
  - Added tests and PR-specific documentation.
- Verification:
  - `cargo test -p codexmanager-service route_conversation_id -- --nocapture`
  - `cargo test -p codexmanager-service prompt_cache_route_binding -- --nocapture`
  - `cargo test -p codexmanager-service parse_request_metadata -- --nocapture`
  - `cargo test -p codexmanager-service existing_only_prompt_cache_binding_is_not_used_as_fallback_thread_anchor -- --nocapture`
  - `cargo check -p codexmanager-service`
  - `git diff --check`

### 2026-05-11 20:56 CST - this commit

- Modified files:
  - `crates/service/src/gateway/routing/conversation_binding.rs`
  - `docs/LOCAL_MODIFICATIONS.md`
  - `docs/report/prompt-cache-route-binding-pr1.md`
- Summary:
  - Added `bound_account_selectable` to distinguish “binding account was not selected” from “binding account is no longer selectable”.
  - Changed pck rebind guard so manual preferred account cannot silently migrate an existing pck binding while the bound account is still selectable.
  - Added tests for manual preferred no-rebind and stale manual-preferred rebind behavior.
- Why it matters:
  - Manual account preference should affect current attempt ordering, not destroy cache affinity for an already-hot prompt-cache binding.
- Merge protection:
  - Preserve `bound_account_selectable` semantics; do not use `binding_selected` alone to decide stale pck rebind.
- Verification:
  - `cargo test -p codexmanager-service prompt_cache_manual_preferred -- --nocapture`

### Template for future updates

```markdown
### YYYY-MM-DD HH:mm CST - <commit or uncommitted>

- Modified files:
  - `path/to/file`
- Summary:
  - ...
- Why it matters:
  - ...
- Merge protection:
  - ...
- Verification:
  - `command`
```
