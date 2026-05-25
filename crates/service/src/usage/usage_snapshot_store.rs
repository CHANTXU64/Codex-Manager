use crate::account_availability::{evaluate_snapshot, Availability};
use crate::account_status::{is_refresh_blocked_status_reason, set_account_status};
use chrono::{Local, LocalResult, TimeZone};
use codexmanager_core::storage::{now_ts, Storage, UsageSnapshotRecord};
use codexmanager_core::usage::parse_usage_snapshot;

const DEFAULT_USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT: usize = 1;
const USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT_ENV: &str =
    "CODEXMANAGER_USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT";
const DAY_SECONDS: i64 = 24 * 60 * 60;
const MINUTES_PER_DAY: i64 = 24 * 60;
const WINDOW_ROUNDING_BIAS: i64 = 3;
const RESET_DETECT_THRESHOLD: f64 = 10.0;
const MIN_CONSUMPTION_DELTA_PERCENT: f64 = 0.01;

fn usage_status_updates_blocked(storage: &Storage, account_id: &str, current_status: &str) -> bool {
    if current_status.trim().eq_ignore_ascii_case("disabled") {
        return true;
    }
    storage
        .latest_account_status_reasons(&[account_id.to_string()])
        .ok()
        .and_then(|mut reasons| reasons.remove(account_id))
        .as_deref()
        .is_some_and(is_refresh_blocked_status_reason)
}

fn is_long_window(window_minutes: Option<i64>) -> bool {
    window_minutes.is_some_and(|value| value > MINUTES_PER_DAY + WINDOW_ROUNDING_BIAS)
}

fn local_day_start_ts(ts: i64) -> i64 {
    let fallback = ts - ts.rem_euclid(DAY_SECONDS);
    let Some(local_dt) = Local.timestamp_opt(ts, 0).single() else {
        return fallback;
    };
    let Some(day_start_naive) = local_dt.date_naive().and_hms_opt(0, 0, 0) else {
        return fallback;
    };
    match Local.from_local_datetime(&day_start_naive) {
        LocalResult::Single(value) => value.timestamp(),
        LocalResult::Ambiguous(a, b) => a.timestamp().min(b.timestamp()),
        LocalResult::None => fallback,
    }
}

fn compute_consumption_delta(
    prev: &UsageSnapshotRecord,
    curr: &UsageSnapshotRecord,
) -> Option<f64> {
    let prev_used = prev.used_percent?;
    let curr_used = curr.used_percent?;
    if !prev_used.is_finite() || !curr_used.is_finite() {
        return None;
    }
    if is_long_window(curr.window_minutes) {
        return None;
    }
    let has_secondary_window =
        curr.secondary_used_percent.is_some() || curr.secondary_window_minutes.is_some();
    if !has_secondary_window && is_long_window(prev.window_minutes) {
        return None;
    }
    if is_reset_between(prev, curr) {
        // 重置后 curr_used 就是新窗口内的消耗量，不应丢弃。
        return (curr_used > MIN_CONSUMPTION_DELTA_PERCENT).then_some(curr_used);
    }
    let delta = curr_used - prev_used;
    (delta > MIN_CONSUMPTION_DELTA_PERCENT).then_some(delta)
}

fn is_reset_between(prev: &UsageSnapshotRecord, curr: &UsageSnapshotRecord) -> bool {
    let (Some(prev_used), Some(curr_used)) = (prev.used_percent, curr.used_percent) else {
        return false;
    };
    prev_used - curr_used > RESET_DETECT_THRESHOLD
}

/// 函数 `usage_snapshots_retain_per_account`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn usage_snapshots_retain_per_account() -> usize {
    std::env::var(USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT)
}

