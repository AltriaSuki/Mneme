use anyhow::{Context, Result};
use async_trait::async_trait;
use mneme_core::{Content, Memory, SocialGraph, Person, OrganismState};
use serde::{Serialize, Deserialize};
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite, Row};
use std::path::Path;
use uuid::Uuid;
use crate::embedding::{EmbeddingModel, cosine_similarity};
use std::sync::Arc;



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

        // Add strength column if it doesn't exist (v2 -> v3 migration)
        // Strength: memory trace intensity (0.0 - 1.0). Default 0.5 for pre-existing episodes.
        // Encoding layer of the three-layer forgetting model (B-10).
        if let Err(e) = sqlx::query("ALTER TABLE episodes ADD COLUMN strength REAL NOT NULL DEFAULT 0.5")
            .execute(&self.pool)
            .await
        {
            tracing::debug!("Column 'strength' likely exists or migration skipped: {}", e);
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
            "#
        )
        .execute(&self.pool)
        .await
        .context("Failed to create facts table")?;

        // Index for fast subject lookup
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_facts_subject ON facts(subject)"
        )
        .execute(&self.pool)
        .await
        .context("Failed to create facts subject index")?;

        // Index for fast predicate lookup (useful for querying "all likes", "all knows", etc.)
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_facts_predicate ON facts(predicate)"
        )
        .execute(&self.pool)
        .await
        .context("Failed to create facts predicate index")?;

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

        // === New tables for Organism State persistence ===
        
        // Organism state (singleton - only one row)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS organism_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                state_json TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );
            "#
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
            "#
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
            "#
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
            "#
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
            "#
        )
        .execute(&self.pool)
        .await
        .context("Failed to create self_knowledge table")?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_self_knowledge_domain ON self_knowledge(domain)"
        )
        .execute(&self.pool)
        .await
        .context("Failed to create self_knowledge domain index")?;

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
        let rows = sqlx::query("SELECT author, body, timestamp, embedding, strength FROM episodes WHERE strength > 0.05 ORDER BY timestamp DESC LIMIT 1000")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch episodes for vector search")?;

        let mut scored_episodes: Vec<(f32, String, String, i64)> = Vec::new();

        for row in rows {
            let author: String = row.get("author");
            let body: String = row.get("body");
            let timestamp: i64 = row.get("timestamp");
            let strength: f64 = row.get("strength");
            let embedding_blob: Option<Vec<u8>> = row.get("embedding");

            if let Some(blob) = embedding_blob {
                if let Ok(embedding) = bincode::deserialize::<Vec<f32>>(&blob) {
                    let similarity = cosine_similarity(&query_embedding, &embedding);
                    // Combined score: semantic relevance × memory strength
                    let score = similarity * strength as f32;
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

    async fn recall_facts_formatted(&self, query: &str) -> Result<String> {
        let facts = self.recall_facts(query).await?;
        Ok(Self::format_facts_for_prompt(&facts))
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

        // Default strength 0.5; caller should use update_episode_strength()
        // to set encoding strength based on emotional intensity (B-10 layer 1).
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

        Ok(())
    }

    async fn store_fact(&self, subject: &str, predicate: &str, object: &str, confidence: f32) -> Result<()> {
        // Delegate to the inherent method, discarding the returned id
        let _ = SqliteMemory::store_fact(self, subject, predicate, object, confidence).await?;
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

// =============================================================================
// Organism State Persistence
// =============================================================================

use crate::narrative::NarrativeChapter;
use crate::feedback_buffer::{FeedbackSignal, SignalType};
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
            let state: OrganismState = serde_json::from_str(&json)
                .context("Failed to deserialize organism state")?;
            Ok(Some(state))
        } else {
            Ok(None)
        }
    }

    /// Save organism state to database
    pub async fn save_organism_state(&self, state: &OrganismState) -> Result<()> {
        let json = serde_json::to_string(state)
            .context("Failed to serialize organism state")?;
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
        let json = serde_json::to_string(state)
            .context("Failed to serialize state for history")?;
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
             ORDER BY timestamp ASC LIMIT ?"
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
             ORDER BY timestamp DESC LIMIT ?"
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
        let age_result = sqlx::query(
            "DELETE FROM organism_state_history WHERE timestamp < ?"
        )
        .bind(cutoff_ts)
        .execute(&self.pool)
        .await
        .context("Failed to prune old state history")?;

        // Delete excess rows (keep only `keep_count` most recent)
        let excess_result = sqlx::query(
            "DELETE FROM organism_state_history WHERE id NOT IN \
             (SELECT id FROM organism_state_history ORDER BY timestamp DESC LIMIT ?)"
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
             FROM narrative_chapters ORDER BY period_start"
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
                period_start: DateTime::from_timestamp(row.get("period_start"), 0).unwrap_or_default(),
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
              updated_at = excluded.updated_at"
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
            "SELECT id, confidence FROM facts WHERE subject = ? AND predicate = ? AND object = ?"
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

            sqlx::query(
                "UPDATE facts SET confidence = ?, updated_at = ? WHERE id = ?"
            )
            .bind(new_confidence)
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to update fact confidence")?;

            tracing::debug!("Updated fact #{}: ({}, {}, {}) confidence {} → {}", id, subject, predicate, object, old_confidence, new_confidence);
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
            tracing::debug!("Stored fact #{}: ({}, {}, {}) confidence={}", id, subject, predicate, object, confidence);
            Ok(id)
        }
    }

    /// Recall facts relevant to a query by keyword matching on subject/predicate/object.
    /// Returns facts sorted by confidence descending.
    pub async fn recall_facts(&self, query: &str) -> Result<Vec<SemanticFact>> {
        // Split query into keywords for flexible matching
        let keywords: Vec<&str> = query.split_whitespace()
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

        let rows = q.fetch_all(&self.pool).await
            .context("Failed to recall facts")?;

        let facts = rows.iter().map(|row| {
            SemanticFact {
                id: row.get("id"),
                subject: row.get("subject"),
                predicate: row.get("predicate"),
                object: row.get("object"),
                confidence: row.get::<f64, _>("confidence") as f32,
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            }
        }).collect();

        Ok(facts)
    }

    /// Get all facts about a specific subject.
    pub async fn get_facts_about(&self, subject: &str) -> Result<Vec<SemanticFact>> {
        let rows = sqlx::query(
            "SELECT id, subject, predicate, object, confidence, \
                    COALESCE(created_at, 0) as created_at, COALESCE(updated_at, 0) as updated_at \
             FROM facts WHERE subject = ? AND confidence > 0.1 ORDER BY confidence DESC"
        )
        .bind(subject)
        .fetch_all(&self.pool)
        .await
        .context("Failed to get facts about subject")?;

        let facts = rows.iter().map(|row| {
            SemanticFact {
                id: row.get("id"),
                subject: row.get("subject"),
                predicate: row.get("predicate"),
                object: row.get("object"),
                confidence: row.get::<f64, _>("confidence") as f32,
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            }
        }).collect();

        Ok(facts)
    }

    /// Get top N facts by confidence (a general "what do I know" query).
    pub async fn get_top_facts(&self, limit: u32) -> Result<Vec<SemanticFact>> {
        let rows = sqlx::query(
            "SELECT id, subject, predicate, object, confidence, \
                    COALESCE(created_at, 0) as created_at, COALESCE(updated_at, 0) as updated_at \
             FROM facts WHERE confidence > 0.1 ORDER BY confidence DESC, updated_at DESC LIMIT ?"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to get top facts")?;

        let facts = rows.iter().map(|row| {
            SemanticFact {
                id: row.get("id"),
                subject: row.get("subject"),
                predicate: row.get("predicate"),
                object: row.get("object"),
                confidence: row.get::<f64, _>("confidence") as f32,
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            }
        }).collect();

        Ok(facts)
    }

    /// Decay a fact's confidence (called when contradicting information appears).
    pub async fn decay_fact(&self, fact_id: i64, decay_factor: f32) -> Result<()> {
        let now = Utc::now().timestamp();
        sqlx::query(
            "UPDATE facts SET confidence = confidence * ?, updated_at = ? WHERE id = ?"
        )
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
                fact.subject, fact.predicate, fact.object,
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
            "SELECT id, confidence FROM self_knowledge WHERE domain = ? AND content = ?"
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
                 source_episode_id = ?, updated_at = ? WHERE id = ?"
            )
            .bind(merged)
            .bind(source)
            .bind(source_episode_id)
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to update self_knowledge")?;

            tracing::debug!("Updated self_knowledge #{}: conf {:.2} → {:.2}", id, old_conf, merged);
            Ok(id)
        } else {
            let result = sqlx::query(
                "INSERT INTO self_knowledge (domain, content, confidence, source, \
                 source_episode_id, is_private, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
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
             ORDER BY confidence DESC"
        )
        .bind(domain)
        .fetch_all(&self.pool)
        .await
        .context("Failed to recall self_knowledge by domain")?;

        Ok(rows.iter().map(|row| self.row_to_self_knowledge(row)).collect())
    }

    /// Get all self-knowledge entries above a confidence threshold.
    pub async fn get_all_self_knowledge(&self, min_confidence: f32) -> Result<Vec<SelfKnowledge>> {
        let rows = sqlx::query(
            "SELECT id, domain, content, confidence, source, source_episode_id, \
             is_private, created_at, updated_at \
             FROM self_knowledge WHERE confidence > ? \
             ORDER BY domain, confidence DESC"
        )
        .bind(min_confidence)
        .fetch_all(&self.pool)
        .await
        .context("Failed to get all self_knowledge")?;

        Ok(rows.iter().map(|row| self.row_to_self_knowledge(row)).collect())
    }

    /// Decay a self-knowledge entry's confidence.
    pub async fn decay_self_knowledge(&self, id: i64, decay_factor: f32) -> Result<()> {
        let now = Utc::now().timestamp();
        sqlx::query(
            "UPDATE self_knowledge SET confidence = confidence * ?, updated_at = ? WHERE id = ?"
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
                    item.content, private_mark,
                    item.confidence * 100.0
                ));
            }
        }
        output
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
        let result = sqlx::query(
            "UPDATE episodes SET strength = strength * ? WHERE strength > 0.05"
        )
        .bind(decay_factor as f64)
        .execute(&self.pool)
        .await
        .context("Failed to decay episode strengths")?;

        let affected = result.rows_affected();
        if affected > 0 {
            tracing::debug!("Decayed {} episode strengths by factor {:.4}", affected, decay_factor);
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
                "UPDATE episodes SET strength = MIN(1.0, strength + ?), body = ? WHERE id = ?"
            )
            .bind(boost as f64)
            .bind(new_body)
            .bind(episode_id)
            .execute(&self.pool)
            .await
            .context("Failed to boost episode with reconstruction")?;
        } else {
            // Just boost strength, no reconstruction
            sqlx::query(
                "UPDATE episodes SET strength = MIN(1.0, strength + ?) WHERE id = ?"
            )
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
}

