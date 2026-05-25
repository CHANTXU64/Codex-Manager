use rusqlite::{params, Result, Row};

use super::{now_ts, Storage};

#[derive(Debug, Clone)]
pub struct QuotaConsumptionDailyRecord {
    pub account_id: String,
    pub day_start_ts: i64,
    pub consumed_percent: f64,
}

impl Storage {
    pub fn ensure_quota_consumption_daily_table(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS quota_consumption_daily (
                account_id TEXT NOT NULL,
                day_start_ts INTEGER NOT NULL,
                consumed_percent REAL NOT NULL DEFAULT 0.0,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (account_id, day_start_ts)
             );
             CREATE INDEX IF NOT EXISTS idx_quota_consumption_daily_day_start_ts
                ON quota_consumption_daily(day_start_ts);",
        )?;
        self.ensure_column(
            "quota_consumption_daily",
            "updated_at",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        Ok(())
    }

    pub fn add_quota_consumption(
        &self,
        account_id: &str,
        day_start_ts: i64,
        delta_percent: f64,
    ) -> Result<()> {
        let account_id = account_id.trim();
        if account_id.is_empty() || delta_percent <= 0.0 {
            return Ok(());
        }
        let updated_at = now_ts();
        self.conn.execute(
            "INSERT INTO quota_consumption_daily (
                account_id, day_start_ts, consumed_percent, updated_at
             ) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(account_id, day_start_ts) DO UPDATE SET
                consumed_percent = quota_consumption_daily.consumed_percent + excluded.consumed_percent,
                updated_at = excluded.updated_at",
            params![account_id, day_start_ts, delta_percent, updated_at],
        )?;
        Ok(())
    }

    pub fn list_quota_consumption_daily_between(
        &self,
        start_ts: i64,
        end_ts: i64,
    ) -> Result<Vec<QuotaConsumptionDailyRecord>> {
        if end_ts <= start_ts {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT account_id, day_start_ts, consumed_percent
             FROM quota_consumption_daily
             WHERE day_start_ts >= ?1 AND day_start_ts < ?2
             ORDER BY day_start_ts ASC, account_id ASC",
        )?;
        let rows = stmt.query_map(params![start_ts, end_ts], map_quota_consumption_daily_row)?;
        rows.collect()
    }
}

fn map_quota_consumption_daily_row(row: &Row<'_>) -> Result<QuotaConsumptionDailyRecord> {
    Ok(QuotaConsumptionDailyRecord {
        account_id: row.get(0)?,
        day_start_ts: row.get(1)?,
        consumed_percent: row.get::<_, f64>(2)?.max(0.0),
    })
}
