use chrono::{Datelike, Local, Timelike};
use rand::Rng;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use super::{
    is_keepalive_error_ignorable, parse_interval_secs,
    refresh_tokens_before_expiry_for_all_accounts, refresh_usage_for_polling_batch,
    run_gateway_keepalive_once, COMMON_POLL_FAILURE_BACKOFF_MAX_ENV, COMMON_POLL_JITTER_ENV,
    DEFAULT_GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_SECS, DEFAULT_GATEWAY_KEEPALIVE_JITTER_SECS,
    DEFAULT_USAGE_POLL_FAILURE_BACKOFF_MAX_SECS, DEFAULT_USAGE_POLL_JITTER_SECS,
    DEFAULT_WARMUP_MESSAGE, GATEWAY_KEEPALIVE_ENABLED, GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_ENV,
    GATEWAY_KEEPALIVE_INTERVAL_SECS, GATEWAY_KEEPALIVE_JITTER_ENV,
    TOKEN_REFRESH_FAILURE_BACKOFF_MAX_SECS, TOKEN_REFRESH_POLLING_ENABLED,
    TOKEN_REFRESH_POLL_INTERVAL_SECS_ATOMIC, USAGE_POLLING_ENABLED,
    USAGE_POLL_FAILURE_BACKOFF_MAX_ENV, USAGE_POLL_INTERVAL_SECS, USAGE_POLL_JITTER_ENV,
    WARMUP_CRON_ENABLED, WARMUP_CRON_EXPRESSION, WARMUP_CRON_NEXT_RUN_AT, WARMUP_MESSAGE,
};

/// 函数 `usage_polling_loop`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
pub(super) fn usage_polling_loop() {
    run_dynamic_poll_loop(
        "usage polling",
        || USAGE_POLLING_ENABLED.load(Ordering::Relaxed),
        || USAGE_POLL_INTERVAL_SECS.load(Ordering::Relaxed),
        || {
            parse_interval_with_fallback(
                USAGE_POLL_JITTER_ENV,
                COMMON_POLL_JITTER_ENV,
                DEFAULT_USAGE_POLL_JITTER_SECS,
                0,
            )
        },
        |interval_secs| {
            parse_interval_with_fallback(
                USAGE_POLL_FAILURE_BACKOFF_MAX_ENV,
                COMMON_POLL_FAILURE_BACKOFF_MAX_ENV,
                DEFAULT_USAGE_POLL_FAILURE_BACKOFF_MAX_SECS,
                interval_secs,
            )
        },
        refresh_usage_for_polling_batch,
        |_| true,
    );
}

/// 函数 `gateway_keepalive_loop`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
pub(super) fn gateway_keepalive_loop() {
    run_dynamic_poll_loop(
        "gateway keepalive",
        || GATEWAY_KEEPALIVE_ENABLED.load(Ordering::Relaxed),
        || GATEWAY_KEEPALIVE_INTERVAL_SECS.load(Ordering::Relaxed),
        || {
            parse_interval_with_fallback(
                GATEWAY_KEEPALIVE_JITTER_ENV,
                COMMON_POLL_JITTER_ENV,
                DEFAULT_GATEWAY_KEEPALIVE_JITTER_SECS,
                0,
            )
        },
        |interval_secs| {
            parse_interval_with_fallback(
                GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_ENV,
                COMMON_POLL_FAILURE_BACKOFF_MAX_ENV,
                DEFAULT_GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_SECS,
                interval_secs,
            )
        },
        run_gateway_keepalive_once,
        |err| !is_keepalive_error_ignorable(err),
    );
}

/// 函数 `token_refresh_polling_loop`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
pub(super) fn token_refresh_polling_loop() {
    run_dynamic_poll_loop(
        "token refresh polling",
        || TOKEN_REFRESH_POLLING_ENABLED.load(Ordering::Relaxed),
        || TOKEN_REFRESH_POLL_INTERVAL_SECS_ATOMIC.load(Ordering::Relaxed),
        || 0,
        |interval_secs| TOKEN_REFRESH_FAILURE_BACKOFF_MAX_SECS.max(interval_secs),
        refresh_tokens_before_expiry_for_all_accounts,
        |_| true,
    );
}

