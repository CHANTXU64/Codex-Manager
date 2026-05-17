use rusqlite::{Result, Row};

use super::Storage;

#[derive(Debug, Clone)]
pub struct QuotaConsumptionDailyRecord {
    pub account_id: String,
    pub day_start_ts: i64,
    pub consumed_percent: f64,
}

impl Storage {
    pub fn ensure_quota_consumption_daily_table(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS quota_consumption_daily (
                account_id TEXT NOT NULL,
                day_start_ts INTEGER NOT NULL,
                consumed_percent REAL NOT NULL DEFAULT 0,
                PRIMARY KEY (account_id, day_start_ts)
            )",
            [],
        )?;
        Ok(())
    }

    pub fn add_quota_consumption(
        &self,
        account_id: &str,
        day_start_ts: i64,
        delta: f64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO quota_consumption_daily (account_id, day_start_ts, consumed_percent)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(account_id, day_start_ts) DO UPDATE SET consumed_percent = consumed_percent + ?3",
            (account_id, day_start_ts, delta),
        )?;
        Ok(())
    }

    pub fn read_quota_consumption_daily_between(
        &self,
        range_start_ts: i64,
        range_end_ts: i64,
    ) -> Result<Vec<QuotaConsumptionDailyRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT account_id, day_start_ts, consumed_percent
             FROM quota_consumption_daily
             WHERE day_start_ts >= ?1 AND day_start_ts < ?2
             ORDER BY day_start_ts, account_id",
        )?;
        let mut rows = stmt.query([range_start_ts, range_end_ts])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map_quota_consumption_row(row)?);
        }
        Ok(out)
    }

    pub fn prune_quota_consumption_before(&self, before_ts: i64) -> Result<usize> {
        self.conn.execute(
            "DELETE FROM quota_consumption_daily WHERE day_start_ts < ?1",
            [before_ts],
        )
    }
}

fn map_quota_consumption_row(row: &Row<'_>) -> Result<QuotaConsumptionDailyRecord> {
    Ok(QuotaConsumptionDailyRecord {
        account_id: row.get(0)?,
        day_start_ts: row.get(1)?,
        consumed_percent: row.get(2)?,
    })
}
