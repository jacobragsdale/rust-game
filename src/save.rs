//! SQLite-backed persistence. Currently just run scores; this grows into the
//! full save system (progress, inventory, quest flags) in Phase 4.

use rusqlite::{params, Connection, Result};
use std::path::Path;

pub struct ScoreStore {
    conn: Connection,
}

impl ScoreStore {
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS scores (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                score REAL NOT NULL,
                recorded_at TEXT DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;

        Ok(Self { conn })
    }

    pub fn add_score(&self, score: f32) -> Result<()> {
        self.conn
            .execute("INSERT INTO scores (score) VALUES (?1)", params![score])?;
        Ok(())
    }

    pub fn top_scores(&self, limit: usize) -> Result<Vec<f32>> {
        let mut statement = self
            .conn
            .prepare("SELECT score FROM scores ORDER BY score DESC LIMIT ?1")?;
        let rows = statement.query_map(params![limit as i64], |row| {
            let value: f64 = row.get(0)?;
            Ok(value as f32)
        })?;

        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_ranks_scores() {
        let store = ScoreStore::new(":memory:").unwrap();
        for score in [12.5, 99.0, 3.25, 47.0] {
            store.add_score(score).unwrap();
        }

        let top = store.top_scores(3).unwrap();
        assert_eq!(top, vec![99.0, 47.0, 12.5]);
    }

    #[test]
    fn top_scores_on_empty_store() {
        let store = ScoreStore::new(":memory:").unwrap();
        assert!(store.top_scores(3).unwrap().is_empty());
    }
}
