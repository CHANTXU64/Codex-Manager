use super::store_usage_snapshot;
use codexmanager_core::storage::{Storage, UsageSnapshotRecord};
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, Barrier, Mutex, Once, OnceLock};

static TEST_LOGGER: UsageSnapshotTestLogger = UsageSnapshotTestLogger;
static TEST_LOGGER_INIT: Once = Once::new();
static TEST_LOG_MESSAGES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

struct UsageSnapshotTestLogger;

impl log::Log for UsageSnapshotTestLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            test_log_messages()
                .lock()
                .expect("lock test log messages")
                .push(format!("{} {}", record.level(), record.args()));
        }
    }

    fn flush(&self) {}
}

fn test_log_messages() -> &'static Mutex<Vec<String>> {
    TEST_LOG_MESSAGES.get_or_init(|| Mutex::new(Vec::new()))
}

fn init_test_logger() {
    TEST_LOGGER_INIT.call_once(|| {
        let _ = log::set_logger(&TEST_LOGGER);
    });
    log::set_max_level(log::LevelFilter::Warn);
    test_log_messages()
        .lock()
        .expect("clear test log messages")
        .clear();
}

fn captured_test_logs() -> Vec<String> {
    test_log_messages()
        .lock()
        .expect("read test log messages")
        .clone()
}

fn temp_usage_db_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "{name}-{}-{}.db",
        std::process::id(),
        codexmanager_core::storage::now_ts()
    ))
}

fn usage_payload(used_percent: f64) -> serde_json::Value {
    usage_payload_with_options(used_percent, None, 18_000, "plus")
}

fn usage_payload_with_reset_at(used_percent: f64, reset_at: Option<i64>) -> serde_json::Value {
    usage_payload_with_options(used_percent, reset_at, 18_000, "plus")
}

fn usage_payload_with_window_seconds(used_percent: f64, window_seconds: i64) -> serde_json::Value {
    usage_payload_with_options(used_percent, None, window_seconds, "plus")
}

fn usage_payload_with_plan_type(used_percent: f64, plan_type: &str) -> serde_json::Value {
    usage_payload_with_options(used_percent, None, 18_000, plan_type)
}

fn usage_payload_without_window(used_percent: f64) -> serde_json::Value {
    json!({
        "rate_limit": {
            "primary_window": {
                "used_percent": used_percent
            },
            "secondary_window": {
                "used_percent": 0.0,
                "limit_window_seconds": 604_800
            }
        },
        "credits": {
            "plan_type": "plus"
        }
    })
}

fn usage_payload_without_secondary_window(
    used_percent: f64,
    window_seconds: i64,
) -> serde_json::Value {
    json!({
        "rate_limit": {
            "primary_window": {
                "used_percent": used_percent,
                "limit_window_seconds": window_seconds
            }
        },
        "credits": {
            "plan_type": "plus"
        }
    })
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

fn insert_snapshot_record(
    storage: &Storage,
    account_id: &str,
    used_percent: f64,
    window_minutes: Option<i64>,
    has_secondary_window: bool,
    captured_at: i64,
) {
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: account_id.to_string(),
            used_percent: Some(used_percent),
            window_minutes,
            resets_at: None,
            secondary_used_percent: has_secondary_window.then_some(0.0),
            secondary_window_minutes: has_secondary_window.then_some(10_080),
            secondary_resets_at: None,
            credits_json: None,
            captured_at,
        })
        .expect("insert snapshot record");
}

fn quota_consumption_rows(
    storage: &Storage,
) -> Vec<codexmanager_core::storage::QuotaConsumptionDailyRecord> {
    storage
        .list_quota_consumption_daily_between(0, i64::MAX)
        .expect("read quota consumption")
}

fn assert_single_consumption(storage: &Storage, account_id: &str, expected_percent: f64) {
    let rows = quota_consumption_rows(storage);
    assert_eq!(rows.len(), 1, "expected one quota row, got {rows:?}");
    assert_eq!(rows[0].account_id, account_id);
    assert!(
        (rows[0].consumed_percent - expected_percent).abs() < 0.000_001,
        "expected {expected_percent}% consumption, got {}",
        rows[0].consumed_percent
    );
}

#[test]
fn normal_usage_increase_counts_delta() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init storage");

    store_usage_snapshot(&storage, "acc-delta", usage_payload(20.0)).expect("store first snapshot");
    store_usage_snapshot(&storage, "acc-delta", usage_payload(23.5))
        .expect("store second snapshot");

    let rows = quota_consumption_rows(&storage);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].account_id, "acc-delta");
    assert!((rows[0].consumed_percent - 3.5).abs() < 0.000_001);
}

