use anyhow::{Context, Result};
use async_trait::async_trait;
use mneme_core::{Content, Memory};
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct SqliteMemory {
    pool: Pool<Sqlite>,
}

impl SqliteMemory {
    pub async fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let db_url = format!("sqlite://{}?mode=rwc", db_path.as_ref().display());
        let pool = SqlitePoolOptions::new()
            .connect(&db_url)
            .await
            .context("Failed to connect to SQLite database")?;

        let memory = Self { pool };
        memory.migrate().await?;
        Ok(memory)
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS episodes (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                author TEXT NOT NULL,
                body TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                modality TEXT NOT NULL
            );
            "#
        )
        .execute(&self.pool)
        .await
        .context("Failed to create episodes table")?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS facts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                subject TEXT NOT NULL,
                predicate TEXT NOT NULL,
                object TEXT NOT NULL,
                confidence REAL NOT NULL
            );
            "#
        )
        .execute(&self.pool)
        .await
        .context("Failed to create facts table")?;

        Ok(())
    }
}

#[async_trait]
impl Memory for SqliteMemory {
    async fn recall(&self, _query: &str) -> Result<String> {
        // Phase 1: Naive Keyword Search (No vectors yet)
        // We just grab the most recent 5 episodes for now to simulate context
        let episodes: Vec<(String, String, String)> = sqlx::query_as(
            r#"
            SELECT author, body, timestamp 
            FROM episodes 
            ORDER BY timestamp DESC 
            LIMIT 5
            "#
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch recent episodes")?
        .into_iter()
        .map(|(author, body, _ts): (String, String, i64)| (author, body, _ts.to_string()))
        .collect();

        if episodes.is_empty() {
             return Ok("No relevant memories found.".to_string());
        }

        let mut context = String::from("RECALLED MEMORIES:\n");
        for (author, body, ts) in episodes {
            context.push_str(&format!("- [{}] {}: {}\n", ts, author, body));
        }

        Ok(context)
    }

    async fn memorize(&self, content: &Content) -> Result<()> {
        let modality_str = format!("{:?}", content.modality); // Simple debug format for enum
        
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO episodes (id, source, author, body, timestamp, modality)
            VALUES (?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(content.id.to_string())
        .bind(&content.source)
        .bind(&content.author)
        .bind(&content.body)
        .bind(content.timestamp)
        .bind(modality_str)
        .execute(&self.pool)
        .await
        .context("Failed to insert episode")?;

        Ok(())
    }
}