// =============================================================================
// State History Types
// =============================================================================

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
    if de.abs() > eps { changes.push(format!("E{:+.2}", de)); }

    let ds = curr.fast.stress - prev.fast.stress;
    if ds.abs() > eps { changes.push(format!("S{:+.2}", ds)); }

    let dv = curr.fast.affect.valence - prev.fast.affect.valence;
    if dv.abs() > eps { changes.push(format!("V{:+.2}", dv)); }

    let da = curr.fast.affect.arousal - prev.fast.affect.arousal;
    if da.abs() > eps { changes.push(format!("Ar{:+.2}", da)); }

    let dc = curr.fast.curiosity - prev.fast.curiosity;
    if dc.abs() > eps { changes.push(format!("C{:+.2}", dc)); }

    let dsn = curr.fast.social_need - prev.fast.social_need;
    if dsn.abs() > eps { changes.push(format!("SN{:+.2}", dsn)); }

    let db = curr.fast.boredom - prev.fast.boredom;
    if db.abs() > eps { changes.push(format!("B{:+.2}", db)); }

    // Medium state
    let dm = curr.medium.mood_bias - prev.medium.mood_bias;
    if dm.abs() > eps { changes.push(format!("M{:+.2}", dm)); }

    let do_ = curr.medium.openness - prev.medium.openness;
    if do_.abs() > eps { changes.push(format!("O{:+.2}", do_)); }

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
