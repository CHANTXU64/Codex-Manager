CREATE TABLE IF NOT EXISTS api_key_active_accounts (
  key_id TEXT PRIMARY KEY,
  active_account_id TEXT NOT NULL,
  active_started_at INTEGER NOT NULL,
  last_used_at INTEGER NOT NULL,
  consecutive_real_errors INTEGER NOT NULL DEFAULT 0,
  last_switch_reason TEXT,
  updated_at INTEGER NOT NULL
);