pub(super) fn warmup_cron_loop() {
    let mut last_invalid_expression = String::new();
    loop {
        if !WARMUP_CRON_ENABLED.load(Ordering::Relaxed) {
            WARMUP_CRON_NEXT_RUN_AT.store(0, Ordering::Relaxed);
            thread::sleep(Duration::from_secs(1));
            continue;
        }

        let expression = current_string(&WARMUP_CRON_EXPRESSION, "");
        let next_run_at = match next_cron_after(expression.as_str(), Local::now()) {
            Ok(next_run_at) => next_run_at,
            Err(err) => {
                WARMUP_CRON_NEXT_RUN_AT.store(0, Ordering::Relaxed);
                if last_invalid_expression != expression {
                    log::warn!("account warmup cron disabled by invalid expression: {err}");
                    last_invalid_expression = expression;
                }
                thread::sleep(Duration::from_secs(60));
                continue;
            }
        };
        last_invalid_expression.clear();
        WARMUP_CRON_NEXT_RUN_AT.store(next_run_at.timestamp(), Ordering::Relaxed);
        log::info!(
            "account warmup cron scheduled: expression=\"{}\" next_run_at={}",
            expression,
            next_run_at.to_rfc3339()
        );

        let delay = delay_until(next_run_at);
        sleep_with_recheck(delay, || {
            WARMUP_CRON_ENABLED.load(Ordering::Relaxed)
                && current_string(&WARMUP_CRON_EXPRESSION, "") == expression
        });
        if !WARMUP_CRON_ENABLED.load(Ordering::Relaxed)
            || current_string(&WARMUP_CRON_EXPRESSION, "") != expression
        {
            continue;
        }

        WARMUP_CRON_NEXT_RUN_AT.store(0, Ordering::Relaxed);
        let message = current_string(&WARMUP_MESSAGE, DEFAULT_WARMUP_MESSAGE);
        match crate::account_warmup::warmup_accounts(Vec::new(), message.as_str()) {
            Ok(result) => log::info!(
                "account warmup cron finished: requested={} succeeded={} failed={}",
                result.requested,
                result.succeeded,
                result.failed
            ),
            Err(err) => log::warn!("account warmup cron error: {err}"),
        }
    }
}

/// 函数 `parse_interval_with_fallback`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - primary_env: 参数 primary_env
/// - fallback_env: 参数 fallback_env
/// - default_secs: 参数 default_secs
/// - min_secs: 参数 min_secs
///
/// # 返回
/// 返回函数执行结果
fn parse_interval_with_fallback(
    primary_env: &str,
    fallback_env: &str,
    default_secs: u64,
    min_secs: u64,
) -> u64 {
    let primary = std::env::var(primary_env).ok();
    let fallback = std::env::var(fallback_env).ok();
    let raw = primary.as_deref().or(fallback.as_deref());
    parse_interval_secs(raw, default_secs, min_secs)
}

fn current_string(
    slot: &'static std::sync::OnceLock<std::sync::Mutex<String>>,
    default_value: &str,
) -> String {
    let guard = slot.get_or_init(|| std::sync::Mutex::new(default_value.to_string()));
    crate::lock_utils::lock_recover(guard, "background_task_string").clone()
}

fn sleep_with_recheck<F>(duration: Duration, keep_waiting: F)
where
    F: Fn() -> bool,
{
    let mut remaining = duration;
    while !remaining.is_zero() && keep_waiting() {
        let chunk = remaining.min(Duration::from_secs(5));
        thread::sleep(chunk);
        remaining = remaining.saturating_sub(chunk);
    }
}

fn delay_until(next: chrono::DateTime<Local>) -> Duration {
    let millis = next
        .signed_duration_since(Local::now())
        .num_milliseconds()
        .max(1) as u64;
    Duration::from_millis(millis)
}

pub(super) fn next_warmup_cron_timestamp(expression: &str) -> Option<i64> {
    next_cron_after(expression, Local::now())
        .ok()
        .map(|next| next.timestamp())
}

pub(crate) fn validate_warmup_cron_expression(expression: &str) -> Result<(), String> {
    next_cron_after(expression, Local::now()).map(|_| ())
}

fn next_cron_after(
    expression: &str,
    after: chrono::DateTime<Local>,
) -> Result<chrono::DateTime<Local>, String> {
    let schedules = parse_cron_schedules(expression)?;
    let mut next_match: Option<chrono::DateTime<Local>> = None;

    for (index, schedule) in schedules.into_iter().enumerate() {
        let candidate = next_cron_after_schedule(&schedule, after)
            .map_err(|err| format!("cron schedule #{} is invalid: {err}", index + 1))?;
        next_match = match next_match {
            Some(current) if current <= candidate => Some(current),
            _ => Some(candidate),
        };
    }

    next_match.ok_or_else(|| "cron expression has no schedule".to_string())
}

