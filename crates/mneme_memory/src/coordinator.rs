//! Organism Coordinator - Integrates all subsystems into a living whole
//!
//! This is the central nervous system that coordinates:
//! - System 1 (Limbic): Fast, intuitive reactions
//! - System 2 (Reasoning): Slow, deliberate thinking
//! - Memory: Episodic and semantic storage
//! - State: Multi-scale personality dynamics
//! - Values: Moral reasoning and judgment
//!
//! The coordinator manages the full life cycle including:
//! - Waking state: Active processing
//! - Sleep state: Consolidation and learning

use anyhow::Result;
use chrono::Timelike;
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::{watch, RwLock};

use crate::goals::GoalManager;
use crate::rules::{RuleAction, RuleContext, RuleEngine, RuleTrigger};
use crate::{
    ConsolidationResult, EpisodeDigest, FeedbackBuffer, SignalType, SleepConfig, SleepConsolidator,
    SqliteMemory,
};
use mneme_core::Memory;
use mneme_core::{
    DefaultDynamics, OrganismState, RuleBasedJudge, SensoryInput, Situation, ValueJudge,
};
use mneme_limbic::{LimbicSystem, SomaticMarker, Stimulus};

/// System lifecycle states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    /// Normal operation - responding to stimuli
    Awake,
    /// Consolidation in progress
    Sleeping,
    /// Idle but ready
    Drowsy,
    /// Shutdown requested
    ShuttingDown,
}

/// Configuration for the organism coordinator
#[derive(Debug, Clone)]
pub struct OrganismConfig {
    /// Sleep schedule
    pub sleep_config: SleepConfig,

    /// State update interval (milliseconds)
    pub state_update_interval_ms: u64,

    /// Whether to auto-sleep during configured hours
    pub auto_sleep: bool,

    /// Minimum interactions before considering sleep
    pub min_interactions_before_sleep: u32,
}

impl Default for OrganismConfig {
    fn default() -> Self {
        Self {
            sleep_config: SleepConfig::default(),
            state_update_interval_ms: 1000,
            auto_sleep: true,
            min_interactions_before_sleep: 10,
        }
    }
}

/// The central coordinator for all organism subsystems
///
/// Lock ordering (to prevent deadlocks):
///   state_mutation_lock → state → prev_somatic → episode_buffer → feedback_buffer
pub struct OrganismCoordinator {
    /// Multi-scale personality state
    state: Arc<RwLock<OrganismState>>,

    /// State dynamics (ODE-based updates)
    dynamics: DefaultDynamics,

    /// System 1: Fast intuitive processing
    limbic: Arc<LimbicSystem>,

    /// Feedback buffer for buffered learning
    pub(crate) feedback_buffer: Arc<RwLock<FeedbackBuffer>>,

    /// Sleep consolidator
    consolidator: Arc<SleepConsolidator>,

    /// Value judgment system
    value_judge: Arc<dyn ValueJudge>,

    /// Current lifecycle state
    lifecycle_state: Arc<RwLock<LifecycleState>>,

    /// Lifecycle state broadcaster
    lifecycle_tx: watch::Sender<LifecycleState>,

    /// Configuration
    config: OrganismConfig,

    /// Interaction counter (for sleep timing)
    interaction_count: Arc<RwLock<u32>>,

    /// Episode buffer for narrative weaving
    pub(crate) episode_buffer: Arc<RwLock<Vec<EpisodeDigest>>>,

    /// Optional database for persistence
    db: Option<Arc<SqliteMemory>>,

    /// Previous state snapshot for computing diffs in state history
    prev_snapshot: Arc<RwLock<Option<OrganismState>>>,

    /// Previous somatic marker for body feeling detection (#40)
    prev_somatic: Arc<RwLock<Option<SomaticMarker>>>,

    /// Behavior rule engine (ADR-004, v0.6.0)
    rule_engine: Option<Arc<RwLock<RuleEngine>>>,

    /// Goal manager (#22, v0.6.0)
    goal_manager: Option<Arc<GoalManager>>,