#[test]
fn usage_delta_uses_same_account_previous_snapshot_only() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init storage");

    store_usage_snapshot(&storage, "acc-a", usage_payload(20.0)).expect("store acc-a baseline");
    store_usage_snapshot(&storage, "acc-b", usage_payload(80.0))
        .expect("store acc-b first snapshot");

    let rows = quota_consumption_rows(&storage);
    assert!(
        rows.is_empty(),
        "first snapshot for acc-b must not compare against acc-a: {rows:?}"
    );

    store_usage_snapshot(&storage, "acc-a", usage_payload(25.0))
        .expect("store acc-a second snapshot");

    assert_single_consumption(&storage, "acc-a", 5.0);
}

#[test]
fn multiple_positive_snapshots_accumulate_same_day() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init storage");

    store_usage_snapshot(&storage, "acc-accumulate", usage_payload(20.0))
        .expect("store first snapshot");
    store_usage_snapshot(&storage, "acc-accumulate", usage_payload(23.0))
        .expect("store second snapshot");
    store_usage_snapshot(&storage, "acc-accumulate", usage_payload(26.0))
        .expect("store third snapshot");

    assert_single_consumption(&storage, "acc-accumulate", 6.0);
}

#[test]
fn quota_rollup_write_failure_is_warned_but_snapshot_still_persists() {
    init_test_logger();
    let db_path = temp_usage_db_path("codexmanager-usage-rollup-warning");
    let storage = Storage::open(&db_path).expect("open temp storage");
    storage.init().expect("init storage");
    rusqlite::Connection::open(&db_path)
        .expect("open raw sqlite connection")
        .execute_batch("DROP TABLE quota_consumption_daily;")
        .expect("drop quota table");

    store_usage_snapshot(&storage, "acc-rollup-warning", usage_payload(20.0))
        .expect("store first snapshot");
    store_usage_snapshot(&storage, "acc-rollup-warning", usage_payload(25.0))
        .expect("store second snapshot despite missing rollup table");

    assert_eq!(
        storage
            .usage_snapshot_count_for_account("acc-rollup-warning")
            .expect("count snapshots"),
        1,
        "snapshot persistence and pruning should continue even if quota rollup fails"
    );
    let logs = captured_test_logs();
    assert!(
        logs.iter().any(|line| {
            line.contains("quota consumption")
                && line.contains("acc-rollup-warning")
                && line.contains("failed")
        }),
        "expected warning about quota rollup failure, got {logs:?}"
    );
    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn concurrent_usage_snapshot_stores_do_not_double_count_same_delta() {
    let db_path = temp_usage_db_path("codexmanager-usage-concurrency");
    let storage = Storage::open(&db_path).expect("open temp storage");
    storage.init().expect("init storage");
    store_usage_snapshot(&storage, "acc-concurrent", usage_payload(20.0))
        .expect("store baseline snapshot");
    drop(storage);

    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let db_path = db_path.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            let storage = Storage::open(&db_path).expect("open worker storage");
            barrier.wait();
            store_usage_snapshot(&storage, "acc-concurrent", usage_payload(25.0))
                .expect("store concurrent snapshot");
        }));
    }
    barrier.wait();
    for handle in handles {
        handle.join().expect("worker thread");
    }

    let storage = Storage::open(&db_path).expect("reopen temp storage");
    let rows = quota_consumption_rows(&storage);
    let _ = std::fs::remove_file(&db_path);
    assert_eq!(rows.len(), 1, "expected one quota row, got {rows:?}");
    assert!(
        (rows[0].consumed_percent - 5.0).abs() < 0.000_001,
        "concurrent writes must not double count the same 20 -> 25 delta: got {}",
        rows[0].consumed_percent
    );
}

#[test]
fn reset_drop_to_one_percent_produces_no_consumption() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init storage");

    store_usage_snapshot(&storage, "acc-reset", usage_payload(87.0)).expect("store first snapshot");
    store_usage_snapshot(&storage, "acc-reset", usage_payload(1.0)).expect("store reset snapshot");

    let rows = quota_consumption_rows(&storage);
    assert!(
        rows.is_empty(),
        "post-reset 1% is not reliable enough to count as new consumption: {rows:?}"
    );
}