fn parse_cron_schedules(expression: &str) -> Result<Vec<CronSchedule>, String> {
    let mut schedules = Vec::new();
    for (index, item) in expression.split('|').enumerate() {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        schedules.push(
            CronSchedule::parse(trimmed)
                .map_err(|err| format!("cron schedule #{} is invalid: {err}", index + 1))?,
        );
    }
    if schedules.is_empty() {
        return Err("cron expression has no schedule".to_string());
    }
    Ok(schedules)
}

fn next_cron_after_schedule(
    schedule: &CronSchedule,
    after: chrono::DateTime<Local>,
) -> Result<chrono::DateTime<Local>, String> {
    let mut candidate =
        after + chrono::Duration::seconds(if schedule.has_seconds { 1 } else { 60 });
    candidate = candidate
        .with_nanosecond(0)
        .ok_or_else(|| "normalize cron timestamp failed".to_string())?;
    if !schedule.has_seconds {
        candidate = candidate
            .with_second(0)
            .ok_or_else(|| "normalize cron minute failed".to_string())?;
    }

    let max_iterations = if schedule.has_seconds {
        31_622_400
    } else {
        527_040
    };
    for _ in 0..max_iterations {
        if schedule.matches(candidate) {
            return Ok(candidate);
        }
        candidate =
            candidate + chrono::Duration::seconds(if schedule.has_seconds { 1 } else { 60 });
    }

    Err("cron expression has no matching time within one year".to_string())
}

#[derive(Debug, Clone)]
struct CronSchedule {
    seconds: Vec<u32>,
    minutes: Vec<u32>,
    hours: Vec<u32>,
    days_of_month: Vec<u32>,
    months: Vec<u32>,
    days_of_week: Vec<u32>,
    has_seconds: bool,
}

impl CronSchedule {
    fn parse(expression: &str) -> Result<Self, String> {
        let parts = expression.split_whitespace().collect::<Vec<_>>();
        let (has_seconds, fields) = match parts.len() {
            5 => (
                false,
                vec!["0", parts[0], parts[1], parts[2], parts[3], parts[4]],
            ),
            6 => (true, parts),
            _ => {
                return Err(
                    "cron expression must contain 5 fields or 6 fields with seconds".to_string(),
                )
            }
        };

        Ok(Self {
            seconds: parse_cron_field(fields[0], 0, 59, "seconds")?,
            minutes: parse_cron_field(fields[1], 0, 59, "minutes")?,
            hours: parse_cron_field(fields[2], 0, 23, "hours")?,
            days_of_month: parse_cron_field(fields[3], 1, 31, "day of month")?,
            months: parse_cron_field(fields[4], 1, 12, "month")?,
            days_of_week: parse_day_of_week_field(fields[5])?,
            has_seconds,
        })
    }

    fn matches(&self, value: chrono::DateTime<Local>) -> bool {
        self.seconds.contains(&value.second())
            && self.minutes.contains(&value.minute())
            && self.hours.contains(&value.hour())
            && self.days_of_month.contains(&value.day())
            && self.months.contains(&value.month())
            && self
                .days_of_week
                .contains(&value.weekday().num_days_from_sunday())
    }
}

fn parse_day_of_week_field(raw: &str) -> Result<Vec<u32>, String> {
    let mut values = parse_cron_field(raw, 0, 7, "day of week")?
        .into_iter()
        .map(|value| if value == 7 { 0 } else { value })
        .collect::<Vec<_>>();
    values.sort_unstable();
    values.dedup();
    Ok(values)
}

fn parse_cron_field(raw: &str, min: u32, max: u32, label: &str) -> Result<Vec<u32>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "?" {
        return Err(format!("{label} field is empty"));
    }

    let mut values = Vec::new();
    for segment in trimmed.split(',') {
        parse_cron_segment(segment.trim(), min, max, label, &mut values)?;
    }
    values.sort_unstable();
    values.dedup();
    if values.is_empty() {
        return Err(format!("{label} field has no values"));
    }
    Ok(values)
}

