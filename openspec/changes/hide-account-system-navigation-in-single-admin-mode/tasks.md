## 1. Route Visibility Model

- [x] 1.1 Extend the top-level route helpers to accept a mode-aware shell access context that includes `role` and `mode`.
- [x] 1.2 Mark `/account-manager` and `/model-groups` as account-system-only top-level routes.
- [x] 1.3 Update allowed-route, route-label, first-allowed-route, and section-building helpers to use the mode-aware visibility contract.
- [x] 1.4 Add focused unit coverage for route helper behavior in `none`, `password`, and `accounts` modes if the existing test setup supports these helpers.

## 2. Shell Integration

- [x] 2.1 Update `Sidebar` to pass the current session mode into route section generation.
- [x] 2.2 Update `Header` and `ShellTabs` to use mode-aware route labels.
- [x] 2.3 Update `PageKeepAliveViewport` so shell tab pruning, direct route rendering, and fallback route selection all use the same mode-aware contract.
- [x] 2.4 Prevent loading-state flashes of account-system-only routes while the current session is still being resolved.

## 3. API Key UI

- [x] 3.1 Update `/apikeys` to keep normal platform key management available in `none` and `password` modes.
- [x] 3.2 Hide the member ownership table column on `/apikeys` unless the current mode is `accounts` and the existing ownership rules require it.
- [x] 3.3 Hide the member ownership selector in the API key create/edit modal unless the current mode is `accounts` and the existing ownership rules require it.
- [x] 3.4 Ensure saving a platform key in `none` or `password` mode does not require or submit a member owner.

## 4. Validation

- [x] 4.1 Run `pnpm run build:desktop` from `apps/` and resolve any static export or TypeScript failures.
- [x] 4.2 Run targeted automated tests for any route helper or UI logic tests added in this change.
- [x] 4.3 Use Codex Browser or Computer Use to run the app and perform full affected-functionality testing; build-only or code-only validation is not sufficient.
- [x] 4.4 In Browser or Computer Use, verify `none` mode: sidebar hides `/account-manager` and `/model-groups`, keeps `/accounts`, `/aggregate-api`, `/apikeys`, `/models`, `/logs`, `/settings`, `/plugins`, and `/author`, and direct navigation to hidden routes is pruned to an allowed route.
- [x] 4.5 In Browser or Computer Use, verify `password` mode after login: the same single-admin route visibility holds, and the logout/login flow still works.
- [x] 4.6 In Browser or Computer Use, verify `accounts` mode as an administrator: `/account-manager` and `/model-groups` are visible and reachable.
- [x] 4.7 In Browser or Computer Use, verify `accounts` mode as a member if a member account is available: member-only navigation remains unchanged and account-system admin routes remain hidden.
- [x] 4.8 In Browser or Computer Use, verify `/apikeys` in `none` and `password` modes: platform keys can be listed/opened, the member ownership column is hidden, the key editor has no member ownership selector, and saving does not require an owner.
- [x] 4.9 In Browser or Computer Use, verify `/apikeys` in `accounts` mode: existing member ownership behavior still appears when account-system ownership rules require it.
