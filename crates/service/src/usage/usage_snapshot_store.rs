use crate::account_availability::{evaluate_snapshot, Availability};
use crate::account_status::set_account_status;
use codexmanager_core::storage::{now_ts, Storage, UsageSnapshotRecord};
use codexmanager_core::usage::parse_usage_snapshot;

const DEFAULT_USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT: usize = 1;
const USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT_ENV: &str =
    "CODEXMANAGER_USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT";
const DAY_SECONDS: i64 = 24 * 60 * 60;
const MINUTES_PER_DAY: i64 = 1440;
const WINDOW_ROUNDING_BIAS: i64 = 3;
const RESET_DETECT_THRESHOLD: f64 = 10.0;

fn usage_status_updates_blocked(current_status: &str) -> bool {
    current_status.trim().eq_ignore_ascii_case("disabled")
}

fn usage_snapshots_retain_per_account() -> usize {
    std::env::var(USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT)
}

fn is_long_window(window_minutes: Option<i64>) -> bool {
    window_minutes.is_some_and(|value| value > MINUTES_PER_DAY + WINDOW_ROUNDING_BIAS)
}

fn is_free_plan_usage(raw: Option<&str>) -> bool {
    let Some(raw_str) = raw else {
        return false;
    };
    let text = raw_str.trim();
    if text.is_empty() {
        return false;
    }
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return false,
    };
    extract_plan_type(&value)
        .map(|t| t.contains("free"))
        .unwrap_or(false)
}

fn extract_plan_type(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Array(items) => items.iter().find_map(extract_plan_type),
        serde_json::Value::Object(map) => {
            for key in [
                "plan_type",
                "planType",
                "subscription_tier",
                "subscriptionTier",
                "tier",
                "account_type",
                "accountType",
                "type",
            ] {
                if let Some(text) = map.get(key).and_then(serde_json::Value::as_str) {
                    let normalized = text.trim().to_ascii_lowercase();
                    if !normalized.is_empty() {
                        return Some(normalized);
                    }
                }
            }
            map.values().find_map(extract_plan_type)
        }
        _ => None,
    }
}

fn local_day_start_ts(ts: i64) -> i64 {
    use chrono::{Local, TimeZone};
    let dt = Local
        .timestamp_opt(ts, 0)
        .single()
        .unwrap_or_else(|| Local::now());
    dt.date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|naive| {
            Local
                .from_local_datetime(&naive)
                .single()
                .map(|local| local.timestamp())
                .unwrap_or(ts - ts.rem_euclid(DAY_SECONDS))
        })
        .unwrap_or(ts - ts.rem_euclid(DAY_SECONDS))
}

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

    if usage_status_updates_blocked(&current_status) {
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

fn compute_consumption_delta(
    prev: &UsageSnapshotRecord,
    curr: &UsageSnapshotRecord,
) -> Option<f64> {
    let prev_used = prev.used_percent?;
    let curr_used = curr.used_percent?;

    if is_long_window(curr.window_minutes) || is_free_plan_usage(curr.credits_json.as_deref()) {
        return None;
    }

    let has_secondary =
        curr.secondary_used_percent.is_some() || curr.secondary_window_minutes.is_some();
    if !has_secondary && is_long_window(prev.window_minutes) {
        return None;
    }

    let reset_detected = prev_used - curr_used > RESET_DETECT_THRESHOLD;

    let delta = if reset_detected {
        0.0
    } else {
        (curr_used - prev_used).max(0.0)
    };

    if delta > 0.01 {
        Some(delta)
    } else {
        None
    }
}

pub(crate) fn store_usage_snapshot(
    storage: &Storage,
    account_id: &str,
    value: serde_json::Value,
) -> Result<(), String> {
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

    let prev = storage
        .latest_usage_snapshot_for_account(account_id)
        .ok()
        .flatten();

    if let Some(ref prev_record) = prev {
        if let Some(delta) = compute_consumption_delta(prev_record, &record) {
            let day_start = local_day_start_ts(record.captured_at);
            let _ = storage.add_quota_consumption(account_id, day_start, delta);
        }
    }

    storage
        .insert_usage_snapshot(&record)
        .map_err(|e| e.to_string())?;
    let retain = usage_snapshots_retain_per_account();
    if retain > 0 {
        let _ = storage.prune_usage_snapshots_for_account(account_id, retain);
    }
    let _ = apply_status_from_snapshot(storage, &record);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::compute_consumption_delta;
    use codexmanager_core::storage::UsageSnapshotRecord;

    fn snapshot(used_percent: f64) -> UsageSnapshotRecord {
        UsageSnapshotRecord {
            account_id: "acc-reset".to_string(),
            used_percent: Some(used_percent),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: Some(0.0),
            secondary_window_minutes: Some(10_080),
            secondary_resets_at: None,
            credits_json: None,
            captured_at: 1_700_000_000,
        }
    }

    #[test]
    fn reset_refresh_does_not_count_previous_remaining_quota_as_consumed() {
        let prev = snapshot(87.0);
        let curr = snapshot(0.0);

        assert_eq!(compute_consumption_delta(&prev, &curr), None);
    }

    #[test]
    fn normal_usage_increase_counts_delta() {
        let prev = snapshot(20.0);
        let curr = snapshot(23.5);

        assert_eq!(compute_consumption_delta(&prev, &curr), Some(3.5));
    }
}
