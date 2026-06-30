use crate::models::{DayStat, ModelStat, RequestRecord, Stats, Trends};
use anyhow::Result;
use rusqlite::{params, Connection};
use std::sync::Mutex;

/// Keep only the most recent N requests on disk. Older rows are pruned on
/// insert so the history (and, once bodies are captured, its size) stays bounded.
const MAX_HISTORY: i64 = 2000;

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS requests (
                id TEXT PRIMARY KEY,
                ts INTEGER NOT NULL,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                model TEXT,
                status INTEGER,
                upstream_id TEXT,
                upstream_label TEXT,
                ttfb_ms INTEGER,
                duration_ms INTEGER,
                input_tokens INTEGER,
                output_tokens INTEGER,
                cache_read_tokens INTEGER,
                cache_creation_tokens INTEGER,
                cost_usd REAL,
                stop_reason TEXT,
                error TEXT,
                stream INTEGER NOT NULL DEFAULT 0,
                request_bytes INTEGER NOT NULL DEFAULT 0,
                response_bytes INTEGER NOT NULL DEFAULT 0,
                request_body TEXT,
                response_text TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_requests_ts ON requests(ts DESC);
            CREATE TABLE IF NOT EXISTS day_stats (
                day TEXT PRIMARY KEY,
                requests INTEGER NOT NULL DEFAULT 0,
                input INTEGER NOT NULL DEFAULT 0,
                output INTEGER NOT NULL DEFAULT 0,
                cache INTEGER NOT NULL DEFAULT 0,
                cost REAL NOT NULL DEFAULT 0,
                errors INTEGER NOT NULL DEFAULT 0
            );",
        )?;

        // Backfill the rollup once from existing detail rows. Safe because it
        // only runs when empty; live inserts increment it from then on, and the
        // detail-row prune never touches it (so trends outlive the 2000-row cap).
        let day_rows: i64 = conn.query_row("SELECT COUNT(*) FROM day_stats", [], |r| r.get(0))?;
        if day_rows == 0 {
            conn.execute(
                "INSERT INTO day_stats(day, requests, input, output, cache, cost, errors)
                 SELECT date(ts/1000,'unixepoch','localtime'), COUNT(*),
                        COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                        COALESCE(SUM(COALESCE(cache_read_tokens,0)+COALESCE(cache_creation_tokens,0)),0),
                        COALESCE(SUM(cost_usd),0),
                        COALESCE(SUM(CASE WHEN error IS NOT NULL OR status>=400 THEN 1 ELSE 0 END),0)
                 FROM requests GROUP BY 1",
                [],
            )?;
        }
        let _ = conn.execute(
            "ALTER TABLE requests ADD COLUMN request_bytes INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE requests ADD COLUMN response_bytes INTEGER NOT NULL DEFAULT 0",
            [],
        );
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert(&self, r: &RequestRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO requests (
                id, ts, method, path, model, status, upstream_id, upstream_label,
                ttfb_ms, duration_ms, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, cost_usd, stop_reason,
                error, stream, request_bytes, response_bytes, request_body, response_text
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                ?16, ?17, ?18, ?19, ?20, ?21, ?22
            )",
            params![
                r.id,
                r.ts,
                r.method,
                r.path,
                r.model,
                r.status.map(|s| s as i64),
                r.upstream_id,
                r.upstream_label,
                r.ttfb_ms.map(|v| v as i64),
                r.duration_ms.map(|v| v as i64),
                r.input_tokens.map(|v| v as i64),
                r.output_tokens.map(|v| v as i64),
                r.cache_read_tokens.map(|v| v as i64),
                r.cache_creation_tokens.map(|v| v as i64),
                r.cost_usd,
                r.stop_reason,
                r.error,
                r.stream as i64,
                r.request_bytes as i64,
                r.response_bytes as i64,
                r.request_body,
                r.response_text,
            ],
        )?;
        // Accumulate the permanent per-day rollup (survives the detail prune).
        let cache = r.cache_read_tokens.unwrap_or(0) + r.cache_creation_tokens.unwrap_or(0);
        let is_err = (r.error.is_some() || r.status.map_or(false, |s| s >= 400)) as i64;
        conn.execute(
            "INSERT INTO day_stats(day, requests, input, output, cache, cost, errors)
             VALUES (date(?1/1000,'unixepoch','localtime'), 1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(day) DO UPDATE SET
                requests = requests + 1,
                input = input + excluded.input,
                output = output + excluded.output,
                cache = cache + excluded.cache,
                cost = cost + excluded.cost,
                errors = errors + excluded.errors",
            params![
                r.ts,
                r.input_tokens.unwrap_or(0) as i64,
                r.output_tokens.unwrap_or(0) as i64,
                cache as i64,
                r.cost_usd.unwrap_or(0.0),
                is_err,
            ],
        )?;

        conn.execute(
            "DELETE FROM requests WHERE ts < (
                SELECT ts FROM requests ORDER BY ts DESC LIMIT 1 OFFSET ?1
            )",
            params![MAX_HISTORY],
        )?;
        Ok(())
    }

    pub fn list(&self, limit: i64, offset: i64) -> Result<Vec<RequestRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, ts, method, path, model, status, upstream_id, upstream_label,
                    ttfb_ms, duration_ms, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, cost_usd, stop_reason,
                    error, stream, request_bytes, response_bytes
             FROM requests ORDER BY ts DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(params![limit, offset], |row| {
            Ok(RequestRecord {
                id: row.get(0)?,
                ts: row.get(1)?,
                method: row.get(2)?,
                path: row.get(3)?,
                model: row.get(4)?,
                status: row.get::<_, Option<i64>>(5)?.map(|v| v as u16),
                upstream_id: row.get(6)?,
                upstream_label: row.get(7)?,
                ttfb_ms: row.get::<_, Option<i64>>(8)?.map(|v| v as u64),
                duration_ms: row.get::<_, Option<i64>>(9)?.map(|v| v as u64),
                input_tokens: row.get::<_, Option<i64>>(10)?.map(|v| v as u64),
                output_tokens: row.get::<_, Option<i64>>(11)?.map(|v| v as u64),
                cache_read_tokens: row.get::<_, Option<i64>>(12)?.map(|v| v as u64),
                cache_creation_tokens: row.get::<_, Option<i64>>(13)?.map(|v| v as u64),
                cost_usd: row.get(14)?,
                stop_reason: row.get(15)?,
                error: row.get(16)?,
                stream: row.get::<_, i64>(17)? != 0,
                request_bytes: row.get::<_, i64>(18)? as u64,
                response_bytes: row.get::<_, i64>(19)? as u64,
                request_body: None,
                response_text: None,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn get(&self, id: &str) -> Result<Option<RequestRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, ts, method, path, model, status, upstream_id, upstream_label,
                    ttfb_ms, duration_ms, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, cost_usd, stop_reason,
                    error, stream, request_bytes, response_bytes, request_body, response_text
             FROM requests WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(RequestRecord {
                id: row.get(0)?,
                ts: row.get(1)?,
                method: row.get(2)?,
                path: row.get(3)?,
                model: row.get(4)?,
                status: row.get::<_, Option<i64>>(5)?.map(|v| v as u16),
                upstream_id: row.get(6)?,
                upstream_label: row.get(7)?,
                ttfb_ms: row.get::<_, Option<i64>>(8)?.map(|v| v as u64),
                duration_ms: row.get::<_, Option<i64>>(9)?.map(|v| v as u64),
                input_tokens: row.get::<_, Option<i64>>(10)?.map(|v| v as u64),
                output_tokens: row.get::<_, Option<i64>>(11)?.map(|v| v as u64),
                cache_read_tokens: row.get::<_, Option<i64>>(12)?.map(|v| v as u64),
                cache_creation_tokens: row.get::<_, Option<i64>>(13)?.map(|v| v as u64),
                cost_usd: row.get(14)?,
                stop_reason: row.get(15)?,
                error: row.get(16)?,
                stream: row.get::<_, i64>(17)? != 0,
                request_bytes: row.get::<_, i64>(18)? as u64,
                response_bytes: row.get::<_, i64>(19)? as u64,
                request_body: row.get(20)?,
                response_text: row.get(21)?,
            })
        })?;
        if let Some(r) = rows.next() {
            Ok(Some(r?))
        } else {
            Ok(None)
        }
    }

    /// since_ts: optional Unix ms timestamp; only rows with ts >= since_ts are included.
    /// Pass None to aggregate all rows (equivalent to since_ts = 0).
    pub fn stats(&self, since_ts: Option<i64>) -> Result<Stats> {
        let conn = self.conn.lock().unwrap();
        let since = since_ts.unwrap_or(0);
        let (total, req_bytes, resp_bytes, ti, to, tcr, tcc, cost, errors): (
            u64,
            u64,
            u64,
            u64,
            u64,
            u64,
            u64,
            f64,
            u64,
        ) = conn.query_row(
            "SELECT
                COUNT(*),
                COALESCE(SUM(request_bytes),0),
                COALESCE(SUM(response_bytes),0),
                COALESCE(SUM(input_tokens),0),
                COALESCE(SUM(output_tokens),0),
                COALESCE(SUM(cache_read_tokens),0),
                COALESCE(SUM(cache_creation_tokens),0),
                COALESCE(SUM(cost_usd),0),
                COALESCE(SUM(CASE WHEN error IS NOT NULL OR status >= 400 THEN 1 ELSE 0 END),0)
             FROM requests WHERE ts >= ?1",
            params![since],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,
                    row.get::<_, i64>(1)? as u64,
                    row.get::<_, i64>(2)? as u64,
                    row.get::<_, i64>(3)? as u64,
                    row.get::<_, i64>(4)? as u64,
                    row.get::<_, i64>(5)? as u64,
                    row.get::<_, i64>(6)? as u64,
                    row.get::<_, f64>(7)?,
                    row.get::<_, i64>(8)? as u64,
                ))
            },
        )?;

        let mut by_model = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT COALESCE(model,'unknown'), COUNT(*),
                    COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(cost_usd),0)
             FROM requests WHERE ts >= ?1 GROUP BY model ORDER BY SUM(cost_usd) DESC",
        )?;
        let rows = stmt.query_map(params![since], |row| {
            Ok(ModelStat {
                model: row.get(0)?,
                requests: row.get::<_, i64>(1)? as u64,
                input_tokens: row.get::<_, i64>(2)? as u64,
                output_tokens: row.get::<_, i64>(3)? as u64,
                cost_usd: row.get(4)?,
            })
        })?;
        for r in rows {
            by_model.push(r?);
        }

        Ok(Stats {
            total_requests: total,
            total_request_bytes: req_bytes,
            total_response_bytes: resp_bytes,
            total_input: ti,
            total_output: to,
            total_cache_read: tcr,
            total_cache_creation: tcc,
            total_cost: cost,
            errors,
            by_model,
        })
    }

    /// Today / yesterday / last-7-day rollups (local time) plus the per-day
    /// series, read from the permanent day_stats table.
    pub fn trends(&self) -> Result<Trends> {
        let conn = self.conn.lock().unwrap();
        let agg = |cond: &str| -> Result<DayStat> {
            let sql = format!(
                "SELECT COALESCE(SUM(requests),0), COALESCE(SUM(input),0),
                        COALESCE(SUM(output),0), COALESCE(SUM(cache),0),
                        COALESCE(SUM(cost),0.0), COALESCE(SUM(errors),0)
                 FROM day_stats WHERE {}",
                cond
            );
            conn.query_row(&sql, [], |r| {
                Ok(DayStat {
                    day: String::new(),
                    requests: r.get::<_, i64>(0)? as u64,
                    input: r.get::<_, i64>(1)? as u64,
                    output: r.get::<_, i64>(2)? as u64,
                    cache: r.get::<_, i64>(3)? as u64,
                    cost: r.get::<_, f64>(4)?,
                    errors: r.get::<_, i64>(5)? as u64,
                })
            })
            .map_err(Into::into)
        };
        let today = agg("day = date('now','localtime')")?;
        let yesterday = agg("day = date('now','-1 day','localtime')")?;
        let last7 = agg("day >= date('now','-6 days','localtime')")?;

        let mut days = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT day, requests, input, output, cache, cost, errors FROM day_stats
             WHERE day >= date('now','-6 days','localtime') ORDER BY day ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(DayStat {
                day: r.get(0)?,
                requests: r.get::<_, i64>(1)? as u64,
                input: r.get::<_, i64>(2)? as u64,
                output: r.get::<_, i64>(3)? as u64,
                cache: r.get::<_, i64>(4)? as u64,
                cost: r.get::<_, f64>(5)?,
                errors: r.get::<_, i64>(6)? as u64,
            })
        })?;
        for d in rows {
            days.push(d?);
        }
        Ok(Trends {
            today,
            yesterday,
            last7,
            days,
        })
    }

    pub fn clear(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM requests", [])?;
        conn.execute("DELETE FROM day_stats", [])?;
        Ok(())
    }
}
