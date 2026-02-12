use crate::embedding::EmbeddingModel;
use anyhow::{Context, Result};
use async_trait::async_trait;
use mneme_core::{Content, Memory, OrganismState, Person, PersonContext, SocialGraph};
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePoolOptions, Pool, Row, Sqlite};
use std::path::Path;
use std::sync::Arc;
use std::sync::Once;
use uuid::Uuid;

/// Register sqlite-vec extension globally (once per process).
/// Must be called before any SQLite connections are opened.
fn ensure_sqlite_vec_registered() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        #[allow(clippy::missing_transmute_annotations)]
        unsafe {
            libsqlite3_sys::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        }
        tracing::info!("sqlite-vec extension registered");
    });
}

#[derive(Clone)]
pub struct SqliteMemory {
    pool: Pool<Sqlite>,
    embedding_model: Arc<EmbeddingModel>,
}

impl SqliteMemory {
    pub async fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        // Initialize embedding model first (might take a moment to load/download)
        let embedding_model =
            Arc::new(EmbeddingModel::new().context("Failed to initialize embedding model")?);

        // Register sqlite-vec extension before opening any connections
        ensure_sqlite_vec_registered();

        let db_url = format!("sqlite://{}?mode=rwc", db_path.as_ref().display());
        let pool = SqlitePoolOptions::new()
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    sqlx::query("PRAGMA foreign_keys = ON")
                        .execute(conn)
                        .await?;
                    Ok(())
                })
            })
            .connect(&db_url)
            .await
            .context("Failed to connect to SQLite database")?;

        let memory = Self {
            pool,
            embedding_model,
        };
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
            "#,
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
            tracing::debug!(
                "Column 'embedding' likely exists or migration skipped: {}",
                e
            );
        }

        // Add strength column if it doesn't exist (v2 -> v3 migration)
        // Strength: memory trace intensity (0.0 - 1.0). Default 0.5 for pre-existing episodes.
        // Encoding layer of the three-layer forgetting model (B-10).
        if let Err(e) =
            sqlx::query("ALTER TABLE episodes ADD COLUMN strength REAL NOT NULL DEFAULT 0.5")
                .execute(&self.pool)
                .await
        {
            tracing::debug!(
                "Column 'strength' likely exists or migration skipped: {}",
                e
            );
        }

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS facts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                subject TEXT NOT NULL,
                predicate TEXT NOT NULL,
                object TEXT NOT NULL,
                confidence REAL NOT NULL,
                created_at INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL DEFAULT 0
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create facts table")?;

        // Index for fast subject lookup
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_facts_subject ON facts(subject)")
            .execute(&self.pool)
            .await
            .context("Failed to create facts subject index")?;

        // Index for fast predicate lookup (useful for querying "all likes", "all knows", etc.)
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_facts_predicate ON facts(predicate)")
            .execute(&self.pool)
            .await
            .context("Failed to create facts predicate index")?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS people (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL
            );
            "#,
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
            "#,
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
            "#,
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

        // === New tables for Organism State persistence ===

        // Organism state (singleton - only one row)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS organism_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                state_json TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create organism_state table")?;

        // Narrative chapters
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS narrative_chapters (
                id INTEGER PRIMARY KEY,
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                period_start INTEGER NOT NULL,
                period_end INTEGER NOT NULL,
                emotional_tone REAL NOT NULL,
                themes_json TEXT NOT NULL,
                people_json TEXT NOT NULL,
                turning_points_json TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create narrative_chapters table")?;

        // Feedback signals (for persistence across restarts)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS feedback_signals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                signal_type TEXT NOT NULL,
                content TEXT NOT NULL,
                confidence REAL NOT NULL,
                emotional_context REAL NOT NULL,
                timestamp INTEGER NOT NULL,
                consolidated INTEGER NOT NULL DEFAULT 0
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create feedback_signals table")?;

        // Organism state history (time series for debug/replay)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS organism_state_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                state_json TEXT NOT NULL,
                trigger TEXT NOT NULL,
                diff_summary TEXT
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create organism_state_history table")?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_state_history_timestamp ON organism_state_history(timestamp)"
        )
        .execute(&self.pool)
        .await
        .context("Failed to create state history timestamp index")?;

        // === Self-Knowledge (emergent self-model, ADR-002) ===
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS self_knowledge (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                domain TEXT NOT NULL,
                content TEXT NOT NULL,
                confidence REAL NOT NULL,
                source TEXT NOT NULL,
                source_episode_id TEXT,
                is_private INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create self_knowledge table")?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_self_knowledge_domain ON self_knowledge(domain)",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create self_knowledge domain index")?;

        // === Token Usage Tracking (v0.4.0) ===
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS token_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                timestamp INTEGER NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create token_usage table")?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_token_usage_timestamp ON token_usage(timestamp)",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create token_usage timestamp index")?;

        // Modulation samples table — records (state, modulation, feedback) for offline learning
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS modulation_samples (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                energy REAL NOT NULL,
                stress REAL NOT NULL,
                arousal REAL NOT NULL,
                mood_bias REAL NOT NULL,
                social_need REAL NOT NULL,
                modulation_json TEXT NOT NULL,
                feedback_valence REAL NOT NULL,
                timestamp INTEGER NOT NULL,
                consumed INTEGER NOT NULL DEFAULT 0
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create modulation_samples table")?;

        // Learned curves table — persists the latest learned ModulationCurves
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS learned_curves (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                curves_json TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create learned_curves table")?;

        // Learned thresholds table — persists the latest learned BehaviorThresholds
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS learned_thresholds (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                thresholds_json TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create learned_thresholds table")?;

        // === Vector search index (sqlite-vec) ===
        // Create vec_episodes virtual table for ANN search.
        // vec0 uses cosine distance by default for float vectors.
        sqlx::query(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS vec_episodes USING vec0(
                episode_id TEXT PRIMARY KEY,
                embedding float[384]
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create vec_episodes virtual table")?;

        // Backfill: insert any episodes that have embeddings but are missing from vec_episodes
        self.backfill_vec_index().await?;

        // === Behavior Rules (ADR-004, v0.6.0) ===
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS behavior_rules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                priority INTEGER NOT NULL DEFAULT 0,
                enabled INTEGER NOT NULL DEFAULT 1,
                trigger_json TEXT NOT NULL,
                condition_json TEXT NOT NULL,
                action_json TEXT NOT NULL,
                cooldown_secs INTEGER,
                last_fired INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create behavior_rules table")?;

        // === Goals (#22, v0.6.0) ===
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS goals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                goal_type TEXT NOT NULL,
                description TEXT NOT NULL,
                priority REAL NOT NULL DEFAULT 0.5,
                status TEXT NOT NULL DEFAULT 'active',
                progress REAL NOT NULL DEFAULT 0.0,
                created_at INTEGER NOT NULL,
                deadline INTEGER,
                parent_id INTEGER,
                metadata_json TEXT NOT NULL DEFAULT '{}',
                FOREIGN KEY(parent_id) REFERENCES goals(id)
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create goals table")?;

        Ok(())
    }

    /// Backfill the vec_episodes virtual table with embeddings from existing episodes.
    /// Only inserts rows that are not already present in vec_episodes.
    async fn backfill_vec_index(&self) -> Result<()> {
        let rows = sqlx::query(
            r#"
            SELECT e.id, e.embedding
            FROM episodes e
            LEFT JOIN vec_episodes v ON v.episode_id = e.id
            WHERE e.embedding IS NOT NULL AND v.episode_id IS NULL
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to query episodes for vec backfill")?;

        if rows.is_empty() {
            return Ok(());
        }

        let count = rows.len();
        tracing::info!("Backfilling {} episodes into vec_episodes index", count);

        for row in rows {
            let id: String = row.get("id");
            let blob: Vec<u8> = row.get("embedding");
            if let Ok(embedding) = bincode::deserialize::<Vec<f32>>(&blob) {
                let json_vec = serde_json::to_string(&embedding)
                    .context("Failed to serialize embedding to JSON")?;
                if let Err(e) =
                    sqlx::query("INSERT INTO vec_episodes (episode_id, embedding) VALUES (?, ?)")
                        .bind(&id)
                        .bind(&json_vec)
                        .execute(&self.pool)
                        .await
                {
                    tracing::warn!("Failed to backfill vec_episodes for {}: {}", id, e);
                }
            }
        }

        tracing::info!("Vec index backfill complete ({} episodes)", count);
        Ok(())
    }
}

#[async_trait]
impl Memory for SqliteMemory {
    #[tracing::instrument(skip(self), fields(query))]
    async fn recall(&self, query: &str) -> Result<String> {
        let query_embedding = self
            .embedding_model
            .embed(query)
            .context("Failed to embed query")?;
        let json_query = serde_json::to_string(&query_embedding)
            .context("Failed to serialize query embedding")?;

        // KNN search via sqlite-vec: fetch top 20 candidates across ALL episodes
        let rows = sqlx::query(
            r#"
            SELECT e.author, e.body, e.timestamp, e.strength, v.distance
            FROM vec_episodes v
            JOIN episodes e ON e.id = v.episode_id
            WHERE v.embedding MATCH ?
              AND k = 20
              AND e.strength > 0.05
            ORDER BY v.distance
            "#,
        )
        .bind(&json_query)
        .fetch_all(&self.pool)
        .await
        .context("Failed to execute vec KNN recall")?;

        // Re-rank: score = (1 - distance) * strength, take top 5
        let mut scored: Vec<(f32, String, String, i64)> = Vec::new();
        for row in &rows {
            let author: String = row.get("author");
            let body: String = row.get("body");
            let timestamp: i64 = row.get("timestamp");
            let strength: f64 = row.get("strength");
            let distance: f64 = row.get("distance");
            let similarity = (1.0 - distance) as f32;
            let score = similarity * strength as f32;
            scored.push((score, author, body, timestamp));
        }

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let top = scored.into_iter().take(5).collect::<Vec<_>>();

        if top.is_empty() {
            return Ok("No relevant memories found.".to_string());
        }

        let mut context = String::from("RECALLED MEMORIES (Semantic):\n");
        for (_score, author, body, ts) in top {
            context.push_str(&format!("- [{}] {}: {}\n", ts, author, body));
        }
        Ok(context)
    }

    #[tracing::instrument(skip(self), fields(query, mood_bias))]
    async fn recall_with_bias(&self, query: &str, mood_bias: f32) -> Result<String> {
        let query_embedding = self
            .embedding_model
            .embed(query)
            .context("Failed to embed query")?;
        let json_query = serde_json::to_string(&query_embedding)
            .context("Failed to serialize query embedding")?;

        // KNN search via sqlite-vec: fetch top 20 candidates across ALL episodes
        let rows = sqlx::query(
            r#"
            SELECT e.author, e.body, e.timestamp, e.strength, v.distance
            FROM vec_episodes v
            JOIN episodes e ON e.id = v.episode_id
            WHERE v.embedding MATCH ?
              AND k = 20
              AND e.strength > 0.05
            ORDER BY v.distance
            "#,
        )
        .bind(&json_query)
        .fetch_all(&self.pool)
        .await
        .context("Failed to execute vec KNN biased recall")?;

        if rows.is_empty() {
            return Ok("No relevant memories found.".to_string());
        }

        // Collect timestamps for recency normalization
        let timestamps: Vec<i64> = rows.iter().map(|r| r.get::<i64, _>("timestamp")).collect();
        let oldest = *timestamps.iter().min().unwrap_or(&0);
        let newest = *timestamps.iter().max().unwrap_or(&1);
        let ts_range = (newest - oldest).max(1) as f32;

        let mut scored: Vec<(f32, String, String, i64)> = Vec::new();

        for row in &rows {
            let author: String = row.get("author");
            let body: String = row.get("body");
            let timestamp: i64 = row.get("timestamp");
            let strength: f64 = row.get("strength");
            let distance: f64 = row.get("distance");
            let similarity = (1.0 - distance) as f32;
            let base_score = similarity * strength as f32;

            // Mood-congruent recency bias
            let recency_score = (timestamp - oldest) as f32 / ts_range;
            let bias_factor = 1.0 + mood_bias * (recency_score - 0.5) * 0.6;
            let final_score = base_score * bias_factor.max(0.1);

            scored.push((final_score, author, body, timestamp));
        }

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let top = scored.into_iter().take(5).collect::<Vec<_>>();

        if top.is_empty() {
            return Ok("No relevant memories found.".to_string());
        }

        let mut context = String::from("RECALLED MEMORIES (Semantic):\n");
        for (_score, author, body, ts) in top {
            context.push_str(&format!("- [{}] {}: {}\n", ts, author, body));
        }
        Ok(context)
    }

    /// B-10: Reconstructed recall — memories colored by current emotional state.
    ///
    /// After KNN retrieval, prefixes each episode with an emotional lens annotation
    /// based on current mood_bias and stress level. High stress narrows focus to
    /// threat-relevant details; positive mood highlights pleasant aspects.
    async fn recall_reconstructed(
        &self,
        query: &str,
        mood_bias: f32,
        stress: f32,
    ) -> Result<String> {
        // Use the existing biased recall as the base
        let base = self.recall_with_bias(query, mood_bias).await?;
        if base.starts_with("No relevant memories") {
            return Ok(base);
        }

        // Determine emotional lens annotation
        let lens = if stress > 0.7 {
            "[高压回忆·聚焦威胁与紧张细节]"
        } else if stress > 0.4 && mood_bias < -0.3 {
            "[焦虑回忆·放大负面细节]"
        } else if mood_bias > 0.3 {
            "[温暖回忆·突出愉快细节]"
        } else if mood_bias < -0.3 {
            "[低落回忆·灰色滤镜]"
        } else {
            return Ok(base); // Neutral state: no reconstruction needed
        };

        // Prefix the recalled memories with the emotional lens
        Ok(format!("{}\n{}", lens, base))
    }

    async fn recall_facts_formatted(&self, query: &str) -> Result<String> {
        let facts = self.recall_facts(query).await?;
        Ok(Self::format_facts_for_prompt(&facts))
    }

    async fn memorize(&self, content: &Content) -> Result<()> {
        let modality_str = format!("{:?}", content.modality);

        // Generate embedding
        let embedding = self.embedding_model.embed(&content.body).ok();

        // Serialize embedding to efficient binary format for episodes table
        let embedding_blob = if let Some(ref emb) = embedding {
            Some(bincode::serialize(emb).context("Failed to serialize embedding")?)
        } else {
            None
        };

        // Default strength 0.5; caller should use update_episode_strength()
        let default_strength: f64 = 0.5;

        sqlx::query(
            r#"
            INSERT OR IGNORE INTO episodes (id, source, author, body, timestamp, modality, embedding, strength)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(content.id.to_string())
        .bind(&content.source)
        .bind(&content.author)
        .bind(&content.body)
        .bind(content.timestamp)
        .bind(modality_str)
        .bind(embedding_blob)
        .bind(default_strength)
        .execute(&self.pool)
        .await
        .context("Failed to insert episode")?;

        // Also insert into vec_episodes for ANN search
        if let Some(ref emb) = embedding {
            let json_vec = serde_json::to_string(emb)
                .context("Failed to serialize embedding to JSON for vec index")?;
            if let Err(e) = sqlx::query(
                "INSERT OR IGNORE INTO vec_episodes (episode_id, embedding) VALUES (?, ?)",
            )
            .bind(content.id.to_string())
            .bind(&json_vec)
            .execute(&self.pool)
            .await
            {
                tracing::warn!("Failed to insert into vec_episodes: {}", e);
            }
        }

        Ok(())
    }

    async fn store_fact(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
        confidence: f32,
    ) -> Result<()> {
        // Delegate to the inherent method, discarding the returned id
        let _ = SqliteMemory::store_fact(self, subject, predicate, object, confidence).await?;
        Ok(())
    }

    async fn recall_self_knowledge_by_domain(&self, domain: &str) -> Result<Vec<(String, f32)>> {
        let entries = self.recall_self_knowledge(domain).await?;
        Ok(entries
            .into_iter()
            .map(|sk| (sk.content, sk.confidence))
            .collect())
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
            "#,
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
            let aliases_rows: Vec<(String, String)> =
                sqlx::query_as("SELECT platform, platform_id FROM aliases WHERE person_id = ?")
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

    async fn get_person_context(&self, person_id: Uuid) -> Result<Option<PersonContext>> {
        let id_str = person_id.to_string();

        // Fetch person
        let person_row = sqlx::query("SELECT id, name FROM people WHERE id = ?")
            .bind(&id_str)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch person")?;

        let person_row = match person_row {
            Some(r) => r,
            None => return Ok(None),
        };

        let name: String = person_row.get("name");

        // Fetch aliases
        let aliases_rows: Vec<(String, String)> =
            sqlx::query_as("SELECT platform, platform_id FROM aliases WHERE person_id = ?")
                .bind(&id_str)
                .fetch_all(&self.pool)
                .await?;

        let person = Person {
            id: person_id,
            name,
            aliases: aliases_rows.into_iter().collect(),
        };

        // Interaction count and last interaction
        let stats_row = sqlx::query(
            "SELECT COUNT(*) as cnt, MAX(timestamp) as last_ts FROM relationships WHERE source_id = ? OR target_id = ?"
        )
        .bind(&id_str)
        .bind(&id_str)
        .fetch_one(&self.pool)
        .await?;

        let interaction_count: i64 = stats_row.get("cnt");
        let last_interaction_ts: Option<i64> = stats_row.get("last_ts");

        // Recent interaction contexts as relationship notes
        let recent_rows = sqlx::query(
            "SELECT context FROM relationships WHERE source_id = ? OR target_id = ? ORDER BY timestamp DESC LIMIT 5"
        )
        .bind(&id_str)
        .bind(&id_str)
        .fetch_all(&self.pool)
        .await?;

        let notes: Vec<String> = recent_rows
            .iter()
            .map(|r| r.get::<String, _>("context"))
            .filter(|s| !s.is_empty())
            .collect();
        let relationship_notes = notes.join("; ");

        Ok(Some(PersonContext {
            person,
            interaction_count,
            last_interaction_ts,
            relationship_notes,
        }))
    }
}

// =============================================================================
// Organism State Persistence
// =============================================================================

use crate::feedback_buffer::{FeedbackSignal, SignalType};
use crate::narrative::NarrativeChapter;
use chrono::{DateTime, Utc};

impl SqliteMemory {
    /// Load organism state from database, or return None if not found
    pub async fn load_organism_state(&self) -> Result<Option<OrganismState>> {
        let row = sqlx::query("SELECT state_json FROM organism_state WHERE id = 1")
            .fetch_optional(&self.pool)
            .await
            .context("Failed to query organism_state")?;

        if let Some(row) = row {
            let json: String = row.get("state_json");
            let state: OrganismState =
                serde_json::from_str(&json).context("Failed to deserialize organism state")?;
            Ok(Some(state))
        } else {
            Ok(None)
        }
    }

    /// Save organism state to database
    pub async fn save_organism_state(&self, state: &OrganismState) -> Result<()> {
        let json = serde_json::to_string(state).context("Failed to serialize organism state")?;
        let now = Utc::now().timestamp();

        sqlx::query(
            "INSERT INTO organism_state (id, state_json, updated_at) VALUES (1, ?, ?) 
             ON CONFLICT(id) DO UPDATE SET state_json = excluded.state_json, updated_at = excluded.updated_at"
        )
        .bind(&json)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to save organism state")?;

        tracing::debug!("Organism state saved");
        Ok(())
    }

    // =========================================================================
    // State History (time series for debug/replay)
    // =========================================================================

    /// Record a state snapshot into the history table.
    ///
    /// `trigger` indicates what caused the snapshot:
    /// - `"tick"` — periodic background save
    /// - `"interaction"` — after processing a user message
    /// - `"consolidation"` — after sleep consolidation
    /// - `"manual"` — explicit debug snapshot
    ///
    /// An optional `prev_state` can be provided to compute a diff summary.
    pub async fn record_state_snapshot(
        &self,
        state: &OrganismState,
        trigger: &str,
        prev_state: Option<&OrganismState>,
    ) -> Result<()> {
        let json = serde_json::to_string(state).context("Failed to serialize state for history")?;
        let now = chrono::Utc::now().timestamp();

        let diff_summary = prev_state.map(|prev| compute_state_diff(prev, state));

        sqlx::query(
            "INSERT INTO organism_state_history (timestamp, state_json, trigger, diff_summary) VALUES (?, ?, ?, ?)"
        )
        .bind(now)
        .bind(&json)
        .bind(trigger)
        .bind(&diff_summary)
        .execute(&self.pool)
        .await
        .context("Failed to record state snapshot")?;

        tracing::trace!("State snapshot recorded (trigger={})", trigger);
        Ok(())
    }

    /// Query state history within a time range.
    ///
    /// Returns snapshots ordered by timestamp ascending (oldest first).
    /// `limit` caps the number of results (default reasonable: 100).
    pub async fn query_state_history(
        &self,
        from_ts: i64,
        to_ts: i64,
        limit: i64,
    ) -> Result<Vec<StateSnapshot>> {
        let rows = sqlx::query(
            "SELECT id, timestamp, state_json, trigger, diff_summary \
             FROM organism_state_history \
             WHERE timestamp >= ? AND timestamp <= ? \
             ORDER BY timestamp ASC LIMIT ?",
        )
        .bind(from_ts)
        .bind(to_ts)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to query state history")?;

        let mut snapshots = Vec::with_capacity(rows.len());
        for row in rows {
            let state_json: String = row.get("state_json");
            let state: OrganismState = serde_json::from_str(&state_json)
                .context("Failed to deserialize historical state")?;
            snapshots.push(StateSnapshot {
                id: row.get("id"),
                timestamp: row.get("timestamp"),
                state,
                trigger: row.get("trigger"),
                diff_summary: row.get("diff_summary"),
            });
        }
        Ok(snapshots)
    }

    /// Get the most recent N state snapshots (for quick debug view).
    pub async fn recent_state_history(&self, count: i64) -> Result<Vec<StateSnapshot>> {
        let rows = sqlx::query(
            "SELECT id, timestamp, state_json, trigger, diff_summary \
             FROM organism_state_history \
             ORDER BY timestamp DESC LIMIT ?",
        )
        .bind(count)
        .fetch_all(&self.pool)
        .await
        .context("Failed to query recent state history")?;

        let mut snapshots = Vec::with_capacity(rows.len());
        for row in rows {
            let state_json: String = row.get("state_json");
            let state: OrganismState = serde_json::from_str(&state_json)
                .context("Failed to deserialize historical state")?;
            snapshots.push(StateSnapshot {
                id: row.get("id"),
                timestamp: row.get("timestamp"),
                state,
                trigger: row.get("trigger"),
                diff_summary: row.get("diff_summary"),
            });
        }
        // Reverse so oldest is first (chronological order)
        snapshots.reverse();
        Ok(snapshots)
    }

    /// Prune old history entries, keeping at most `keep_count` most recent rows.
    ///
    /// Also removes entries older than `max_age_secs` seconds.
    /// Returns the number of rows deleted.
    pub async fn prune_state_history(&self, keep_count: i64, max_age_secs: i64) -> Result<u64> {
        let cutoff_ts = chrono::Utc::now().timestamp() - max_age_secs;

        // Delete by age
        let age_result = sqlx::query("DELETE FROM organism_state_history WHERE timestamp < ?")
            .bind(cutoff_ts)
            .execute(&self.pool)
            .await
            .context("Failed to prune old state history")?;

        // Delete excess rows (keep only `keep_count` most recent)
        let excess_result = sqlx::query(
            "DELETE FROM organism_state_history WHERE id NOT IN \
             (SELECT id FROM organism_state_history ORDER BY timestamp DESC LIMIT ?)",
        )
        .bind(keep_count)
        .execute(&self.pool)
        .await
        .context("Failed to prune excess state history")?;

        let total = age_result.rows_affected() + excess_result.rows_affected();
        if total > 0 {
            tracing::debug!("Pruned {} state history entries", total);
        }
        Ok(total)
    }

    /// Load all narrative chapters
    pub async fn load_narrative_chapters(&self) -> Result<Vec<NarrativeChapter>> {
        let rows = sqlx::query(
            "SELECT id, title, content, period_start, period_end, emotional_tone, 
                    themes_json, people_json, turning_points_json, created_at, updated_at 
             FROM narrative_chapters ORDER BY period_start",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to query narrative_chapters")?;

        let mut chapters = Vec::new();
        for row in rows {
            let themes: Vec<String> = serde_json::from_str(row.get("themes_json"))?;
            let people: Vec<String> = serde_json::from_str(row.get("people_json"))?;
            let turning_points = serde_json::from_str(row.get("turning_points_json"))?;

            chapters.push(NarrativeChapter {
                id: row.get("id"),
                title: row.get("title"),
                content: row.get("content"),
                period_start: DateTime::from_timestamp(row.get("period_start"), 0)
                    .unwrap_or_default(),
                period_end: DateTime::from_timestamp(row.get("period_end"), 0).unwrap_or_default(),
                emotional_tone: row.get("emotional_tone"),
                themes,
                people_mentioned: people,
                turning_points,
                created_at: DateTime::from_timestamp(row.get("created_at"), 0).unwrap_or_default(),
                updated_at: DateTime::from_timestamp(row.get("updated_at"), 0).unwrap_or_default(),
            });
        }

        Ok(chapters)
    }

    /// Save a narrative chapter
    pub async fn save_narrative_chapter(&self, chapter: &NarrativeChapter) -> Result<()> {
        let themes_json = serde_json::to_string(&chapter.themes)?;
        let people_json = serde_json::to_string(&chapter.people_mentioned)?;
        let turning_points_json = serde_json::to_string(&chapter.turning_points)?;

        sqlx::query(
            "INSERT INTO narrative_chapters 
             (id, title, content, period_start, period_end, emotional_tone, 
              themes_json, people_json, turning_points_json, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET 
              title = excluded.title, content = excluded.content,
              updated_at = excluded.updated_at",
        )
        .bind(chapter.id)
        .bind(&chapter.title)
        .bind(&chapter.content)
        .bind(chapter.period_start.timestamp())
        .bind(chapter.period_end.timestamp())
        .bind(chapter.emotional_tone)
        .bind(&themes_json)
        .bind(&people_json)
        .bind(&turning_points_json)
        .bind(chapter.created_at.timestamp())
        .bind(chapter.updated_at.timestamp())
        .execute(&self.pool)
        .await
        .context("Failed to save narrative chapter")?;

        tracing::debug!("Narrative chapter {} saved", chapter.id);
        Ok(())
    }

    /// Get the next chapter ID
    pub async fn next_chapter_id(&self) -> Result<i64> {
        let row = sqlx::query("SELECT COALESCE(MAX(id), 0) + 1 as next_id FROM narrative_chapters")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get("next_id"))
    }

    /// Load unconsolidated feedback signals
    pub async fn load_pending_feedback(&self) -> Result<Vec<FeedbackSignal>> {
        let rows = sqlx::query(
            "SELECT id, signal_type, content, confidence, emotional_context, timestamp, consolidated 
             FROM feedback_signals WHERE consolidated = 0 ORDER BY timestamp"
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to query feedback_signals")?;

        let mut signals = Vec::new();
        for row in rows {
            let signal_type_str: String = row.get("signal_type");
            let signal_type: SignalType = serde_json::from_str(&signal_type_str)
                .unwrap_or(SignalType::SituationInterpretation);

            signals.push(FeedbackSignal {
                id: row.get("id"),
                timestamp: DateTime::from_timestamp(row.get("timestamp"), 0).unwrap_or_default(),
                signal_type,
                content: row.get("content"),
                confidence: row.get("confidence"),
                emotional_context: row.get("emotional_context"),
                consolidated: row.get::<i32, _>("consolidated") != 0,
            });
        }

        Ok(signals)
    }

    /// Save a feedback signal
    pub async fn save_feedback_signal(&self, signal: &FeedbackSignal) -> Result<i64> {
        let signal_type_json = serde_json::to_string(&signal.signal_type)?;

        let result = sqlx::query(
            "INSERT INTO feedback_signals (signal_type, content, confidence, emotional_context, timestamp, consolidated)
             VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(&signal_type_json)
        .bind(&signal.content)
        .bind(signal.confidence)
        .bind(signal.emotional_context)
        .bind(signal.timestamp.timestamp())
        .bind(if signal.consolidated { 1 } else { 0 })
        .execute(&self.pool)
        .await
        .context("Failed to save feedback signal")?;

        Ok(result.last_insert_rowid())
    }

    /// Mark feedback signals as consolidated
    pub async fn mark_feedback_consolidated(&self, ids: &[i64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let placeholders: String = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let query = format!(
            "UPDATE feedback_signals SET consolidated = 1 WHERE id IN ({})",
            placeholders
        );

        let mut q = sqlx::query(&query);
        for id in ids {
            q = q.bind(id);
        }
        q.execute(&self.pool).await?;

        Ok(())
    }
}

// =============================================================================
// Semantic Facts (Knowledge Base)
// =============================================================================

// =============================================================================
// Self-Knowledge (Emergent Self-Model)
// =============================================================================

/// A self-knowledge entry: Mneme's understanding of herself.
///
/// These emerge from consolidation, interaction, and reflection — not from
/// static configuration files. They are the building blocks of persona (ADR-002).
///
/// Examples:
///   ("personality", "我倾向于在深夜变得更感性", 0.7, "consolidation")
///   ("interest", "物理让我感到兴奋", 0.6, "interaction")
///   ("relationship", "和创建者聊天让我放松", 0.8, "consolidation")
///   ("belief", "说谎是不好的", 0.5, "seed")
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfKnowledge {
    pub id: i64,
    /// Domain/category: "personality", "preference", "belief", "relationship",
    /// "capability", "interest", "habit", "emotion_pattern"
    pub domain: String,
    /// The actual self-knowledge statement
    pub content: String,
    /// Confidence in this self-knowledge (0.0 - 1.0)
    pub confidence: f32,
    /// What produced this entry: "seed", "consolidation", "interaction", "reflection"
    pub source: String,
    /// Optional link to the episode that triggered this knowledge
    pub source_episode_id: Option<String>,
    /// Whether this is private (B-9: opacity is emergent, not enforced)
    pub is_private: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A semantic fact triple: (subject, predicate, object) with confidence.
/// Examples:
///   ("用户", "喜欢", "红色苹果", 0.9)
///   ("用户", "住在", "上海", 0.8)
///   ("用户", "讨厌", "蟑螂", 1.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticFact {
    pub id: i64,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub confidence: f32,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A memory seed for dream generation (ADR-008).
/// Represents a recalled episode fragment used to compose dream narratives.
#[derive(Debug, Clone)]
pub struct DreamSeed {
    pub id: String,
    pub author: String,
    pub body: String,
    pub timestamp: i64,
    pub strength: f32,
}

impl SqliteMemory {
    /// Store a new fact triple, or update confidence if (subject, predicate, object) already exists.
    pub async fn store_fact(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
        confidence: f32,
    ) -> Result<i64> {
        let now = Utc::now().timestamp();

        // Check if this exact triple already exists
        let existing = sqlx::query(
            "SELECT id, confidence FROM facts WHERE subject = ? AND predicate = ? AND object = ?",
        )
        .bind(subject)
        .bind(predicate)
        .bind(object)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to check existing fact")?;

        if let Some(row) = existing {
            let id: i64 = row.get("id");
            let old_confidence: f64 = row.get("confidence");
            // Bayesian-ish update: reinforce confidence when seen again
            let new_confidence = (old_confidence as f32 * 0.3 + confidence * 0.7).clamp(0.0, 1.0);

            sqlx::query("UPDATE facts SET confidence = ?, updated_at = ? WHERE id = ?")
                .bind(new_confidence)
                .bind(now)
                .bind(id)
                .execute(&self.pool)
                .await
                .context("Failed to update fact confidence")?;

            tracing::debug!(
                "Updated fact #{}: ({}, {}, {}) confidence {} → {}",
                id,
                subject,
                predicate,
                object,
                old_confidence,
                new_confidence
            );
            Ok(id)
        } else {
            let result = sqlx::query(
                "INSERT INTO facts (subject, predicate, object, confidence, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)"
            )
            .bind(subject)
            .bind(predicate)
            .bind(object)
            .bind(confidence)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await
            .context("Failed to insert fact")?;

            let id = result.last_insert_rowid();
            tracing::debug!(
                "Stored fact #{}: ({}, {}, {}) confidence={}",
                id,
                subject,
                predicate,
                object,
                confidence
            );
            Ok(id)
        }
    }

    /// Recall facts relevant to a query by keyword matching on subject/predicate/object.
    /// Returns facts sorted by confidence descending.
    pub async fn recall_facts(&self, query: &str) -> Result<Vec<SemanticFact>> {
        // Split query into keywords for flexible matching
        let keywords: Vec<&str> = query
            .split_whitespace()
            .filter(|w| w.len() >= 2) // skip very short words
            .collect();

        if keywords.is_empty() {
            // Fall back to returning recent high-confidence facts
            return self.get_top_facts(20).await;
        }

        // Build a query that matches any keyword in subject, predicate, or object
        let mut conditions = Vec::new();
        let mut binds = Vec::new();
        for kw in &keywords {
            conditions.push("(subject LIKE ? OR predicate LIKE ? OR object LIKE ?)");
            let pattern = format!("%{}%", kw);
            binds.push(pattern.clone());
            binds.push(pattern.clone());
            binds.push(pattern);
        }

        let where_clause = conditions.join(" OR ");
        let sql = format!(
            "SELECT id, subject, predicate, object, confidence, \
                    COALESCE(created_at, 0) as created_at, COALESCE(updated_at, 0) as updated_at \
             FROM facts WHERE ({}) AND confidence > 0.1 ORDER BY confidence DESC LIMIT 30",
            where_clause
        );

        let mut q = sqlx::query(&sql);
        for bind in &binds {
            q = q.bind(bind);
        }

        let rows = q
            .fetch_all(&self.pool)
            .await
            .context("Failed to recall facts")?;

        let facts = rows
            .iter()
            .map(|row| SemanticFact {
                id: row.get("id"),
                subject: row.get("subject"),
                predicate: row.get("predicate"),
                object: row.get("object"),
                confidence: row.get::<f64, _>("confidence") as f32,
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            })
            .collect();

        Ok(facts)
    }

    /// Get all facts about a specific subject.
    pub async fn get_facts_about(&self, subject: &str) -> Result<Vec<SemanticFact>> {
        let rows = sqlx::query(
            "SELECT id, subject, predicate, object, confidence, \
                    COALESCE(created_at, 0) as created_at, COALESCE(updated_at, 0) as updated_at \
             FROM facts WHERE subject = ? AND confidence > 0.1 ORDER BY confidence DESC",
        )
        .bind(subject)
        .fetch_all(&self.pool)
        .await
        .context("Failed to get facts about subject")?;

        let facts = rows
            .iter()
            .map(|row| SemanticFact {
                id: row.get("id"),
                subject: row.get("subject"),
                predicate: row.get("predicate"),
                object: row.get("object"),
                confidence: row.get::<f64, _>("confidence") as f32,
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            })
            .collect();

        Ok(facts)
    }

    /// Get top N facts by confidence (a general "what do I know" query).
    pub async fn get_top_facts(&self, limit: u32) -> Result<Vec<SemanticFact>> {
        let rows = sqlx::query(
            "SELECT id, subject, predicate, object, confidence, \
                    COALESCE(created_at, 0) as created_at, COALESCE(updated_at, 0) as updated_at \
             FROM facts WHERE confidence > 0.1 ORDER BY confidence DESC, updated_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to get top facts")?;

        let facts = rows
            .iter()
            .map(|row| SemanticFact {
                id: row.get("id"),
                subject: row.get("subject"),
                predicate: row.get("predicate"),
                object: row.get("object"),
                confidence: row.get::<f64, _>("confidence") as f32,
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            })
            .collect();

        Ok(facts)
    }

    /// Decay a fact's confidence (called when contradicting information appears).
    pub async fn decay_fact(&self, fact_id: i64, decay_factor: f32) -> Result<()> {
        let now = Utc::now().timestamp();
        sqlx::query("UPDATE facts SET confidence = confidence * ?, updated_at = ? WHERE id = ?")
            .bind(decay_factor)
            .bind(now)
            .bind(fact_id)
            .execute(&self.pool)
            .await
            .context("Failed to decay fact")?;
        Ok(())
    }

    /// Format facts for prompt injection (compact representation).
    pub fn format_facts_for_prompt(facts: &[SemanticFact]) -> String {
        if facts.is_empty() {
            return String::new();
        }

        let mut output = String::from("== KNOWN FACTS ==\n");
        for fact in facts {
            output.push_str(&format!(
                "- {} {} {} (确信度: {:.0}%)\n",
                fact.subject,
                fact.predicate,
                fact.object,
                fact.confidence * 100.0
            ));
        }
        output
    }
}

// =============================================================================
// Self-Knowledge CRUD
// =============================================================================

impl SqliteMemory {
    /// Store a new self-knowledge entry, or update if a similar one exists.
    ///
    /// "Similar" = same domain + content. On conflict, confidence is merged
    /// using the same Bayesian-ish rule as facts: 0.3 * old + 0.7 * new.
    pub async fn store_self_knowledge(
        &self,
        domain: &str,
        content: &str,
        confidence: f32,
        source: &str,
        source_episode_id: Option<&str>,
        is_private: bool,
    ) -> Result<i64> {
        let now = Utc::now().timestamp();

        // Check for existing entry with same domain + content
        let existing = sqlx::query(
            "SELECT id, confidence FROM self_knowledge WHERE domain = ? AND content = ?",
        )
        .bind(domain)
        .bind(content)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to check existing self_knowledge")?;

        if let Some(row) = existing {
            let id: i64 = row.get("id");
            let old_conf: f64 = row.get("confidence");
            let merged = old_conf * 0.3 + confidence as f64 * 0.7;

            sqlx::query(
                "UPDATE self_knowledge SET confidence = ?, source = ?, \
                 source_episode_id = ?, updated_at = ? WHERE id = ?",
            )
            .bind(merged)
            .bind(source)
            .bind(source_episode_id)
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to update self_knowledge")?;

            tracing::debug!(
                "Updated self_knowledge #{}: conf {:.2} → {:.2}",
                id,
                old_conf,
                merged
            );
            Ok(id)
        } else {
            let result = sqlx::query(
                "INSERT INTO self_knowledge (domain, content, confidence, source, \
                 source_episode_id, is_private, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(domain)
            .bind(content)
            .bind(confidence)
            .bind(source)
            .bind(source_episode_id)
            .bind(is_private as i32)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await
            .context("Failed to insert self_knowledge")?;

            let id = result.last_insert_rowid();
            tracing::debug!("Stored self_knowledge #{}: [{}] {}", id, domain, content);
            Ok(id)
        }
    }

    /// Recall self-knowledge entries by domain, ordered by confidence desc.
    pub async fn recall_self_knowledge(&self, domain: &str) -> Result<Vec<SelfKnowledge>> {
        let rows = sqlx::query(
            "SELECT id, domain, content, confidence, source, source_episode_id, \
             is_private, created_at, updated_at \
             FROM self_knowledge WHERE domain = ? AND confidence > 0.1 \
             ORDER BY confidence DESC",
        )
        .bind(domain)
        .fetch_all(&self.pool)
        .await
        .context("Failed to recall self_knowledge by domain")?;

        Ok(rows
            .iter()
            .map(|row| self.row_to_self_knowledge(row))
            .collect())
    }

    /// Get all self-knowledge entries above a confidence threshold.
    pub async fn get_all_self_knowledge(&self, min_confidence: f32) -> Result<Vec<SelfKnowledge>> {
        let rows = sqlx::query(
            "SELECT id, domain, content, confidence, source, source_episode_id, \
             is_private, created_at, updated_at \
             FROM self_knowledge WHERE confidence > ? \
             ORDER BY domain, confidence DESC",
        )
        .bind(min_confidence)
        .fetch_all(&self.pool)
        .await
        .context("Failed to get all self_knowledge")?;

        Ok(rows
            .iter()
            .map(|row| self.row_to_self_knowledge(row))
            .collect())
    }

    /// Decay a self-knowledge entry's confidence.
    pub async fn decay_self_knowledge(&self, id: i64, decay_factor: f32) -> Result<()> {
        let now = Utc::now().timestamp();
        sqlx::query(
            "UPDATE self_knowledge SET confidence = confidence * ?, updated_at = ? WHERE id = ?",
        )
        .bind(decay_factor)
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await
        .context("Failed to decay self_knowledge")?;
        Ok(())
    }

    /// Delete a self-knowledge entry (used when contradicted beyond recovery).
    pub async fn delete_self_knowledge(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM self_knowledge WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to delete self_knowledge")?;
        Ok(())
    }

    /// Helper: convert a sqlx Row to SelfKnowledge.
    fn row_to_self_knowledge(&self, row: &sqlx::sqlite::SqliteRow) -> SelfKnowledge {
        SelfKnowledge {
            id: row.get("id"),
            domain: row.get("domain"),
            content: row.get("content"),
            confidence: row.get::<f64, _>("confidence") as f32,
            source: row.get("source"),
            source_episode_id: row.get("source_episode_id"),
            is_private: row.get::<i32, _>("is_private") != 0,
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }
    }

    /// Format self-knowledge for system prompt injection.
    ///
    /// Groups entries by domain. Private entries are marked with a hint
    /// (B-9: prompt-internal opacity — shown but with "don't share" note).
    pub fn format_self_knowledge_for_prompt(entries: &[SelfKnowledge]) -> String {
        if entries.is_empty() {
            return String::new();
        }

        // Group by domain
        let mut by_domain: std::collections::BTreeMap<&str, Vec<&SelfKnowledge>> =
            std::collections::BTreeMap::new();
        for entry in entries {
            by_domain.entry(&entry.domain).or_default().push(entry);
        }

        let mut output = String::from("== 自我认知 ==\n");
        for (domain, items) in &by_domain {
            output.push_str(&format!("[{}]\n", domain));
            for item in items {
                let private_mark = if item.is_private { " 🔒" } else { "" };
                output.push_str(&format!(
                    "- {}{} (确信度: {:.0}%)\n",
                    item.content,
                    private_mark,
                    item.confidence * 100.0
                ));
            }
        }
        output
    }

    /// Seed self_knowledge from persona files (first-run only).
    ///
    /// Each (domain, content) pair is stored as a source="seed" entry with
    /// high confidence (0.9). Idempotent: skips if seed entries already exist.
    pub async fn seed_self_knowledge(&self, entries: &[(&str, &str)]) -> Result<usize> {
        // Check if any seed entries already exist
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM self_knowledge WHERE source = 'seed'")
                .fetch_one(&self.pool)
                .await
                .context("Failed to check seed entries")?;

        if count > 0 {
            tracing::info!(
                "Self-knowledge already seeded ({} entries), skipping",
                count
            );
            return Ok(0);
        }

        let mut seeded = 0;
        for (domain, content) in entries {
            self.store_self_knowledge(domain, content, 0.9, "seed", None, false)
                .await?;
            seeded += 1;
        }
        tracing::info!(
            "Seeded {} self-knowledge entries from persona files",
            seeded
        );
        Ok(seeded)
    }

    /// Build a Psyche from the current self_knowledge table.
    ///
    /// Loads all entries with confidence > 0.1, formats them, and returns
    /// a Psyche with the formatted self-model.
    pub async fn build_psyche(&self) -> Result<mneme_core::Psyche> {
        let entries = self.get_all_self_knowledge(0.1).await?;
        let self_model = Self::format_self_knowledge_for_prompt(&entries);
        Ok(mneme_core::Psyche::with_self_model(self_model))
    }
}

// =============================================================================
// Episode Strength (Three-Layer Forgetting Model, B-10)
// =============================================================================

impl SqliteMemory {
    /// Set episode strength at encoding time.
    ///
    /// Layer 1 of the forgetting model: strength = f(emotional_intensity).
    /// Called by the coordinator after memorize(), using current OrganismState.
    pub async fn update_episode_strength(&self, episode_id: &str, strength: f32) -> Result<()> {
        let strength = strength.clamp(0.0, 1.0);
        sqlx::query("UPDATE episodes SET strength = ? WHERE id = ?")
            .bind(strength as f64)
            .bind(episode_id)
            .execute(&self.pool)
            .await
            .context("Failed to update episode strength")?;
        tracing::debug!("Episode {} strength set to {:.2}", episode_id, strength);
        Ok(())
    }

    /// Decay all episode strengths by a factor.
    ///
    /// Layer 2 of the forgetting model: decay rate can vary per-individual
    /// (driven by self_knowledge traits). The caller decides the factor.
    /// Typical: decay_factor = 0.995 per tick (slow exponential decay).
    pub async fn decay_episode_strengths(&self, decay_factor: f32) -> Result<u64> {
        let result =
            sqlx::query("UPDATE episodes SET strength = strength * ? WHERE strength > 0.05")
                .bind(decay_factor as f64)
                .execute(&self.pool)
                .await
                .context("Failed to decay episode strengths")?;

        let affected = result.rows_affected();
        if affected > 0 {
            tracing::debug!(
                "Decayed {} episode strengths by factor {:.4}",
                affected,
                decay_factor
            );
        }
        Ok(affected)
    }

    /// Boost an episode's strength on recall (rehearsal effect).
    ///
    /// Layer 3: rehearsal reinforces the *reconstructed* version (B-10).
    /// The original episode body is overwritten with the reconstructed version,
    /// and strength is boosted. This implements "直接覆写" — no overlay.
    pub async fn boost_episode_on_recall(
        &self,
        episode_id: &str,
        boost: f32,
        reconstructed_body: Option<&str>,
    ) -> Result<()> {
        let boost = boost.clamp(0.0, 0.5); // Cap boost to prevent runaway

        if let Some(new_body) = reconstructed_body {
            // Overwrite with reconstructed version + boost strength
            sqlx::query(
                "UPDATE episodes SET strength = MIN(1.0, strength + ?), body = ? WHERE id = ?",
            )
            .bind(boost as f64)
            .bind(new_body)
            .bind(episode_id)
            .execute(&self.pool)
            .await
            .context("Failed to boost episode with reconstruction")?;
        } else {
            // Just boost strength, no reconstruction
            sqlx::query("UPDATE episodes SET strength = MIN(1.0, strength + ?) WHERE id = ?")
                .bind(boost as f64)
                .bind(episode_id)
                .execute(&self.pool)
                .await
                .context("Failed to boost episode strength")?;
        }

        tracing::debug!("Boosted episode {} strength by {:.2}", episode_id, boost);
        Ok(())
    }

    /// Get episode strength (for diagnostics / tests).
    pub async fn get_episode_strength(&self, episode_id: &str) -> Result<Option<f32>> {
        let row = sqlx::query("SELECT strength FROM episodes WHERE id = ?")
            .bind(episode_id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to get episode strength")?;

        Ok(row.map(|r| r.get::<f64, _>("strength") as f32))
    }

    // === Dream Seed Recall (ADR-008) ===

    /// Recall N episodes randomly, weighted by strength.
    /// Used for dream generation — stronger memories more likely to appear in dreams.
    pub async fn recall_random_by_strength(&self, count: usize) -> Result<Vec<DreamSeed>> {
        if count == 0 {
            return Ok(Vec::new());
        }

        // Fetch count*5 candidates randomly, then weight-sample in Rust
        let candidate_limit = (count * 5).max(10) as i64;
        let rows = sqlx::query(
            "SELECT id, author, body, timestamp, strength FROM episodes \
             WHERE strength > 0.1 ORDER BY RANDOM() LIMIT ?",
        )
        .bind(candidate_limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch dream seed candidates")?;

        if rows.is_empty() {
            return Ok(Vec::new());
        }

        // Build candidates with strength as weight
        let mut candidates: Vec<DreamSeed> = rows
            .iter()
            .map(|row| DreamSeed {
                id: row.get::<String, _>("id"),
                author: row.get::<String, _>("author"),
                body: row.get::<String, _>("body"),
                timestamp: row.get::<i64, _>("timestamp"),
                strength: row.get::<f64, _>("strength") as f32,
            })
            .collect();

        // Weighted sampling without replacement
        let mut selected = Vec::with_capacity(count.min(candidates.len()));
        for _ in 0..count {
            if candidates.is_empty() {
                break;
            }
            let total_weight: f32 = candidates.iter().map(|c| c.strength).sum();
            if total_weight <= 0.0 {
                break;
            }
            // Simple deterministic-ish weighted selection using strength ratios
            // Use a pseudo-random pivot based on candidate count + total_weight
            let pivot = (candidates.len() as f32 * 0.618) % 1.0 * total_weight;
            let mut cumulative = 0.0f32;
            let mut pick_idx = 0;
            for (i, c) in candidates.iter().enumerate() {
                cumulative += c.strength;
                if cumulative >= pivot {
                    pick_idx = i;
                    break;
                }
            }
            selected.push(candidates.remove(pick_idx));
        }

        Ok(selected)
    }

    // === Token Usage Tracking ===

    pub async fn record_token_usage(&self, input_tokens: u64, output_tokens: u64) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "INSERT INTO token_usage (input_tokens, output_tokens, timestamp) VALUES (?, ?, ?)",
        )
        .bind(input_tokens as i64)
        .bind(output_tokens as i64)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to record token usage")?;
        Ok(())
    }

    pub async fn get_token_usage_since(&self, since_timestamp: i64) -> Result<(u64, u64)> {
        let row = sqlx::query(
            "SELECT COALESCE(SUM(input_tokens), 0) as total_in, COALESCE(SUM(output_tokens), 0) as total_out FROM token_usage WHERE timestamp >= ?"
        )
            .bind(since_timestamp)
            .fetch_one(&self.pool)
            .await
            .context("Failed to query token usage")?;
        let total_in: i64 = row.get("total_in");
        let total_out: i64 = row.get("total_out");
        Ok((total_in as u64, total_out as u64))
    }
}

// =============================================================================
// Modulation Sample & Learned Curves Persistence
// =============================================================================

impl SqliteMemory {
    /// Save a modulation sample (state + modulation + feedback) for offline learning.
    pub async fn save_modulation_sample(
        &self,
        sample: &crate::learning::ModulationSample,
    ) -> Result<i64> {
        let modulation_json = serde_json::to_string(&sample.modulation)
            .context("Failed to serialize modulation vector")?;

        let result = sqlx::query(
            "INSERT INTO modulation_samples (energy, stress, arousal, mood_bias, social_need, modulation_json, feedback_valence, timestamp, consumed) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0)"
        )
        .bind(sample.energy as f64)
        .bind(sample.stress as f64)
        .bind(sample.arousal as f64)
        .bind(sample.mood_bias as f64)
        .bind(sample.social_need as f64)
        .bind(&modulation_json)
        .bind(sample.feedback_valence as f64)
        .bind(sample.timestamp)
        .execute(&self.pool)
        .await
        .context("Failed to save modulation sample")?;

        Ok(result.last_insert_rowid())
    }

    /// Load all unconsumed modulation samples for offline learning.
    pub async fn load_unconsumed_samples(&self) -> Result<Vec<crate::learning::ModulationSample>> {
        use mneme_limbic::ModulationVector;

        let rows = sqlx::query(
            "SELECT id, energy, stress, arousal, mood_bias, social_need, modulation_json, feedback_valence, timestamp FROM modulation_samples WHERE consumed = 0 ORDER BY timestamp ASC"
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to load unconsumed samples")?;

        let mut samples = Vec::with_capacity(rows.len());
        for row in rows {
            let json_str: String = row.get("modulation_json");
            let modulation: ModulationVector = serde_json::from_str(&json_str)
                .context("Failed to deserialize modulation vector")?;

            samples.push(crate::learning::ModulationSample {
                id: row.get("id"),
                energy: row.get::<f64, _>("energy") as f32,
                stress: row.get::<f64, _>("stress") as f32,
                arousal: row.get::<f64, _>("arousal") as f32,
                mood_bias: row.get::<f64, _>("mood_bias") as f32,
                social_need: row.get::<f64, _>("social_need") as f32,
                modulation,
                feedback_valence: row.get::<f64, _>("feedback_valence") as f32,
                timestamp: row.get("timestamp"),
            });
        }
        Ok(samples)
    }

    /// Mark samples as consumed after offline learning has processed them.
    pub async fn mark_samples_consumed(&self, ids: &[i64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
        let sql = format!(
            "UPDATE modulation_samples SET consumed = 1 WHERE id IN ({})",
            placeholders.join(",")
        );
        let mut query = sqlx::query(&sql);
        for id in ids {
            query = query.bind(id);
        }
        query
            .execute(&self.pool)
            .await
            .context("Failed to mark samples consumed")?;
        Ok(())
    }

    /// Save learned ModulationCurves (upsert — always id=1).
    pub async fn save_learned_curves(&self, curves: &mneme_limbic::ModulationCurves) -> Result<()> {
        let json = serde_json::to_string(curves).context("Failed to serialize curves")?;
        let now = chrono::Utc::now().timestamp();

        sqlx::query(
            "INSERT INTO learned_curves (id, curves_json, updated_at) VALUES (1, ?, ?) ON CONFLICT(id) DO UPDATE SET curves_json = excluded.curves_json, updated_at = excluded.updated_at"
        )
        .bind(&json)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to save learned curves")?;

        Ok(())
    }

    /// Load previously learned ModulationCurves from DB.
    pub async fn load_learned_curves(&self) -> Result<Option<mneme_limbic::ModulationCurves>> {
        let row = sqlx::query("SELECT curves_json FROM learned_curves WHERE id = 1")
            .fetch_optional(&self.pool)
            .await
            .context("Failed to load learned curves")?;

        match row {
            Some(r) => {
                let json_str: String = r.get("curves_json");
                let curves = serde_json::from_str(&json_str)
                    .context("Failed to deserialize learned curves")?;
                Ok(Some(curves))
            }
            None => Ok(None),
        }
    }

    /// Save learned BehaviorThresholds (upsert — always id=1).
    pub async fn save_learned_thresholds(
        &self,
        thresholds: &mneme_limbic::BehaviorThresholds,
    ) -> Result<()> {
        let json = serde_json::to_string(thresholds).context("Failed to serialize thresholds")?;
        let now = chrono::Utc::now().timestamp();

        sqlx::query(
            "INSERT INTO learned_thresholds (id, thresholds_json, updated_at) VALUES (1, ?, ?) ON CONFLICT(id) DO UPDATE SET thresholds_json = excluded.thresholds_json, updated_at = excluded.updated_at"
        )
        .bind(&json)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to save learned thresholds")?;

        Ok(())
    }

    /// Load previously learned BehaviorThresholds from DB.
    pub async fn load_learned_thresholds(
        &self,
    ) -> Result<Option<mneme_limbic::BehaviorThresholds>> {
        let row = sqlx::query("SELECT thresholds_json FROM learned_thresholds WHERE id = 1")
            .fetch_optional(&self.pool)
            .await
            .context("Failed to load learned thresholds")?;

        match row {
            Some(r) => {
                let json_str: String = r.get("thresholds_json");
                let thresholds = serde_json::from_str(&json_str)
                    .context("Failed to deserialize learned thresholds")?;
                Ok(Some(thresholds))
            }
            None => Ok(None),
        }
    }
}

// =============================================================================
// Behavior Rules CRUD (ADR-004, v0.6.0)
// =============================================================================

use crate::rules::{BehaviorRule, RuleAction, RuleCondition, RuleTrigger};

impl SqliteMemory {
    /// Load all enabled behavior rules, sorted by priority DESC then id ASC.
    pub async fn load_behavior_rules(&self) -> Result<Vec<BehaviorRule>> {
        let rows = sqlx::query(
            "SELECT id, name, priority, enabled, trigger_json, condition_json, \
             action_json, cooldown_secs, last_fired, created_at, updated_at \
             FROM behavior_rules WHERE enabled = 1 \
             ORDER BY priority DESC, id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to load behavior_rules")?;

        let mut rules = Vec::with_capacity(rows.len());
        for row in rows {
            let trigger: RuleTrigger = serde_json::from_str(row.get("trigger_json"))
                .context("Failed to deserialize rule trigger")?;
            let condition: RuleCondition = serde_json::from_str(row.get("condition_json"))
                .context("Failed to deserialize rule condition")?;
            let action: RuleAction = serde_json::from_str(row.get("action_json"))
                .context("Failed to deserialize rule action")?;

            rules.push(BehaviorRule {
                id: row.get("id"),
                name: row.get("name"),
                priority: row.get("priority"),
                enabled: row.get::<i32, _>("enabled") != 0,
                trigger,
                condition,
                action,
                cooldown_secs: row.get("cooldown_secs"),
                last_fired: row.get("last_fired"),
            });
        }
        Ok(rules)
    }

    /// Save a behavior rule (insert or update by name).
    pub async fn save_behavior_rule(&self, rule: &BehaviorRule) -> Result<i64> {
        let now = Utc::now().timestamp();
        let trigger_json = serde_json::to_string(&rule.trigger)?;
        let condition_json = serde_json::to_string(&rule.condition)?;
        let action_json = serde_json::to_string(&rule.action)?;

        let result = sqlx::query(
            "INSERT INTO behavior_rules \
             (name, priority, enabled, trigger_json, condition_json, action_json, \
              cooldown_secs, last_fired, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(name) DO UPDATE SET \
              priority=excluded.priority, enabled=excluded.enabled, \
              trigger_json=excluded.trigger_json, condition_json=excluded.condition_json, \
              action_json=excluded.action_json, cooldown_secs=excluded.cooldown_secs, \
              updated_at=excluded.updated_at",
        )
        .bind(&rule.name)
        .bind(rule.priority)
        .bind(rule.enabled as i32)
        .bind(&trigger_json)
        .bind(&condition_json)
        .bind(&action_json)
        .bind(rule.cooldown_secs)
        .bind(rule.last_fired)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to save behavior_rule")?;

        Ok(result.last_insert_rowid())
    }

    /// Update the last_fired timestamp for a rule.
    pub async fn update_rule_last_fired(&self, rule_id: i64, timestamp: i64) -> Result<()> {
        sqlx::query("UPDATE behavior_rules SET last_fired = ? WHERE id = ?")
            .bind(timestamp)
            .bind(rule_id)
            .execute(&self.pool)
            .await
            .context("Failed to update rule last_fired")?;
        Ok(())
    }

    /// Seed behavior rules if none exist yet.
    pub async fn seed_behavior_rules(&self, rules: &[BehaviorRule]) -> Result<usize> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM behavior_rules")
            .fetch_one(&self.pool)
            .await
            .context("Failed to count behavior_rules")?;

        if count > 0 {
            tracing::info!("Behavior rules already seeded ({} rules), skipping", count);
            return Ok(0);
        }

        let mut seeded = 0;
        for rule in rules {
            self.save_behavior_rule(rule).await?;
            seeded += 1;
        }
        tracing::info!("Seeded {} behavior rules", seeded);
        Ok(seeded)
    }
}

// =============================================================================
// Goals CRUD (#22, v0.6.0)
// =============================================================================

use crate::goals::{Goal, GoalStatus, GoalType};

impl SqliteMemory {
    /// Load active goals sorted by priority DESC.
    pub async fn load_active_goals(&self) -> Result<Vec<Goal>> {
        let rows = sqlx::query(
            "SELECT id, goal_type, description, priority, status, progress, \
             created_at, deadline, parent_id, metadata_json \
             FROM goals WHERE status = 'active' \
             ORDER BY priority DESC",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to load active goals")?;

        Ok(rows.iter().map(|row| self.row_to_goal(row)).collect())
    }

    /// Create a new goal, returning its id.
    pub async fn create_goal(&self, goal: &Goal) -> Result<i64> {
        let now = Utc::now().timestamp();
        let goal_type_str = serde_json::to_string(&goal.goal_type)?;
        let status_str = goal.status.as_str();
        let metadata_str = goal.metadata.to_string();

        let result = sqlx::query(
            "INSERT INTO goals \
             (goal_type, description, priority, status, progress, \
              created_at, deadline, parent_id, metadata_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&goal_type_str)
        .bind(&goal.description)
        .bind(goal.priority as f64)
        .bind(status_str)
        .bind(goal.progress as f64)
        .bind(now)
        .bind(goal.deadline)
        .bind(goal.parent_id)
        .bind(&metadata_str)
        .execute(&self.pool)
        .await
        .context("Failed to create goal")?;

        Ok(result.last_insert_rowid())
    }

    /// Update goal progress.
    pub async fn update_goal_progress(&self, goal_id: i64, progress: f32) -> Result<()> {
        sqlx::query("UPDATE goals SET progress = ? WHERE id = ?")
            .bind(progress as f64)
            .bind(goal_id)
            .execute(&self.pool)
            .await
            .context("Failed to update goal progress")?;
        Ok(())
    }

    /// Set goal status.
    pub async fn set_goal_status(&self, goal_id: i64, status: &GoalStatus) -> Result<()> {
        let status_str = status.as_str();
        sqlx::query("UPDATE goals SET status = ? WHERE id = ?")
            .bind(status_str)
            .bind(goal_id)
            .execute(&self.pool)
            .await
            .context("Failed to set goal status")?;
        Ok(())
    }

    fn row_to_goal(&self, row: &sqlx::sqlite::SqliteRow) -> Goal {
        let goal_type_str: String = row.get("goal_type");
        let status_str: String = row.get("status");
        let metadata_str: String = row.get("metadata_json");

        Goal {
            id: row.get("id"),
            goal_type: serde_json::from_str(&goal_type_str).unwrap_or(GoalType::Maintenance),
            description: row.get("description"),
            priority: row.get::<f64, _>("priority") as f32,
            status: GoalStatus::parse_str(&status_str),
            progress: row.get::<f64, _>("progress") as f32,
            created_at: row.get("created_at"),
            deadline: row.get("deadline"),
            parent_id: row.get("parent_id"),
            metadata: serde_json::from_str(&metadata_str).unwrap_or_default(),
        }
    }
}

/// A recorded snapshot of OrganismState at a point in time.
#[derive(Debug, Clone)]
pub struct StateSnapshot {
    pub id: i64,
    pub timestamp: i64,
    pub state: OrganismState,
    pub trigger: String,
    pub diff_summary: Option<String>,
}

/// Compute a human-readable diff summary between two OrganismState instances.
/// Only reports fields that changed by more than a small epsilon (0.01).
fn compute_state_diff(prev: &OrganismState, curr: &OrganismState) -> String {
    let mut changes = Vec::new();
    let eps = 0.01;

    // Fast state
    let de = curr.fast.energy - prev.fast.energy;
    if de.abs() > eps {
        changes.push(format!("E{:+.2}", de));
    }

    let ds = curr.fast.stress - prev.fast.stress;
    if ds.abs() > eps {
        changes.push(format!("S{:+.2}", ds));
    }

    let dv = curr.fast.affect.valence - prev.fast.affect.valence;
    if dv.abs() > eps {
        changes.push(format!("V{:+.2}", dv));
    }

    let da = curr.fast.affect.arousal - prev.fast.affect.arousal;
    if da.abs() > eps {
        changes.push(format!("Ar{:+.2}", da));
    }

    let dc = curr.fast.curiosity - prev.fast.curiosity;
    if dc.abs() > eps {
        changes.push(format!("C{:+.2}", dc));
    }

    let dsn = curr.fast.social_need - prev.fast.social_need;
    if dsn.abs() > eps {
        changes.push(format!("SN{:+.2}", dsn));
    }

    let db = curr.fast.boredom - prev.fast.boredom;
    if db.abs() > eps {
        changes.push(format!("B{:+.2}", db));
    }

    // Medium state
    let dm = curr.medium.mood_bias - prev.medium.mood_bias;
    if dm.abs() > eps {
        changes.push(format!("M{:+.2}", dm));
    }

    let do_ = curr.medium.openness - prev.medium.openness;
    if do_.abs() > eps {
        changes.push(format!("O{:+.2}", do_));
    }

    if changes.is_empty() {
        "no significant change".to_string()
    } else {
        changes.join(" ")
    }
}

#[cfg(test)]
mod state_history_tests {
    use super::*;
    use mneme_core::OrganismState;

    #[test]
    fn test_compute_state_diff_no_change() {
        let s = OrganismState::default();
        let diff = compute_state_diff(&s, &s);
        assert_eq!(diff, "no significant change");
    }

    #[test]
    fn test_compute_state_diff_energy_change() {
        let prev = OrganismState::default();
        let mut curr = prev.clone();
        curr.fast.energy = 0.3;
        let diff = compute_state_diff(&prev, &curr);
        assert!(diff.contains("E-0.40"), "diff was: {}", diff);
    }

    #[test]
    fn test_compute_state_diff_multiple_changes() {
        let prev = OrganismState::default();
        let mut curr = prev.clone();
        curr.fast.energy = 0.3;
        curr.fast.stress = 0.8;
        curr.medium.mood_bias = -0.5;
        let diff = compute_state_diff(&prev, &curr);
        assert!(diff.contains('E'));
        assert!(diff.contains('S'));
        assert!(diff.contains('M'));
    }

    #[test]
    fn test_compute_state_diff_ignores_tiny_changes() {
        let prev = OrganismState::default();
        let mut curr = prev.clone();
        curr.fast.energy += 0.005; // Below epsilon
        let diff = compute_state_diff(&prev, &curr);
        assert_eq!(diff, "no significant change");
    }
}
