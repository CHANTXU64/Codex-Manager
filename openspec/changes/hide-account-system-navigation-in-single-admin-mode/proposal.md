## Why

`none` and `password` Web access modes are single-admin modes, but the current shell exposes the same member-account administration navigation that is only meaningful in the `accounts` mode. This makes the product model ambiguous and can lead users to configure member, wallet, and model-group workflows that do not apply to single-admin operation.

## What Changes

- Hide member-account-system navigation when the current Web access mode is `none` or `password`.
- Keep single-admin essentials visible in `none` and `password` modes: dashboard, OpenAI account pool, aggregate APIs, platform keys, model catalog, request logs, system settings, plugins, and author/sponsor page.
- Keep member-account-system navigation visible for `accounts` mode administrators.
- Apply the same visibility contract to sidebar rendering, shell tab pruning, page rendering, route labels, and direct shell navigation checks.
- Preserve backend authorization behavior for this change; this is a UX/navigation scope change, not a security boundary.

## Capabilities

### New Capabilities
- `single-admin-navigation`: Defines which top-level shell routes are visible and reachable in single-admin Web access modes versus account-system mode.

### Modified Capabilities

## Impact

- Affected frontend shell routing and navigation files under `apps/src/lib/app-shell/` and `apps/src/components/layout/`.
- Affected pages whose entry points should be hidden outside `accounts` mode, especially `/account-manager` and `/model-groups`.
- Affected API key UI only where member ownership controls are shown; platform key management remains available in single-admin modes.
- No new runtime dependencies or backend RPC changes are expected.