fn parse_cron_segment(
    segment: &str,
    min: u32,
    max: u32,
    label: &str,
    output: &mut Vec<u32>,
) -> Result<(), String> {
    if segment.is_empty() {
        return Err(format!("{label} field contains an empty segment"));
    }

    let (range_part, step) = match segment.split_once('/') {
        Some((range, step_raw)) => {
            let parsed_step = step_raw
                .parse::<u32>()
                .map_err(|_| format!("{label} step is invalid"))?;
            if parsed_step == 0 {
                return Err(format!("{label} step must be greater than 0"));
            }
            (range, parsed_step)
        }
        None => (segment, 1),
    };

    let (start, end) = if range_part == "*" || range_part == "?" {
        (min, max)
    } else if let Some((start_raw, end_raw)) = range_part.split_once('-') {
        (
            parse_cron_number(start_raw, min, max, label)?,
            parse_cron_number(end_raw, min, max, label)?,
        )
    } else {
        let value = parse_cron_number(range_part, min, max, label)?;
        (value, value)
    };

    if start > end {
        return Err(format!("{label} range start is greater than end"));
    }

    let mut value = start;
    while value <= end {
        output.push(value);
        match value.checked_add(step) {
            Some(next) => value = next,
            None => break,
        }
    }
    Ok(())
}

