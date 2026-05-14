## Context

The frontend shell currently decides top-level navigation from `session.role`. In `none` and `password` Web access modes, RPC requests do not carry an app-user actor, so the service returns `system_admin`. That makes single-admin modes render the same administrator menu as `accounts` mode, including member-account and model-group administration.

This change should clarify the product model without changing backend authorization. `none` and `password` remain single-admin modes, while `accounts` remains the mode that exposes CodexManager member accounts, member wallets, model group assignment, and member-owned platform keys.

## Goals / Non-Goals

**Goals:**

- Hide account-system-only navigation in `none` and `password` modes.
- Preserve access to single-admin administration workflows, including OpenAI account pool management and platform key management.
- Keep route visibility consistent across sidebar items, shell tabs, keep-alive page rendering, titles, and direct shell navigation.
- Keep account-system-only controls on the API key page out of single-admin modes.

**Non-Goals:**

- Do not change backend RPC authorization or introduce a new permission boundary.
- Do not remove the account-system pages or disable them in `accounts` mode.
- Do not change the meaning of `none`, `password`, or `accounts` Web access modes.

## Decisions

1. Add a shell access context that includes both role and Web auth mode.

   The route helpers should accept a context such as `{ role, mode }` or equivalent parameters. Role-only checks are not expressive enough because `none/password` intentionally return an administrator role but should not expose account-system navigation.

   Alternative considered: filter only in `Sidebar`. This would leave direct route rendering and shell tabs inconsistent, so it is not sufficient.

2. Treat `/account-manager` and `/model-groups` as account-system-only top-level routes.

   These routes primarily manage app users, member wallets, member model assignments, and team quota distribution. They are meaningful in `accounts` mode and confusing in single-admin modes. `/accounts` remains visible because it manages upstream OpenAI accounts, not CodexManager login users.

   Alternative considered: hide every route containing the word "account". This would incorrectly remove the OpenAI account pool, which is core to single-admin operation.

3. Keep platform key management visible but mode-adjust member ownership UI.

   `/apikeys` is required in single-admin modes, but member ownership columns and selectors belong to the account-system workflow. The platform key page should render without member ownership controls when the current mode is `none` or `password`.

   Alternative considered: hide `/apikeys` in single-admin modes. This would remove a core single-admin workflow and is not acceptable.

4. Use frontend gating as product navigation, not security.

   The implementation should not claim that hidden routes are inaccessible as a security boundary unless the backend is also changed. For this change, hidden navigation and route pruning are the expected behavior.

## Risks / Trade-offs

- Hidden routes may still be callable through RPC in single-admin modes -> Make the scope explicit in docs and tests as a UX/navigation change.
- Stale shell tabs may reference now-hidden routes after switching modes -> Reuse the centralized route helper in tab pruning and active page rendering.
- `session` can be briefly undefined while loading -> Preserve current loading skeleton behavior and avoid flashing account-system routes before the session mode is known.
- API key page may still need admin-only data in single-admin mode -> Keep normal platform key queries available and only suppress member ownership controls.
