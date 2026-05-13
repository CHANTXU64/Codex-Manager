use serde::Serialize;
use std::sync::atomic::Ordering;

use super::{
    parse_interval_secs, BACKGROUND_TASKS_CONFIG_LOADED, BACKGROUND_TASK_RESTART_REQUIRED_KEYS,
    DEFAULT_GATEWAY_KEEPALIVE_INTERVAL_SECS, DEFAULT_HTTP_STREAM_WORKER_FACTOR,
    DEFAULT_HTTP_STREAM_WORKER_MIN, DEFAULT_HTTP_WORKER_FACTOR, DEFAULT_HTTP_WORKER_MIN,
    DEFAULT_TOKEN_REFRESH_POLL_INTERVAL_SECS, DEFAULT_USAGE_POLL_INTERVAL_SECS,
    DEFAULT_USAGE_REFRESH_WORKERS, DEFAULT_WARMUP_CRON_EXPRESSION, DEFAULT_WARMUP_MESSAGE,
    ENV_DISABLE_POLLING, ENV_GATEWAY_KEEPALIVE_ENABLED, ENV_GATEWAY_KEEPALIVE_INTERVAL_SECS,
    ENV_HTTP_STREAM_WORKER_FACTOR, ENV_HTTP_STREAM_WORKER_MIN, ENV_HTTP_WORKER_FACTOR,
    ENV_HTTP_WORKER_MIN, ENV_TOKEN_REFRESH_POLLING_ENABLED, ENV_TOKEN_REFRESH_POLL_INTERVAL_SECS,
    ENV_USAGE_POLLING_ENABLED, ENV_USAGE_POLL_INTERVAL_SECS, ENV_WARMUP_CRON_ENABLED,
    ENV_WARMUP_CRON_EXPRESSION, ENV_WARMUP_MESSAGE, GATEWAY_KEEPALIVE_ENABLED,
    GATEWAY_KEEPALIVE_INTERVAL_SECS, HTTP_STREAM_WORKER_FACTOR, HTTP_STREAM_WORKER_MIN,
    HTTP_WORKER_FACTOR, HTTP_WORKER_MIN, MIN_GATEWAY_KEEPALIVE_INTERVAL_SECS,
    MIN_TOKEN_REFRESH_POLL_INTERVAL_SECS, MIN_USAGE_POLL_INTERVAL_SECS,
    TOKEN_REFRESH_POLLING_ENABLED, TOKEN_REFRESH_POLL_INTERVAL_SECS_ATOMIC, USAGE_POLLING_ENABLED,
    USAGE_POLL_INTERVAL_SECS, USAGE_REFRESH_WORKERS, USAGE_REFRESH_WORKERS_ENV,
    WARMUP_CRON_ENABLED, WARMUP_CRON_EXPRESSION, WARMUP_CRON_NEXT_RUN_AT, WARMUP_MESSAGE,
};

use super::runner::next_warmup_cron_timestamp;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BackgroundTasksSettings {
    usage_polling_enabled: bool,
    usage_poll_interval_secs: u64,
    gateway_keepalive_enabled: bool,
    gateway_keepalive_interval_secs: u64,
    token_refresh_polling_enabled: bool,
    token_refresh_poll_interval_secs: u64,
    usage_refresh_workers: usize,
    http_worker_factor: usize,
    http_worker_min: usize,
    http_stream_worker_factor: usize,
    http_stream_worker_min: usize,
    warmup_cron_enabled: bool,
    warmup_cron_expression: String,
    warmup_message: String,
    warmup_cron_next_run_at: Option<i64>,
    requires_restart_keys: Vec<&'static str>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BackgroundTasksSettingsPatch {
    pub usage_polling_enabled: Option<bool>,
    pub usage_poll_interval_secs: Option<u64>,
    pub gateway_keepalive_enabled: Option<bool>,
    pub gateway_keepalive_interval_secs: Option<u64>,
    pub token_refresh_polling_enabled: Option<bool>,
    pub token_refresh_poll_interval_secs: Option<u64>,
    pub usage_refresh_workers: Option<usize>,
    pub http_worker_factor: Option<usize>,
    pub http_worker_min: Option<usize>,
    pub http_stream_worker_factor: Option<usize>,
    pub http_stream_worker_min: Option<usize>,
    pub warmup_cron_enabled: Option<bool>,
    pub warmup_cron_expression: Option<String>,
    pub warmup_message: Option<String>,
}