/// 函数 `apply_status_from_snapshot`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn apply_status_from_snapshot(
    storage: &Storage,
    record: &UsageSnapshotRecord,
) -> Availability {
    let availability = evaluate_snapshot(record);
    let current_status = storage
        .find_account_by_id(&record.account_id)
        .ok()
        .flatten()
        .map(|account| account.status)
        .unwrap_or_default();

    if usage_status_updates_blocked(storage, &record.account_id, &current_status) {
        return availability;
    }

    match availability {
        Availability::Available => {
            set_account_status(storage, &record.account_id, "active", "usage_ok");
        }
        Availability::Unavailable("usage_exhausted_primary" | "usage_exhausted_secondary") => {
            set_account_status(
                storage,
                &record.account_id,
                "limited",
                "usage_limit_exhausted",
            );
        }
        Availability::Unavailable(_) => {}
    }
    availability
}

/// 函数 `store_usage_snapshot`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn store_usage_snapshot(
    storage: &Storage,
    account_id: &str,
    value: serde_json::Value,
) -> Result<(), String> {
    // 解析并写入用量快照
    let parsed = parse_usage_snapshot(&value);
    let record = UsageSnapshotRecord {
        account_id: account_id.to_string(),
        used_percent: parsed.used_percent,
        window_minutes: parsed.window_minutes,
        resets_at: parsed.resets_at,
        secondary_used_percent: parsed.secondary_used_percent,
        secondary_window_minutes: parsed.secondary_window_minutes,
        secondary_resets_at: parsed.secondary_resets_at,
        credits_json: parsed.credits_json,
        captured_at: now_ts(),
    };
    let consumption_delta = storage
        .latest_usage_snapshot_for_account(account_id)
        .ok()
        .flatten()
        .as_ref()
        .and_then(|prev| compute_consumption_delta(prev, &record));
    storage
        .insert_usage_snapshot(&record)
        .map_err(|e| e.to_string())?;
    if let Some(delta) = consumption_delta {
        let day_start = local_day_start_ts(record.captured_at);
        let _ = storage.add_quota_consumption(account_id, day_start, delta);
    }
    let retain = usage_snapshots_retain_per_account();
    if retain > 0 {
        let _ = storage.prune_usage_snapshots_for_account(account_id, retain);
    }
    let _ = apply_status_from_snapshot(storage, &record);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::store_usage_snapshot;
    use codexmanager_core::storage::Storage;
    use serde_json::json;

    fn usage_payload(used_percent: f64) -> serde_json::Value {
        usage_payload_with_options(used_percent, None, 18_000, "plus")
    }

    fn usage_payload_with_reset_at(
        used_percent: f64,
        reset_at: Option<i64>,
    ) -> serde_json::Value {
        usage_payload_with_options(used_percent, reset_at, 18_000, "plus")
    }

    fn usage_payload_with_window_seconds(
        used_percent: f64,
        window_seconds: i64,
    ) -> serde_json::Value {
        usage_payload_with_options(used_percent, None, window_seconds, "plus")
    }

    fn usage_payload_with_plan_type(used_percent: f64, plan_type: &str) -> serde_json::Value {
        usage_payload_with_options(used_percent, None, 18_000, plan_type)
    }

    fn usage_payload_with_options(
        used_percent: f64,
        reset_at: Option<i64>,
        window_seconds: i64,
        plan_type: &str,
    ) -> serde_json::Value {
        let mut primary = json!({
            "used_percent": used_percent,
            "limit_window_seconds": window_seconds
        });
        if let Some(ts) = reset_at {
            primary["reset_at"] = json!(ts);
        }
        json!({
            "rate_limit": {
                "primary_window": primary,
                "secondary_window": {
                    "used_percent": 0.0,
                    "limit_window_seconds": 604_800
                }
            },
            "credits": {
                "plan_type": plan_type
            }
        })
    }

    fn quota_consumption_rows(
        storage: &Storage,
    ) -> Vec<codexmanager_core::storage::QuotaConsumptionDailyRecord> {
        storage
            .list_quota_consumption_daily_between(0, i64::MAX)
            .expect("read quota consumption")
    }

    #[test]
    fn normal_usage_increase_counts_delta() {
        let storage = Storage::open_in_memory().expect("open in memory");
        storage.init().expect("init storage");

        store_usage_snapshot(&storage, "acc-delta", usage_payload(20.0))
            .expect("store first snapshot");
        store_usage_snapshot(&storage, "acc-delta", usage_payload(23.5))
            .expect("store second snapshot");

        let rows = quota_consumption_rows(&storage);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].account_id, "acc-delta");
        assert!((rows[0].consumed_percent - 3.5).abs() < 0.000_001);
    }

    #[test]
    fn reset_captures_post_reset_consumption() {
        let storage = Storage::open_in_memory().expect("open in memory");
        storage.init().expect("init storage");

        store_usage_snapshot(&storage, "acc-reset", usage_payload(87.0))
            .expect("store first snapshot");
        store_usage_snapshot(&storage, "acc-reset", usage_payload(1.0))
            .expect("store reset snapshot");

        let rows = quota_consumption_rows(&storage);
        assert_eq!(rows.len(), 1);
        assert!(
            (rows[0].consumed_percent - 1.0).abs() < 0.000_001,
            "post-reset consumption should be captured: got {}",
            rows[0].consumed_percent
        );
    }

    #[test]
    fn reset_to_zero_produces_no_consumption() {
        let storage = Storage::open_in_memory().expect("open in memory");
        storage.init().expect("init storage");

        store_usage_snapshot(&storage, "acc-reset-zero", usage_payload(87.0))
            .expect("store first snapshot");
        store_usage_snapshot(&storage, "acc-reset-zero", usage_payload(0.0))
            .expect("store reset snapshot");

        let rows = quota_consumption_rows(&storage);
        assert!(rows.is_empty(), "zero post-reset should produce no record: {rows:?}");
    }

    #[test]
    fn new_account_first_snapshot_no_consumption() {
        let storage = Storage::open_in_memory().expect("open in memory");
        storage.init().expect("init storage");

        store_usage_snapshot(&storage, "acc-new", usage_payload(45.0))
            .expect("store first snapshot");

        let rows = quota_consumption_rows(&storage);
        assert!(
            rows.is_empty(),
            "first snapshot should not produce consumption: {rows:?}"
        );

        store_usage_snapshot(&storage, "acc-new", usage_payload(52.0))
            .expect("store second snapshot");

        let rows = quota_consumption_rows(&storage);
        assert_eq!(rows.len(), 1);
        assert!(
            (rows[0].consumed_percent - 7.0).abs() < 0.000_001,
            "second snapshot delta should be 7%: got {}",
            rows[0].consumed_percent
        );
    }

    #[test]
    fn resets_at_drift_during_growth_counts_only_positive_delta() {
        let storage = Storage::open_in_memory().expect("open in memory");
        storage.init().expect("init storage");

        store_usage_snapshot(
            &storage,
            "acc-reset-at-drift-growth",
            usage_payload_with_reset_at(20.0, Some(1_700_000_000)),
        )
        .expect("store first snapshot");
        store_usage_snapshot(
            &storage,
            "acc-reset-at-drift-growth",
            usage_payload_with_reset_at(23.5, Some(1_700_000_300)),
        )
        .expect("store second snapshot");

        let rows = quota_consumption_rows(&storage);
        assert_eq!(rows.len(), 1);
        assert!(
            (rows[0].consumed_percent - 3.5).abs() < 0.000_001,
            "resets_at drift must not count the full current used percent: got {}",
            rows[0].consumed_percent
        );
    }

    #[test]
    fn resets_at_drift_with_same_percent_does_not_create_consumption() {
        let storage = Storage::open_in_memory().expect("open in memory");
        storage.init().expect("init storage");

        store_usage_snapshot(
            &storage,
            "acc-reset-at-drift-same",
            usage_payload_with_reset_at(30.0, Some(1_700_000_000)),
        )
        .expect("store first snapshot");
        store_usage_snapshot(
            &storage,
            "acc-reset-at-drift-same",
            usage_payload_with_reset_at(30.0, Some(1_700_000_840)),
        )
        .expect("store second snapshot");

        let rows = quota_consumption_rows(&storage);
        assert!(
            rows.is_empty(),
            "resets_at drift alone must not create consumption: {rows:?}"
        );
    }

    #[test]
    fn resets_at_drift_with_small_drop_does_not_create_consumption() {
        let storage = Storage::open_in_memory().expect("open in memory");
        storage.init().expect("init storage");

        store_usage_snapshot(
            &storage,
            "acc-reset-at-drift-drop",
            usage_payload_with_reset_at(5.0, Some(1_700_000_000)),
        )
        .expect("store first snapshot");
        store_usage_snapshot(
            &storage,
            "acc-reset-at-drift-drop",
            usage_payload_with_reset_at(3.0, Some(1_700_000_300)),
        )
        .expect("store second snapshot");

        let rows = quota_consumption_rows(&storage);
        assert!(
            rows.is_empty(),
            "small drops plus resets_at drift are not enough to infer reset: {rows:?}"
        );
    }

    #[test]
    fn drop_equal_to_reset_threshold_does_not_count_as_reset() {
        let storage = Storage::open_in_memory().expect("open in memory");
        storage.init().expect("init storage");

        store_usage_snapshot(&storage, "acc-threshold", usage_payload(30.0))
            .expect("store first snapshot");
        store_usage_snapshot(&storage, "acc-threshold", usage_payload(20.0))
            .expect("store second snapshot");

        let rows = quota_consumption_rows(&storage);
        assert!(
            rows.is_empty(),
            "reset detection uses a drop greater than the threshold: {rows:?}"
        );
    }

    #[test]
    fn tiny_positive_delta_is_ignored() {
        let storage = Storage::open_in_memory().expect("open in memory");
        storage.init().expect("init storage");

        store_usage_snapshot(&storage, "acc-tiny-delta", usage_payload(20.0))
            .expect("store first snapshot");
        store_usage_snapshot(&storage, "acc-tiny-delta", usage_payload(20.009))
            .expect("store second snapshot");

        let rows = quota_consumption_rows(&storage);
        assert!(rows.is_empty(), "tiny delta should be ignored: {rows:?}");
    }

    #[test]
    fn long_window_usage_is_not_counted() {
        let storage = Storage::open_in_memory().expect("open in memory");
        storage.init().expect("init storage");

        store_usage_snapshot(
            &storage,
            "acc-long-window",
            usage_payload_with_window_seconds(20.0, 604_800),
        )
        .expect("store first snapshot");
        store_usage_snapshot(
            &storage,
            "acc-long-window",
            usage_payload_with_window_seconds(25.0, 604_800),
        )
        .expect("store second snapshot");

        let rows = quota_consumption_rows(&storage);
        assert!(rows.is_empty(), "long window usage should be ignored: {rows:?}");
    }

    #[test]
    fn free_plan_usage_with_primary_window_is_counted() {
        let storage = Storage::open_in_memory().expect("open in memory");
        storage.init().expect("init storage");

        store_usage_snapshot(
            &storage,
            "acc-free-plan",
            usage_payload_with_plan_type(20.0, "free"),
        )
        .expect("store first snapshot");
        store_usage_snapshot(
            &storage,
            "acc-free-plan",
            usage_payload_with_plan_type(25.0, "free"),
        )
        .expect("store second snapshot");

        let rows = quota_consumption_rows(&storage);
        assert_eq!(rows.len(), 1);
        assert!(
            (rows[0].consumed_percent - 5.0).abs() < 0.000_001,
            "free plan 5h primary window usage should be counted: got {}",
            rows[0].consumed_percent
        );
    }

    #[test]
    fn business_plan_usage_with_primary_window_is_counted() {
        let storage = Storage::open_in_memory().expect("open in memory");
        storage.init().expect("init storage");

        store_usage_snapshot(
            &storage,
            "acc-business-plan",
            usage_payload_with_plan_type(20.0, "business"),
        )
        .expect("store first snapshot");
        store_usage_snapshot(
            &storage,
            "acc-business-plan",
            usage_payload_with_plan_type(26.0, "business"),
        )
        .expect("store second snapshot");

        let rows = quota_consumption_rows(&storage);
        assert_eq!(rows.len(), 1);
        assert!(
            (rows[0].consumed_percent - 6.0).abs() < 0.000_001,
            "business plan 5h primary window usage should be counted: got {}",
            rows[0].consumed_percent
        );
    }
}
