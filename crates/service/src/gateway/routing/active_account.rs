use codexmanager_core::storage::{
    Account, ApiKeyActiveAccount, Storage, Token, UsageSnapshotRecord,
};
use std::collections::HashMap;

pub(crate) const ACTIVE_ACCOUNT_IDLE_TTL_SECS: i64 = 3600;
pub(crate) const ACTIVE_ACCOUNT_MAX_STICKY_SECS: i64 = 14400;
#[allow(dead_code)]
pub(crate) const MAX_SAME_ACCOUNT_TRANSIENT_ATTEMPTS: usize = 3;
pub(crate) const MAX_CONSECUTIVE_REAL_ERRORS: i64 = 3;
const URGENCY_NORMALIZATION_SECS: f64 = 518_400.0;
const URGENCY_MIN_TIME_UNTIL_RESET_SECS: i64 = 3600;
const URGENCY_MAX_TIME_UNTIL_RESET_SECS: i64 = 604_800;
const EXHAUSTED_PERCENT: f64 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveAccountDecisionReason {
    Reused,
    Selected,
    SelectedFallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveAccountDecision {
    pub account_id: String,
    pub reason: ActiveAccountDecisionReason,
}

pub(crate) fn get_or_select_active_account(
    storage: &Storage,
    key_id: &str,
    candidates: &[(Account, Token)],
    now: i64,
) -> Result<ActiveAccountDecision, String> {
    if candidates.is_empty() {
        return Err("no active account candidates".to_string());
    }

    let snapshots = load_snapshots(storage);
    if let Some(record) = storage
        .get_api_key_active_account(key_id)
        .map_err(|err| format!("load active account failed: {err}"))?
    {
        if active_record_is_usable(&record, candidates, &snapshots, now) {
            return Ok(ActiveAccountDecision {
                account_id: record.active_account_id,
                reason: ActiveAccountDecisionReason::Reused,
            });
        }
    }

    let (account_id, fallback) = select_candidate(candidates, &snapshots, now);
    let record = ApiKeyActiveAccount {
        key_id: key_id.to_string(),
        active_account_id: account_id.clone(),
        active_started_at: now,
        last_used_at: now,
        consecutive_real_errors: 0,
        last_switch_reason: Some(
            if fallback {
                "fallback_original_order"
            } else {
                "selected_by_weekly_urgency"
            }
            .to_string(),
        ),
        updated_at: now,
    };
    storage
        .upsert_api_key_active_account(&record)
        .map_err(|err| format!("upsert active account failed: {err}"))?;
    Ok(ActiveAccountDecision {
        account_id,
        reason: if fallback {
            ActiveAccountDecisionReason::SelectedFallback
        } else {
            ActiveAccountDecisionReason::Selected
        },
    })
}

pub(crate) fn apply_active_account_to_candidates(
    storage: &Storage,
    key_id: &str,
    candidates: &mut Vec<(Account, Token)>,
    now: i64,
) -> Result<Option<ActiveAccountDecision>, String> {
    if candidates.is_empty() {
        return Ok(None);
    }
    let decision = get_or_select_active_account(storage, key_id, candidates.as_slice(), now)?;
    rotate_to_account(candidates, decision.account_id.as_str());
    Ok(Some(decision))
}

pub(crate) fn record_active_account_success(
    storage: &Storage,
    key_id: &str,
    account_id: &str,
    now: i64,
) -> Result<(), String> {
    storage
        .touch_api_key_active_account_if_matches(key_id, account_id, now)
        .map_err(|err| format!("touch active account failed: {err}"))?;
    Ok(())
}

pub(crate) fn record_active_account_real_error(
    storage: &Storage,
    key_id: &str,
    account_id: &str,
    reason: &str,
    now: i64,
) -> Result<bool, String> {
    let updated = storage
        .increment_api_key_active_account_real_error_if_matches(key_id, account_id, now, reason)
        .map_err(|err| format!("increment active account errors failed: {err}"))?;
    if updated == 0 {
        return Ok(false);
    }
    let reached_threshold = storage
        .get_api_key_active_account(key_id)
        .map_err(|err| format!("load active account failed: {err}"))?
        .filter(|record| record.active_account_id == account_id)
        .is_some_and(|record| record.consecutive_real_errors >= MAX_CONSECUTIVE_REAL_ERRORS);
    if reached_threshold {
        super::mark_account_cooldown(
            account_id,
            super::CooldownReason::ActiveAccountRealErrorThreshold,
        );
        clear_active_account_if_matches(storage, key_id, account_id, reason)?;
        return Ok(true);
    }
    Ok(false)
}

pub(crate) fn clear_active_account_if_matches(
    storage: &Storage,
    key_id: &str,
    account_id: &str,
    reason: &str,
) -> Result<bool, String> {
    storage
        .clear_api_key_active_account_if_matches(key_id, account_id, reason)
        .map_err(|err| format!("clear active account failed: {err}"))
}

pub(crate) fn record_active_account_terminal_error(
    storage: &Storage,
    key_id: &str,
    account_id: &str,
    error: &str,
    now: i64,
) -> Result<(), String> {
    if is_client_disconnect_error(error) {
        return Ok(());
    }
    if is_direct_clear_error(error) {
        let _ = clear_active_account_if_matches(storage, key_id, account_id, error)?;
        return Ok(());
    }
    if is_transient_error(error) {
        let _ = record_active_account_real_error(storage, key_id, account_id, error, now)?;
    }
    Ok(())
}

pub(crate) fn is_client_disconnect_error(message: &str) -> bool {
    let normalized = message.trim().to_ascii_lowercase();
    normalized.contains("broken pipe")
        || normalized.contains("client disconnected")
        || normalized.contains("downstream disconnected")
        || normalized.contains("stream_interrupted")
        || normalized.contains("connection reset by peer")
        || normalized.contains("connection aborted")
        || normalized.contains("os error 32")
        || normalized.contains("os error 54")
        || normalized.contains("os error 104")
}

pub(crate) fn is_direct_clear_error(message: &str) -> bool {
    let normalized = message.trim().to_ascii_lowercase();
    crate::account_status::usage_limit_reason_from_message(message).is_some()
        || crate::account_status::deactivation_reason_from_message(message).is_some()
        || normalized.contains("rate-limited")
        || normalized.contains("rate limited")
        || normalized.contains("unauthorized")
        || normalized.contains("invalid token")
        || normalized.contains("challenge")
        || normalized.contains("account not found")
        || normalized.contains("workspace unavailable")
        || normalized.contains("account unavailable")
}

pub(crate) fn is_transient_error(message: &str) -> bool {
    let normalized = message.trim().to_ascii_lowercase();
    normalized.contains("upstream timeout")
        || normalized.contains("temporary upstream failure")
        || normalized.contains("network error")
        || normalized.contains("eof before response")
        || normalized.contains("connection reset by upstream")
        || normalized.contains("status 500")
        || normalized.contains("status=500")
        || normalized.contains("status 502")
        || normalized.contains("status=502")
        || normalized.contains("status 503")
        || normalized.contains("status=503")
        || normalized.contains("status 504")
        || normalized.contains("status=504")
}

fn rotate_to_account(candidates: &mut [(Account, Token)], account_id: &str) -> bool {
    let Some(index) = candidates
        .iter()
        .position(|(account, _)| account.id == account_id)
    else {
        return false;
    };
    if index > 0 {
        candidates.rotate_left(index);
    }
    true
}

fn load_snapshots(storage: &Storage) -> HashMap<String, UsageSnapshotRecord> {
    storage
        .latest_usage_snapshots_by_account()
        .unwrap_or_default()
        .into_iter()
        .map(|snap| (snap.account_id.clone(), snap))
        .collect()
}

fn active_record_is_usable(
    record: &ApiKeyActiveAccount,
    candidates: &[(Account, Token)],
    snapshots: &HashMap<String, UsageSnapshotRecord>,
    now: i64,
) -> bool {
    if now.saturating_sub(record.last_used_at) > ACTIVE_ACCOUNT_IDLE_TTL_SECS {
        return false;
    }
    if now.saturating_sub(record.active_started_at) > ACTIVE_ACCOUNT_MAX_STICKY_SECS {
        return false;
    }
    candidates.iter().any(|(account, token)| {
        account.id == record.active_account_id && candidate_is_usable(account, token, snapshots)
    })
}

fn candidate_is_usable(
    account: &Account,
    token: &Token,
    snapshots: &HashMap<String, UsageSnapshotRecord>,
) -> bool {
    account.status.trim().eq_ignore_ascii_case("active")
        && !token.access_token.trim().is_empty()
        && !super::is_account_in_cooldown(account.id.as_str())
        && !snapshot_is_exhausted(snapshots.get(account.id.as_str()))
}

fn snapshot_is_exhausted(snapshot: Option<&UsageSnapshotRecord>) -> bool {
    let Some(snapshot) = snapshot else {
        return false;
    };
    snapshot
        .used_percent
        .is_some_and(|pct| pct >= EXHAUSTED_PERCENT)
        || snapshot
            .secondary_used_percent
            .is_some_and(|pct| pct >= EXHAUSTED_PERCENT)
}

fn select_candidate(
    candidates: &[(Account, Token)],
    snapshots: &HashMap<String, UsageSnapshotRecord>,
    now: i64,
) -> (String, bool) {
    let mut eligible = candidates
        .iter()
        .filter(|(account, token)| candidate_is_usable(account, token, snapshots))
        .collect::<Vec<_>>();
    if eligible.is_empty() {
        log::warn!("event=active_account_no_eligible_candidates using original candidate order");
        return (candidates[0].0.id.clone(), true);
    }
    eligible.sort_by(|(left, _), (right, _)| {
        let left_score = urgency_score(snapshots.get(left.id.as_str()), now);
        let right_score = urgency_score(snapshots.get(right.id.as_str()), now);
        right_score
            .partial_cmp(&left_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.sort.cmp(&right.sort))
            .then_with(|| left.id.cmp(&right.id))
    });
    (eligible[0].0.id.clone(), false)
}

fn urgency_score(snapshot: Option<&UsageSnapshotRecord>, now: i64) -> f64 {
    let Some(snapshot) = snapshot else {
        return 0.0;
    };
    let weekly_remaining = snapshot
        .secondary_used_percent
        .map(|used| (100.0 - used).clamp(0.0, 100.0))
        .unwrap_or(50.0);
    let Some(resets_at) = snapshot.secondary_resets_at else {
        return weekly_remaining;
    };
    let effective_time = resets_at.saturating_sub(now).clamp(
        URGENCY_MIN_TIME_UNTIL_RESET_SECS,
        URGENCY_MAX_TIME_UNTIL_RESET_SECS,
    ) as f64;
    weekly_remaining * URGENCY_NORMALIZATION_SECS / effective_time
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexmanager_core::storage::now_ts;

    fn account(id: &str, sort: i64) -> Account {
        Account {
            id: id.to_string(),
            label: id.to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort,
            status: "active".to_string(),
            created_at: 1,
            updated_at: 1,
        }
    }

    fn token(account_id: &str) -> Token {
        Token {
            account_id: account_id.to_string(),
            id_token: "id".to_string(),
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: 1,
        }
    }

    fn usage(
        account_id: &str,
        primary: f64,
        secondary: Option<f64>,
        reset: Option<i64>,
    ) -> UsageSnapshotRecord {
        UsageSnapshotRecord {
            account_id: account_id.to_string(),
            used_percent: Some(primary),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: secondary,
            secondary_window_minutes: Some(10_080),
            secondary_resets_at: reset,
            credits_json: None,
            captured_at: 1,
        }
    }

    #[test]
    fn same_key_reuses_active_account() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();
        let candidates = vec![
            (account("acc-1", 0), token("acc-1")),
            (account("acc-2", 1), token("acc-2")),
        ];

        let first =
            get_or_select_active_account(&storage, "key-1", &candidates, now).expect("select");
        let second =
            get_or_select_active_account(&storage, "key-1", &candidates, now + 10).expect("reuse");

        assert_eq!(first.account_id, second.account_id);
        assert_eq!(second.reason, ActiveAccountDecisionReason::Reused);
    }

    #[test]
    fn different_keys_keep_independent_active_accounts() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();
        let candidates = vec![
            (account("acc-1", 0), token("acc-1")),
            (account("acc-2", 1), token("acc-2")),
        ];

        get_or_select_active_account(&storage, "key-a", &candidates, now).expect("select a");
        storage
            .upsert_api_key_active_account(&ApiKeyActiveAccount {
                key_id: "key-b".to_string(),
                active_account_id: "acc-2".to_string(),
                active_started_at: now,
                last_used_at: now,
                consecutive_real_errors: 0,
                last_switch_reason: None,
                updated_at: now,
            })
            .expect("seed b");

        assert_eq!(
            get_or_select_active_account(&storage, "key-b", &candidates, now + 1)
                .expect("select b")
                .account_id,
            "acc-2"
        );
    }

    #[test]
    fn idle_and_sticky_expiry_trigger_reselection() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = 10_000;
        let candidates = vec![(account("acc-expiry-only", 0), token("acc-expiry-only"))];
        storage
            .upsert_api_key_active_account(&ApiKeyActiveAccount {
                key_id: "key-1".to_string(),
                active_account_id: "acc-expiry-only".to_string(),
                active_started_at: now - ACTIVE_ACCOUNT_MAX_STICKY_SECS - 1,
                last_used_at: now,
                consecutive_real_errors: 0,
                last_switch_reason: None,
                updated_at: now,
            })
            .expect("seed");

        let selected =
            get_or_select_active_account(&storage, "key-1", &candidates, now).expect("select");

        assert_eq!(selected.reason, ActiveAccountDecisionReason::Selected);
        let record = storage
            .get_api_key_active_account("key-1")
            .expect("load")
            .expect("record");
        assert_eq!(record.active_started_at, now);

        storage
            .upsert_api_key_active_account(&ApiKeyActiveAccount {
                last_used_at: now - ACTIVE_ACCOUNT_IDLE_TTL_SECS - 1,
                ..record
            })
            .expect("seed idle");
        let idle =
            get_or_select_active_account(&storage, "key-1", &candidates, now).expect("select");
        assert_eq!(idle.reason, ActiveAccountDecisionReason::Selected);
    }

    #[test]
    fn weekly_urgency_prefers_expiring_remaining_quota() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = 10_000;
        storage
            .insert_usage_snapshot(&usage("acc-a", 10.0, Some(60.0), Some(now + 86_400)))
            .expect("usage a");
        storage
            .insert_usage_snapshot(&usage("acc-b", 10.0, Some(20.0), Some(now + 6 * 86_400)))
            .expect("usage b");
        let candidates = vec![
            (account("acc-b", 1), token("acc-b")),
            (account("acc-a", 0), token("acc-a")),
        ];

        let selected =
            get_or_select_active_account(&storage, "key-1", &candidates, now).expect("select");

        assert_eq!(selected.account_id, "acc-a");
    }

    #[test]
    fn primary_exhausted_account_is_not_selected_by_weekly_urgency() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = 10_000;
        storage
            .insert_usage_snapshot(&usage("acc-a", 100.0, Some(0.0), Some(now + 3600)))
            .expect("usage a");
        storage
            .insert_usage_snapshot(&usage("acc-b", 10.0, Some(80.0), Some(now + 6 * 86_400)))
            .expect("usage b");
        let candidates = vec![
            (account("acc-a", 0), token("acc-a")),
            (account("acc-b", 1), token("acc-b")),
        ];

        let selected =
            get_or_select_active_account(&storage, "key-1", &candidates, now).expect("select");

        assert_eq!(selected.account_id, "acc-b");
    }

    #[test]
    fn missing_secondary_reset_falls_back_to_stable_remaining_sort() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = 10_000;
        storage
            .insert_usage_snapshot(&usage("acc-a", 10.0, Some(20.0), None))
            .expect("usage a");
        storage
            .insert_usage_snapshot(&usage("acc-b", 10.0, Some(20.0), None))
            .expect("usage b");
        let candidates = vec![
            (account("acc-b", 2), token("acc-b")),
            (account("acc-a", 1), token("acc-a")),
        ];

        let selected =
            get_or_select_active_account(&storage, "key-1", &candidates, now).expect("select");

        assert_eq!(selected.account_id, "acc-a");
    }

    #[test]
    fn error_helpers_classify_transient_disconnect_and_direct_clear() {
        assert_eq!(MAX_SAME_ACCOUNT_TRANSIENT_ATTEMPTS, 3);
        assert!(is_transient_error("upstream timeout"));
        assert!(is_client_disconnect_error("broken pipe"));
        assert!(is_direct_clear_error("unauthorized"));
    }

    #[test]
    fn real_errors_clear_after_threshold() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = 10_000;
        storage
            .upsert_api_key_active_account(&ApiKeyActiveAccount {
                key_id: "key-1".to_string(),
                active_account_id: "acc-1".to_string(),
                active_started_at: now,
                last_used_at: now,
                consecutive_real_errors: MAX_CONSECUTIVE_REAL_ERRORS - 1,
                last_switch_reason: None,
                updated_at: now,
            })
            .expect("seed");

        let cleared = record_active_account_real_error(
            &storage,
            "key-1",
            "acc-1",
            "upstream timeout",
            now + 1,
        )
        .expect("record");

        assert!(cleared);
        assert!(storage
            .get_api_key_active_account("key-1")
            .expect("load")
            .is_none());
    }

    #[test]
    fn threshold_real_error_cooldown_forces_next_selection_to_other_account() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = 10_000;
        storage
            .insert_usage_snapshot(&usage(
                "acc-threshold-a",
                10.0,
                Some(10.0),
                Some(now + 3600),
            ))
            .expect("usage a");
        storage
            .insert_usage_snapshot(&usage(
                "acc-threshold-b",
                10.0,
                Some(90.0),
                Some(now + 6 * 86_400),
            ))
            .expect("usage b");
        let candidates = vec![
            (account("acc-threshold-a", 0), token("acc-threshold-a")),
            (account("acc-threshold-b", 1), token("acc-threshold-b")),
        ];
        storage
            .upsert_api_key_active_account(&ApiKeyActiveAccount {
                key_id: "key-threshold".to_string(),
                active_account_id: "acc-threshold-a".to_string(),
                active_started_at: now,
                last_used_at: now,
                consecutive_real_errors: MAX_CONSECUTIVE_REAL_ERRORS - 1,
                last_switch_reason: None,
                updated_at: now,
            })
            .expect("seed");

        let cleared = record_active_account_real_error(
            &storage,
            "key-threshold",
            "acc-threshold-a",
            "upstream timeout",
            now + 1,
        )
        .expect("record");
        let selected =
            get_or_select_active_account(&storage, "key-threshold", &candidates, now + 2)
                .expect("select next");

        assert!(cleared);
        assert_eq!(selected.account_id, "acc-threshold-b");
    }

    #[test]
    fn direct_clear_only_removes_matching_active_account() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = 10_000;
        storage
            .upsert_api_key_active_account(&ApiKeyActiveAccount {
                key_id: "key-direct-clear".to_string(),
                active_account_id: "acc-active".to_string(),
                active_started_at: now,
                last_used_at: now,
                consecutive_real_errors: 0,
                last_switch_reason: None,
                updated_at: now,
            })
            .expect("seed");

        let mismatched = clear_active_account_if_matches(
            &storage,
            "key-direct-clear",
            "acc-failover",
            "unauthorized",
        )
        .expect("clear mismatch");
        let still_active = storage
            .get_api_key_active_account("key-direct-clear")
            .expect("load")
            .expect("record");
        let matched = clear_active_account_if_matches(
            &storage,
            "key-direct-clear",
            "acc-active",
            "unauthorized",
        )
        .expect("clear match");

        assert!(!mismatched);
        assert_eq!(still_active.active_account_id, "acc-active");
        assert!(matched);
        assert!(storage
            .get_api_key_active_account("key-direct-clear")
            .expect("load cleared")
            .is_none());
    }

    #[test]
    fn gateway_candidate_entry_selects_and_reuses_same_key_active_account() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = 10_000;
        storage
            .insert_usage_snapshot(&usage("acc-entry-a", 10.0, Some(20.0), Some(now + 3600)))
            .expect("usage a");
        storage
            .insert_usage_snapshot(&usage(
                "acc-entry-b",
                10.0,
                Some(20.0),
                Some(now + 6 * 86_400),
            ))
            .expect("usage b");
        let mut first = vec![
            (account("acc-entry-b", 1), token("acc-entry-b")),
            (account("acc-entry-a", 0), token("acc-entry-a")),
        ];

        let selected = apply_active_account_to_candidates(&storage, "key-entry", &mut first, now)
            .expect("apply")
            .expect("decision");
        let mut second = vec![
            (account("acc-entry-b", 1), token("acc-entry-b")),
            (account("acc-entry-a", 0), token("acc-entry-a")),
        ];
        let reused =
            apply_active_account_to_candidates(&storage, "key-entry", &mut second, now + 10)
                .expect("apply reuse")
                .expect("decision");

        assert_eq!(selected.account_id, "acc-entry-a");
        assert_eq!(first[0].0.id, "acc-entry-a");
        assert_eq!(reused.reason, ActiveAccountDecisionReason::Reused);
        assert_eq!(second[0].0.id, "acc-entry-a");
    }

    #[test]
    fn gateway_candidate_entry_keeps_key_scoped_active_accounts_independent() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = 10_000;
        let candidates = vec![
            (account("acc-key-a", 0), token("acc-key-a")),
            (account("acc-key-b", 1), token("acc-key-b")),
        ];
        storage
            .upsert_api_key_active_account(&ApiKeyActiveAccount {
                key_id: "key-a".to_string(),
                active_account_id: "acc-key-a".to_string(),
                active_started_at: now,
                last_used_at: now,
                consecutive_real_errors: 0,
                last_switch_reason: None,
                updated_at: now,
            })
            .expect("seed key a");
        storage
            .upsert_api_key_active_account(&ApiKeyActiveAccount {
                key_id: "key-b".to_string(),
                active_account_id: "acc-key-b".to_string(),
                active_started_at: now,
                last_used_at: now,
                consecutive_real_errors: 0,
                last_switch_reason: None,
                updated_at: now,
            })
            .expect("seed key b");
        let mut key_a_candidates = candidates.clone();
        let mut key_b_candidates = candidates;

        apply_active_account_to_candidates(&storage, "key-a", &mut key_a_candidates, now + 1)
            .expect("apply key a");
        apply_active_account_to_candidates(&storage, "key-b", &mut key_b_candidates, now + 1)
            .expect("apply key b");

        assert_eq!(key_a_candidates[0].0.id, "acc-key-a");
        assert_eq!(key_b_candidates[0].0.id, "acc-key-b");
    }

    #[test]
    fn transient_errors_below_threshold_keep_active_account() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = 10_000;
        storage
            .upsert_api_key_active_account(&ApiKeyActiveAccount {
                key_id: "key-transient".to_string(),
                active_account_id: "acc-transient-a".to_string(),
                active_started_at: now,
                last_used_at: now,
                consecutive_real_errors: 0,
                last_switch_reason: None,
                updated_at: now,
            })
            .expect("seed");

        record_active_account_terminal_error(
            &storage,
            "key-transient",
            "acc-transient-a",
            "upstream timeout",
            now + 1,
        )
        .expect("record first");
        record_active_account_terminal_error(
            &storage,
            "key-transient",
            "acc-transient-a",
            "upstream timeout",
            now + 2,
        )
        .expect("record second");
        let record = storage
            .get_api_key_active_account("key-transient")
            .expect("load")
            .expect("record");

        assert_eq!(record.active_account_id, "acc-transient-a");
        assert_eq!(record.consecutive_real_errors, 2);
        assert!(!crate::gateway::is_account_in_cooldown("acc-transient-a"));
    }

    #[test]
    fn client_disconnect_terminal_error_does_not_mutate_active_account() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = 10_000;
        storage
            .upsert_api_key_active_account(&ApiKeyActiveAccount {
                key_id: "key-disconnect".to_string(),
                active_account_id: "acc-disconnect".to_string(),
                active_started_at: now,
                last_used_at: now,
                consecutive_real_errors: 1,
                last_switch_reason: Some("prior".to_string()),
                updated_at: now,
            })
            .expect("seed");

        record_active_account_terminal_error(
            &storage,
            "key-disconnect",
            "acc-disconnect",
            "broken pipe",
            now + 1,
        )
        .expect("record disconnect");
        let record = storage
            .get_api_key_active_account("key-disconnect")
            .expect("load")
            .expect("record");

        assert_eq!(record.active_account_id, "acc-disconnect");
        assert_eq!(record.consecutive_real_errors, 1);
        assert_eq!(record.last_switch_reason.as_deref(), Some("prior"));
    }

    #[test]
    fn stale_success_does_not_restore_previous_active_account() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = 10_000;
        storage
            .upsert_api_key_active_account(&ApiKeyActiveAccount {
                key_id: "key-cas-success".to_string(),
                active_account_id: "acc-cas-b".to_string(),
                active_started_at: now + 1,
                last_used_at: now + 1,
                consecutive_real_errors: 0,
                last_switch_reason: Some("selected_by_weekly_urgency".to_string()),
                updated_at: now + 1,
            })
            .expect("seed b");

        record_active_account_success(&storage, "key-cas-success", "acc-cas-a", now + 2)
            .expect("stale success");
        let record = storage
            .get_api_key_active_account("key-cas-success")
            .expect("load")
            .expect("record");

        assert_eq!(record.active_account_id, "acc-cas-b");
        assert_eq!(record.last_used_at, now + 1);
    }

    #[test]
    fn stale_transient_error_does_not_increment_new_active_account() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = 10_000;
        storage
            .upsert_api_key_active_account(&ApiKeyActiveAccount {
                key_id: "key-cas-error".to_string(),
                active_account_id: "acc-cas-b".to_string(),
                active_started_at: now + 1,
                last_used_at: now + 1,
                consecutive_real_errors: 0,
                last_switch_reason: Some("selected_by_weekly_urgency".to_string()),
                updated_at: now + 1,
            })
            .expect("seed b");

        let cleared = record_active_account_real_error(
            &storage,
            "key-cas-error",
            "acc-cas-a",
            "upstream timeout",
            now + 2,
        )
        .expect("stale error");
        let record = storage
            .get_api_key_active_account("key-cas-error")
            .expect("load")
            .expect("record");

        assert!(!cleared);
        assert_eq!(record.active_account_id, "acc-cas-b");
        assert_eq!(record.consecutive_real_errors, 0);
        assert_eq!(
            record.last_switch_reason.as_deref(),
            Some("selected_by_weekly_urgency")
        );
    }
}