/// 函数 `background_tasks_settings`
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
pub(crate) fn background_tasks_settings() -> BackgroundTasksSettings {
    ensure_background_tasks_config_loaded();
    let warmup_cron_enabled = WARMUP_CRON_ENABLED.load(Ordering::Relaxed);
    let warmup_cron_expression =
        current_mutex_string(&WARMUP_CRON_EXPRESSION, DEFAULT_WARMUP_CRON_EXPRESSION);
    let warmup_cron_next_run_at = if warmup_cron_enabled {
        positive_timestamp_or_none(WARMUP_CRON_NEXT_RUN_AT.load(Ordering::Relaxed))
            .or_else(|| next_warmup_cron_timestamp(warmup_cron_expression.as_str()))
    } else {
        None
    };
    BackgroundTasksSettings {
        usage_polling_enabled: USAGE_POLLING_ENABLED.load(Ordering::Relaxed),
        usage_poll_interval_secs: USAGE_POLL_INTERVAL_SECS.load(Ordering::Relaxed),
        gateway_keepalive_enabled: GATEWAY_KEEPALIVE_ENABLED.load(Ordering::Relaxed),
        gateway_keepalive_interval_secs: GATEWAY_KEEPALIVE_INTERVAL_SECS.load(Ordering::Relaxed),
        token_refresh_polling_enabled: TOKEN_REFRESH_POLLING_ENABLED.load(Ordering::Relaxed),
        token_refresh_poll_interval_secs: TOKEN_REFRESH_POLL_INTERVAL_SECS_ATOMIC
            .load(Ordering::Relaxed),
        usage_refresh_workers: USAGE_REFRESH_WORKERS.load(Ordering::Relaxed),
        http_worker_factor: HTTP_WORKER_FACTOR.load(Ordering::Relaxed),
        http_worker_min: HTTP_WORKER_MIN.load(Ordering::Relaxed),
        http_stream_worker_factor: HTTP_STREAM_WORKER_FACTOR.load(Ordering::Relaxed),
        http_stream_worker_min: HTTP_STREAM_WORKER_MIN.load(Ordering::Relaxed),
        warmup_cron_enabled,
        warmup_cron_expression,
        warmup_message: current_mutex_string(&WARMUP_MESSAGE, DEFAULT_WARMUP_MESSAGE),
        warmup_cron_next_run_at,
        requires_restart_keys: BACKGROUND_TASK_RESTART_REQUIRED_KEYS.to_vec(),
    }
}

fn positive_timestamp_or_none(value: i64) -> Option<i64> {
    (value > 0).then_some(value)
}

