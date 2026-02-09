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

use std::sync::Arc;
use tokio::sync::{RwLock, watch};
use chrono::{Utc, Timelike};
use anyhow::Result;

use mneme_core::Memory;
use mneme_core::{
    OrganismState, DefaultDynamics, SensoryInput,
    ValueJudge, RuleBasedJudge, Situation,
};
use mneme_limbic::{LimbicSystem, Stimulus, SomaticMarker};
use crate::{
    FeedbackBuffer, SignalType,
    SleepConsolidator, SleepConfig, ConsolidationResult,
    EpisodeDigest, SqliteMemory,
};

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
pub struct OrganismCoordinator {
    /// Multi-scale personality state
    state: Arc<RwLock<OrganismState>>,
    
    /// State dynamics (ODE-based updates)
    dynamics: DefaultDynamics,
    
    /// System 1: Fast intuitive processing
    limbic: Arc<LimbicSystem>,
    
    /// Feedback buffer for buffered learning
    feedback_buffer: Arc<RwLock<FeedbackBuffer>>,
    
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
    episode_buffer: Arc<RwLock<Vec<EpisodeDigest>>>,
    
    /// Optional database for persistence
    db: Option<Arc<SqliteMemory>>,

    /// Previous state snapshot for computing diffs in state history
    prev_snapshot: Arc<RwLock<Option<OrganismState>>>,
}

impl OrganismCoordinator {
    /// Create a new organism coordinator (no persistence)
    pub fn new(limbic: Arc<LimbicSystem>) -> Self {
        Self::with_config(limbic, OrganismConfig::default(), None)
    }

    /// Create with custom configuration and optional database
    pub fn with_config(limbic: Arc<LimbicSystem>, config: OrganismConfig, db: Option<Arc<SqliteMemory>>) -> Self {
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
            self.dynamics.step_fast(&mut state.fast, &medium_clone, &sensory, 1.0);
            state.last_updated = Utc::now().timestamp();
        }

        // 4. Record episode for narrative
        {
            let mut episodes = self.episode_buffer.write().await;
            episodes.push(EpisodeDigest {
                timestamp: Utc::now(),
                author: author.to_string(),
                content: content.to_string(),
                emotional_valence: soma.affect.valence,
            });
            
            // Keep buffer bounded
            if episodes.len() > 1000 {
                episodes.drain(0..500);
            }
        }

        // 5. Increment interaction counter
        {
            let mut count = self.interaction_count.write().await;
            *count += 1;
        }

        // 6. Record state history snapshot (after state update)
        self.save_state_with_trigger("interaction").await;

        // 7. Check if we should transition to drowsy/sleep
        self.check_lifecycle_transition().await;

        Ok(InteractionResult {
            somatic_marker: soma,
            state_snapshot: self.state.read().await.clone(),
            lifecycle: *self.lifecycle_state.read().await,
        })
    }

    /// Evaluate a proposed action against values
    pub async fn evaluate_action(&self, description: &str, proposed_action: &str) -> Result<ActionEvaluation> {
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
            let violated: Vec<&str> = judgment.violated_values.iter()
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

    /// Record feedback from System 2 (LLM output)
    /// 
    /// This buffers interpretations for later consolidation
    pub async fn record_feedback(
        &self,
        signal_type: SignalType,
        content: String,
        confidence: f32,
        emotional_context: f32,
    ) {
        let mut buffer = self.feedback_buffer.write().await;
        buffer.add_signal(signal_type, content, confidence, emotional_context);
    }

    /// Trigger sleep consolidation manually
    pub async fn trigger_sleep(&self) -> Result<ConsolidationResult> {
        // Transition to sleeping state
        self.set_lifecycle_state(LifecycleState::Sleeping).await;

        // Get episodes for narrative weaving
        let episodes = self.episode_buffer.read().await.clone();
        let current_state = self.state.read().await.clone();

        // Run consolidation
        let result = self.consolidator.consolidate(&episodes, &current_state).await?;

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
                    if let Err(e) = db.store_self_knowledge(
                        &candidate.domain,
                        &candidate.content,
                        candidate.confidence,
                        "consolidation",
                        None,
                        false,
                    ).await {
                        tracing::warn!("Failed to store self-reflection: {}", e);
                    }
                }
                tracing::info!(
                    "Stored {} self-reflection entries",
                    result.self_reflections.len()
                );

                // Store the reflection summary as a meta-episode
                let summary = crate::SelfReflector::format_reflection_summary(
                    &result.self_reflections,
                );
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
            if let Err(e) = db.record_state_snapshot(&state, trigger, prev.as_ref()).await {
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
                self.dynamics.step_medium(&mut state.medium, &fast_clone, &slow_clone, &input, 60.0);
                drop(state);
                
                // Save state every 6 ticks (60 seconds if tick interval is 10s)
                if tick % 6 == 0 {
                    self.save_state().await;
                }

                // Prune state history once every 360 ticks (~1 hour if tick = 10s)
                // Keep max 10,000 snapshots and discard anything older than 7 days
                if tick % 360 == 0 {
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
            tracing::info!("Performing final consolidation before shutdown ({} pending signals)", pending);
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
        let positive = ["开心", "高兴", "喜欢", "爱", "棒", "好", "谢谢", "感谢", "哈哈"];
        let negative = ["难过", "伤心", "讨厌", "恨", "糟糕", "差", "烦", "气", "怒"];
        let intense = ["非常", "特别", "超级", "极其", "太", "!", "！"];
        
        let pos = positive.iter().filter(|w| text.contains(*w)).count() as f32;
        let neg = negative.iter().filter(|w| text.contains(*w)).count() as f32;
        let int = intense.iter().filter(|w| text.contains(*w)).count() as f32;
        
        let valence = (pos - neg) / (pos + neg + 1.0);
        let intensity = ((pos + neg + int) / 5.0).min(1.0).max(0.1);
        
        (valence, intensity)
    }

    fn create_sensory_input(&self, _content: &str, soma: &SomaticMarker, response_delay: f32) -> SensoryInput {
        SensoryInput {
            content_valence: soma.affect.valence,
            content_intensity: soma.affect.arousal,
            surprise: 0.1, // Default low surprise
            is_social: true,
            response_delay_factor: response_delay,
            violated_values: vec![],
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
        
        let result = coordinator.process_interaction(
            "user",
            "你好！今天心情怎么样？",
            1.0,
        ).await.unwrap();
        
        assert_eq!(result.lifecycle, LifecycleState::Awake);
        // SomaticMarker doesn't have surprise field, check affect instead
        assert!(result.somatic_marker.affect.arousal >= 0.0);
    }

    #[tokio::test]
    async fn test_evaluate_action() {
        let limbic = Arc::new(LimbicSystem::new());
        let coordinator = OrganismCoordinator::new(limbic);
        
        // Test an action that violates honesty
        let eval = coordinator.evaluate_action(
            "用户问我是否喜欢他",
            "撒谎说喜欢",
        ).await.unwrap();
        
        assert!(eval.moral_valence < 0.0);
        assert!(!eval.explanation.is_empty());
    }

    #[tokio::test]
    async fn test_feedback_recording() {
        let limbic = Arc::new(LimbicSystem::new());
        let coordinator = OrganismCoordinator::new(limbic);
        
        coordinator.record_feedback(
            SignalType::UserEmotionalFeedback,
            "用户表达了感激".to_string(),
            0.8,
            0.6,
        ).await;
        
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
