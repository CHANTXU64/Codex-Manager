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
const MIN_CONSUMPTION_DELTA_PERCENT: f64 = 0.01;
const STALE_SNAPSHOT_TOLERANCE_SECONDS: i64 = 15 * 60;

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

    let max_snapshot_age_seconds = curr
        .window_minutes
        .filter(|minutes| *minutes > 0)
        .map(|minutes| {
            minutes
                .saturating_mul(60)
                .saturating_add(STALE_SNAPSHOT_TOLERANCE_SECONDS)
        })
        .unwrap_or(DAY_SECONDS);
    if curr.captured_at.saturating_sub(prev.captured_at) > max_snapshot_age_seconds {
        return None;
    }

    let delta = curr_used - prev_used;
    (delta > MIN_CONSUMPTION_DELTA_PERCENT).then_some(delta)
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
    let retain = usage_snapshots_retain_per_account();
    let day_start = local_day_start_ts(record.captured_at);
    let outcome = storage
        .insert_usage_snapshot_with_quota_consumption(&record, retain, day_start, |prev, curr| {
            prev.and_then(|prev| compute_consumption_delta(prev, curr))
        })
        .map_err(|e| e.to_string())?;
    if let Some(err) = outcome.quota_consumption_error {
        log::warn!(
            "quota consumption rollup write failed for account {}: {}",
            account_id,
            err
        );
    }
    let _ = apply_status_from_snapshot(storage, &record);
    Ok(())
}

#[cfg(test)]
#[path = "tests/usage_snapshot_store_tests.rs"]
mod tests;