/// 函数 `set_background_tasks_settings`
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
pub(crate) fn set_background_tasks_settings(
    patch: BackgroundTasksSettingsPatch,
) -> BackgroundTasksSettings {
    ensure_background_tasks_config_loaded();

    if let Some(enabled) = patch.usage_polling_enabled {
        USAGE_POLLING_ENABLED.store(enabled, Ordering::Relaxed);
        std::env::set_var(ENV_USAGE_POLLING_ENABLED, if enabled { "1" } else { "0" });
        if enabled {
            std::env::remove_var(ENV_DISABLE_POLLING);
        } else {
            std::env::set_var(ENV_DISABLE_POLLING, "1");
        }
    }
    if let Some(secs) = patch.usage_poll_interval_secs {
        let normalized = secs.max(MIN_USAGE_POLL_INTERVAL_SECS);
        USAGE_POLL_INTERVAL_SECS.store(normalized, Ordering::Relaxed);
        std::env::set_var(ENV_USAGE_POLL_INTERVAL_SECS, normalized.to_string());
    }
    if let Some(enabled) = patch.gateway_keepalive_enabled {
        GATEWAY_KEEPALIVE_ENABLED.store(enabled, Ordering::Relaxed);
        std::env::set_var(
            ENV_GATEWAY_KEEPALIVE_ENABLED,
            if enabled { "1" } else { "0" },
        );
    }
    if let Some(secs) = patch.gateway_keepalive_interval_secs {
        let normalized = secs.max(MIN_GATEWAY_KEEPALIVE_INTERVAL_SECS);
        GATEWAY_KEEPALIVE_INTERVAL_SECS.store(normalized, Ordering::Relaxed);
        std::env::set_var(ENV_GATEWAY_KEEPALIVE_INTERVAL_SECS, normalized.to_string());
    }
    if let Some(enabled) = patch.token_refresh_polling_enabled {
        TOKEN_REFRESH_POLLING_ENABLED.store(enabled, Ordering::Relaxed);
        std::env::set_var(
            ENV_TOKEN_REFRESH_POLLING_ENABLED,
            if enabled { "1" } else { "0" },
        );
    }
    if let Some(secs) = patch.token_refresh_poll_interval_secs {
        let normalized = secs.max(MIN_TOKEN_REFRESH_POLL_INTERVAL_SECS);
        TOKEN_REFRESH_POLL_INTERVAL_SECS_ATOMIC.store(normalized, Ordering::Relaxed);
        std::env::set_var(ENV_TOKEN_REFRESH_POLL_INTERVAL_SECS, normalized.to_string());
    }
    if let Some(workers) = patch.usage_refresh_workers {
        let normalized = workers.max(1);
        USAGE_REFRESH_WORKERS.store(normalized, Ordering::Relaxed);
        std::env::set_var(USAGE_REFRESH_WORKERS_ENV, normalized.to_string());
    }
    if let Some(value) = patch.http_worker_factor {
        let normalized = value.max(1);
        HTTP_WORKER_FACTOR.store(normalized, Ordering::Relaxed);
        std::env::set_var(ENV_HTTP_WORKER_FACTOR, normalized.to_string());
    }
    if let Some(value) = patch.http_worker_min {
        let normalized = value.max(1);
        HTTP_WORKER_MIN.store(normalized, Ordering::Relaxed);
        std::env::set_var(ENV_HTTP_WORKER_MIN, normalized.to_string());
    }
    if let Some(value) = patch.http_stream_worker_factor {
        let normalized = value.max(1);
        HTTP_STREAM_WORKER_FACTOR.store(normalized, Ordering::Relaxed);
        std::env::set_var(ENV_HTTP_STREAM_WORKER_FACTOR, normalized.to_string());
    }
    if let Some(value) = patch.http_stream_worker_min {
        let normalized = value.max(1);
        HTTP_STREAM_WORKER_MIN.store(normalized, Ordering::Relaxed);
        std::env::set_var(ENV_HTTP_STREAM_WORKER_MIN, normalized.to_string());
    }
    if let Some(enabled) = patch.warmup_cron_enabled {
        WARMUP_CRON_ENABLED.store(enabled, Ordering::Relaxed);
        std::env::set_var(ENV_WARMUP_CRON_ENABLED, if enabled { "1" } else { "0" });
    }
    if let Some(expression) = patch.warmup_cron_expression {
        let normalized = normalize_text_setting(&expression, DEFAULT_WARMUP_CRON_EXPRESSION);
        set_mutex_string(&WARMUP_CRON_EXPRESSION, normalized.as_str());
        std::env::set_var(ENV_WARMUP_CRON_EXPRESSION, normalized);
    }
    if let Some(message) = patch.warmup_message {
        let normalized = normalize_text_setting(&message, DEFAULT_WARMUP_MESSAGE);
        set_mutex_string(&WARMUP_MESSAGE, normalized.as_str());
        std::env::set_var(ENV_WARMUP_MESSAGE, normalized);
    }

    background_tasks_settings()
}

/// 函数 `reload_background_tasks_runtime_from_env`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn reload_background_tasks_runtime_from_env() {
    reload_background_tasks_from_env();
}

/// 函数 `ensure_background_tasks_config_loaded`
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
pub(super) fn ensure_background_tasks_config_loaded() {
    let _ = BACKGROUND_TASKS_CONFIG_LOADED.get_or_init(reload_background_tasks_from_env);
}

