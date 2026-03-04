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
    /// B-12: Optional encryption for memory at rest.
    encryptor: Option<mneme_core::encrypt::MemoryEncryptor>,
}

impl SqliteMemory {
    pub async fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        // Initialize embedding model (first run downloads ~100MB, may take a minute)
        tracing::info!("Loading embedding model (首次运行需下载模型，请稍候)...");
        let embedding_model =
            Arc::new(EmbeddingModel::new().context("Failed to initialize embedding model")?);
        tracing::info!("Embedding model ready");

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
            encryptor: None,
        };
        memory.migrate().await?;
        Ok(memory)
    }

    /// B-12: Set encryption for memory at rest.
    pub fn set_encryptor(&mut self, enc: mneme_core::encrypt::MemoryEncryptor) {
        self.encryptor = Some(enc);
    }

    /// Encrypt body if encryptor is set, otherwise return as-is.
    fn encrypt_body(&self, body: &str) -> String {
        match &self.encryptor {
            Some(enc) => enc.encrypt(body).unwrap_or_else(|e| {
                tracing::warn!("Encryption failed, storing plaintext: {}", e);
                body.to_string()
            }),
            None => body.to_string(),
        }
    }

    /// Decrypt body if encryptor is set and content looks encrypted (base64).
    fn decrypt_body(&self, body: &str) -> String {
        match &self.encryptor {
            Some(enc) => enc.decrypt(body).unwrap_or_else(|_| {
                // Heuristic: if body looks like base64 (no CJK, no spaces, length > 20),
                // it's likely encrypted with a different key — return placeholder instead
                // of leaking ciphertext into prompts.
                let looks_encrypted = body.len() > 20
                    && !body.contains(' ')
                    && !body.chars().any(|c| c > '\u{2E80}');
                if looks_encrypted {
                    tracing::warn!(
                        "Decryption failed for content (len={}), likely key mismatch — returning placeholder",
                        body.len()
                    );
                    "[encrypted: key mismatch]".to_string()
                } else {
                    // Plaintext that was stored before encryption was enabled
                    body.to_string()
                }
            }),
            None => body.to_string(),
        }
    }

    /// B-5: Somatic dissonance check — cross-reference a recalled episode's
    /// timestamp against ODE state history. Returns a penalty factor (0.0–1.0)
    /// where 1.0 = no dissonance, lower = suspicious.
    async fn somatic_dissonance_penalty(&self, episode_ts: i64) -> f32 {
        // Look for state history within ±5 minutes of the episode timestamp
        let window = 300i64;
        let from = episode_ts - window;
        let to = episode_ts + window;

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM organism_state_history \
             WHERE timestamp >= ? AND timestamp <= ?",
        )
        .bind(from)
        .bind(to)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        if count > 0 {
            // History exists near this timestamp — memory is corroborated
            1.0
        } else {
            // No ODE history at this timestamp — body has no record of this event
            tracing::warn!(
                episode_ts,
                "Somatic dissonance: no ODE history near episode timestamp"
            );
            0.15
        }
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .context("Failed to run database migrations")?;

        // Backfill: insert any episodes that have embeddings but are missing from vec_episodes
        self.backfill_vec_index().await?;

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

