//! Local dictation history (text only - audio is never persisted).

use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: i64,
    pub created_at: String,
    pub app_name: String,
    pub app_bundle: String,
    pub text: String,
    pub raw_text: String,
    pub source: String,
    pub audio_secs: f64,
    pub latency_ms: i64,
    pub outcome: String,
}

pub struct History {
    conn: Mutex<Connection>,
}

impl History {
    pub fn open(dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let conn = Connection::open(dir.join("history.sqlite")).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at TEXT NOT NULL,
                app_name TEXT NOT NULL DEFAULT '',
                app_bundle TEXT NOT NULL DEFAULT '',
                text TEXT NOT NULL,
                raw_text TEXT NOT NULL DEFAULT '',
                source TEXT NOT NULL DEFAULT '',
                audio_secs REAL NOT NULL DEFAULT 0,
                latency_ms INTEGER NOT NULL DEFAULT 0,
                outcome TEXT NOT NULL DEFAULT ''
             );",
        )
        .map_err(|e| e.to_string())?;
        Ok(History { conn: Mutex::new(conn) })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &self,
        app_name: &str,
        app_bundle: &str,
        text: &str,
        raw_text: &str,
        source: &str,
        audio_secs: f32,
        latency_ms: u64,
        outcome: &str,
    ) -> Option<i64> {
        let c = self.conn.lock();
        c.execute(
            "INSERT INTO entries (created_at, app_name, app_bundle, text, raw_text, source, audio_secs, latency_ms, outcome)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                chrono::Local::now().to_rfc3339(),
                app_name,
                app_bundle,
                text,
                raw_text,
                source,
                audio_secs as f64,
                latency_ms as i64,
                outcome
            ],
        )
        .ok()?;
        Some(c.last_insert_rowid())
    }

    pub fn update_text(&self, id: i64, text: &str, source: &str) {
        let _ = self.conn.lock().execute(
            "UPDATE entries SET text = ?1, source = ?2 WHERE id = ?3",
            params![text, source, id],
        );
    }

    pub fn list(&self, limit: u32) -> Vec<HistoryEntry> {
        let c = self.conn.lock();
        let mut st = match c.prepare(
            "SELECT id, created_at, app_name, app_bundle, text, raw_text, source, audio_secs, latency_ms, outcome
             FROM entries ORDER BY id DESC LIMIT ?1",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        st.query_map(params![limit], |r| {
            Ok(HistoryEntry {
                id: r.get(0)?,
                created_at: r.get(1)?,
                app_name: r.get(2)?,
                app_bundle: r.get(3)?,
                text: r.get(4)?,
                raw_text: r.get(5)?,
                source: r.get(6)?,
                audio_secs: r.get(7)?,
                latency_ms: r.get(8)?,
                outcome: r.get(9)?,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    pub fn delete(&self, id: i64) {
        let _ = self.conn.lock().execute("DELETE FROM entries WHERE id = ?1", params![id]);
    }

    pub fn clear(&self) {
        let c = self.conn.lock();
        let _ = c.execute("DELETE FROM entries", []);
        let _ = c.execute_batch("VACUUM;");
    }

    pub fn prune(&self, days: u32) {
        let cutoff = (chrono::Local::now() - chrono::Duration::days(days as i64)).to_rfc3339();
        let _ = self.conn.lock().execute("DELETE FROM entries WHERE created_at < ?1", params![cutoff]);
    }
}