    /// Mutex to serialize state-mutating operations (process_interaction, trigger_sleep).
    /// Prevents concurrent mutations from interleaving reads and writes.
    state_mutation_lock: tokio::sync::Mutex<()>,
}

impl OrganismCoordinator {
    /// Create a new organism coordinator (no persistence)
    pub fn new(limbic: Arc<LimbicSystem>) -> Self {
        Self::with_config(limbic, OrganismConfig::default(), None)
    }

    /// Create with custom configuration and optional database
    pub fn with_config(
        limbic: Arc<LimbicSystem>,
        config: OrganismConfig,
        db: Option<Arc<SqliteMemory>>,
    ) -> Self {
        let feedback_buffer = Arc::new(RwLock::new(FeedbackBuffer::new()));
        let consolidator = Arc::new(SleepConsolidator::with_config(
            feedback_buffer.clone(),
            config.sleep_config.clone(),
        ));

        let (lifecycle_tx, _) = watch::channel(LifecycleState::Awake);

        Self {
            state: Arc::new(RwLock::new(OrganismState::default())),
            dynamics: DefaultDynamics::default(),
            limbic,
            feedback_buffer,
            consolidator,
            value_judge: Arc::new(RuleBasedJudge::new()),
            lifecycle_state: Arc::new(RwLock::new(LifecycleState::Awake)),
            lifecycle_tx,
            config,
            interaction_count: Arc::new(RwLock::new(0)),
            episode_buffer: Arc::new(RwLock::new(Vec::new())),
            db,
            prev_snapshot: Arc::new(RwLock::new(None)),
            prev_somatic: Arc::new(RwLock::new(None)),
            rule_engine: None,
            goal_manager: None,
            state_mutation_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// Create with database for persistence
    pub async fn with_persistence(
        limbic: Arc<LimbicSystem>,
        config: OrganismConfig,
        db: Arc<SqliteMemory>,
    ) -> Result<Self> {
        let coordinator = Self::with_config(limbic, config, Some(db.clone()));

        // Load persisted state
        if let Some(state) = db.load_organism_state().await? {
            tracing::info!("Loaded persisted organism state");
            *coordinator.state.write().await = state;
        } else {
            tracing::info!("No persisted state found, using defaults");
        }

        // Load pending feedback signals
        let signals = db.load_pending_feedback().await?;
        if !signals.is_empty() {
            tracing::info!("Loaded {} pending feedback signals", signals.len());
            let mut buffer = coordinator.feedback_buffer.write().await;
            for signal in signals {
                buffer.add_signal(
                    signal.signal_type,
                    signal.content,
                    signal.confidence,
                    signal.emotional_context,
                );
            }
        }

        // Load learned ModulationCurves from DB
        if let Ok(Some(curves)) = db.load_learned_curves().await {
            coordinator.limbic.set_curves(curves).await;
            tracing::info!("Loaded learned ModulationCurves from DB");
        }

        // Load behavior rule engine (ADR-004)
        let seed = crate::rules::seed_rules();
        let _ = db.seed_behavior_rules(&seed).await;
        let rule_engine = RuleEngine::load(db.clone()).await?;
        let coordinator = Self {
            rule_engine: Some(Arc::new(RwLock::new(rule_engine))),
            goal_manager: Some(Arc::new(GoalManager::new(db.clone()))),
            ..coordinator
        };

        Ok(coordinator)
    }

    /// Get a watch receiver for lifecycle state changes
    pub fn subscribe_lifecycle(&self) -> watch::Receiver<LifecycleState> {
        self.lifecycle_tx.subscribe()
    }

    /// Get current lifecycle state
    pub async fn lifecycle_state(&self) -> LifecycleState {
        *self.lifecycle_state.read().await
    }

    /// Get shared state reference
    pub fn state(&self) -> Arc<RwLock<OrganismState>> {
        self.state.clone()
    }

    /// Get limbic system reference
    pub fn limbic(&self) -> &Arc<LimbicSystem> {
        &self.limbic
    }

    /// Get rule engine reference (v0.6.0)
    pub fn rule_engine(&self) -> Option<&Arc<RwLock<RuleEngine>>> {
        self.rule_engine.as_ref()
    }

    /// Get goal manager reference (v0.6.0)
    pub fn goal_manager(&self) -> Option<&Arc<GoalManager>> {
        self.goal_manager.as_ref()
    }

    /// Get interaction count
    pub async fn interaction_count(&self) -> u32 {
        *self.interaction_count.read().await
    }

    /// Expose interaction count for MetacognitionEvaluator.
    pub fn interaction_count_ref(&self) -> Arc<RwLock<u32>> {
        self.interaction_count.clone()
    }

    /// Store a metacognition insight as self-knowledge.
    pub async fn store_metacognition_insight(
        &self,
        domain: &str,
        content: &str,
        confidence: f32,
    ) {
        if let Some(ref db) = self.db {
            if let Err(e) = db
                .store_self_knowledge(
                    domain,
                    content,
                    confidence,
                    "self:metacognition",
                    None,
                )
                .await
            {
                tracing::warn!("Failed to store metacognition insight: {}", e);
            }
        }
    }

    /// Process an incoming interaction
    ///
    /// This is the main entry point for handling user messages.
    /// It coordinates System 1 and System 2 processing.
    pub async fn process_interaction(
        &self,
        author: &str,
        content: &str,
        response_delay_secs: f32,
    ) -> Result<InteractionResult> {
        let _guard = self.state_mutation_lock.lock().await;

        // Check if we should be processing
        let lifecycle = *self.lifecycle_state.read().await;
        if lifecycle == LifecycleState::Sleeping {
            return Ok(InteractionResult::sleeping());
        }

        // 1. Send stimulus to System 1 (limbic)
        let stimulus = self.create_stimulus(author, content);
        let _ = self.limbic.receive_stimulus(stimulus).await;

        // 2. Get somatic marker
        let soma = self.limbic.get_somatic_marker().await;

        // 3. Update fast state based on stimulus
        let sensory = self.create_sensory_input(content, &soma, response_delay_secs);
        {
            let mut state = self.state.write().await;
            let medium_clone = state.medium.clone();
            self.dynamics
                .step_fast(&mut state.fast, &medium_clone, &sensory, 1.0);
            state.last_updated = Utc::now().timestamp();
        }

        // 4. Record episode for narrative
        {
            let mut episodes = self.episode_buffer.write().await;
            episodes.push(EpisodeDigest {
                timestamp: Utc::now(),
                author: author.to_string(),
                content: content.chars().take(500).collect(),
                emotional_valence: soma.affect.valence,
            });

            // Keep buffer bounded (tighter cap to limit memory if sleep never fires)
            if episodes.len() > 200 {
                episodes.drain(0..100);
            }
        }

        // 5. Increment interaction counter
        {
            let mut count = self.interaction_count.write().await;
            *count += 1;
        }

        // 6. Record state history snapshot (after state update)
        self.save_state_with_trigger("interaction").await;

        // 7. Body feeling detection (#40): compare somatic markers across interactions
        {
            let prev = self.prev_somatic.read().await.clone();
            if let Some(ref prev_marker) = prev {
                let feelings = soma.describe_body_feeling(prev_marker, 0.15);
                if !feelings.is_empty() {
                    if let Some(ref db) = self.db {
                        for (text, intensity) in &feelings {
                            tracing::debug!("Body feeling: {} (intensity={:.2})", text, intensity);
                            if let Err(e) = db
                                .store_self_knowledge(
                                    "body_feeling",
                                    text,
                                    *intensity,
                                    "somatic",
                                    None,
                                )
                                .await
                            {
                                tracing::warn!("Failed to store body feeling: {}", e);
                            }
                        }
                    }
                }
            }
            // Update prev_somatic for next comparison
            *self.prev_somatic.write().await = Some(soma.clone());
        }

        // 8. Check if we should transition to drowsy/sleep
        self.check_lifecycle_transition().await;

        Ok(InteractionResult {
            somatic_marker: soma,
            state_snapshot: self.state.read().await.clone(),
            lifecycle: *self.lifecycle_state.read().await,
        })
    }

    /// Evaluate a proposed action against values
    pub async fn evaluate_action(
        &self,
        description: &str,
        proposed_action: &str,
    ) -> Result<ActionEvaluation> {
        let state = self.state.read().await;

        let situation = Situation {
            description: description.to_string(),
            proposed_action: proposed_action.to_string(),
            features: vec![],
            actors: vec![],
            emotional_valence: state.fast.affect.valence,
        };

        let judgment = self.value_judge.evaluate(&situation, &state.slow.values);

        // Apply moral cost to state if there are violations
        if !judgment.violated_values.is_empty() {
            let violated: Vec<&str> = judgment
                .violated_values
                .iter()
                .map(|v| v.value_name.as_str())
                .collect();
            let moral_cost = state.slow.values.compute_moral_cost(&violated);

            drop(state);
            let mut state = self.state.write().await;
            self.dynamics.apply_moral_cost(&mut state.fast, moral_cost);
        }

        Ok(ActionEvaluation {
            moral_valence: judgment.moral_valence,
            has_conflict: judgment.has_conflict,
            explanation: judgment.explanation,
            should_proceed: judgment.moral_valence > -0.3, // Allow if not strongly negative
        })
    }

    /// Evaluate behavior rules against current context (ADR-004, v0.6.0).
    /// Returns matched rule actions for the caller to process.
    pub async fn evaluate_rules(&self, trigger: RuleTrigger) -> Vec<(i64, RuleAction)> {
        let engine = match &self.rule_engine {
            Some(e) => e,
            None => return Vec::new(),
        };

        let state = self.state.read().await.clone();
        let lifecycle = *self.lifecycle_state.read().await;
        let interaction_count = *self.interaction_count.read().await;
        let now = chrono::Utc::now();

        let ctx = RuleContext {
            trigger_type: trigger,
            state,
            current_hour: now.hour(),
            interaction_count,
            lifecycle,
            now: now.timestamp(),
            message_text: None,
        };

        let mut engine = engine.write().await;
        engine.evaluate(&ctx).await
    }

    /// Record feedback from System 2 (LLM output)
    ///
    /// This buffers interpretations for later consolidation.
    /// If persistence is enabled, also writes to SQLite for crash recovery.
    pub async fn record_feedback(
        &self,
        signal_type: SignalType,
        content: String,
        confidence: f32,
        emotional_context: f32,
    ) {
        // Clone for DB persistence before buffer consumes them
        let signal_type_clone = signal_type.clone();
        let content_clone = content.clone();

        let mut buffer = self.feedback_buffer.write().await;
        buffer.add_signal(signal_type, content, confidence, emotional_context);

        // Persist to DB for crash recovery
        if let Some(ref db) = self.db {
            let signal = crate::FeedbackSignal {
                id: 0,
                timestamp: chrono::Utc::now(),
                signal_type: signal_type_clone,
                content: content_clone,
                confidence,
                emotional_context,
                consolidated: false,
            };
            if let Err(e) = db.save_feedback_signal(&signal).await {
                tracing::warn!("Failed to persist feedback signal: {}", e);
            }
        }
    }

    /// Record a modulation sample for offline curve learning.
    ///
    /// Called after each interaction with the modulation vector that was used
    /// and the feedback valence from the user's response.
    pub async fn record_modulation_sample(
        &self,
        modulation: &mneme_limbic::ModulationVector,
        feedback_valence: f32,
    ) {
        if let Some(ref db) = self.db {
            let state = self.state.read().await;
            let sample = crate::learning::ModulationSample {
                id: 0,
                energy: state.fast.energy,
                stress: state.fast.stress,
                arousal: state.fast.affect.arousal,
                mood_bias: state.medium.mood_bias,
                social_need: state.fast.social_need,
                modulation: modulation.clone(),
                feedback_valence,
                timestamp: chrono::Utc::now().timestamp(),
            };
            if let Err(e) = db.save_modulation_sample(&sample).await {
                tracing::warn!("Failed to save modulation sample: {}", e);
            }
        }
    }

    /// Trigger sleep consolidation manually
    pub async fn trigger_sleep(&self) -> Result<ConsolidationResult> {
        let _guard = self.state_mutation_lock.lock().await;

        // Transition to sleeping state
        self.set_lifecycle_state(LifecycleState::Sleeping).await;

        // Get episodes for narrative weaving
        let episodes = self.episode_buffer.read().await.clone();
        let current_state = self.state.read().await.clone();

        // Run consolidation
        let mut result = self
            .consolidator
            .consolidate(&episodes, &current_state)
            .await?;

        // Apply state updates if consolidation was performed
        if result.performed && !result.state_updates.is_empty() {
            let mut state = self.state.write().await;
            SleepConsolidator::apply_updates(&mut state, &result.state_updates);
            tracing::info!("Applied state updates from sleep consolidation");
        }

        // Handle crisis if detected
        if let Some(ref crisis) = result.crisis {
            let mut state = self.state.write().await;
            let collapsed = SleepConsolidator::handle_crisis(&mut state, crisis, &self.dynamics);
            if collapsed {
                tracing::warn!("Narrative collapse occurred during sleep");
            }
        }

        // Save narrative chapter if created
        if let Some(ref chapter) = result.new_chapter {
            if let Some(ref db) = self.db {
                if let Err(e) = db.save_narrative_chapter(chapter).await {
                    tracing::error!("Failed to save narrative chapter: {}", e);
                }
            }
        }

        // Store self-reflection results in self_knowledge table
        if !result.self_reflections.is_empty() {
            if let Some(ref db) = self.db {
                for candidate in &result.self_reflections {
                    if let Err(e) = db
                        .store_self_knowledge(
                            &candidate.domain,
                            &candidate.content,
                            candidate.confidence,
                            "consolidation",
                            None,
                        )
                        .await
                    {
                        tracing::warn!("Failed to store self-reflection: {}", e);
                    }
                }
                tracing::info!(
                    "Stored {} self-reflection entries",
                    result.self_reflections.len()
                );

                // Store the reflection summary as a meta-episode
                let summary =
                    crate::SelfReflector::format_reflection_summary(&result.self_reflections);
                let meta_content = mneme_core::Content {
                    id: uuid::Uuid::new_v4(),
                    source: "self:reflection".to_string(),
                    author: "Mneme".to_string(),
                    body: summary,
                    timestamp: chrono::Utc::now().timestamp(),
                    modality: mneme_core::Modality::Text,
                };
                if let Err(e) = db.memorize(&meta_content).await {
                    tracing::warn!("Failed to store reflection meta-episode: {}", e);
                }
            }
        }

        // Dream generation (ADR-008): recall random memories and weave a dream
        if let Some(ref db) = self.db {
            match db.recall_random_by_strength(3).await {
                Ok(seeds) if seeds.len() >= 2 => {
                    let current = self.state.read().await.clone();
                    if let Some(dream) = crate::dream::DreamGenerator::generate(&seeds, &current) {
                        tracing::info!(
                            "Dream generated from {} seeds, tone={:.2}",
                            dream.source_ids.len(),
                            dream.emotional_tone,
                        );
                        let dream_content = mneme_core::Content {
                            id: uuid::Uuid::new_v4(),
                            source: "self:dream".to_string(),
                            author: "Mneme".to_string(),
                            body: dream.narrative.clone(),
                            timestamp: chrono::Utc::now().timestamp(),
                            modality: mneme_core::Modality::Text,
                        };
                        if let Err(e) = db.memorize(&dream_content).await {
                            tracing::warn!("Failed to store dream episode: {}", e);
                        } else {
                            // Set dream strength to 0.4 (weaker than real memories)
                            let _ = db
                                .update_episode_strength(&dream_content.id.to_string(), 0.4)
                                .await;
                        }
                        result.dream = Some(dream);
                    }
                }
                Ok(_) => {
                    tracing::debug!("Not enough memory seeds for dream generation");
                }
                Err(e) => {
                    tracing::warn!("Failed to recall seeds for dreaming: {}", e);
                }
            }
        }

        // Mark persisted feedback signals as consolidated
        if let Some(ref db) = self.db {
            if let Ok(pending) = db.load_pending_feedback().await {
                let ids: Vec<i64> = pending.iter().map(|s| s.id).collect();
                if !ids.is_empty() {
                    if let Err(e) = db.mark_feedback_consolidated(&ids).await {
                        tracing::warn!("Failed to mark feedback consolidated: {}", e);
                    }
                }
            }
        }

        // Episode strength decay (Ebbinghaus forgetting curve)
        if let Some(ref db) = self.db {
            let decay_factor = 0.95; // Each sleep cycle decays 5%
            if let Err(e) = db.decay_episode_strengths(decay_factor).await {
                tracing::warn!("Failed to decay episode strengths: {}", e);
            } else {
                tracing::info!("Applied episode strength decay (factor={})", decay_factor);
            }
        }

        // Offline learning: adjust ModulationCurves from collected samples
        if let Some(ref db) = self.db {
            let samples = db.load_unconsumed_samples().await.unwrap_or_default();
            if !samples.is_empty() {
                let learner = crate::learning::CurveLearner::new();
                let current_curves = self.limbic.get_curves().await;
                if let Some(new_curves) = learner.learn(&current_curves, &samples) {
                    self.limbic.set_curves(new_curves.clone()).await;
                    let _ = db.save_learned_curves(&new_curves).await;
                    tracing::info!("Adjusted ModulationCurves from {} samples", samples.len());
                }
                let ids: Vec<i64> = samples.iter().map(|s| s.id).collect();
                let _ = db.mark_samples_consumed(&ids).await;
            }
        }

        // Clear processed episodes (keep recent ones)
        {
            let mut episodes = self.episode_buffer.write().await;
            let keep_count = episodes.len().saturating_sub(100);
            if keep_count > 0 {
                episodes.drain(0..keep_count);
            }
        }

        // Save state after consolidation
        self.save_state_with_trigger("consolidation").await;

        // Generate goal suggestions from post-sleep state (#22)
        if let Some(ref gm) = self.goal_manager {
            let state = self.state.read().await.clone();
            let suggestions = GoalManager::suggest_goals(&state);
            for goal in &suggestions {
                if let Err(e) = gm.create_goal(goal).await {
                    tracing::warn!("Failed to create suggested goal: {}", e);
                }
            }
            if !suggestions.is_empty() {
                tracing::info!(
                    "Generated {} goal suggestions during sleep",
                    suggestions.len()
                );
            }
        }

        // Transition back to awake
        self.set_lifecycle_state(LifecycleState::Awake).await;

        Ok(result)
    }

    /// Save current state to database (if persistence is enabled).
    /// Also records a snapshot into the state history table.
    pub async fn save_state(&self) {
        self.save_state_with_trigger("tick").await;
    }

    /// Save state with a specific trigger label for the history record.
    pub async fn save_state_with_trigger(&self, trigger: &str) {
        if let Some(ref db) = self.db {
            let state = self.state.read().await.clone();

            // Save the singleton current state
            if let Err(e) = db.save_organism_state(&state).await {
                tracing::error!("Failed to save organism state: {}", e);
                return;
            }

            // Record history snapshot with diff
            let prev = self.prev_snapshot.read().await.clone();
            if let Err(e) = db
                .record_state_snapshot(&state, trigger, prev.as_ref())
                .await
            {
                tracing::error!("Failed to record state history: {}", e);
            }

            // Update prev_snapshot for next diff
            *self.prev_snapshot.write().await = Some(state);
        }
    }

    /// Run periodic maintenance (call this from a timer)
    pub async fn tick(&self) -> Result<()> {
        static TICK_COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let tick = TICK_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let lifecycle = *self.lifecycle_state.read().await;

        match lifecycle {
            LifecycleState::Awake => {
                // Update medium state periodically
                let mut state = self.state.write().await;
                let input = SensoryInput::default();
                let fast_clone = state.fast.clone();
                let slow_clone = state.slow.clone();
                self.dynamics.step_medium(
                    &mut state.medium,
                    &fast_clone,
                    &slow_clone,
                    &input,
                    60.0,
                );
                drop(state);

                // Save state every 6 ticks (60 seconds if tick interval is 10s)
                if tick.is_multiple_of(6) {
                    self.save_state().await;
                }

                // Prune state history once every 360 ticks (~1 hour if tick = 10s)
                // Keep max 10,000 snapshots and discard anything older than 7 days
                if tick.is_multiple_of(360) {
                    if let Some(ref db) = self.db {
                        let _ = db.prune_state_history(10_000, 7 * 86400).await;
                    }
                }
            }
            LifecycleState::Drowsy => {
                // Check if we should sleep
                if self.should_sleep().await {
                    drop(self.trigger_sleep().await);
                }
            }
            LifecycleState::Sleeping => {
                // Consolidation in progress, do nothing
            }
            LifecycleState::ShuttingDown => {
                // Cleanup if needed
            }
        }

        Ok(())
    }

    /// Request graceful shutdown
    pub async fn shutdown(&self) {
        self.set_lifecycle_state(LifecycleState::ShuttingDown).await;

        // Perform final consolidation if we have pending data
        let pending = self.feedback_buffer.read().await.pending_count();
        if pending > 0 {
            tracing::info!(
                "Performing final consolidation before shutdown ({} pending signals)",
                pending
            );
            let _ = self.trigger_sleep().await;
        }

        // Final state save
        self.save_state_with_trigger("shutdown").await;
        tracing::info!("Organism state saved before shutdown");
    }

    // === Private helpers ===

    fn create_stimulus(&self, _author: &str, content: &str) -> Stimulus {
        // Simple sentiment analysis for stimulus creation
        let (valence, intensity) = Self::analyze_sentiment(content);
        Stimulus {
            valence,
            intensity,
            is_social: true,
            content: content.to_string(),
            violated_values: vec![],
        }
    }

    fn analyze_sentiment(text: &str) -> (f32, f32) {
        mneme_core::sentiment::analyze_sentiment(text)
    }

    fn create_sensory_input(
        &self,
        content: &str,
        soma: &SomaticMarker,
        response_delay: f32,
    ) -> SensoryInput {
        // ADR-007: Extract topic hint for curiosity vectorization
        let topic_hint = extract_topic_hint(content);
        SensoryInput {
            content_valence: soma.affect.valence,
            content_intensity: soma.affect.arousal,
            surprise: 0.1, // Default low surprise
            is_social: true,
            response_delay_factor: response_delay,
            violated_values: vec![],
            topic_hint,
        }
    }

    async fn check_lifecycle_transition(&self) {
        let hour = Utc::now().hour();
        let interaction_count = *self.interaction_count.read().await;

        // Auto-transition to drowsy if conditions met
        if self.config.auto_sleep
            && hour >= self.config.sleep_config.sleep_start_hour
            && hour < self.config.sleep_config.sleep_end_hour
            && interaction_count >= self.config.min_interactions_before_sleep
        {
            let current = *self.lifecycle_state.read().await;
            if current == LifecycleState::Awake {
                self.set_lifecycle_state(LifecycleState::Drowsy).await;
            }
        }
    }

    async fn should_sleep(&self) -> bool {
        self.consolidator.is_sleep_time() && self.consolidator.is_consolidation_due().await
    }

    async fn set_lifecycle_state(&self, new_state: LifecycleState) {
        let mut state = self.lifecycle_state.write().await;
        if *state != new_state {
            tracing::info!("Lifecycle transition: {:?} -> {:?}", *state, new_state);
            *state = new_state;
            let _ = self.lifecycle_tx.send(new_state);
        }
    }
}

/// Extract a topic hint from user message content for curiosity vectorization (ADR-007).
///
/// Uses simple heuristics: picks the longest non-stopword segment as the topic.
/// Returns None for very short or empty messages.
fn extract_topic_hint(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.len() < 4 {
        return None;
    }
    // For Chinese text: take the first meaningful clause (up to punctuation)
    let clause = trimmed
        .split(|c: char| "，。！？、；：…—,.!?;:".contains(c))
        .next()
        .unwrap_or(trimmed)
        .trim();
    if clause.len() < 2 {
        return None;
    }
    // Cap at 30 chars to keep interests concise
    let topic: String = clause.chars().take(30).collect();
    Some(topic)
}

/// Result of processing an interaction
#[derive(Debug, Clone)]
pub struct InteractionResult {
    /// Somatic marker from System 1
    pub somatic_marker: SomaticMarker,

    /// Current organism state snapshot
    pub state_snapshot: OrganismState,

    /// Current lifecycle state
    pub lifecycle: LifecycleState,
}

impl InteractionResult {
    fn sleeping() -> Self {
        let default_state = OrganismState::default();
        Self {
            somatic_marker: SomaticMarker::from_state(&default_state),
            state_snapshot: default_state,
            lifecycle: LifecycleState::Sleeping,
        }
    }
}

/// Result of evaluating an action against values
#[derive(Debug, Clone)]
pub struct ActionEvaluation {
    /// Overall moral valence (-1.0 to 1.0)
    pub moral_valence: f32,

    /// Whether there's a value conflict
    pub has_conflict: bool,

    /// Human-readable explanation
    pub explanation: String,

    /// Recommendation: should the action proceed?
    pub should_proceed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_coordinator_basic() {
        let limbic = Arc::new(LimbicSystem::new());
        let coordinator = OrganismCoordinator::new(limbic);

        assert_eq!(coordinator.lifecycle_state().await, LifecycleState::Awake);
    }

    #[tokio::test]
    async fn test_process_interaction() {
        let limbic = Arc::new(LimbicSystem::new());
        let coordinator = OrganismCoordinator::new(limbic);

        let result = coordinator
            .process_interaction("user", "你好！今天心情怎么样？", 1.0)
            .await
            .unwrap();

        assert_eq!(result.lifecycle, LifecycleState::Awake);
        // SomaticMarker doesn't have surprise field, check affect instead
        assert!(result.somatic_marker.affect.arousal >= 0.0);
    }

    #[tokio::test]
    async fn test_evaluate_action() {
        let limbic = Arc::new(LimbicSystem::new());
        let coordinator = OrganismCoordinator::new(limbic);

        // Seed values so moral evaluation has something to judge against
        {
            let state_arc = coordinator.state();
            let mut state = state_arc.write().await;
            state.slow.values = mneme_core::state::ValueNetwork::seed();
        }

        // Test an action that violates honesty
        let eval = coordinator
            .evaluate_action("用户问我是否喜欢他", "撒谎说喜欢")
            .await
            .unwrap();

        assert!(eval.moral_valence < 0.0);
        assert!(!eval.explanation.is_empty());
    }

    #[tokio::test]
    async fn test_feedback_recording() {
        let limbic = Arc::new(LimbicSystem::new());
        let coordinator = OrganismCoordinator::new(limbic);

        coordinator
            .record_feedback(
                SignalType::UserEmotionalFeedback,
                "用户表达了感激".to_string(),
                0.8,
                0.6,
            )
            .await;

        let buffer = coordinator.feedback_buffer.read().await;
        assert_eq!(buffer.pending_count(), 1);
    }

    #[tokio::test]
    async fn test_manual_sleep() {
        let limbic = Arc::new(LimbicSystem::new());
        let mut config = OrganismConfig::default();
        config.sleep_config.allow_manual_trigger = true;

        let coordinator = OrganismCoordinator::with_config(limbic, config, None);

        // Add some episodes
        {
            let mut episodes = coordinator.episode_buffer.write().await;
            for i in 0..15 {
                episodes.push(EpisodeDigest {
                    timestamp: Utc::now(),
                    author: "user".to_string(),
                    content: format!("Test message {}", i),
                    emotional_valence: 0.5,
                });
            }
        }

        // Trigger sleep
        let result = coordinator.trigger_sleep().await.unwrap();
        assert!(result.performed);

        // Should be back to awake
        assert_eq!(coordinator.lifecycle_state().await, LifecycleState::Awake);
    }
}