#[test]
fn resets_at_refresh_drift_drop_to_one_percent_produces_no_consumption() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init storage");

    let reset_ts = codexmanager_core::storage::now_ts();
    store_usage_snapshot(
        &storage,
        "acc-reset-drift-one",
        usage_payload_with_reset_at(87.0, Some(reset_ts)),
    )
    .expect("store first snapshot");
    store_usage_snapshot(
        &storage,
        "acc-reset-drift-one",
        usage_payload_with_reset_at(1.0, Some(reset_ts + 300)),
    )
    .expect("store reset snapshot");

    let rows = quota_consumption_rows(&storage);
    assert!(
        rows.is_empty(),
        "resets_at drift near refresh must not repeatedly count 1% as consumption: {rows:?}"
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
    assert!(
        rows.is_empty(),
        "zero post-reset should produce no record: {rows:?}"
    );
}

#[test]
fn new_account_first_snapshot_no_consumption() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init storage");

    store_usage_snapshot(&storage, "acc-new", usage_payload(45.0)).expect("store first snapshot");

    let rows = quota_consumption_rows(&storage);
    assert!(
        rows.is_empty(),
        "first snapshot should not produce consumption: {rows:?}"
    );

    store_usage_snapshot(&storage, "acc-new", usage_payload(52.0)).expect("store second snapshot");

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

    let future_ts = codexmanager_core::storage::now_ts() + 3600;
    store_usage_snapshot(
        &storage,
        "acc-reset-at-drift-growth",
        usage_payload_with_reset_at(20.0, Some(future_ts)),
    )
    .expect("store first snapshot");
    store_usage_snapshot(
        &storage,
        "acc-reset-at-drift-growth",
        usage_payload_with_reset_at(23.5, Some(future_ts + 300)),
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

    let future_ts = codexmanager_core::storage::now_ts() + 3600;
    store_usage_snapshot(
        &storage,
        "acc-reset-at-drift-same",
        usage_payload_with_reset_at(30.0, Some(future_ts)),
    )
    .expect("store first snapshot");
    store_usage_snapshot(
        &storage,
        "acc-reset-at-drift-same",
        usage_payload_with_reset_at(30.0, Some(future_ts + 840)),
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

    let future_ts = codexmanager_core::storage::now_ts() + 3600;
    store_usage_snapshot(
        &storage,
        "acc-reset-at-drift-drop",
        usage_payload_with_reset_at(5.0, Some(future_ts)),
    )
    .expect("store first snapshot");
    store_usage_snapshot(
        &storage,
        "acc-reset-at-drift-drop",
        usage_payload_with_reset_at(3.0, Some(future_ts + 300)),
    )
    .expect("store second snapshot");

    let rows = quota_consumption_rows(&storage);
    assert!(
        rows.is_empty(),
        "small drops plus resets_at drift are not enough to infer reset: {rows:?}"
    );
}

#[test]
fn negative_delta_does_not_create_consumption() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init storage");

    store_usage_snapshot(&storage, "acc-threshold", usage_payload(30.0))
        .expect("store first snapshot");
    store_usage_snapshot(&storage, "acc-threshold", usage_payload(20.0))
        .expect("store second snapshot");

    let rows = quota_consumption_rows(&storage);
    assert!(
        rows.is_empty(),
        "negative deltas are not reliable quota consumption: {rows:?}"
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
fn positive_delta_threshold_boundary_is_enforced() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init storage");

    store_usage_snapshot(&storage, "acc-boundary", usage_payload(0.0))
        .expect("store first snapshot");
    store_usage_snapshot(&storage, "acc-boundary", usage_payload(0.01))
        .expect("store threshold snapshot");

    let rows = quota_consumption_rows(&storage);
    assert!(
        rows.is_empty(),
        "exactly 0.01% should still be ignored: {rows:?}"
    );

    store_usage_snapshot(&storage, "acc-boundary", usage_payload(0.021))
        .expect("store above threshold snapshot");

    assert_single_consumption(&storage, "acc-boundary", 0.011);
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
    assert!(
        rows.is_empty(),
        "long window usage should be ignored: {rows:?}"
    );
}

#[test]
fn current_long_window_usage_is_not_counted_after_short_window() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init storage");

    store_usage_snapshot(&storage, "acc-current-long", usage_payload(20.0))
        .expect("store short-window baseline");
    store_usage_snapshot(
        &storage,
        "acc-current-long",
        usage_payload_with_window_seconds(25.0, 604_800),
    )
    .expect("store long-window snapshot");

    let rows = quota_consumption_rows(&storage);
    assert!(
        rows.is_empty(),
        "current long-window usage should not be counted: {rows:?}"
    );
}