/// B-6: Format a Unix timestamp as relative Chinese time string.
/// Gives Mneme temporal grounding for recalled memories.
fn format_relative_time(ts: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let delta = (now - ts).max(0);
    if delta < 60 {
        "刚才".to_string()
    } else if delta < 3600 {
        format!("{}分钟前", delta / 60)
    } else if delta < 86400 {
        let h = delta / 3600;
        let m = (delta % 3600) / 60;
        if m > 0 {
            format!("{}小时{}分钟前", h, m)
        } else {
            format!("{}小时前", h)
        }
    } else {
        let d = delta / 86400;
        let h = (delta % 86400) / 3600;
        if d > 30 {
            format!("{}个月前", d / 30)
        } else if h > 0 {
            format!("{}天{}小时前", d, h)
        } else {
            format!("{}天前", d)
        }
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
            SELECT e.id, e.author, e.body, e.timestamp, e.strength, v.distance
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
        let mut scored: Vec<(f32, String, String, String, i64)> = Vec::new();
        for row in &rows {
            let id: String = row.get("id");
            let author: String = row.get("author");
            let body_raw: String = row.get("body");
            let body = self.decrypt_body(&body_raw);
            // Physical isolation: silently drop undecryptable memories.
            // The LLM must never learn that encrypted content exists.
            if body.starts_with("[encrypted:") { continue; }
            let timestamp: i64 = row.get("timestamp");
            let strength: f64 = row.get("strength");
            // B-10: Physical memory degradation — old/weak memories lose detail
            let body = degrade_by_strength(&body, strength);
            let distance: f64 = row.get("distance");
            let similarity = (1.0 - distance) as f32;
            let score = similarity * strength as f32;
            scored.push((score, id, author, body, timestamp));
        }

        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        let top = scored.into_iter().take(5).collect::<Vec<_>>();

        if top.is_empty() {
            // P0-1: Recency fallback for base recall (same logic as recall_with_bias)
            tracing::info!("Recall: no scored matches, attempting recency fallback");
            let recent_rows = sqlx::query(
                r#"
                SELECT id, author, body, timestamp, strength
                FROM episodes
                WHERE strength > 0.05
                ORDER BY timestamp DESC
                LIMIT 5
                "#,
            )
            .fetch_all(&self.pool)
            .await
            .context("Failed to execute recency fallback query")?;

            if recent_rows.is_empty() {
                return Ok("No relevant memories found.".to_string());
            }

            let mut context = String::from("RECALLED MEMORIES (Recent):\n");
            for row in &recent_rows {
                let author: String = row.get("author");
                let body_raw: String = row.get("body");
                let body = self.decrypt_body(&body_raw);
                if body.starts_with("[encrypted:") { continue; }
                let timestamp: i64 = row.get("timestamp");
                let strength: f64 = row.get("strength");
                let body = degrade_by_strength(&body, strength);
                context.push_str(&format!("- [{}] {}: {}\n", format_relative_time(timestamp), author, body));
            }
            return Ok(context);
        }

        // ACT-R retrieval reinforcement: boost recalled episodes
        for (_, id, _, _, _) in &top {
            let _ = self.boost_episode_on_recall(id, 0.02, None).await;
        }

        let episode_count = top.len();
        let mut context = String::from("RECALLED MEMORIES (Semantic):\n");
        for (_score, _id, author, body, ts) in top {
            context.push_str(&format!("- [{}] {}: {}\n", format_relative_time(ts), author, body));
        }
        tracing::info!("Recall: returning {} chars, {} episodes", context.len(), episode_count);
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
            SELECT e.id, e.author, e.body, e.timestamp, e.strength, v.distance
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
            tracing::info!("Recall: no embedding matches, attempting recency fallback");
            // P0-1: Recency fallback — when semantic search finds nothing,
            // fall back to the N most recent episodes so the LLM at least knows
            // prior conversations happened. Prevents "this is our first interaction"
            // disaster (bench R63/79/81). B-6: temporal grounding.
            let recent_rows = sqlx::query(
                r#"
                SELECT id, author, body, timestamp, strength
                FROM episodes
                WHERE strength > 0.05
                ORDER BY timestamp DESC
                LIMIT 5
                "#,
            )
            .fetch_all(&self.pool)
            .await
            .context("Failed to execute recency fallback query")?;

            if recent_rows.is_empty() {
                return Ok("No relevant memories found.".to_string());
            }

            tracing::info!("Recall: recency fallback returning {} episodes", recent_rows.len());
            let mut context = String::from("RECALLED MEMORIES (Recent):\n");
            for row in &recent_rows {
                let author: String = row.get("author");
                let body_raw: String = row.get("body");
                let body = self.decrypt_body(&body_raw);
                if body.starts_with("[encrypted:") { continue; }
                let timestamp: i64 = row.get("timestamp");
                let strength: f64 = row.get("strength");
                let body = degrade_by_strength(&body, strength);
                context.push_str(&format!("- [{}] {}: {}\n", format_relative_time(timestamp), author, body));
            }
            return Ok(context);
        }
        tracing::info!("Recall: found {} candidate episodes", rows.len());

        // Collect timestamps for recency normalization
        let timestamps: Vec<i64> = rows.iter().map(|r| r.get::<i64, _>("timestamp")).collect();
        let oldest = *timestamps.iter().min().unwrap_or(&0);
        let newest = *timestamps.iter().max().unwrap_or(&1);
        let ts_range = (newest - oldest).max(1) as f32;

        let mut scored: Vec<(f32, String, String, String, i64)> = Vec::new();

        for row in &rows {
            let id: String = row.get("id");
            let author: String = row.get("author");
            let body_raw: String = row.get("body");
            let body = self.decrypt_body(&body_raw);
            // Physical isolation: silently drop undecryptable memories.
            if body.starts_with("[encrypted:") { continue; }
            let timestamp: i64 = row.get("timestamp");
            let strength: f64 = row.get("strength");
            // B-10: Physical memory degradation — old/weak memories lose detail
            let body = degrade_by_strength(&body, strength);
            let distance: f64 = row.get("distance");
            let similarity = (1.0 - distance) as f32;
            let base_score = similarity * strength as f32;

            // Mood-congruent recency bias
            let recency_score = (timestamp - oldest) as f32 / ts_range;
            let bias_factor = 1.0 + mood_bias * (recency_score - 0.5) * 0.6;

            // B-5: Somatic dissonance — penalize memories with no ODE corroboration
            let dissonance = self.somatic_dissonance_penalty(timestamp).await;
            let final_score = base_score * bias_factor.max(0.1) * dissonance;

            // Physical isolation: dissonance affects score only, no text injection.
            // The score penalty IS the physical mechanism — low-dissonance memories
            // rank lower, naturally reducing their influence on the LLM.
            scored.push((final_score, id, author, body, timestamp));
        }

        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        let top = scored.into_iter().take(5).collect::<Vec<_>>();

        if top.is_empty() {
            return Ok("No relevant memories found.".to_string());
        }

        // ACT-R retrieval reinforcement: boost recalled episodes
        for (_, id, _, _, _) in &top {
            let _ = self.boost_episode_on_recall(id, 0.02, None).await;
        }

        let episode_count = top.len();
        let mut context = String::from("RECALLED MEMORIES (Semantic):\n");
        for (_score, _id, author, body, ts) in top {
            context.push_str(&format!("- [{}] {}: {}\n", format_relative_time(ts), author, body));
        }
        tracing::info!("Recall: returning {} chars, {} episodes", context.len(), episode_count);
        Ok(context)
    }

    /// B-10: Reconstructed recall — memories colored by current emotional state.
    ///
    /// Physical reconstruction mechanisms (no narrative injection):
    /// 1. `mood_bias` biases which memories surface (via recall_with_bias scoring)
    /// 2. `stress` increases memory degradation (stressed recall = more detail loss)
    ///    This models real human cognition: high stress impairs episodic recall fidelity.
    ///
    /// Physical isolation: no text annotations. Stress makes memories physically
    /// shorter/vaguer — the LLM infers emotional coloring from the degraded context.
    async fn recall_reconstructed(
        &self,
        query: &str,
        mood_bias: f32,
        stress: f32,
    ) -> Result<String> {
        let query_embedding = self
            .embedding_model
            .embed(query)
            .context("Failed to embed query")?;
        let json_query = serde_json::to_string(&query_embedding)
            .context("Failed to serialize query embedding")?;

        let rows = sqlx::query(
            r#"
            SELECT e.id, e.author, e.body, e.timestamp, e.strength, v.distance
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
        .context("Failed to execute vec KNN reconstructed recall")?;

        if rows.is_empty() {
            // P0-1: Recency fallback
            tracing::info!("Reconstructed recall: no embedding matches, attempting recency fallback");
            let recent_rows = sqlx::query(
                r#"
                SELECT id, author, body, timestamp, strength
                FROM episodes
                WHERE strength > 0.05
                ORDER BY timestamp DESC
                LIMIT 5
                "#,
            )
            .fetch_all(&self.pool)
            .await
            .context("Failed to execute recency fallback query")?;

            if recent_rows.is_empty() {
                return Ok("No relevant memories found.".to_string());
            }

            let mut context = String::from("RECALLED MEMORIES (Recent):\n");
            for row in &recent_rows {
                let author: String = row.get("author");
                let body_raw: String = row.get("body");
                let body = self.decrypt_body(&body_raw);
                if body.starts_with("[encrypted:") { continue; }
                let timestamp: i64 = row.get("timestamp");
                let strength: f64 = row.get("strength");
                // B-10: Stress degrades recall further
                let effective_strength = strength * (1.0 - stress as f64 * 0.5);
                let body = degrade_by_strength(&body, effective_strength);
                context.push_str(&format!("- [{}] {}: {}\n", format_relative_time(timestamp), author, body));
            }
            return Ok(context);
        }

        // Scoring with mood-bias + stress
        let timestamps: Vec<i64> = rows.iter().map(|r| r.get::<i64, _>("timestamp")).collect();
        let oldest = *timestamps.iter().min().unwrap_or(&0);
        let newest = *timestamps.iter().max().unwrap_or(&1);
        let ts_range = (newest - oldest).max(1) as f32;

        let mut scored: Vec<(f32, String, String, String, i64, f64)> = Vec::new();

        for row in &rows {
            let id: String = row.get("id");
            let author: String = row.get("author");
            let body_raw: String = row.get("body");
            let body = self.decrypt_body(&body_raw);
            if body.starts_with("[encrypted:") { continue; }
            let timestamp: i64 = row.get("timestamp");
            let strength: f64 = row.get("strength");
            let distance: f64 = row.get("distance");
            let similarity = (1.0 - distance) as f32;
            let base_score = similarity * strength as f32;

            let recency_score = (timestamp - oldest) as f32 / ts_range;
            let bias_factor = 1.0 + mood_bias * (recency_score - 0.5) * 0.6;
            let dissonance = self.somatic_dissonance_penalty(timestamp).await;
            let final_score = base_score * bias_factor.max(0.1) * dissonance;

            scored.push((final_score, id, author, body, timestamp, strength));
        }

        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        let top = scored.into_iter().take(5).collect::<Vec<_>>();

        if top.is_empty() {
            return Ok("No relevant memories found.".to_string());
        }

        for (_, id, _, _, _, _) in &top {
            let _ = self.boost_episode_on_recall(id, 0.02, None).await;
        }

        let episode_count = top.len();
        let mut context = String::from("RECALLED MEMORIES (Semantic):\n");
        for (_score, _id, author, body, ts, strength) in top {
            // B-10: Stress reduces effective strength → more degradation under stress.
            // stress=0.0: no extra degradation; stress=1.0: halves effective strength.
            // This models cortisol-impaired episodic recall in real biology.
            let effective_strength = strength * (1.0 - stress as f64 * 0.5);
            let body = degrade_by_strength(&body, effective_strength);
            context.push_str(&format!("- [{}] {}: {}\n", format_relative_time(ts), author, body));
        }
        tracing::info!("Reconstructed recall: returning {} chars, {} episodes (stress={:.2})", context.len(), episode_count, stress);
        Ok(context)
    }

    async fn recall_facts_formatted(&self, query: &str) -> Result<String> {
        let facts = self.recall_facts(query).await?;
        Ok(Self::format_facts_for_prompt(&facts))
    }

    #[tracing::instrument(skip(self, content), fields(id = %content.id, author = %content.author))]
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

        // B-12: Encrypt body at rest if encryptor is configured
        let stored_body = self.encrypt_body(&content.body);

        sqlx::query(
            r#"
            INSERT OR IGNORE INTO episodes (id, source, author, body, timestamp, modality, embedding, strength)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(content.id.to_string())
        .bind(&content.source)
        .bind(&content.author)
        .bind(&stored_body)
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

    async fn memorize_with_strength(&self, content: &Content, strength: f32) -> Result<()> {
        let modality_str = format!("{:?}", content.modality);
        let embedding = self.embedding_model.embed(&content.body).ok();
        let embedding_blob = if let Some(ref emb) = embedding {
            Some(bincode::serialize(emb).context("Failed to serialize embedding")?)
        } else {
            None
        };
        let stored_body = self.encrypt_body(&content.body);
        sqlx::query(
            "INSERT OR IGNORE INTO episodes (id, source, author, body, timestamp, modality, embedding, strength) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(content.id.to_string())
        .bind(&content.source)
        .bind(&content.author)
        .bind(&stored_body)
        .bind(content.timestamp)
        .bind(modality_str)
        .bind(embedding_blob)
        .bind(strength as f64)
        .execute(&self.pool)
        .await
        .context("Failed to insert episode")?;
        if let Some(ref emb) = embedding {
            let json_vec = serde_json::to_string(emb)?;
            let _ = sqlx::query(
                "INSERT OR IGNORE INTO vec_episodes (episode_id, embedding) VALUES (?, ?)",
            )
            .bind(content.id.to_string())
            .bind(&json_vec)
            .execute(&self.pool)
            .await;
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

    /// B-21: Detect repeated patterns across self_knowledge entries.
    ///
    /// Groups entries by domain and looks for content that appears multiple times
    /// (from different sources or timestamps). Returns (pattern_summary, count) pairs.
    async fn detect_repeated_patterns(&self, min_count: usize) -> Result<Vec<(String, usize)>> {
        let rows = sqlx::query(
            "SELECT domain, content, COUNT(*) as cnt \
             FROM self_knowledge \
             WHERE confidence > 0.3 \
             GROUP BY domain, content \
             HAVING cnt >= ? \
             ORDER BY cnt DESC \
             LIMIT 20",
        )
        .bind(min_count as i64)
        .fetch_all(&self.pool)
        .await
        .context("Failed to detect repeated patterns")?;

        Ok(rows
            .iter()
            .map(|row| {
                let domain: String = row.get("domain");
                let content: String = row.get("content");
                let count: i64 = row.get("cnt");
                (format!("[{}] {}", domain, content), count as usize)
            })
            .collect())
    }

    async fn episode_count(&self) -> Result<u64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM episodes")
            .fetch_one(&self.pool)
            .await
            .context("Failed to count episodes")?;
        Ok(count as u64)
    }

    /// B-6: First episode timestamp — birth moment for temporal grounding.
    async fn first_episode_timestamp(&self) -> Result<Option<i64>> {
        let ts: Option<i64> =
            sqlx::query_scalar("SELECT MIN(timestamp) FROM episodes WHERE strength > 0.05")
                .fetch_one(&self.pool)
                .await
                .context("Failed to get first episode timestamp")?;
        Ok(ts)
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

    async fn list_recent_contacts(&self, limit: usize) -> Result<Vec<PersonContext>> {
        // Find people ordered by most recent interaction
        let rows = sqlx::query(
            "SELECT DISTINCT p.id, p.name, \
             (SELECT COUNT(*) FROM relationships r WHERE r.source_id = p.id OR r.target_id = p.id) as cnt, \
             (SELECT MAX(r2.timestamp) FROM relationships r2 WHERE r2.source_id = p.id OR r2.target_id = p.id) as last_ts \
             FROM people p \
             INNER JOIN relationships rel ON rel.source_id = p.id OR rel.target_id = p.id \
             GROUP BY p.id ORDER BY last_ts DESC LIMIT ?",
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut contacts = Vec::new();
        for row in rows {
            let id_str: String = row.get("id");
            let id = Uuid::parse_str(&id_str).unwrap_or_default();
            let name: String = row.get("name");
            let cnt: i64 = row.get("cnt");
            let last_ts: Option<i64> = row.get("last_ts");

            let aliases: Vec<(String, String)> =
                sqlx::query_as("SELECT platform, platform_id FROM aliases WHERE person_id = ?")
                    .bind(&id_str)
                    .fetch_all(&self.pool)
                    .await?;

            contacts.push(PersonContext {
                person: Person {
                    id,
                    name,
                    aliases: aliases.into_iter().collect(),
                },
                interaction_count: cnt,
                last_interaction_ts: last_ts,
                relationship_notes: String::new(),
            });
        }
        Ok(contacts)
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

    /// B-18: Record a file created by Mneme's own tool use.
    pub async fn record_created_artifact(&self, path: &str) {
        let now = chrono::Utc::now().timestamp();
        // Read file content and compute normalized hash for later recognition
        let content_hash = tokio::fs::read_to_string(path)
            .await
            .ok()
            .map(|c| Self::content_fingerprint(&c));
        let _ = sqlx::query(
            "INSERT OR REPLACE INTO created_artifacts (path, created_at, last_used_at, content_hash) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(path)
        .bind(now)
        .bind(now)
        .bind(&content_hash)
        .execute(&self.pool)
        .await;
        tracing::info!(path, ?content_hash, "Recorded owned artifact");
    }

    /// B-18: Check if a file path is a self-created artifact.
    pub async fn is_owned_artifact(&self, path: &str) -> bool {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM created_artifacts WHERE path = ?",
        )
        .bind(path)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0)
            > 0
    }

    /// Normalize text and compute fingerprint for content recognition.
    fn content_fingerprint(content: &str) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let normalized: String = content.split_whitespace().collect();
        normalized.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// B-18: Check if text contains code matching an owned artifact's content.
    /// Extracts code-like line blocks from the message and checks each against stored hashes.
    pub async fn check_content_ownership(&self, text: &str) -> Option<String> {
        // Extract code blocks: lines with programming tokens, bridging empty-line gaps
        let code_tokens = ["import", "def ", "class ", "fn ", "let ", "const ", "var ",
                           "print", "return", "if ", "for ", "while ", "=", "(", "{",
                           "//", "#", "\"\"\"", "'''", "->"];
        let lines: Vec<&str> = text.lines().collect();

        // Track triple-quote state: lines inside """...""" or '''...''' are code
        let mut in_triple_quote = false;
        let mut is_code: Vec<bool> = Vec::with_capacity(lines.len());
        for l in &lines {
            let trimmed = l.trim();
            // Count triple-quote toggles on this line
            let dq_count = trimmed.matches("\"\"\"").count();
            let sq_count = trimmed.matches("'''").count();
            let toggles = dq_count + sq_count;

            let code = in_triple_quote
                || trimmed.is_empty()
                || code_tokens.iter().any(|t| l.contains(t))
                || l.starts_with(' ') || l.starts_with('\t');
            is_code.push(code);

            // Odd number of triple-quotes toggles the state
            if toggles % 2 == 1 {
                in_triple_quote = !in_triple_quote;
            }
        }

        // Find contiguous runs, then trim leading/trailing empty lines
        let mut i = 0;
        while i < lines.len() {
            if !is_code[i] { i += 1; continue; }
            let start = i;
            while i < lines.len() && is_code[i] { i += 1; }
            // Trim empty lines from edges
            let s = lines[start..i].iter().position(|l| !l.trim().is_empty());
            let e = lines[start..i].iter().rposition(|l| !l.trim().is_empty());
            if let (Some(s), Some(e)) = (s, e) {
                let block: String = lines[start + s..=start + e].join("\n");
                if block.lines().count() >= 3 {
                    let hash = Self::content_fingerprint(&block);
                    if let Ok(Some(path)) = sqlx::query_scalar::<_, String>(
                        "SELECT path FROM created_artifacts WHERE content_hash = ?",
                    ).bind(&hash).fetch_optional(&self.pool).await {
                        return Some(path);
                    }
                }
            }
        }
        None
    }
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
              period_start = excluded.period_start, period_end = excluded.period_end,
              emotional_tone = excluded.emotional_tone,
              themes_json = excluded.themes_json, people_json = excluded.people_json,
              turning_points_json = excluded.turning_points_json,
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
    ///
    /// B-5 Cognitive Sovereignty: when an external source tries to override a
    /// self-sourced entry, the new confidence is capped at `min(new, old * 0.8)`.
    /// This prevents user assertions from easily overwriting self-discovered knowledge.
    pub async fn store_self_knowledge(
        &self,
        domain: &str,
        content: &str,
        confidence: f32,
        source: &str,
        source_episode_id: Option<&str>,
    ) -> Result<i64> {
        let now = Utc::now().timestamp();

        // B-7: Sensitive period — early experiences have greater weight.
        // During the first SENSITIVE_PERIOD_EPISODES interactions, boost confidence
        // and make the merge formula favor new knowledge more strongly.
        const SENSITIVE_PERIOD_EPISODES: u64 = 50;
        let episode_count = self.episode_count().await.unwrap_or(u64::MAX);
        let in_sensitive_period = episode_count < SENSITIVE_PERIOD_EPISODES;

        // Confidence boost: linearly decays from 1.3× at episode 0 to 1.0× at threshold
        let sensitive_boost = if in_sensitive_period {
            let progress = episode_count as f64 / SENSITIVE_PERIOD_EPISODES as f64;
            1.3 - 0.3 * progress // 1.3 → 1.0
        } else {
            1.0
        };

        // Merge weights: during sensitive period, new knowledge has more impact
        // Normal: 0.3×old + 0.7×new → Sensitive: 0.15×old + 0.85×new
        let (old_weight, new_weight) = if in_sensitive_period {
            let progress = episode_count as f64 / SENSITIVE_PERIOD_EPISODES as f64;
            let old_w = 0.15 + 0.15 * progress; // 0.15 → 0.30
            (old_w, 1.0 - old_w)
        } else {
            (0.3, 0.7)
        };

        if in_sensitive_period {
            tracing::debug!(
                "B-7 sensitive period: episode {}/{}, boost={:.2}×, merge={:.2}/{:.2}",
                episode_count,
                SENSITIVE_PERIOD_EPISODES,
                sensitive_boost,
                old_weight,
                new_weight,
            );
        }

        let boosted_confidence = (confidence as f64 * sensitive_boost).min(1.0);

        // Check for existing entry with same domain + content.
        // Because content may be encrypted (ChaCha20 random nonce → same plaintext
        // produces different ciphertext), we fetch all entries for the domain and
        // decrypt in Rust for comparison.
        let candidates = sqlx::query(
            "SELECT id, content, confidence, source, is_private FROM self_knowledge WHERE domain = ?",
        )
        .bind(domain)
        .fetch_all(&self.pool)
        .await
        .context("Failed to check existing self_knowledge")?;

        let existing = candidates.iter().find(|row| {
            let is_priv = row.get::<i32, _>("is_private") != 0;
            let raw: String = row.get("content");
            let decrypted = if is_priv { self.decrypt_body(&raw) } else { raw };
            decrypted == content
        });

        if let Some(row) = existing {
            let id: i64 = row.get("id");
            let old_conf: f64 = row.get("confidence");
            let old_source: String = row.get("source");

            // B-5: Cognitive sovereignty — self-sourced knowledge resists external override.
            // If existing entry is self-sourced and new source is external,
            // cap the incoming confidence so it can't easily overwrite.
            let is_self_sourced = old_source.starts_with("self:");
            let is_external_new = !source.starts_with("self:");
            let effective_confidence = if is_self_sourced && is_external_new {
                let capped = boosted_confidence.min(old_conf * 0.8);
                tracing::debug!(
                    "B-5 sovereignty: capping external confidence {:.2} → {:.2} (self-sourced entry)",
                    boosted_confidence,
                    capped
                );
                capped
            } else {
                boosted_confidence
            };

            let merged = old_conf * old_weight + effective_confidence * new_weight;

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
            // B-12 Level 3: encrypt by default when encryptor is available
            let has_enc = self.encryptor.is_some();
            let stored_content = if has_enc { self.encrypt_body(content) } else { content.to_string() };
            let result = sqlx::query(
                "INSERT INTO self_knowledge (domain, content, confidence, source, \
                 source_episode_id, is_private, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(domain)
            .bind(&stored_content)
            .bind(boosted_confidence)
            .bind(source)
            .bind(source_episode_id)
            .bind(has_enc as i32)
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
    ///
    /// All entries are visible — opacity is emergent, not enforced (B-9).
    pub async fn recall_self_knowledge(&self, domain: &str) -> Result<Vec<SelfKnowledge>> {
        let rows = sqlx::query(
            "SELECT id, domain, content, confidence, source, source_episode_id, \
             is_private, created_at, updated_at \
             FROM self_knowledge WHERE domain = ? AND confidence > 0.1 \
             ORDER BY confidence DESC \
             LIMIT 50",
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
             ORDER BY domain, confidence DESC \
             LIMIT 100",
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

    /// §14.3: Get high-confidence belief content for belief-tension detection.
    pub async fn get_emotional_beliefs(&self) -> Vec<(String, f32)> {
        sqlx::query_as::<_, (String, f64)>(
            "SELECT content, confidence FROM self_knowledge \
             WHERE domain = 'belief' AND confidence > 0.5 \
             ORDER BY confidence DESC LIMIT 20",
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(c, conf)| (c, conf as f32))
        .collect()
    }

    /// Phase II Step 6: Expose embedding for belief tension cosine similarity.
    pub fn embed_text(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        self.embedding_model.embed(text)
    }

    /// B-9: Check whether any private self_knowledge entries exist.
    /// Used by Privacy-Somatic Coupling to detect if the organism has secrets worth protecting.
    pub async fn has_private_self_knowledge(&self) -> bool {
        sqlx::query_scalar::<_, i32>(
            "SELECT COUNT(*) FROM self_knowledge WHERE is_private = 1 AND confidence > 0.3",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0)
            > 0
    }

    /// B-12 Level 2: Mark a self_knowledge entry as private, encrypting its content at rest.
    pub async fn mark_self_knowledge_private(&self, id: i64, private: bool) -> Result<bool> {
        let row = sqlx::query(
            "SELECT content, is_private FROM self_knowledge WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        let row = match row {
            Some(r) => r,
            None => return Ok(false),
        };

        let old_private = row.get::<i32, _>("is_private") != 0;
        if old_private == private {
            return Ok(true); // already in desired state
        }

        let raw: String = row.get("content");
        let new_content = if private {
            // going private: decrypt first (in case already encrypted), then encrypt
            let plain = if old_private { self.decrypt_body(&raw) } else { raw };
            self.encrypt_body(&plain)
        } else {
            // going public: decrypt
            self.decrypt_body(&raw)
        };

        sqlx::query(
            "UPDATE self_knowledge SET content = ?, is_private = ?, updated_at = ? WHERE id = ?",
        )
        .bind(&new_content)
        .bind(private as i32)
        .bind(Utc::now().timestamp())
        .bind(id)
        .execute(&self.pool)
        .await
        .context("Failed to update self_knowledge privacy")?;

        Ok(true)
    }

    /// Helper: convert a sqlx Row to SelfKnowledge.
    fn row_to_self_knowledge(&self, row: &sqlx::sqlite::SqliteRow) -> SelfKnowledge {
        let is_private = row.get::<i32, _>("is_private") != 0;
        let raw_content: String = row.get("content");
        // B-12 Level 2: decrypt private entries at rest
        let content = if is_private { self.decrypt_body(&raw_content) } else { raw_content };
        SelfKnowledge {
            id: row.get("id"),
            domain: row.get("domain"),
            content,
            confidence: row.get::<f64, _>("confidence") as f32,
            source: row.get("source"),
            source_episode_id: row.get("source_episode_id"),
            is_private,
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }
    }

    /// Format self-knowledge for system prompt injection.
    ///
    /// Groups entries by domain. All entries are visible to the LLM —
    /// she decides what to share (B-9: opacity is emergent, not enforced).
    pub fn format_self_knowledge_for_prompt(entries: &[SelfKnowledge]) -> String {
        if entries.is_empty() {
            return String::new();
        }

        // Group by domain, filtering out undecryptable entries
        let mut by_domain: std::collections::BTreeMap<&str, Vec<&SelfKnowledge>> =
            std::collections::BTreeMap::new();
        for entry in entries {
            // Skip entries that couldn't be decrypted (key mismatch)
            if entry.content.starts_with("[encrypted:") {
                continue;
            }
            by_domain.entry(&entry.domain).or_default().push(entry);
        }

        let mut output = String::from("== 自我认知 ==\n");
        for (domain, items) in &by_domain {
            output.push_str(&format!("[{}]\n", domain));
            for item in items.iter().take(5) {
                output.push_str(&format!(
                    "- {} (确信度: {:.0}%)\n",
                    item.content,
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
        // Per-domain idempotency: only seed domains that don't already have seed entries.
        // This allows new persona .md files to be picked up on existing databases.
        let existing: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT domain FROM self_knowledge WHERE source = 'seed'",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to check seed entries")?;

        let mut seeded = 0;
        for (domain, content) in entries {
            if existing.contains(&domain.to_string()) {
                continue;
            }
            self.store_self_knowledge(domain, content, 0.9, "seed", None)
                .await?;
            seeded += 1;
        }
        if seeded > 0 {
            tracing::info!(
                "Seeded {} self-knowledge entries from persona files",
                seeded
            );
        }
        Ok(seeded)
    }

    /// Build a Psyche from the current self_knowledge table.
    ///
    /// Loads all entries with confidence > 0.1, formats them, and returns
    /// a Psyche with the formatted self-model.
    /// `language` controls meta-instruction language ("zh" or "en").
    pub async fn build_psyche(&self, language: &str) -> Result<mneme_core::Psyche> {
        let entries = self.get_all_self_knowledge(0.1).await?;
        let self_model = Self::format_self_knowledge_for_prompt(&entries);
        Ok(mneme_core::Psyche::with_language(language, self_model))
    }
    /// Get the timestamp of the most recent episode in the database.
    ///
    /// Used on startup to detect time gaps from restarts (#93).
    /// Returns None if no episodes exist yet.
    pub async fn last_episode_timestamp(&self) -> Result<Option<i64>> {
        let ts: Option<i64> =
            sqlx::query_scalar("SELECT MAX(timestamp) FROM episodes")
                .fetch_one(&self.pool)
                .await
                .context("Failed to query last episode timestamp")?;
        Ok(ts)
    }

    // =========================================================================
    // Recent messages (repetition detection persistence)
    // =========================================================================

    /// Load the most recent N messages for repetition detection.
    pub async fn load_recent_messages(&self, limit: i64) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT content FROM recent_messages ORDER BY id DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to load recent_messages")?;
        // Reverse so oldest is first (VecDeque order)
        Ok(rows.into_iter().rev().map(|r| r.0).collect())
    }

    /// Append a message and prune to keep at most `max_keep` entries.
    pub async fn save_recent_message(&self, content: &str, max_keep: i64) -> Result<()> {
        sqlx::query("INSERT INTO recent_messages (content) VALUES (?)")
            .bind(content)
            .execute(&self.pool)
            .await
            .context("Failed to insert recent_message")?;
        // Prune old entries
        sqlx::query(
            "DELETE FROM recent_messages WHERE id NOT IN \
             (SELECT id FROM recent_messages ORDER BY id DESC LIMIT ?)",
        )
        .bind(max_keep)
        .execute(&self.pool)
        .await
        .context("Failed to prune recent_messages")?;
        Ok(())
    }
}

// =============================================================================
// Memory Degradation (B-10: Memory is Reconstruction)
// =============================================================================

/// Degrade memory content based on strength — physical information loss.
///
/// Low-strength memories lose detail progressively. This is NOT a narrative
/// hint ("this memory is old") — it's actual truncation of the text the LLM
/// receives, forcing naturally vaguer responses. Respects Physical Isolation Law.
///
/// - strength > 0.4: full fidelity
/// - strength 0.2–0.4: truncate to ~60% of chars
/// - strength 0.1–0.2: truncate to ~30% of chars
/// - strength < 0.1: first sentence only
fn degrade_by_strength(body: &str, strength: f64) -> String {
    if strength > 0.4 || body.is_empty() {
        return body.to_string();
    }

    let chars: Vec<char> = body.chars().collect();
    let total = chars.len();

    if strength > 0.2 {
        // Moderate decay: keep ~60%
        let keep = (total as f64 * 0.6).ceil() as usize;
        let truncated: String = chars[..keep.min(total)].iter().collect();
        format!("{}……", truncated.trim_end())
    } else if strength > 0.1 {
        // Heavy decay: keep ~30%
        let keep = (total as f64 * 0.3).ceil() as usize;
        let truncated: String = chars[..keep.min(total)].iter().collect();
        format!("{}……（记忆模糊）", truncated.trim_end())
    } else {
        // Near-forgotten: first sentence only
        let first_sentence = body
            .split_once(['。', '！', '？', '.', '!', '?'])
            .map(|(s, _)| format!("{}。", s))
            .unwrap_or_else(|| {
                let keep = (total as f64 * 0.15).ceil() as usize;
                let t: String = chars[..keep.min(total)].iter().collect();
                format!("{}……", t.trim_end())
            });
        format!("{}（只剩模糊印象）", first_sentence)
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
            sqlx::query("UPDATE episodes SET strength = strength * ? WHERE strength > 0.05 AND pinned = 0")
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

    // === #84: Autonomous Memory Management ===

    /// Pin an episode to prevent decay. Optionally boost strength.
    pub async fn pin_episode(&self, episode_id: &str) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE episodes SET pinned = 1, strength = MAX(strength, 0.9) WHERE id = ?",
        )
        .bind(episode_id)
        .execute(&self.pool)
        .await
        .context("Failed to pin episode")?;
        Ok(result.rows_affected() > 0)
    }

    /// Unpin an episode, restoring normal decay behavior.
    pub async fn unpin_episode(&self, episode_id: &str) -> Result<bool> {
        let result = sqlx::query("UPDATE episodes SET pinned = 0 WHERE id = ?")
            .bind(episode_id)
            .execute(&self.pool)
            .await
            .context("Failed to unpin episode")?;
        Ok(result.rows_affected() > 0)
    }

    /// Actively forget an episode by setting strength to 0.
    pub async fn forget_episode(&self, episode_id: &str) -> Result<bool> {
        let result =
            sqlx::query("UPDATE episodes SET strength = 0.0, pinned = 0 WHERE id = ?")
                .bind(episode_id)
                .execute(&self.pool)
                .await
                .context("Failed to forget episode")?;
        Ok(result.rows_affected() > 0)
    }

    /// List pinned (important) episodes.
    pub async fn list_pinned_episodes(&self, limit: u32) -> Result<Vec<(String, String, f64)>> {
        let rows = sqlx::query(
            "SELECT id, substr(body, 1, 120) as summary, strength \
             FROM episodes WHERE pinned = 1 ORDER BY timestamp DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list pinned episodes")?;

        Ok(rows
            .iter()
            .map(|r| {
                (
                    r.get::<String, _>("id"),
                    r.get::<String, _>("summary"),
                    r.get::<f64, _>("strength"),
                )
            })
            .collect())
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
                body: self.decrypt_body(&row.get::<String, _>("body")),
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

    // === #981: Batch Training Data Export ===

    /// Export conversation episodes as JSONL suitable for LLM fine-tuning.
    ///
    /// Groups consecutive user→Mneme message pairs into conversation turns.
    /// Each line is `{"messages": [{"role":"user","content":"..."},{"role":"assistant","content":"..."}]}`.
    pub async fn export_training_jsonl(&self, writer: &mut dyn std::io::Write) -> Result<u64> {
        let rows = sqlx::query(
            "SELECT author, body FROM episodes \
             WHERE strength > 0.1 AND source != 'self:restart' \
             ORDER BY timestamp ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch episodes for export")?;

        let mut count = 0u64;
        let mut pending_user: Option<String> = None;

        for row in &rows {
            let author: String = row.get("author");
            let body: String = self.decrypt_body(&row.get::<String, _>("body"));

            if author == "Mneme" {
                if let Some(user_msg) = pending_user.take() {
                    let line = serde_json::json!({
                        "messages": [
                            {"role": "user", "content": user_msg},
                            {"role": "assistant", "content": body}
                        ]
                    });
                    writeln!(writer, "{}", line).context("Failed to write JSONL line")?;
                    count += 1;
                }
            } else {
                // New user message — overwrite any unpaired previous one
                pending_user = Some(body);
            }
        }

        Ok(count)
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
            "INSERT INTO modulation_samples (energy, stress, arousal, mood_bias, social_need, boredom, modulation_json, feedback_valence, timestamp, consumed) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0)"
        )
        .bind(sample.energy as f64)
        .bind(sample.stress as f64)
        .bind(sample.arousal as f64)
        .bind(sample.mood_bias as f64)
        .bind(sample.social_need as f64)
        .bind(sample.boredom as f64)
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
            "SELECT id, energy, stress, arousal, mood_bias, social_need, boredom, modulation_json, feedback_valence, timestamp FROM modulation_samples WHERE consumed = 0 ORDER BY timestamp ASC"
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
                boredom: row.get::<f64, _>("boredom") as f32,
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

    /// Phase II Step 3: Sample a random mini-batch from experience buffer for LTC training.
    pub async fn sample_experience_batch(&self, batch_size: usize) -> Result<Vec<crate::learning::ModulationSample>> {
        use mneme_limbic::ModulationVector;

        let rows = sqlx::query(
            "SELECT id, energy, stress, arousal, mood_bias, social_need, boredom, modulation_json, feedback_valence, timestamp FROM modulation_samples ORDER BY RANDOM() LIMIT ?"
        )
        .bind(batch_size as i64)
        .fetch_all(&self.pool)
        .await
        .context("Failed to sample experience batch")?;

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
                boredom: row.get::<f64, _>("boredom") as f32,
                modulation,
                feedback_valence: row.get::<f64, _>("feedback_valence") as f32,
                timestamp: row.get("timestamp"),
            });
        }
        Ok(samples)
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

    /// Save learned NeuralModulator weights (upsert — always id=1).
    pub async fn save_learned_neural(&self, nn: &mneme_limbic::NeuralModulator) -> Result<()> {
        let json = serde_json::to_string(nn).context("Failed to serialize neural modulator")?;
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "INSERT INTO learned_neural (id, neural_json, updated_at) VALUES (1, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET neural_json = excluded.neural_json, updated_at = excluded.updated_at"
        )
        .bind(&json)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to save learned neural")?;
        Ok(())
    }

    /// Load previously learned NeuralModulator from DB.
    pub async fn load_learned_neural(&self) -> Result<Option<mneme_limbic::NeuralModulator>> {
        let row = sqlx::query("SELECT neural_json FROM learned_neural WHERE id = 1")
            .fetch_optional(&self.pool)
            .await
            .context("Failed to load learned neural")?;
        match row {
            Some(r) => {
                let json_str: String = r.get("neural_json");
                let nn = serde_json::from_str(&json_str)
                    .context("Failed to deserialize learned neural")?;
                Ok(Some(nn))
            }
            None => Ok(None),
        }
    }

    /// Save learned dynamics parameters (upsert — always id=1).
    pub async fn save_learned_dynamics(&self, ld: &mneme_core::LearnableDynamics) -> Result<()> {
        let json = serde_json::to_string(ld).context("Failed to serialize learnable dynamics")?;
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "INSERT INTO learned_dynamics (id, dynamics_json, updated_at) VALUES (1, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET dynamics_json = excluded.dynamics_json, updated_at = excluded.updated_at"
        )
        .bind(&json)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to save learned dynamics")?;
        Ok(())
    }

    /// Load previously learned dynamics parameters from DB.
    pub async fn load_learned_dynamics(&self) -> Result<Option<mneme_core::LearnableDynamics>> {
        let row = sqlx::query("SELECT dynamics_json FROM learned_dynamics WHERE id = 1")
            .fetch_optional(&self.pool)
            .await
            .context("Failed to load learned dynamics")?;
        match row {
            Some(r) => {
                let json_str: String = r.get("dynamics_json");
                let ld = serde_json::from_str(&json_str)
                    .context("Failed to deserialize learned dynamics")?;
                Ok(Some(ld))
            }
            None => Ok(None),
        }
    }

    /// Save LTC weights (upsert — always id=1).
    pub async fn save_learned_ltc(&self, ltc: &mneme_limbic::LiquidNeuralModulator) -> Result<()> {
        let json = serde_json::to_string(ltc).context("Failed to serialize LTC")?;
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "INSERT INTO learned_ltc (id, ltc_json, updated_at) VALUES (1, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET ltc_json = excluded.ltc_json, updated_at = excluded.updated_at"
        )
        .bind(&json)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to save learned LTC")?;
        Ok(())
    }

    /// Load previously learned LTC weights from DB.
    pub async fn load_learned_ltc(&self) -> Result<Option<mneme_limbic::LiquidNeuralModulator>> {
        let row = sqlx::query("SELECT ltc_json FROM learned_ltc WHERE id = 1")
            .fetch_optional(&self.pool)
            .await
            .context("Failed to load learned LTC")?;
        match row {
            Some(r) => {
                let json_str: String = r.get("ltc_json");
                let ltc = serde_json::from_str(&json_str)
                    .context("Failed to deserialize learned LTC")?;
                Ok(Some(ltc))
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
