CREATE TABLE IF NOT EXISTS quota_consumption_daily (
    account_id TEXT NOT NULL,
    day_start_ts INTEGER NOT NULL,
    consumed_percent REAL NOT NULL DEFAULT 0.0,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (account_id, day_start_ts)
);

CREATE INDEX IF NOT EXISTS idx_quota_consumption_daily_day_start_ts
    ON quota_consumption_daily(day_start_ts);