#[test]
fn previous_long_window_without_current_secondary_does_not_count() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init storage");

    store_usage_snapshot(
        &storage,
        "acc-prev-long-no-secondary",
        usage_payload_without_secondary_window(20.0, 604_800),
    )
    .expect("store previous long-window snapshot");
    store_usage_snapshot(
        &storage,
        "acc-prev-long-no-secondary",
        usage_payload_without_secondary_window(25.0, 18_000),
    )
    .expect("store current single-window snapshot");

    let rows = quota_consumption_rows(&storage);
    assert!(
        rows.is_empty(),
        "single-window current usage after long-window baseline should not be counted: {rows:?}"
    );
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

#[test]
fn snapshot_within_current_window_tolerance_counts_consumption() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init storage");

    let old_ts = codexmanager_core::storage::now_ts() - (5 * 3600 + 10 * 60);
    insert_snapshot_record(&storage, "acc-window-fresh", 20.0, Some(300), true, old_ts);

    store_usage_snapshot(&storage, "acc-window-fresh", usage_payload(80.0))
        .expect("store current snapshot");

    assert_single_consumption(&storage, "acc-window-fresh", 60.0);
}

#[test]
fn stale_snapshot_does_not_create_consumption() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init storage");

    let old_ts = codexmanager_core::storage::now_ts() - 2 * 24 * 3600;
    let prev_record = codexmanager_core::storage::UsageSnapshotRecord {
        account_id: "acc-stale".to_string(),
        used_percent: Some(20.0),
        window_minutes: Some(300),
        resets_at: None,
        secondary_used_percent: None,
        secondary_window_minutes: None,
        secondary_resets_at: None,
        credits_json: None,
        captured_at: old_ts,
    };
    storage
        .insert_usage_snapshot(&prev_record)
        .expect("insert stale snapshot");

    store_usage_snapshot(&storage, "acc-stale", usage_payload(80.0))
        .expect("store current snapshot");

    let rows = quota_consumption_rows(&storage);
    assert!(
        rows.is_empty(),
        "stale snapshots should not be used to compute consumption delta: {rows:?}"
    );
}

#[test]
fn snapshot_older_than_current_window_does_not_create_consumption() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init storage");

    let old_ts = codexmanager_core::storage::now_ts() - (5 * 3600 + 16 * 60);
    insert_snapshot_record(
        &storage,
        "acc-window-expired",
        20.0,
        Some(300),
        true,
        old_ts,
    );

    store_usage_snapshot(&storage, "acc-window-expired", usage_payload(80.0))
        .expect("store current snapshot");

    let rows = quota_consumption_rows(&storage);
    assert!(
        rows.is_empty(),
        "snapshots older than the current 5h window should start a new baseline: {rows:?}"
    );
}

#[test]
fn unknown_window_uses_twenty_four_hour_freshness_fallback() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init storage");

    let old_ts = codexmanager_core::storage::now_ts() - 23 * 3600;
    insert_snapshot_record(&storage, "acc-unknown-window", 20.0, None, true, old_ts);

    store_usage_snapshot(
        &storage,
        "acc-unknown-window",
        usage_payload_without_window(50.0),
    )
    .expect("store current unknown-window snapshot");

    assert_single_consumption(&storage, "acc-unknown-window", 30.0);
}

#[test]
fn unknown_window_older_than_twenty_four_hours_does_not_count() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init storage");

    let old_ts = codexmanager_core::storage::now_ts() - 25 * 3600;
    insert_snapshot_record(
        &storage,
        "acc-unknown-window-stale",
        20.0,
        None,
        true,
        old_ts,
    );

    store_usage_snapshot(
        &storage,
        "acc-unknown-window-stale",
        usage_payload_without_window(50.0),
    )
    .expect("store current unknown-window snapshot");

    let rows = quota_consumption_rows(&storage);
    assert!(
        rows.is_empty(),
        "unknown-window snapshots older than 24h should start a new baseline: {rows:?}"
    );
}

#[test]
fn rolling_window_large_drop_does_not_create_consumption() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init storage");

    let future_ts = codexmanager_core::storage::now_ts() + 3600;
    store_usage_snapshot(
        &storage,
        "acc-rolling-drop",
        usage_payload_with_reset_at(90.0, Some(future_ts)),
    )
    .expect("store first snapshot");

    store_usage_snapshot(
        &storage,
        "acc-rolling-drop",
        usage_payload_with_reset_at(10.0, Some(future_ts + 120)),
    )
    .expect("store second snapshot");

    let rows = quota_consumption_rows(&storage);
    assert!(
        rows.is_empty(),
        "large drops can be rolling-window expiry and must not be counted: {rows:?}"
    );
}
