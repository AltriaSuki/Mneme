use anyhow::{Context, Result};
use async_trait::async_trait;
use mneme_core::{Content, Memory, SocialGraph, Person};
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite, Row};
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SqliteMemory {
    pool: Pool<Sqlite>,
}

impl SqliteMemory {
    pub async fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let db_url = format!("sqlite://{}?mode=rwc", db_path.as_ref().display());
        let pool = SqlitePoolOptions::new()
            .after_connect(|conn, _meta| Box::pin(async move {
                sqlx::query("PRAGMA foreign_keys = ON").execute(conn).await?;
                Ok(())
            }))
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
