use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection};
use std::sync::{Arc, Mutex};

use crate::models::IndexInfo;

pub struct MetadataStore {
    conn: Arc<Mutex<Connection>>,
}

impl MetadataStore {
    pub fn new(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS indices (
                name TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS documents (
                id TEXT PRIMARY KEY,
                index_name TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (index_name) REFERENCES indices(name) ON DELETE CASCADE
            )",
            [],
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn create_index(&self, name: &str) -> Result<()> {
        let conn = self.conn.lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire database lock: {}", e))?;
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO indices (name, created_at, updated_at) VALUES (?1, ?2, ?3)",
            params![name, now, now],
        )?;

        Ok(())
    }

    pub fn sync_indices_from_disk(&self, index_names: &[String]) -> Result<()> {
        if index_names.is_empty() {
            return Ok(());
        }

        let mut conn = self.conn.lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire database lock: {}", e))?;
        let now = Utc::now().to_rfc3339();

        let tx = conn.transaction()?;
        for name in index_names {
            tx.execute(
                "INSERT OR IGNORE INTO indices (name, created_at, updated_at) VALUES (?1, ?2, ?3)",
                params![name, now, now],
            )?;
        }
        tx.commit()?;

        Ok(())
    }

    pub fn delete_index(&self, name: &str) -> Result<()> {
        let conn = self.conn.lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire database lock: {}", e))?;

        conn.execute("DELETE FROM documents WHERE index_name = ?1", params![name])?;
        conn.execute("DELETE FROM indices WHERE name = ?1", params![name])?;

        Ok(())
    }

    pub fn list_indices(&self) -> Result<Vec<IndexInfo>> {
        let conn = self.conn.lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire database lock: {}", e))?;

        let mut stmt = conn.prepare(
            "SELECT i.name, i.created_at, COUNT(d.id) as doc_count 
             FROM indices i 
             LEFT JOIN documents d ON i.name = d.index_name 
             GROUP BY i.name, i.created_at",
        )?;

        let indices = stmt
            .query_map([], |row| {
                Ok(IndexInfo {
                    name: row.get(0)?,
                    created_at: row.get(1)?,
                    document_count: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(indices)
    }

    pub fn add_document(&self, index_name: &str, doc_id: &str) -> Result<()> {
        let conn = self.conn.lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire database lock: {}", e))?;
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT OR REPLACE INTO documents (id, index_name, created_at, updated_at) 
             VALUES (?1, ?2, ?3, ?4)",
            params![doc_id, index_name, now, now],
        )?;

        Ok(())
    }

    pub fn reset_index_documents(&self, index_name: &str, doc_ids: &[String]) -> Result<()> {
        let mut conn = self.conn.lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire database lock: {}", e))?;
        let now = Utc::now().to_rfc3339();

        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM documents WHERE index_name = ?1",
            params![index_name],
        )?;

        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO documents (id, index_name, created_at, updated_at) 
                 VALUES (?1, ?2, ?3, ?4)",
            )?;

            for doc_id in doc_ids {
                stmt.execute(params![doc_id, index_name, now, now])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    pub fn delete_document(&self, doc_id: &str) -> Result<()> {
        let conn = self.conn.lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire database lock: {}", e))?;
        conn.execute("DELETE FROM documents WHERE id = ?1", params![doc_id])?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn get_document_count(&self, index_name: &str) -> Result<u64> {
        let conn = self.conn.lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire database lock: {}", e))?;

        let count: u64 = conn.query_row(
            "SELECT COUNT(*) FROM documents WHERE index_name = ?1",
            params![index_name],
            |row| row.get(0),
        )?;

        Ok(count)
    }

    /// Health check - verifies database connectivity
    pub fn health_check(&self) -> Result<()> {
        let conn = self.conn.lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire database lock: {}", e))?;

        // Simple query to verify database is responsive
        conn.query_row("SELECT 1", [], |_| Ok(()))?;
        Ok(())
    }
}