/// 函数 `reload_background_tasks_from_env`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
fn reload_background_tasks_from_env() {
    let usage_polling_default_enabled = std::env::var(ENV_DISABLE_POLLING).is_err();
    USAGE_POLLING_ENABLED.store(
        env_bool_or(ENV_USAGE_POLLING_ENABLED, usage_polling_default_enabled),
        Ordering::Relaxed,
    );
    USAGE_POLL_INTERVAL_SECS.store(
        parse_interval_secs(
            std::env::var(ENV_USAGE_POLL_INTERVAL_SECS).ok().as_deref(),
            DEFAULT_USAGE_POLL_INTERVAL_SECS,
            MIN_USAGE_POLL_INTERVAL_SECS,
        ),
        Ordering::Relaxed,
    );
    GATEWAY_KEEPALIVE_ENABLED.store(
        env_bool_or(ENV_GATEWAY_KEEPALIVE_ENABLED, true),
        Ordering::Relaxed,
    );
    GATEWAY_KEEPALIVE_INTERVAL_SECS.store(
        parse_interval_secs(
            std::env::var(ENV_GATEWAY_KEEPALIVE_INTERVAL_SECS)
                .ok()
                .as_deref(),
            DEFAULT_GATEWAY_KEEPALIVE_INTERVAL_SECS,
            MIN_GATEWAY_KEEPALIVE_INTERVAL_SECS,
        ),
        Ordering::Relaxed,
    );
    TOKEN_REFRESH_POLLING_ENABLED.store(
        env_bool_or(ENV_TOKEN_REFRESH_POLLING_ENABLED, true),
        Ordering::Relaxed,
    );
    TOKEN_REFRESH_POLL_INTERVAL_SECS_ATOMIC.store(
        parse_interval_secs(
            std::env::var(ENV_TOKEN_REFRESH_POLL_INTERVAL_SECS)
                .ok()
                .as_deref(),
            DEFAULT_TOKEN_REFRESH_POLL_INTERVAL_SECS,
            MIN_TOKEN_REFRESH_POLL_INTERVAL_SECS,
        ),
        Ordering::Relaxed,
    );
    USAGE_REFRESH_WORKERS.store(
        env_usize_or(USAGE_REFRESH_WORKERS_ENV, DEFAULT_USAGE_REFRESH_WORKERS).max(1),
        Ordering::Relaxed,
    );
    HTTP_WORKER_FACTOR.store(
        env_usize_or(ENV_HTTP_WORKER_FACTOR, DEFAULT_HTTP_WORKER_FACTOR).max(1),
        Ordering::Relaxed,
    );
    HTTP_WORKER_MIN.store(
        env_usize_or(ENV_HTTP_WORKER_MIN, DEFAULT_HTTP_WORKER_MIN).max(1),
        Ordering::Relaxed,
    );
    HTTP_STREAM_WORKER_FACTOR.store(
        env_usize_or(
            ENV_HTTP_STREAM_WORKER_FACTOR,
            DEFAULT_HTTP_STREAM_WORKER_FACTOR,
        )
        .max(1),
        Ordering::Relaxed,
    );
    HTTP_STREAM_WORKER_MIN.store(
        env_usize_or(ENV_HTTP_STREAM_WORKER_MIN, DEFAULT_HTTP_STREAM_WORKER_MIN).max(1),
        Ordering::Relaxed,
    );
    WARMUP_CRON_ENABLED.store(
        env_bool_or(ENV_WARMUP_CRON_ENABLED, false),
        Ordering::Relaxed,
    );
    let warmup_cron_expression = std::env::var(ENV_WARMUP_CRON_EXPRESSION)
        .ok()
        .map(|value| normalize_text_setting(&value, DEFAULT_WARMUP_CRON_EXPRESSION))
        .unwrap_or_else(|| DEFAULT_WARMUP_CRON_EXPRESSION.to_string());
    set_mutex_string(&WARMUP_CRON_EXPRESSION, warmup_cron_expression.as_str());
    let warmup_message = std::env::var(ENV_WARMUP_MESSAGE)
        .ok()
        .map(|value| normalize_text_setting(&value, DEFAULT_WARMUP_MESSAGE))
        .unwrap_or_else(|| DEFAULT_WARMUP_MESSAGE.to_string());
    set_mutex_string(&WARMUP_MESSAGE, warmup_message.as_str());
}

fn current_mutex_string(
    slot: &'static std::sync::OnceLock<std::sync::Mutex<String>>,
    default_value: &str,
) -> String {
    let guard = slot.get_or_init(|| std::sync::Mutex::new(default_value.to_string()));
    crate::lock_utils::lock_recover(guard, "background_task_string").clone()
}

fn set_mutex_string(slot: &'static std::sync::OnceLock<std::sync::Mutex<String>>, value: &str) {
    let guard = slot.get_or_init(|| std::sync::Mutex::new(value.to_string()));
    *crate::lock_utils::lock_recover(guard, "background_task_string") = value.to_string();
}

fn normalize_text_setting(value: &str, default_value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        default_value.to_string()
    } else {
        trimmed.to_string()
    }
}

/// 函数 `env_usize_or`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
/// - default: 参数 default
///
/// # 返回
/// 返回函数执行结果
fn env_usize_or(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

/// 函数 `env_bool_or`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
/// - default: 参数 default
///
/// # 返回
/// 返回函数执行结果
fn env_bool_or(name: &str, default: bool) -> bool {
    let Some(raw) = std::env::var(name).ok() else {
        return default;
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}
