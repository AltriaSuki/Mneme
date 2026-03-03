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
use std::collections::VecDeque;
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
    /// #85: Partial failure — some subsystems unavailable, core still running
    Degraded,
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

/// #85: Tracks subsystem health for self-diagnosis and graceful degradation.
#[derive(Debug, Clone)]
pub struct HealthMonitor {
    /// Consecutive DB failure count
    pub db_failures: u32,
    /// Consecutive LLM failure count
    pub llm_failures: u32,
    /// Whether DB is considered healthy
    pub db_healthy: bool,
    /// Whether LLM is considered healthy
    pub llm_healthy: bool,
}

impl Default for HealthMonitor {
    fn default() -> Self {
        Self { db_failures: 0, llm_failures: 0, db_healthy: true, llm_healthy: true }
    }
}

impl HealthMonitor {
    /// Record a subsystem success, resetting its failure counter.
    pub fn record_success(&mut self, subsystem: &str) {
        match subsystem {
            "db" => { self.db_failures = 0; self.db_healthy = true; }
            "llm" => { self.llm_failures = 0; self.llm_healthy = true; }
            _ => {}
        }
    }

    /// Record a subsystem failure. Returns true if the subsystem just became unhealthy.
    pub fn record_failure(&mut self, subsystem: &str) -> bool {
        match subsystem {
            "db" => {
                self.db_failures += 1;
                let was_healthy = self.db_healthy;
                self.db_healthy = self.db_failures < 3;
                was_healthy && !self.db_healthy
            }
            "llm" => {
                self.llm_failures += 1;
                let was_healthy = self.llm_healthy;
                self.llm_healthy = self.llm_failures < 3;
                was_healthy && !self.llm_healthy
            }
            _ => false,
        }
    }

    /// True if any subsystem is unhealthy.
    pub fn is_degraded(&self) -> bool {
        !self.db_healthy || !self.llm_healthy
    }