fn parse_cron_number(raw: &str, min: u32, max: u32, label: &str) -> Result<u32, String> {
    let value = raw
        .parse::<u32>()
        .map_err(|_| format!("{label} value is invalid"))?;
    if value < min || value > max {
        return Err(format!("{label} value {value} is outside {min}-{max}"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{next_cron_after, validate_warmup_cron_expression, CronSchedule};
    use chrono::{Datelike, Local, TimeZone, Timelike};

    #[test]
    fn next_cron_after_supports_five_field_every_four_hours() {
        let now = Local
            .with_ymd_and_hms(2026, 5, 12, 9, 30, 45)
            .single()
            .expect("local time");

        let next = next_cron_after("0 */4 * * *", now).expect("next cron");

        assert_eq!(next.hour(), 12);
        assert_eq!(next.minute(), 0);
        assert_eq!(next.second(), 0);
    }

    #[test]
    fn next_cron_after_supports_six_field_seconds() {
        let now = Local
            .with_ymd_and_hms(2026, 5, 12, 9, 30, 10)
            .single()
            .expect("local time");

        let next = next_cron_after("15 30 9 * * *", now).expect("next cron");

        assert_eq!(next.hour(), 9);
        assert_eq!(next.minute(), 30);
        assert_eq!(next.second(), 15);
    }

    #[test]
    fn next_cron_after_supports_five_field_daily_at_seven() {
        let now = Local
            .with_ymd_and_hms(2026, 5, 12, 6, 30, 0)
            .single()
            .expect("local time");

        let next = next_cron_after("0 7 * * *", now).expect("next cron");

        assert_eq!(next.day(), 12);
        assert_eq!(next.hour(), 7);
        assert_eq!(next.minute(), 0);
        assert_eq!(next.second(), 0);
    }

    #[test]
    fn next_cron_after_supports_pipe_separated_schedules() {
        let now = Local
            .with_ymd_and_hms(2026, 5, 12, 9, 30, 0)
            .single()
            .expect("local time");

        let next = next_cron_after("0 7 * * *|10 12 * * *|20 17 * * *", now).expect("next cron");

        assert_eq!(next.day(), 12);
        assert_eq!(next.hour(), 12);
        assert_eq!(next.minute(), 10);
        assert_eq!(next.second(), 0);
    }

    #[test]
    fn next_cron_after_rejects_any_invalid_pipe_schedule() {
        let now = Local
            .with_ymd_and_hms(2026, 5, 12, 9, 30, 0)
            .single()
            .expect("local time");

        let err = next_cron_after("0 7 * * *|60 12 * * *", now).expect_err("invalid cron");

        assert!(
            err.contains("schedule #2"),
            "error should identify item: {err}"
        );
        assert!(
            err.contains("minutes"),
            "error should identify field: {err}"
        );
    }

    #[test]
    fn validate_warmup_cron_expression_rejects_unreachable_schedule_item() {
        let err =
            validate_warmup_cron_expression("0 7 * * *|0 0 31 2 *").expect_err("invalid cron");

        assert!(
            err.contains("schedule #2"),
            "error should identify item: {err}"
        );
        assert!(
            err.contains("no matching time"),
            "error should explain schedule cannot run: {err}"
        );
    }

    #[test]
    fn cron_schedule_rejects_zero_step() {
        assert!(CronSchedule::parse("*/0 * * * *").is_err());
    }
}

/// 函数 `run_dynamic_poll_loop`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - loop_name: 参数 loop_name
/// - enabled: 参数 enabled
/// - interval_secs: 参数 interval_secs
/// - jitter_secs: 参数 jitter_secs
/// - failure_backoff_cap_secs: 参数 failure_backoff_cap_secs
/// - task: 参数 task
/// - should_log_error: 参数 should_log_error
///
/// # 返回
/// 无
fn run_dynamic_poll_loop<F, L, E, I, J, B>(
    loop_name: &str,
    enabled: E,
    interval_secs: I,
    jitter_secs: J,
    failure_backoff_cap_secs: B,
    mut task: F,
    mut should_log_error: L,
) where
    F: FnMut() -> Result<(), String>,
    L: FnMut(&str) -> bool,
    E: Fn() -> bool,
    I: Fn() -> u64,
    J: Fn() -> u64,
    B: Fn(u64) -> u64,
{
    let mut rng = rand::thread_rng();
    let mut consecutive_failures = 0u32;
    loop {
        if !enabled() {
            consecutive_failures = 0;
            thread::sleep(Duration::from_secs(1));
            continue;
        }

        let succeeded = match task() {
            Ok(_) => true,
            Err(err) => {
                if should_log_error(err.as_str()) {
                    log::warn!("{loop_name} error: {err}");
                }
                false
            }
        };

        if succeeded {
            consecutive_failures = 0;
        } else {
            consecutive_failures = consecutive_failures.saturating_add(1);
        }

        let base_interval_secs = interval_secs().max(1);
        let jitter_cap_secs = jitter_secs();
        let sampled_jitter = if jitter_cap_secs == 0 {
            Duration::ZERO
        } else {
            Duration::from_secs(rng.gen_range(0..=jitter_cap_secs))
        };
        let delay = next_dynamic_poll_delay(
            Duration::from_secs(base_interval_secs),
            Duration::from_secs(jitter_cap_secs),
            Duration::from_secs(
                failure_backoff_cap_secs(base_interval_secs).max(base_interval_secs),
            ),
            consecutive_failures,
            sampled_jitter,
        );
        thread::sleep(delay);
    }
}

/// 函数 `next_dynamic_poll_delay`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - interval: 参数 interval
/// - jitter_cap: 参数 jitter_cap
/// - failure_backoff_cap: 参数 failure_backoff_cap
/// - consecutive_failures: 参数 consecutive_failures
/// - sampled_jitter: 参数 sampled_jitter
///
/// # 返回
/// 返回函数执行结果
fn next_dynamic_poll_delay(
    interval: Duration,
    jitter_cap: Duration,
    failure_backoff_cap: Duration,
    consecutive_failures: u32,
    sampled_jitter: Duration,
) -> Duration {
    let base_delay =
        next_dynamic_failure_backoff(interval, failure_backoff_cap, consecutive_failures);
    let bounded_jitter = if jitter_cap.is_zero() {
        Duration::ZERO
    } else {
        sampled_jitter.min(jitter_cap)
    };
    base_delay
        .checked_add(bounded_jitter)
        .unwrap_or(Duration::MAX)
}

/// 函数 `next_dynamic_failure_backoff`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - interval: 参数 interval
/// - failure_backoff_cap: 参数 failure_backoff_cap
/// - consecutive_failures: 参数 consecutive_failures
///
/// # 返回
/// 返回函数执行结果
fn next_dynamic_failure_backoff(
    interval: Duration,
    failure_backoff_cap: Duration,
    consecutive_failures: u32,
) -> Duration {
    if consecutive_failures == 0 {
        return interval;
    }

    let base_ms = interval.as_millis();
    if base_ms == 0 {
        return interval;
    }

    let cap_ms = failure_backoff_cap.max(interval).as_millis();
    let shift = (consecutive_failures.saturating_sub(1)).min(20);
    let multiplier = 1u128 << shift;
    let scaled_ms = base_ms.saturating_mul(multiplier);
    let bounded_ms = scaled_ms.min(cap_ms).max(base_ms);
    if bounded_ms > u64::MAX as u128 {
        Duration::from_millis(u64::MAX)
    } else {
        Duration::from_millis(bounded_ms as u64)
    }
}
