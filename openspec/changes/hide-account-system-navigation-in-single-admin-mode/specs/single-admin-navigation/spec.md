## ADDED Requirements

### Requirement: Single-admin modes hide account-system navigation
The shell SHALL treat `none` and `password` Web access modes as single-admin navigation modes and MUST NOT show account-system-only top-level routes in those modes.

#### Scenario: Sidebar in none mode
- **WHEN** the current session mode is `none` and the current role is `system_admin` or `admin`
- **THEN** the sidebar MUST NOT render top-level entries for `/account-manager` or `/model-groups`
- **AND** the sidebar MUST continue to render `/accounts`, `/aggregate-api`, `/apikeys`, `/models`, `/logs`, `/settings`, `/plugins`, and `/author` when allowed by role

#### Scenario: Sidebar in password mode
- **WHEN** the current session mode is `password` and the current role is `system_admin` or `admin`
- **THEN** the sidebar MUST NOT render top-level entries for `/account-manager` or `/model-groups`
- **AND** the sidebar MUST continue to render the single-admin administration routes

### Requirement: Accounts mode keeps account-system administration visible
The shell SHALL keep account-system-only top-level routes visible for administrator roles when the current Web access mode is `accounts`.

#### Scenario: Accounts mode administrator
- **WHEN** the current session mode is `accounts` and the current role is `system_admin` or `admin`
- **THEN** the sidebar MUST render `/account-manager` and `/model-groups`
- **AND** the routes MUST use their administrator labels

#### Scenario: Accounts mode member
- **WHEN** the current session mode is `accounts` and the current role is `member`
- **THEN** the sidebar MUST continue to render only member-allowed routes
- **AND** `/account-manager` and `/model-groups` MUST remain hidden

### Requirement: Route rendering uses the same mode-aware visibility contract
The application SHALL use one mode-aware top-level route visibility contract for sidebar rendering, shell tab pruning, keep-alive page rendering, page titles, and direct shell navigation.

#### Scenario: Direct navigation to hidden route
- **WHEN** a single-admin mode user directly navigates to `/account-manager` or `/model-groups`
- **THEN** the shell MUST NOT render that page panel
- **AND** the shell MUST redirect or prune the active shell path to the first allowed route

#### Scenario: Mode switch prunes stale tabs
- **WHEN** the user switches from `accounts` mode to `none` or `password` mode
- **THEN** any open shell tabs for `/account-manager` or `/model-groups` MUST be removed
- **AND** the active route MUST become an allowed single-admin route if it was previously account-system-only

### Requirement: Platform key management remains available without member ownership controls
The application SHALL keep platform key management available in `none` and `password` modes while hiding account-system member ownership UI in those modes.

#### Scenario: API key list in single-admin mode
- **WHEN** the current session mode is `none` or `password` and an administrator views `/apikeys`
- **THEN** the page MUST allow normal platform key listing and management
- **AND** the table MUST NOT show the member ownership column

#### Scenario: API key editor in single-admin mode
- **WHEN** the current session mode is `none` or `password` and an administrator creates or edits a platform key
- **THEN** the key editor MUST NOT show a member ownership selector
- **AND** saving a key MUST NOT require selecting a member owner

#### Scenario: API key ownership in accounts mode
- **WHEN** the current session mode is `accounts` and account distribution or member ownership is enabled
- **THEN** the API key page MAY show member ownership columns and selectors according to existing account-system rules
