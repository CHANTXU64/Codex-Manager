CREATE TABLE IF NOT EXISTS quota_consumption_daily (
    account_id TEXT NOT NULL,
    day_start_ts INTEGER NOT NULL,
    consumed_percent REAL NOT NULL DEFAULT 0,
    PRIMARY KEY (account_id, day_start_ts)
);
