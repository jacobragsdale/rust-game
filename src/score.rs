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

        let mut scores = Vec::new();
        for row in rows {
            scores.push(row?);
        }
        Ok(scores)
    }
}