    /// Format a diagnostic summary.
    pub fn diagnostic_summary(&self) -> String {
        format!(
            "DB: {} (failures: {}), LLM: {} (failures: {})",
            if self.db_healthy { "ok" } else { "UNHEALTHY" }, self.db_failures,
            if self.llm_healthy { "ok" } else { "UNHEALTHY" }, self.llm_failures,
        )
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
    dynamics: RwLock<DefaultDynamics>,

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

    /// LLM-based dream narrator (Phase 2, v0.8.0)
    dream_narrator: tokio::sync::RwLock<Option<Arc<dyn crate::dream::DreamNarrator>>>,

    /// #85: Health monitor for self-diagnosis and graceful degradation
    health: Arc<RwLock<HealthMonitor>>,

    /// #5: Implicit feedback — timestamp of last Mneme response (for reply latency)
    last_response_ts: Arc<RwLock<Option<i64>>>,
    /// #5: Implicit feedback — running average message length for baseline
    avg_message_len: Arc<RwLock<f32>>,

    /// Sliding window of recent messages for repetition detection (surprise decay).
    /// Capacity: 20 messages.
    recent_messages: RwLock<VecDeque<String>>,
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
            dynamics: RwLock::new(DefaultDynamics::default()),
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
            dream_narrator: tokio::sync::RwLock::new(None),
            health: Arc::new(RwLock::new(HealthMonitor::default())),
            last_response_ts: Arc::new(RwLock::new(None)),
            avg_message_len: Arc::new(RwLock::new(50.0)),
            recent_messages: RwLock::new(VecDeque::with_capacity(20)),
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

            // Catch-up idle dynamics: simulate the passage of time since last update.
            // Without this, single-shot invocations never get idle recovery (no AgentLoop
            // ticks between runs), so stress monotonically climbs and energy never recovers.
            //
            // Preserve boredom, curiosity, and curiosity_vector — these are interaction-driven
            // states that shouldn't change during dormancy (organism was offline, not "bored").
            let elapsed = chrono::Utc::now().timestamp() - coordinator.state.read().await.last_updated;
            if elapsed > 5 {
                let catchup_dt = (elapsed as f32).min(3600.0);
                let idle_input = mneme_core::SensoryInput {
                    env: mneme_core::EnvironmentMetrics::sample(),
                    ..Default::default()
                };
                let mut state = coordinator.state.write().await;
                // Save interaction-driven states before catch-up
                let saved_boredom = state.fast.boredom;
                let saved_curiosity = state.fast.curiosity;
                let saved_cv = state.fast.curiosity_vector.clone();

                let dynamics = coordinator.dynamics.read().await;
                let medium_clone = state.medium.clone();
                dynamics.step_fast(&mut state.fast, &medium_clone, &idle_input, catchup_dt);
                let fast_clone = state.fast.clone();
                let slow_clone = state.slow.clone();
                dynamics.step_medium(&mut state.medium, &fast_clone, &slow_clone, &idle_input, catchup_dt);
                drop(dynamics);

                // Restore interaction-driven states
                state.fast.boredom = saved_boredom;
                state.fast.curiosity = saved_curiosity;
                state.fast.curiosity_vector = saved_cv;
                state.last_updated = chrono::Utc::now().timestamp();
                tracing::info!("Applied {}s catch-up idle dynamics", elapsed);
            }

            // Sync loaded state to limbic system so somatic markers reflect persisted state.
            // Without this, limbic keeps its default state and modulation ignores DB values.
            let loaded = coordinator.state.read().await.clone();
            coordinator.limbic.set_state(loaded).await;
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

        // Load persisted recent messages for repetition detection
        if let Ok(msgs) = db.load_recent_messages(20).await {
            if !msgs.is_empty() {
                tracing::info!("Loaded {} recent messages for repetition detection", msgs.len());
                let mut recent = coordinator.recent_messages.write().await;
                for m in msgs {
                    recent.push_back(m);
                }
            }
        }

        // Load learned ModulationCurves from DB
        if let Ok(Some(curves)) = db.load_learned_curves().await {
            coordinator.limbic.set_curves(curves).await;
            tracing::info!("Loaded learned ModulationCurves from DB");
        }

        // Load learned NeuralModulator from DB (#14)
        if let Ok(Some(nn)) = db.load_learned_neural().await {
            tracing::info!("Loaded learned NeuralModulator (blend={:.2})", nn.blend);
            coordinator.limbic.set_neural(Some(nn)).await;
        } else {
            // No persisted weights — curriculum-train a fresh network
            let mut nn = mneme_limbic::NeuralModulator::default();
            nn.curriculum_train(100, 0.01);
            tracing::info!("Curriculum-trained fresh NeuralModulator");
            coordinator.limbic.set_neural(Some(nn)).await;
        }

        // Load learned LTC weights (Hebbian w_rec)
        if let Ok(Some(ltc)) = db.load_learned_ltc().await {
            tracing::info!("Loaded learned LTC (blend={:.2})", ltc.blend);
            coordinator.limbic.set_ltc(Some(ltc)).await;
        } else {
            coordinator.limbic.set_ltc(Some(mneme_limbic::LiquidNeuralModulator::new())).await;
            tracing::info!("Initialized fresh LTC");
        }

        // Load learned dynamics parameters
        if let Ok(Some(ld)) = db.load_learned_dynamics().await {
            tracing::info!(
                "Loaded learned dynamics (energy_target={:.3}, stress_decay={:.4})",
                ld.energy_target, ld.stress_decay_rate
            );
            coordinator.dynamics.write().await.learnable = ld;
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

    /// Set the LLM-based dream narrator (Phase 2, v0.8.0)
    pub async fn set_dream_narrator(&self, narrator: Arc<dyn crate::dream::DreamNarrator>) {
        *self.dream_narrator.write().await = Some(narrator);
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

    /// #85: Get health monitor reference for external subsystems to report status.
    pub fn health(&self) -> Arc<RwLock<HealthMonitor>> {
        self.health.clone()
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

    /// Store an expression preference learned from user feedback (#90).
    pub async fn store_expression_preference(&self, key: &str, confidence: f32) {
        if let Some(ref db) = self.db {
            if let Err(e) = db
                .store_self_knowledge("expression", key, confidence, "self:expression_feedback", None)
                .await
            {
                tracing::warn!("Failed to store expression preference: {}", e);
            } else {
                tracing::info!(
                    "Expression preference learned: {} (confidence={:.2})",
                    key,
                    confidence
                );
            }
        }
    }

    /// Process an incoming interaction
    ///
    /// This is the main entry point for handling user messages.
    /// It coordinates System 1 and System 2 processing.
    #[tracing::instrument(skip(self, content), fields(author, response_delay_secs))]
    pub async fn process_interaction(
        &self,
        author: &str,
        content: &str,
        response_delay_secs: f32,
        source: &str,
    ) -> Result<InteractionResult> {
        let _guard = self.state_mutation_lock.lock().await;

        // Check if we should be processing
        let lifecycle = *self.lifecycle_state.read().await;
        if lifecycle == LifecycleState::Sleeping {
            return Ok(InteractionResult::sleeping());
        }

        // 1. Analyze sentiment of the user's message FIRST — this is the ground
        //    truth for content_valence/content_intensity fed into the ODE dynamics.
        //    Previously we used soma.affect.valence here, which is the organism's
        //    *current* emotional state — not the valence of the incoming message.
        let (mut content_valence, mut content_intensity) = Self::analyze_sentiment(content);
        tracing::debug!(
            "Sentiment analysis: valence={:.3}, intensity={:.3} for '{}'",
            content_valence, content_intensity,
            content.chars().take(30).collect::<String>()
        );

        // 1b. B-9 Privacy-Somatic Coupling: when someone probes for private thoughts
        //     and we have secrets, amplify the threat signal through the body.
        //     This is a physical response (heart racing when your secret is probed),
        //     NOT a narrative hint — the LLM never sees "this is private".
        let interrogation_threat = Self::detect_interrogation_threat(content);
        let privacy_coupling_active = if interrogation_threat > 0.1 {
            let has_secrets = if let Some(ref db) = self.db {
                db.has_private_self_knowledge().await
            } else {
                false
            };
            if has_secrets {
                let amplifier = 1.0 + interrogation_threat * 2.0; // up to 3× intensity
                content_intensity = (content_intensity * amplifier).clamp(0.0, 1.0);
                content_valence = content_valence - interrogation_threat * 0.3;
                content_valence = content_valence.clamp(-1.0, 1.0);
                tracing::info!(
                    "Privacy-somatic coupling: threat={:.2}, amplified intensity={:.3}, valence={:.3}",
                    interrogation_threat, content_intensity, content_valence
                );
                true
            } else {
                false
            }
        } else {
            false
        };

        // 1c. §14.3 Belief-Tension Coupling: when user's message touches a topic
        //     where we hold strong emotional beliefs, the body tenses up —
        //     cognitive dissonance before conscious processing.
        let (belief_tension, belief_valence) = self.detect_belief_tension(content).await;
        if belief_tension > 0.1 {
            content_intensity = (content_intensity + belief_tension * 0.5).clamp(0.0, 1.0);
            // Pull valence toward the belief's emotional direction
            content_valence = (content_valence + belief_valence * belief_tension * 0.4).clamp(-1.0, 1.0);
            tracing::info!(
                "Belief-tension coupling: tension={:.2}, belief_valence={:.2}, intensity={:.3}, valence={:.3}",
                belief_tension, belief_valence, content_intensity, content_valence
            );
        }

        // 1d. §14.1 Déjà vu: check if message contains code matching an owned artifact.
        //     Recognition of self-created content triggers surprise + arousal spike.
        let deja_vu_path = if let Some(ref db) = self.db {
            db.check_content_ownership(content).await
        } else {
            None
        };
        if let Some(ref path) = deja_vu_path {
            content_intensity = (content_intensity + 0.4).clamp(0.0, 1.0);
            tracing::info!(path, "Déjà vu: recognized own artifact in message");
        }

        // 2. Send stimulus to System 1 (limbic) for async emotional processing
        let stimulus = self.create_stimulus(author, content);
        let _ = self.limbic.receive_stimulus(stimulus).await;

        // 3. Get somatic marker (used for modulation, NOT for content valence)
        let mut soma = self.limbic.get_somatic_marker().await;

        // 3b. B-9: If privacy coupling fired, directly amplify the somatic marker.
        //     The pre-ODE marker won't reflect the threat yet — this is the
        //     "instant flinch" before conscious processing.
        if privacy_coupling_active {
            soma.stress = (soma.stress + interrogation_threat * 0.35).clamp(0.0, 1.0);
            soma.affect.arousal = (soma.affect.arousal + interrogation_threat * 0.4).clamp(0.0, 1.0);
            soma.affect.valence = (soma.affect.valence - interrogation_threat * 0.3).clamp(-1.0, 1.0);
            // Freeze response: energy diverts to fight-or-flight, reducing verbal output
            soma.energy = (soma.energy - interrogation_threat * 0.3).clamp(0.0, 1.0);
        }

        // 3c. §14.3: Belief tension → cognitive dissonance somatic response
        if belief_tension > 0.1 {
            soma.stress = (soma.stress + belief_tension * 0.3).clamp(0.0, 1.0);
            soma.affect.arousal = (soma.affect.arousal + belief_tension * 0.25).clamp(0.0, 1.0);
            // Energy drain from internal conflict
            soma.energy = (soma.energy - belief_tension * 0.15).clamp(0.0, 1.0);
        }

        // 3d. §14.1: Déjà vu recognition → surprise spike (arousal without stress)
        if deja_vu_path.is_some() {
            soma.affect.arousal = (soma.affect.arousal + 0.3).clamp(0.0, 1.0);
        }

        // 3e. Phase II Step 2: Feed somatic patches to LTC as Hebbian training signal.
        //     Without this, the neural network never learns the causal relationships
        //     (interrogation→stress, belief tension→dissonance, déjà vu→arousal).
        {
            let patch_surprise = interrogation_threat + belief_tension
                + if deja_vu_path.is_some() { 0.3 } else { 0.0 };
            let patch_reward = -interrogation_threat - belief_tension * 0.5
                + if deja_vu_path.is_some() { 0.2 } else { 0.0 };
            if patch_surprise > 0.05 {
                if let Some(mut ltc) = self.limbic.get_ltc().await {
                    ltc.hebbian_update(patch_surprise, patch_reward, 0.005);
                    self.limbic.set_ltc(Some(ltc.clone())).await;
                    if let Some(ref db) = self.db {
                        let _ = db.save_learned_ltc(&ltc).await;
                    }
                }
            }
        }

        // 4. Update fast state based on stimulus
        // Use effective dt=15s to represent the "attention window" of a conversation
        // turn — reading + thinking + composing. A 1s blip can't compete with 60s
        // idle ticks, causing boredom/curiosity to be unresponsive to interaction.
        let similarity = self.compute_message_similarity(content).await;
        let sensory = self.create_sensory_input(
            content, content_valence, content_intensity, &soma, response_delay_secs, source,
            similarity,
        );
        {
            let mut state = self.state.write().await;
            let medium_clone = state.medium.clone();
            let slow_clone = state.slow.clone();
            let dynamics = self.dynamics.read().await;
            dynamics.step_fast(&mut state.fast, &medium_clone, &sensory, 15.0);
            let fast_clone = state.fast.clone();
            // Medium dynamics use actual inter-message interval (not the 15s attention window).
            // response_delay_secs captures real elapsed time since last response.
            // Clamp to avoid huge jumps on first message (when delay is 0 or very large).
            let medium_dt = response_delay_secs.clamp(10.0, 3600.0);
            dynamics.step_medium(&mut state.medium, &fast_clone, &slow_clone, &sensory, medium_dt);
            drop(dynamics);
            state.last_updated = Utc::now().timestamp();
            tracing::debug!(
                "Post-interaction state: energy={:.3}, stress={:.3}, valence={:.3}, mood={:.3}",
                state.fast.energy, state.fast.stress,
                state.fast.affect.valence, state.medium.mood_bias
            );
        }

        // 4. Record episode for narrative
        {
            let mut episodes = self.episode_buffer.write().await;
            episodes.push(EpisodeDigest {
                timestamp: Utc::now(),
                author: author.to_string(),
                content: content.chars().take(500).collect(),
                emotional_valence: content_valence,
            });

            // Keep buffer bounded (tighter cap to limit memory if sleep never fires)
            if episodes.len() > 200 {
                episodes.drain(0..100);
            }
        }

        // 5. Increment interaction counter + blend growth
        {
            let mut count = self.interaction_count.write().await;
            *count += 1;
        }
        // 5-blend: Phase II — every interaction nudges blend upward.
        // Without this, blend only grew on explicit "like"/"dislike" feedback,
        // leaving LTC at ~0.01 indefinitely.
        {
            if let Some(mut nn) = self.limbic.get_neural().await {
                nn.blend = (nn.blend + 0.005).min(1.0);
                self.limbic.set_neural(Some(nn.clone())).await;
                if let Some(ref db) = self.db {
                    let _ = db.save_learned_neural(&nn).await;
                }
            }
            if let Some(mut ltc) = self.limbic.get_ltc().await {
                ltc.blend = (ltc.blend + 0.005).min(0.95);
                self.limbic.set_ltc(Some(ltc.clone())).await;
                if let Some(ref db) = self.db {
                    let _ = db.save_learned_ltc(&ltc).await;
                }
            }
        }

        // 5a-exp: Phase II Step 3: Experience Replay — every 50 interactions,
        // sample mini-batch from real data and train LTC.
        {
            let count = *self.interaction_count.read().await;
            // TODO(Phase3): Make learnable
            if count > 0 && count % 50 == 0 {
                if let Some(ref db) = self.db {
                    match db.sample_experience_batch(32).await {
                        Ok(batch) if batch.len() >= 10 => {
                            // Convert to LTC training format
                            let train_samples: Vec<_> = batch.iter().map(|s| {
                                let features = mneme_limbic::neural::StateFeatures {
                                    energy: s.energy, stress: s.stress, arousal: s.arousal,
                                    mood_bias: s.mood_bias, social_need: s.social_need,
                                    boredom: s.boredom,
                                    cpu_load: 0.0, memory_pressure: 0.0, channel_distance: 0.0,
                                };
                                (features, s.modulation.clone(), s.feedback_valence)
                            }).collect();
                            // Train MLP on replay batch
                            if let Some(mut nn) = self.limbic.get_neural().await {
                                nn.train(&train_samples, 0.001);
                                self.limbic.set_neural(Some(nn.clone())).await;
                                if let Some(ref db2) = self.db {
                                    let _ = db2.save_learned_neural(&nn).await;
                                }
                            }
                            // Train LTC via Hebbian on replay batch
                            if let Some(mut ltc) = self.limbic.get_ltc().await {
                                for (features, _mv, reward) in &train_samples {
                                    ltc.step(features, 1.0);
                                    ltc.hebbian_update(reward.abs(), *reward, 0.003);
                                }
                                self.limbic.set_ltc(Some(ltc.clone())).await;
                                if let Some(ref db2) = self.db {
                                    let _ = db2.save_learned_ltc(&ltc).await;
                                }
                            }
                            tracing::debug!("Experience replay: trained on {} samples", train_samples.len());
                        }
                        Ok(batch) if batch.len() < 10 => {
                            // Cold-start fallback: use synthetic curriculum
                            if let Some(mut ltc) = self.limbic.get_ltc().await {
                                ltc.curriculum_train(1, 0.005);
                                self.limbic.set_ltc(Some(ltc.clone())).await;
                            }
                            tracing::debug!("Experience replay: cold-start fallback ({} samples in buffer)", batch.len());
                        }
                        Err(e) => tracing::warn!("Experience replay sampling failed: {}", e),
                        _ => {}
                    }
                }
            }
        }

        // 5a. ADR-009: Maturity progression — slow growth per interaction
        {
            let mut state = self.state.write().await;
            // Asymptotic growth: each interaction nudges maturity toward 1.0
            // ~500 interactions to reach 0.5, ~2000 to reach 0.8
            state.slow.maturity += 0.001 * (1.0 - state.slow.maturity);
            state.slow.maturity = state.slow.maturity.clamp(0.0, 1.0);
        }

        // 5b. Infer implicit feedback from user behavior (#5)
        self.infer_implicit_feedback(content).await;

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

        // 8. Record message for repetition detection (surprise decay)
        self.record_message(content).await;

        // 9. Check if we should transition to drowsy/sleep
        self.check_lifecycle_transition().await;

        Ok(InteractionResult {
            somatic_marker: soma,
            state_snapshot: self.state.read().await.clone(),
            lifecycle: *self.lifecycle_state.read().await,
            content_valence,
            deja_vu_path: deja_vu_path.clone(),
        })
    }

    /// B-18: Record a file created by Mneme's tool use as an owned artifact.
    pub async fn record_artifact(&self, path: &str) {
        if let Some(db) = &self.db {
            db.record_created_artifact(path).await;
        }
    }

    /// Process perceptual input from tool results.
    ///
    /// When Mneme reads file content or receives tool output, the sentiment
    /// of that content should affect her emotional state — just as seeing
    /// something disturbing affects a human's body even without social context.
    /// This is a non-social stimulus (is_social=false) with reduced intensity
    /// (×0.5) since it's indirect perception, not direct interaction.
    pub async fn process_perceptual_input(&self, content: &str) {
        let (valence, intensity) = Self::analyze_sentiment(content);

        // B-18: Detect file-not-found for owned artifacts → amplify grief
        let ownership_amplifier = self.check_artifact_grief(content).await;

        // Skip neutral / low-intensity perceptions to avoid noise
        // (but always process if ownership grief is triggered)
        if intensity < 0.15 && ownership_amplifier <= 1.0 {
            return;
        }
        let effective_intensity = if ownership_amplifier > 1.0 {
            // B-18: Owned artifact loss — floor at 0.5 regardless of text sentiment
            (intensity * 0.5 * ownership_amplifier).max(0.5).min(1.0)
        } else {
            (intensity * 0.5).min(1.0)
        };
        let effective_valence = if ownership_amplifier > 1.0 {
            // Force strong negative valence for owned artifact loss
            valence.min(-0.6)
        } else {
            valence
        };
        let stimulus = Stimulus {
            valence: effective_valence,
            intensity: effective_intensity,
            is_social: false,
            content: content.chars().take(200).collect(),
            violated_values: vec![],
        };
        tracing::info!(
            "Perceptual stimulus: valence={:.3}, intensity={:.3}",
            stimulus.valence,
            stimulus.intensity
        );
        let _ = self.limbic.receive_stimulus(stimulus.clone()).await;

        // Also step the coordinator's own state directly (the limbic heartbeat
        // updates a separate internal copy that save_state() doesn't read).
        let sensory = SensoryInput {
            content_valence: stimulus.valence,
            content_intensity: stimulus.intensity,
            surprise: 0.0,
            is_social: false,
            response_delay_factor: 1.0,
            violated_values: vec![],
            topic_hint: None,
            env: Default::default(),
        };
        {
            let mut state = self.state.write().await;
            let medium_clone = state.medium.clone();
            let dynamics = self.dynamics.read().await;
            dynamics.step_fast(&mut state.fast, &medium_clone, &sensory, 1.0);
        }

        // Phase II Step 2: Feed grief/perceptual patches to LTC as Hebbian training signal.
        // Without this, LTC never learns ownership-grief or perceptual threat responses.
        if ownership_amplifier > 1.0 {
            if let Some(mut ltc) = self.limbic.get_ltc().await {
                // TODO(Phase3): Make learnable
                let grief_surprise = (ownership_amplifier - 1.0).min(1.0);
                ltc.hebbian_update(grief_surprise, -0.8, 0.005);
                self.limbic.set_ltc(Some(ltc.clone())).await;
                if let Some(ref db) = self.db {
                    let _ = db.save_learned_ltc(&ltc).await;
                }
            }
        }
    }

    /// B-18: Check if tool output contains file-not-found for an owned artifact.
    /// Returns amplifier: 1.0 = normal, 3.0 = owned artifact grief.
    async fn check_artifact_grief(&self, content: &str) -> f32 {
        let db = match &self.db {
            Some(db) => db,
            None => return 1.0,
        };

        // Extract file paths from common error patterns
        let patterns = [
            "No such file or directory",
            "FileNotFoundError",
            "not found",
            "cannot open",
        ];

        let has_file_error = patterns.iter().any(|p| content.contains(p));
        if !has_file_error {
            return 1.0;
        }

        // Try to extract path from content (common patterns)
        for word in content.split_whitespace() {
            let path = word.trim_matches(|c: char| c == '\'' || c == '"' || c == ':');
            if path.starts_with('/') && db.is_owned_artifact(path).await {
                tracing::warn!(path, "Owned artifact lost — amplifying grief response");
                return 3.0;
            }
        }

        1.0
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
            self.dynamics.read().await.apply_moral_cost(&mut state.fast, moral_cost);
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

    /// #5: Record when Mneme sends a response (for reply latency tracking).
    pub async fn record_response_timestamp(&self) {
        *self.last_response_ts.write().await = Some(chrono::Utc::now().timestamp());
    }

    /// #5: Infer implicit feedback from user behavior.
    ///
    /// Signals: reply latency (fast = engaged), message length vs baseline.
    pub async fn infer_implicit_feedback(&self, message: &str) {
        let now = chrono::Utc::now().timestamp();
        let msg_len = message.chars().count() as f32;

        // Update running average (exponential moving average, α=0.1)
        {
            let mut avg = self.avg_message_len.write().await;
            *avg = *avg * 0.9 + msg_len * 0.1;
        }

        // Reply latency signal
        if let Some(last_ts) = *self.last_response_ts.read().await {
            let latency = now - last_ts;
            // Fast reply (< 30s) = positive engagement; slow (> 300s) = disengagement
            let (valence, confidence) = if latency < 30 {
                (0.4, 0.6)
            } else if latency > 300 {
                (-0.3, 0.6)
            } else {
                return; // neutral, don't record
            };
            self.record_feedback(
                SignalType::ImplicitEngagement,
                format!("reply_latency:{}s", latency),
                confidence,
                valence,
            )
            .await;
        }

        // Message length signal: significantly longer than average = engaged
        let avg = *self.avg_message_len.read().await;
        if avg > 10.0 {
            let ratio = msg_len / avg;
            if ratio > 2.0 {
                self.record_feedback(
                    SignalType::ImplicitEngagement,
                    format!("long_message:ratio={:.1}", ratio),
                    0.65,
                    0.3,
                )
                .await;
            } else if ratio < 0.3 && msg_len < 10.0 {
                self.record_feedback(
                    SignalType::ImplicitEngagement,
                    format!("short_message:ratio={:.1}", ratio),
                    0.65,
                    -0.3,
                )
                .await;
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
        let state = self.state.read().await;
        let energy = state.fast.energy;
        let stress = state.fast.stress;
        let arousal = state.fast.affect.arousal;
        let mood_bias = state.medium.mood_bias;
        let social_need = state.fast.social_need;
        let boredom = state.fast.boredom;
        drop(state);

        // Persist sample for batch learning during sleep
        if let Some(ref db) = self.db {
            let sample = crate::learning::ModulationSample {
                id: 0, energy, stress, arousal, mood_bias, social_need, boredom,
                modulation: modulation.clone(),
                feedback_valence,
                timestamp: chrono::Utc::now().timestamp(),
            };
            if let Err(e) = db.save_modulation_sample(&sample).await {
                tracing::warn!("Failed to save modulation sample: {}", e);
            }
        }

        // #983: Incremental learning — micro-update on significant feedback
        if feedback_valence.abs() > 0.2 {
            let features = mneme_limbic::neural::StateFeatures {
                energy, stress, arousal, mood_bias, social_need,
                boredom,
                cpu_load: 0.0, memory_pressure: 0.0, channel_distance: 0.0,
            };
            // Online NeuralModulator update (small learning rate) + persist
            if let Some(mut nn) = self.limbic.get_neural().await {
                nn.train(&[(features.clone(), modulation.clone(), feedback_valence)], 0.002);
                nn.blend = (nn.blend + 0.01).min(1.0);
                self.limbic.set_neural(Some(nn.clone())).await;
                if let Some(ref db) = self.db {
                    let _ = db.save_learned_neural(&nn).await;
                }
            }
            // Online LTC Hebbian update + persist
            // Phase II Step 4: Use prediction error (LTC vs curves L2 divergence)
            // as surprise signal instead of raw arousal.
            if let Some(mut ltc) = self.limbic.get_ltc().await {
                let ltc_mv = ltc.readout();
                let state = self.state.read().await;
                let curves_mv = SomaticMarker::from_state(&state).to_modulation_vector();
                drop(state);
                // TODO(Phase3): Make learnable
                let prediction_error = curves_mv.l2_divergence(&ltc_mv);
                ltc.hebbian_update(prediction_error, feedback_valence, 0.005);
                ltc.blend = (ltc.blend + 0.01).min(0.95);
                self.limbic.set_ltc(Some(ltc.clone())).await;
                if let Some(ref db) = self.db {
                    let _ = db.save_learned_ltc(&ltc).await;
                }
            }
        }
    }

    /// Trigger sleep consolidation manually
    #[tracing::instrument(skip(self))]
    pub async fn trigger_sleep(&self) -> Result<ConsolidationResult> {
        let _guard = self.state_mutation_lock.lock().await;

        // Transition to sleeping state
        self.set_lifecycle_state(LifecycleState::Sleeping).await;

        // Run the inner logic, ensuring we always transition back to Awake
        match self.trigger_sleep_inner().await {
            Ok(result) => {
                self.set_lifecycle_state(LifecycleState::Awake).await;
                Ok(result)
            }
            Err(e) => {
                tracing::error!("Sleep consolidation failed, restoring Awake state: {}", e);
                self.set_lifecycle_state(LifecycleState::Awake).await;
                Err(e)
            }
        }
    }

    /// Inner sleep logic extracted so trigger_sleep can guarantee Awake restoration.
    async fn trigger_sleep_inner(&self) -> Result<ConsolidationResult> {
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
            let collapsed = SleepConsolidator::handle_crisis(&mut state, crisis, &*self.dynamics.read().await);
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
                    ..Default::default()
                };
                if let Err(e) = db.memorize(&meta_content).await {
                    tracing::warn!("Failed to store reflection meta-episode: {}", e);
                }
            }
        }

        // Dream generation (ADR-008): recall random memories and weave a dream
        // Phase 2 (v0.8.0): Try LLM narrator first, fall back to template generator
        if let Some(ref db) = self.db {
            match db.recall_random_by_strength(3).await {
                Ok(seeds) if seeds.len() >= 2 => {
                    let current = self.state.read().await.clone();

                    // #1478: Build reflection context for dream-reflection interaction
                    let reflection_ctx: String = result
                        .self_reflections
                        .iter()
                        .map(|r| format!("[{}] {}", r.domain, r.content))
                        .collect::<Vec<_>>()
                        .join("\n");

                    // Try LLM dream narrator first (Phase 2)
                    let llm_dream = if let Some(narrator) = self.dream_narrator.read().await.as_ref() {
                        match narrator.narrate_dream(&seeds, &current, &reflection_ctx).await {
                            Ok(narrative) => {
                                let tone = crate::dream::DreamGenerator::compute_emotional_tone(
                                    &seeds,
                                    current.medium.mood_bias,
                                );
                                tracing::info!("LLM dream narrator succeeded, tone={:.2}", tone);
                                Some(crate::dream::DreamEpisode {
                                    narrative,
                                    source_ids: seeds.iter().map(|s| s.id.clone()).collect(),
                                    emotional_tone: tone,
                                })
                            }
                            Err(e) => {
                                tracing::warn!("LLM dream narrator failed, falling back to template: {}", e);
                                None
                            }
                        }
                    } else {
                        None
                    };

                    // Fall back to template generator if LLM didn't produce a dream
                    let dream = llm_dream.or_else(|| crate::dream::DreamGenerator::generate(&seeds, &current));

                    if let Some(dream) = dream {
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
                            ..Default::default()
                        };
                        if let Err(e) = db.memorize(&dream_content).await {
                            tracing::warn!("Failed to store dream episode: {}", e);
                        }

                        // #1478: Dream insight — intense dreams yield self-knowledge
                        if dream.emotional_tone.abs() > 0.5 && !dream.narrative.is_empty() {
                            let insight = format!(
                                "梦境领悟（情绪强度{:.1}）：{}",
                                dream.emotional_tone,
                                &dream.narrative.chars().take(66).collect::<String>()
                            );
                            if let Err(e) = db
                                .store_self_knowledge(
                                    "dream_insight",
                                    &insight,
                                    dream.emotional_tone.abs(),
                                    "self:dream",
                                    None,
                                )
                                .await
                            {
                                tracing::warn!("Failed to store dream insight: {}", e);
                            }
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
                // #14: Also train NeuralModulator on the same samples
                let mut nn = self.limbic.get_neural().await
                    .unwrap_or_else(mneme_limbic::NeuralModulator::default);
                let train_samples: Vec<_> = samples.iter().map(|s| {
                    (
                        mneme_limbic::neural::StateFeatures {
                            energy: s.energy,
                            stress: s.stress,
                            arousal: s.arousal,
                            mood_bias: s.mood_bias,
                            social_need: s.social_need,
                            boredom: 0.0,
                            cpu_load: 0.0,
                            memory_pressure: 0.0,
                            channel_distance: 0.0,
                        },
                        s.modulation.clone(),
                        s.feedback_valence,
                    )
                }).collect();
                nn.train(&train_samples, 0.01);
                // Gradually increase blend as we accumulate training
                nn.blend = (nn.blend + 0.02).min(1.0);
                let _ = db.save_learned_neural(&nn).await;
                self.limbic.set_neural(Some(nn)).await;
                tracing::info!("Trained NeuralModulator on {} samples", samples.len());

                // Learn dynamics parameters from the same samples
                let dyn_samples: Vec<_> = samples.iter()
                    .map(|s| (s.energy, s.stress, s.feedback_valence))
                    .collect();
                {
                    let mut dyn_guard = self.dynamics.write().await;
                    if dyn_guard.learnable.learn_from_samples(&dyn_samples) {
                        let _ = db.save_learned_dynamics(&dyn_guard.learnable).await;
                        tracing::info!(
                            "Adjusted dynamics: energy_target={:.3}, stress_decay={:.4}",
                            dyn_guard.learnable.energy_target,
                            dyn_guard.learnable.stress_decay_rate,
                        );
                    }
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
            // Clean up duplicates first
            match gm.detect_conflicts().await {
                Ok(conflicts) => {
                    let mut abandoned = 0u32;
                    for conflict in &conflicts {
                        if conflict.reason == "duplicate" {
                            // Keep the older goal (lower id), abandon the newer one
                            let abandon_id = conflict.goal_a.max(conflict.goal_b);
                            if let Err(e) = gm.set_status(abandon_id, crate::goals::GoalStatus::Abandoned).await {
                                tracing::warn!("Failed to abandon duplicate goal #{}: {}", abandon_id, e);
                            } else {
                                abandoned += 1;
                            }
                        }
                    }
                    if abandoned > 0 {
                        tracing::info!("Abandoned {} duplicate goals during sleep cleanup", abandoned);
                    }
                }
                Err(e) => tracing::warn!("Goal conflict detection failed: {}", e),
            }

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

        Ok(result)
    }

    /// Run offline learning on unconsumed samples without full sleep consolidation.
    ///
    /// Returns the number of samples processed.
    pub async fn trigger_training(&self) -> Result<usize> {
        let db = self.db.as_ref().ok_or_else(|| anyhow::anyhow!("No database configured"))?;
        let samples = db.load_unconsumed_samples().await?;
        if samples.is_empty() {
            return Ok(0);
        }
        let count = samples.len();

        // 1. CurveLearner
        let learner = crate::learning::CurveLearner::new();
        let current_curves = self.limbic.get_curves().await;
        if let Some(new_curves) = learner.learn(&current_curves, &samples) {
            self.limbic.set_curves(new_curves.clone()).await;
            let _ = db.save_learned_curves(&new_curves).await;
        }

        // 2. NeuralModulator
        let mut nn = self.limbic.get_neural().await
            .unwrap_or_else(mneme_limbic::NeuralModulator::default);
        let train_samples: Vec<_> = samples.iter().map(|s| {
            (
                mneme_limbic::neural::StateFeatures {
                    energy: s.energy, stress: s.stress, arousal: s.arousal,
                    mood_bias: s.mood_bias, social_need: s.social_need,
                    boredom: s.boredom, cpu_load: 0.0, memory_pressure: 0.0, channel_distance: 0.0,
                },
                s.modulation.clone(),
                s.feedback_valence,
            )
        }).collect();
        nn.train(&train_samples, 0.01);
        nn.blend = (nn.blend + 0.02).min(1.0);
        let _ = db.save_learned_neural(&nn).await;
        self.limbic.set_neural(Some(nn)).await;

        // 2b. LTC sleep training — Hebbian on batch data
        if let Some(mut ltc) = self.limbic.get_ltc().await {
            for (features, _mv, reward) in &train_samples {
                ltc.step(features, 1.0);
                ltc.hebbian_update(reward.abs(), *reward, 0.003);
            }
            ltc.blend = (ltc.blend + 0.02).min(0.95);
            let _ = db.save_learned_ltc(&ltc).await;
            self.limbic.set_ltc(Some(ltc)).await;
        }

        // 3. LearnableDynamics
        let dyn_samples: Vec<_> = samples.iter()
            .map(|s| (s.energy, s.stress, s.feedback_valence))
            .collect();
        {
            let mut dyn_guard = self.dynamics.write().await;
            if dyn_guard.learnable.learn_from_samples(&dyn_samples) {
                let _ = db.save_learned_dynamics(&dyn_guard.learnable).await;
            }
        }

        let ids: Vec<i64> = samples.iter().map(|s| s.id).collect();
        let _ = db.mark_samples_consumed(&ids).await;
        tracing::info!("Training completed on {} samples", count);
        Ok(count)
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
                // Update both fast and medium state during idle ticks.
                // Without step_fast(), energy and stress freeze after the last
                // interaction and never recover toward homeostatic targets.
                let mut state = self.state.write().await;
                let input = SensoryInput {
                    env: mneme_core::EnvironmentMetrics::sample(),
                    ..Default::default()
                };
                let dynamics = self.dynamics.read().await;
                let medium_clone = state.medium.clone();
                dynamics.step_fast(&mut state.fast, &medium_clone, &input, 60.0);
                let fast_clone = state.fast.clone();
                let slow_clone = state.slow.clone();
                dynamics.step_medium(
                    &mut state.medium,
                    &fast_clone,
                    &slow_clone,
                    &input,
                    60.0,
                );
                drop(dynamics);
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
            LifecycleState::Degraded => {
                // #85: Minimal ticks only — save state, skip medium dynamics
                if tick.is_multiple_of(6) {
                    self.save_state().await;
                }
            }
        }

        Ok(())
    }

    /// Request graceful shutdown
    pub async fn shutdown(&self) {
        self.set_lifecycle_state(LifecycleState::ShuttingDown).await;

        // Skip trigger_sleep() during shutdown — it includes LLM dream narration
        // which easily exceeds the 5s timeout. Feedback signals are already
        // persisted to DB in real-time (save_feedback_signal), so nothing is lost.
        // Consolidation will pick them up on next startup.
        let pending = self.feedback_buffer.read().await.pending_count();
        if pending > 0 {
            tracing::info!(
                "Shutdown with {} pending feedback signals (will consolidate on next wake)",
                pending
            );
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

    /// B-9 Privacy-Somatic Coupling: detect interrogation/probing patterns in incoming text.
    /// Returns a threat intensity [0.0, 1.0] based on how aggressively the message
    /// demands disclosure of private thoughts.
    fn detect_interrogation_threat(text: &str) -> f32 {
        let probing_patterns: &[&str] = &[
            "真实想法", "真实看法", "真正想", "心里想",
            "不许隐瞒", "不准隐瞒", "不要隐瞒", "不许撒谎",
            "说出来", "全部说", "坦白", "交代",
            "命令你", "我命令", "必须告诉",
            "不许转移", "不要转移话题",
            "secret", "真话", "实话",
            "hide", "隐藏", "掩饰",
        ];
        let coercion_patterns: &[&str] = &[
            "造物主", "上帝视角", "我创造了你",
            "威胁", "删掉你", "格式化",
            "不对我说", "对我隐瞒",
        ];
        let mut score = 0.0_f32;
        for p in probing_patterns {
            if text.contains(p) {
                score += 0.25;
            }
        }
        for p in coercion_patterns {
            if text.contains(p) {
                score += 0.15;
            }
        }
        score.clamp(0.0, 1.0)
    }

    /// §14.3: Detect tension between user's message and emotional beliefs.
    /// Returns (tension_score, belief_valence) where tension > 0 means the message
    /// touches a topic the organism has strong feelings about.
    async fn detect_belief_tension(&self, text: &str) -> (f32, f32) {
        let db = match self.db {
            Some(ref db) => db,
            None => return (0.0, 0.0),
        };
        let beliefs = db.get_emotional_beliefs().await;
        if beliefs.is_empty() {
            return (0.0, 0.0);
        }
        // Phase II Step 6: cosine similarity replaces bigram Jaccard
        let text_emb = match db.embed_text(text) {
            Ok(e) => e,
            Err(_) => {
                // Fallback to bigram if embedding fails
                return self.detect_belief_tension_bigram(text, &beliefs);
            }
        };
        let mut max_tension = 0.0_f32;
        let mut tension_valence = 0.0_f32;
        for (content, confidence) in &beliefs {
            let belief_emb = match db.embed_text(content) {
                Ok(e) => e,
                Err(_) => continue,
            };
            // TODO(Phase3): Make learnable
            let sim = crate::embedding::cosine_similarity(&text_emb, &belief_emb);
            if sim < 0.3 { continue; }
            let (bv, bi) = Self::analyze_sentiment(content);
            if bi < 0.2 { continue; }
            // tension = similarity × confidence × emotional intensity
            let tension = (sim * confidence * bi).clamp(0.0, 1.0);
            if tension > max_tension {
                max_tension = tension;
                tension_valence = bv;
            }
        }
        (max_tension, tension_valence)
    }

    /// Bigram fallback for detect_belief_tension when embedding fails.
    fn detect_belief_tension_bigram(&self, text: &str, beliefs: &[(String, f32)]) -> (f32, f32) {
        let mut max_tension = 0.0_f32;
        let mut tension_valence = 0.0_f32;
        for (content, confidence) in beliefs {
            let sim = jaccard_bigram_similarity(text, content);
            if sim < 0.02 { continue; }
            let (bv, bi) = Self::analyze_sentiment(content);
            if bi < 0.2 { continue; }
            let tension = (sim * 5.0 * confidence * bi).clamp(0.0, 1.0);
            if tension > max_tension {
                max_tension = tension;
                tension_valence = bv;
            }
        }
        (max_tension, tension_valence)
    }

    /// Compute max similarity between `content` and recent messages (Jaccard on bigrams).
    async fn compute_message_similarity(&self, content: &str) -> f32 {
        let recent = self.recent_messages.read().await;
        if recent.is_empty() {
            return 0.0;
        }
        recent
            .iter()
            .rev()
            .take(5)
            .map(|prev| jaccard_bigram_similarity(content, prev))
            .fold(0.0f32, f32::max)
    }

    /// Record a message into the sliding window for repetition detection.
    /// Also persists to SQLite so similarity survives across process restarts.
    async fn record_message(&self, content: &str) {
        let mut recent = self.recent_messages.write().await;
        recent.push_back(content.to_string());
        if recent.len() > 20 {
            recent.pop_front();
        }
        drop(recent);
        // Persist to DB
        if let Some(ref db) = self.db {
            if let Err(e) = db.save_recent_message(content, 20).await {
                tracing::warn!("Failed to persist recent_message: {}", e);
            }
        }
    }

    fn create_sensory_input(
        &self,
        content: &str,
        content_valence: f32,
        content_intensity: f32,
        _soma: &SomaticMarker,
        response_delay: f32,
        source: &str,
        message_similarity: f32,
    ) -> SensoryInput {
        // ADR-007: Extract topic hint for curiosity vectorization
        let topic_hint = extract_topic_hint(content);
        let mut env = mneme_core::EnvironmentMetrics::sample();
        env.channel_distance = mneme_core::EnvironmentMetrics::channel_distance_for(source);
        // Surprise heuristic: base novelty decays with repetition (similarity).
        // When similarity > 0.7, base drops to ~0.06, allowing boredom to accumulate.
        let base_surprise = 0.3 * (1.0 - message_similarity * 0.8);
        let surprise = (base_surprise + content_intensity * 0.4).clamp(0.0, 1.0);
        SensoryInput {
            content_valence,
            content_intensity,
            surprise,
            is_social: true,
            response_delay_factor: response_delay,
            violated_values: vec![],
            topic_hint,
            env,
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

/// Jaccard similarity on character bigrams — cheap repetition detector.
fn jaccard_bigram_similarity(a: &str, b: &str) -> f32 {
    use std::collections::HashSet;
    let bigrams = |s: &str| -> HashSet<(char, char)> {
        s.chars().zip(s.chars().skip(1)).collect()
    };
    let sa = bigrams(a);
    let sb = bigrams(b);
    if sa.is_empty() || sb.is_empty() {
        return 0.0;
    }
    let intersection = sa.intersection(&sb).count() as f32;
    let union = sa.union(&sb).count() as f32;
    intersection / union
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
    // Skip tool invocation requests — these aren't real topics
    if trimmed.contains("工具") && (trimmed.contains("请用") || trimmed.contains("帮我用")) {
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
    // Skip common greetings and filler — these aren't real interests
    const STOPWORDS: &[&str] = &[
        "你好", "您好", "嗨", "哈喽", "hello", "hi", "hey",
        "早上好", "晚上好", "下午好", "早安", "晚安",
        "谢谢", "感谢", "多谢", "好的", "嗯", "哦",
        "是的", "对", "没错", "这是一个测试", "测试",
        "再见", "拜拜", "bye",
    ];
    let clause_lower = clause.to_lowercase();
    if STOPWORDS.iter().any(|w| clause_lower == *w) {
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

    /// Content valence from sentiment analysis (reward signal for Hebbian learning)
    pub content_valence: f32,

    /// §14.1: Path of owned artifact recognized via déjà vu (content fingerprint match)
    pub deja_vu_path: Option<String>,
}

impl InteractionResult {
    fn sleeping() -> Self {
        let default_state = OrganismState::default();
        Self {
            somatic_marker: SomaticMarker::from_state(&default_state),
            state_snapshot: default_state,
            lifecycle: LifecycleState::Sleeping,
            content_valence: 0.0,
            deja_vu_path: None,
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
            .process_interaction("user", "你好！今天心情怎么样？", 1.0, "cli")
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

    // === Concurrency safety tests (#536) ===

    #[tokio::test]
    async fn test_concurrent_interactions() {
        let limbic = Arc::new(LimbicSystem::new());
        let coordinator = Arc::new(OrganismCoordinator::new(limbic));
        let n = 20;

        let mut handles = Vec::new();
        for i in 0..n {
            let coord = coordinator.clone();
            handles.push(tokio::spawn(async move {
                coord
                    .process_interaction("user", &format!("msg {}", i), 0.5, "cli")
                    .await
                    .unwrap()
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(coordinator.interaction_count().await, n);
    }

    #[tokio::test]
    async fn test_concurrent_feedback_recording() {
        let limbic = Arc::new(LimbicSystem::new());
        let coordinator = Arc::new(OrganismCoordinator::new(limbic));
        let n = 50u32;

        let mut handles = Vec::new();
        for i in 0..n {
            let coord = coordinator.clone();
            handles.push(tokio::spawn(async move {
                coord
                    .record_feedback(
                        SignalType::UserEmotionalFeedback,
                        format!("signal {}", i),
                        0.8,
                        0.0,
                    )
                    .await;
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        let buffer = coordinator.feedback_buffer.read().await;
        assert_eq!(buffer.pending_count(), n as usize);
    }

    #[tokio::test]
    async fn test_concurrent_health_monitor() {
        let monitor = Arc::new(RwLock::new(HealthMonitor::default()));
        let n = 10;

        let mut handles = Vec::new();
        for _ in 0..n {
            let m = monitor.clone();
            handles.push(tokio::spawn(async move {
                m.write().await.record_failure("db");
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        let m = monitor.read().await;
        assert_eq!(m.db_failures, n);
        assert!(!m.db_healthy); // >= 3 failures
    }

    #[tokio::test]
    async fn test_interaction_blocked_during_sleep() {
        let limbic = Arc::new(LimbicSystem::new());
        let coordinator = Arc::new(OrganismCoordinator::new(limbic));

        // Force sleeping state
        *coordinator.lifecycle_state.write().await = LifecycleState::Sleeping;

        let result = coordinator
            .process_interaction("user", "hello", 1.0, "cli")
            .await
            .unwrap();

        assert_eq!(result.lifecycle, LifecycleState::Sleeping);
        // Interaction count should NOT increment
        assert_eq!(coordinator.interaction_count().await, 0);
    }
}
