use anyhow::{Context, Result};
use async_trait::async_trait;
use mneme_core::{Content, Memory, SocialGraph, Person};
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite, Row};
use std::path::Path;
use uuid::Uuid;
use crate::embedding::{EmbeddingModel, cosine_similarity};
use serde::{Serialize, Deserialize};
use std::sync::Arc;

// Removed unused Episode struct per code review

#[derive(Clone)]
pub struct SqliteMemory {
    pool: Pool<Sqlite>,
    embedding_model: Arc<EmbeddingModel>,
}

impl SqliteMemory {
    pub async fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        // Initialize embedding model first (might take a moment to load/download)
        let embedding_model = Arc::new(EmbeddingModel::new().context("Failed to initialize embedding model")?);

        let db_url = format!("sqlite://{}?mode=rwc", db_path.as_ref().display());
        let pool = SqlitePoolOptions::new()
            .after_connect(|conn, _meta| Box::pin(async move {
                sqlx::query("PRAGMA foreign_keys = ON").execute(conn).await?;
                Ok(())
            }))
            .connect(&db_url)
            .await
            .context("Failed to connect to SQLite database")?;

        let memory = Self { pool, embedding_model };
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

        // Add embedding column if it doesn't exist (v1 -> v2 migration)
        if let Err(e) = sqlx::query("ALTER TABLE episodes ADD COLUMN embedding BLOB")
            .execute(&self.pool)
            .await 
        {
            // This is expected if the column already exists
            tracing::debug!("Column 'embedding' likely exists or migration skipped: {}", e);
        }

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

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS people (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL
            );
            "#
        )
        .execute(&self.pool)
        .await
        .context("Failed to create people table")?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS aliases (
                person_id TEXT NOT NULL,
                platform TEXT NOT NULL,
                platform_id TEXT NOT NULL,
                PRIMARY KEY (platform, platform_id),
                FOREIGN KEY(person_id) REFERENCES people(id)
            );
            "#
        )
        .execute(&self.pool)
        .await
        .context("Failed to create aliases table")?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS relationships (
                source_id TEXT NOT NULL,
                target_id TEXT NOT NULL,
                context TEXT,
                timestamp INTEGER NOT NULL,
                FOREIGN KEY(source_id) REFERENCES people(id),
                FOREIGN KEY(target_id) REFERENCES people(id)
            );
            "#
        )
        .execute(&self.pool)
        .await
        .context("Failed to create relationships table")?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_relationships_source_target ON relationships(source_id, target_id)"
        )
        .execute(&self.pool)
        .await
        .context("Failed to create relationships index")?;

        Ok(())
    }
}

#[async_trait]
impl Memory for SqliteMemory {
    async fn recall(&self, query: &str) -> Result<String> {
        // Generate embedding for the query
        let query_embedding = self.embedding_model.embed(query).context("Failed to embed query")?;

        // Fetch recent episodes with embeddings
        // Optimization: Added LIMIT 1000 to avoid full table scan at scale.
        // We assume recent memories are more relevant contextually anyway.
        // Future: Implement proper Approximate Nearest Neighbor (ANN) index.
        let rows = sqlx::query("SELECT author, body, timestamp, embedding FROM episodes ORDER BY timestamp DESC LIMIT 1000")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch episodes for vector search")?;

        let mut scored_episodes: Vec<(f32, String, String, i64)> = Vec::new();

        for row in rows {
            let author: String = row.get("author");
            let body: String = row.get("body");
            let timestamp: i64 = row.get("timestamp");
            let embedding_blob: Option<Vec<u8>> = row.get("embedding");

            if let Some(blob) = embedding_blob {
                // Deserialize blob back to Vec<f32> using bincode (more efficient than JSON)
                if let Ok(embedding) = bincode::deserialize::<Vec<f32>>(&blob) {
                    let score = cosine_similarity(&query_embedding, &embedding);
                    scored_episodes.push((score, author, body, timestamp));
                }
            }
        }

        // Sort by score descending
        scored_episodes.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Take top 5
        let top_episodes = scored_episodes.into_iter().take(5).collect::<Vec<_>>();

        if top_episodes.is_empty() {
             return Ok("No relevant memories found.".to_string());
        }

        let mut context = String::from("RECALLED MEMORIES (Semantic):\n");
        for (_score, author, body, ts) in top_episodes {
            context.push_str(&format!("- [{}] {}: {}\n", ts, author, body));
        }

        Ok(context)
    }

    async fn memorize(&self, content: &Content) -> Result<()> {
        let modality_str = format!("{:?}", content.modality); // Simple debug format for enum
        
        // Generate embedding
        let embedding = self.embedding_model.embed(&content.body)
            .ok(); // If embedding fails (e.g. empty text), we still store the episode, just no vector.

        // Serialize embedding to efficient binary format
        let embedding_blob = if let Some(emb) = embedding {
            Some(bincode::serialize(&emb).context("Failed to serialize embedding")?)
        } else {
            None
        };

        sqlx::query(
            r#"
            INSERT OR IGNORE INTO episodes (id, source, author, body, timestamp, modality, embedding)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(content.id.to_string())
        .bind(&content.source)
        .bind(&content.author)
        .bind(&content.body)
        .bind(content.timestamp)
        .bind(modality_str)
        .bind(embedding_blob)
        .execute(&self.pool)
        .await
        .context("Failed to insert episode")?;

        Ok(())
    }
}
#[async_trait]
impl SocialGraph for SqliteMemory {
    async fn find_person(&self, platform: &str, platform_id: &str) -> Result<Option<Person>> {
        let row = sqlx::query(
            r#"
            SELECT p.id, p.name 
            FROM people p
            JOIN aliases a ON p.id = a.person_id
            WHERE a.platform = ? AND a.platform_id = ?
            "#
        )
        .bind(platform)
        .bind(platform_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to find person")?;

        if let Some(row) = row {
            let id_str: String = row.get("id");
            let name: String = row.get("name");
            let id = Uuid::parse_str(&id_str)?;

            // Fetch aliases
            let aliases_rows: Vec<(String, String)> = sqlx::query_as(
                "SELECT platform, platform_id FROM aliases WHERE person_id = ?"
            )
            .bind(&id_str)
            .fetch_all(&self.pool)
            .await?;

            let aliases = aliases_rows.into_iter().collect();

            Ok(Some(Person { id, name, aliases }))
        } else {
            Ok(None)
        }
    }
    
    async fn upsert_person(&self, person: &Person) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        // Upsert person
        sqlx::query(
            "INSERT INTO people (id, name) VALUES (?, ?) ON CONFLICT(id) DO UPDATE SET name = excluded.name"
        )
        .bind(person.id.to_string())
        .bind(&person.name)
        .execute(&mut *tx)
        .await?;

        // Upsert aliases
        for (platform, platform_id) in &person.aliases {
            sqlx::query(
                "INSERT INTO aliases (person_id, platform, platform_id) VALUES (?, ?, ?) ON CONFLICT(platform, platform_id) DO UPDATE SET person_id = excluded.person_id"
            )
            .bind(person.id.to_string())
            .bind(platform)
            .bind(platform_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }
    
    async fn record_interaction(&self, from: Uuid, to: Uuid, context: &str) -> Result<()> {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;
            
        sqlx::query(
            "INSERT INTO relationships (source_id, target_id, context, timestamp) VALUES (?, ?, ?, ?)"
        )
        .bind(from.to_string())
        .bind(to.to_string())
        .bind(context)
        .bind(ts)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
}
