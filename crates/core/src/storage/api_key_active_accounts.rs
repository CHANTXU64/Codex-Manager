use rusqlite::params;

use super::{ApiKeyActiveAccount, Storage};

impl Storage {
    pub fn get_api_key_active_account(
        &self,
        key_id: &str,
    ) -> rusqlite::Result<Option<ApiKeyActiveAccount>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                key_id,
                active_account_id,
                active_started_at,
                last_used_at,
                consecutive_real_errors,
                last_switch_reason,
                updated_at
             FROM api_key_active_accounts
             WHERE key_id = ?1
             LIMIT 1",
        )?;
        let mut rows = stmt.query([key_id])?;
        if let Some(row) = rows.next()? {
            return Ok(Some(ApiKeyActiveAccount {
                key_id: row.get(0)?,
                active_account_id: row.get(1)?,
                active_started_at: row.get(2)?,
                last_used_at: row.get(3)?,
                consecutive_real_errors: row.get(4)?,
                last_switch_reason: row.get(5)?,
                updated_at: row.get(6)?,
            }));
        }
        Ok(None)
    }

    pub fn upsert_api_key_active_account(
        &self,
        record: &ApiKeyActiveAccount,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO api_key_active_accounts (
                key_id,
                active_account_id,
                active_started_at,
                last_used_at,
                consecutive_real_errors,
                last_switch_reason,
                updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(key_id) DO UPDATE SET
                active_account_id = excluded.active_account_id,
                active_started_at = excluded.active_started_at,
                last_used_at = excluded.last_used_at,
                consecutive_real_errors = excluded.consecutive_real_errors,
                last_switch_reason = excluded.last_switch_reason,
                updated_at = excluded.updated_at",
            params![
                &record.key_id,
                &record.active_account_id,
                record.active_started_at,
                record.last_used_at,
                record.consecutive_real_errors,
                &record.last_switch_reason,
                record.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn clear_api_key_active_account(
        &self,
        key_id: &str,
        _reason: &str,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "DELETE FROM api_key_active_accounts WHERE key_id = ?1",
            [key_id],
        )?;
        Ok(())
    }

    pub fn clear_api_key_active_account_if_matches(
        &self,
        key_id: &str,
        active_account_id: &str,
        _reason: &str,
    ) -> rusqlite::Result<bool> {
        let updated = self.conn.execute(
            "DELETE FROM api_key_active_accounts
             WHERE key_id = ?1
               AND active_account_id = ?2",
            params![key_id, active_account_id],
        )?;
        Ok(updated > 0)
    }

    pub fn increment_api_key_active_account_real_error(
        &self,
        key_id: &str,
        updated_at: i64,
        reason: &str,
    ) -> rusqlite::Result<usize> {
        self.conn.execute(
            "UPDATE api_key_active_accounts
             SET consecutive_real_errors = consecutive_real_errors + 1,
                 last_switch_reason = ?2,
                 updated_at = ?3
             WHERE key_id = ?1",
            params![key_id, reason, updated_at],
        )
    }

    pub fn increment_api_key_active_account_real_error_if_matches(
        &self,
        key_id: &str,
        active_account_id: &str,
        updated_at: i64,
        reason: &str,
    ) -> rusqlite::Result<usize> {
        self.conn.execute(
            "UPDATE api_key_active_accounts
             SET consecutive_real_errors = consecutive_real_errors + 1,
                 last_switch_reason = ?3,
                 updated_at = ?4
             WHERE key_id = ?1
               AND active_account_id = ?2",
            params![key_id, active_account_id, reason, updated_at],
        )
    }

    pub fn touch_api_key_active_account_if_matches(
        &self,
        key_id: &str,
        active_account_id: &str,
        updated_at: i64,
    ) -> rusqlite::Result<bool> {
        let updated = self.conn.execute(
            "UPDATE api_key_active_accounts
             SET last_used_at = ?3,
                 consecutive_real_errors = 0,
                 updated_at = ?3
             WHERE key_id = ?1
               AND active_account_id = ?2",
            params![key_id, active_account_id, updated_at],
        )?;
        Ok(updated > 0)
    }

    pub fn reset_api_key_active_account_errors(
        &self,
        key_id: &str,
        updated_at: i64,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE api_key_active_accounts
             SET consecutive_real_errors = 0,
                 updated_at = ?2
             WHERE key_id = ?1",
            params![key_id, updated_at],
        )?;
        Ok(())
    }

    pub(super) fn ensure_api_key_active_accounts_table(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS api_key_active_accounts (
                key_id TEXT PRIMARY KEY,
                active_account_id TEXT NOT NULL,
                active_started_at INTEGER NOT NULL,
                last_used_at INTEGER NOT NULL,
                consecutive_real_errors INTEGER NOT NULL DEFAULT 0,
                last_switch_reason TEXT,
                updated_at INTEGER NOT NULL
            );",
        )?;
        Ok(())
    }
}
