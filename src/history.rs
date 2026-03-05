use std::path::Path;

use chrono::Local;
use rusqlite::{Connection, params};

#[allow(dead_code)]
pub struct HistoryEntry {
    pub id: i64,
    pub query: String,
    pub command: String,
    pub explanation: String,
    pub timestamp: String,
}

pub struct HistoryDB {
    conn: Connection,
}

impl HistoryDB {
    pub fn open(db_path: &Path) -> rusqlite::Result<Self> {
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                query TEXT NOT NULL,
                command TEXT NOT NULL,
                explanation TEXT NOT NULL DEFAULT '',
                timestamp TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_history_timestamp
                ON history(timestamp DESC);",
        )?;
        Ok(Self { conn })
    }

    pub fn add(&self, query: &str, command: &str, explanation: &str) -> rusqlite::Result<()> {
        let ts = Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        self.conn.execute(
            "INSERT INTO history (query, command, explanation, timestamp) VALUES (?1, ?2, ?3, ?4)",
            params![query, command, explanation, ts],
        )?;
        Ok(())
    }

    pub fn search(&self, term: &str, limit: usize) -> rusqlite::Result<Vec<HistoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, query, command, explanation, timestamp
             FROM history WHERE query LIKE ?1
             ORDER BY timestamp DESC LIMIT ?2",
        )?;
        let pattern = format!("%{}%", term);
        let rows = stmt.query_map(params![pattern, limit as i64], |row| {
            Ok(HistoryEntry {
                id: row.get(0)?,
                query: row.get(1)?,
                command: row.get(2)?,
                explanation: row.get(3)?,
                timestamp: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    pub fn recent(&self, limit: usize) -> rusqlite::Result<Vec<HistoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, query, command, explanation, timestamp
             FROM history ORDER BY timestamp DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(HistoryEntry {
                id: row.get(0)?,
                query: row.get(1)?,
                command: row.get(2)?,
                explanation: row.get(3)?,
                timestamp: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    pub fn clear(&self) -> rusqlite::Result<usize> {
        self.conn.execute("DELETE FROM history", [])
    }
}
