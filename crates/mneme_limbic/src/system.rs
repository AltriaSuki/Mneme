//! Core Limbic System implementation
//!
//! The LimbicSystem is the central coordinator for System 1. It:
//! - Maintains the OrganismState
//! - Runs continuous state evolution (heartbeat)
//! - Processes incoming stimuli
//! - Provides state snapshots for System 2

use crate::heartbeat::HeartbeatConfig;
use crate::somatic::{BehaviorThresholds, ModulationCurves, ModulationVector, SomaticMarker};
use crate::surprise::SurpriseDetector;
use mneme_core::{Affect, DefaultDynamics, Dynamics, FastState, OrganismState, SensoryInput};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch, RwLock};

/// Stimulus received by the limbic system
#[derive(Debug, Clone)]
pub struct Stimulus {
    /// Emotional valence of the content (-1.0 to 1.0)
    pub valence: f32,
    /// Intensity/arousal (0.0 to 1.0)
    pub intensity: f32,
    /// Is this a social interaction?
    pub is_social: bool,
    /// Raw content for surprise detection
    pub content: String,
    /// Values potentially violated
    pub violated_values: Vec<String>,
}

impl Default for Stimulus {
    fn default() -> Self {
        Self {
            valence: 0.0,
            intensity: 0.1,
            is_social: false,
            content: String::new(),
            violated_values: Vec::new(),
        }
    }
}

/// The Limbic System - System 1 of the dual-brain architecture
pub struct LimbicSystem {
    /// Current organism state (protected by RwLock for concurrent access)
    state: Arc<RwLock<OrganismState>>,

    /// Dynamics engine for state evolution
    dynamics: Arc<DefaultDynamics>,

    /// Surprise detector for predictive coding
    surprise_detector: Arc<RwLock<SurpriseDetector>>,

    /// Channel to send stimuli
    stimulus_tx: mpsc::Sender<Stimulus>,

    /// Watch channel for state updates (System 2 subscribes to this)
    state_watch_tx: watch::Sender<SomaticMarker>,

    /// Receiver for state updates (cloneable)
    state_watch_rx: watch::Receiver<SomaticMarker>,

    /// Heartbeat configuration
    heartbeat_config: HeartbeatConfig,

    /// Last interaction timestamp (for social need calculation)
    last_interaction: Arc<RwLock<Instant>>,

    /// Previous modulation vector for temporal smoothing (emotion inertia).
    /// Emotions don't switch instantly — they have momentum.
    prev_modulation: Arc<RwLock<ModulationVector>>,

    /// Smoothing factor for modulation lerp (0.0 = frozen, 1.0 = instant).
    /// Lower values = heavier inertia. This is a 🧬 personality parameter.
    modulation_smoothing: f32,

    /// Surprise threshold: if max_delta between prev and current exceeds this,
    /// bypass smoothing and jump directly (startle response).
    surprise_bypass_threshold: f32,

    /// Learnable modulation curves — how state maps to LLM parameters.
    /// Different Mneme instances can have different curves (sensitive vs resilient).
    /// Protected by RwLock so offline learning can update curves via `&self`.
    curves: RwLock<ModulationCurves>,

    /// Learnable behavior thresholds — when specific behaviors trigger.
    /// Protected by RwLock so offline learning can update thresholds via `&self`.
    thresholds: RwLock<BehaviorThresholds>,
}

impl LimbicSystem {
    /// Create a new limbic system with default configuration
    pub fn new() -> Self {
        Self::with_config(HeartbeatConfig::default(), DefaultDynamics::default())
    }

    /// Create with custom configuration
    pub fn with_config(heartbeat_config: HeartbeatConfig, dynamics: DefaultDynamics) -> Self {
        let (stimulus_tx, stimulus_rx) = mpsc::channel(64);
        let initial_marker = SomaticMarker::from_state(&OrganismState::default());
        let (state_watch_tx, state_watch_rx) = watch::channel(initial_marker);

        let system = Self {
            state: Arc::new(RwLock::new(OrganismState::default())),
            dynamics: Arc::new(dynamics),
            surprise_detector: Arc::new(RwLock::new(SurpriseDetector::new())),
            stimulus_tx,
            state_watch_tx,
            state_watch_rx,
            heartbeat_config,
            last_interaction: Arc::new(RwLock::new(Instant::now())),
            prev_modulation: Arc::new(RwLock::new(ModulationVector::default())),
            modulation_smoothing: 0.3,      // moderate inertia by default
            surprise_bypass_threshold: 0.5, // large jumps bypass smoothing
            curves: RwLock::new(ModulationCurves::default()),
            thresholds: RwLock::new(BehaviorThresholds::default()),
        };

        // Spawn the heartbeat task
        system.spawn_heartbeat(stimulus_rx);

        system
    }

    /// Spawn the background heartbeat task
    fn spawn_heartbeat(&self, mut stimulus_rx: mpsc::Receiver<Stimulus>) {
        let state = Arc::clone(&self.state);
        let dynamics = Arc::clone(&self.dynamics);
        let surprise_detector = Arc::clone(&self.surprise_detector);
        let state_watch_tx = self.state_watch_tx.clone();
        let heartbeat_interval = self.heartbeat_config.interval;
        let last_interaction = Arc::clone(&self.last_interaction);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(heartbeat_interval);
            let mut last_tick = Instant::now();

            loop {
                tokio::select! {
                    // Regular heartbeat
                    _ = interval.tick() => {
                        let now = Instant::now();
                        let dt = now.duration_since(last_tick);
                        last_tick = now;

                        // Calculate time since last interaction for social need
                        let time_alone = {
                            let last = last_interaction.read().await;
                            now.duration_since(*last)
                        };

                        // Create baseline input (no external stimulus)
                        let mut input = SensoryInput::default();

                        // Social need grows when alone
                        if time_alone > Duration::from_secs(300) {
                            // After 5 minutes alone, social need starts growing
                            input.response_delay_factor = (time_alone.as_secs_f32() / 300.0).min(3.0);
                        }

                        // Update state
                        {
                            let mut state_guard = state.write().await;
                            dynamics.step(&mut state_guard, &input, dt);

                            // Broadcast new somatic marker
                            let marker = SomaticMarker::from_state(&state_guard);
                            let _ = state_watch_tx.send(marker);
                        }
                    }

                    // External stimulus received
                    Some(stimulus) = stimulus_rx.recv() => {
                        let now = Instant::now();
                        let dt = now.duration_since(last_tick);
                        last_tick = now;

                        // Update last interaction time if social
                        if stimulus.is_social {
                            let mut last = last_interaction.write().await;
                            *last = now;
                        }

                        // Compute surprise score
                        let surprise = {
                            let mut detector = surprise_detector.write().await;
                            detector.compute_surprise(&stimulus.content)
                        };

                        // Build sensory input
                        let input = SensoryInput {
                            content_valence: stimulus.valence,
                            content_intensity: stimulus.intensity,
                            surprise,
                            is_social: stimulus.is_social,
                            response_delay_factor: 1.0,
                            violated_values: stimulus.violated_values,
                            topic_hint: None,
                        };

                        // Update state
                        {
                            let mut state_guard = state.write().await;
                            dynamics.step(&mut state_guard, &input, dt);

                            // Apply moral cost if values violated
                            if !input.violated_values.is_empty() {
                                let cost = state_guard.slow.values.compute_moral_cost(
                                    &input.violated_values.iter().map(|s| s.as_str()).collect::<Vec<_>>()
                                );
                                dynamics.apply_moral_cost(&mut state_guard.fast, cost);
                                tracing::debug!("Applied moral cost: {:.2}", cost);
                            }

                            // Broadcast new somatic marker
                            let marker = SomaticMarker::from_state(&state_guard);
                            let _ = state_watch_tx.send(marker);
                        }

                        tracing::trace!(
                            "Processed stimulus: valence={:.2}, intensity={:.2}, surprise={:.2}",
                            stimulus.valence, stimulus.intensity, surprise
                        );
                    }
                }
            }
        });
    }

    /// Send a stimulus to the limbic system
    pub async fn receive_stimulus(&self, stimulus: Stimulus) -> anyhow::Result<()> {
        self.stimulus_tx
            .send(stimulus)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send stimulus: {}", e))
    }

    /// Get the current somatic marker (for System 2 context injection)
    pub async fn get_somatic_marker(&self) -> SomaticMarker {
        self.state_watch_rx.borrow().clone()
    }

    /// Get a temporally-smoothed modulation vector (emotion inertia).
    ///
    /// Instead of snapping instantly to the new emotional state, this lerps
    /// between the previous modulation and the current one. This models the
    /// biological reality that emotions have momentum — you don't go from
    /// laughing to crying in a single frame.
    ///
    /// Exception: if the jump is larger than `surprise_bypass_threshold`,
    /// we skip smoothing (startle response / sudden shock).
    pub async fn get_modulation_vector(&self) -> ModulationVector {
        let marker = self.get_somatic_marker().await;
        let curves = self.curves.read().await;
        let thresholds = self.thresholds.read().await;
        let raw = marker.to_modulation_vector_full(&curves, &thresholds);
        drop(curves);
        drop(thresholds);

        let mut prev = self.prev_modulation.write().await;
        let delta = prev.max_delta(&raw);

        let smoothed = if delta > self.surprise_bypass_threshold {
            // Large jump — bypass smoothing (startle response)
            raw.clone()
        } else {
            prev.lerp(&raw, self.modulation_smoothing)
        };

        *prev = smoothed.clone();
        smoothed
    }

    /// Subscribe to state updates
    pub fn subscribe(&self) -> watch::Receiver<SomaticMarker> {
        self.state_watch_rx.clone()
    }

    /// Get a snapshot of the full organism state
    pub async fn get_state(&self) -> OrganismState {
        self.state.read().await.clone()
    }

    /// Get the current affect
    pub async fn get_affect(&self) -> Affect {
        self.state.read().await.fast.affect
    }

    /// Get the current fast state
    pub async fn get_fast_state(&self) -> FastState {
        self.state.read().await.fast.clone()
    }

    /// Force a state update (for testing or manual intervention)
    pub async fn set_state(&self, new_state: OrganismState) {
        let mut state = self.state.write().await;
        *state = new_state;
        let marker = SomaticMarker::from_state(&state);
        let _ = self.state_watch_tx.send(marker);
    }

    /// Update predictions for surprise detection
    pub async fn update_prediction(&self, expected_response: &str) {
        let mut detector = self.surprise_detector.write().await;
        detector.set_prediction(expected_response);
    }

    /// Get a clone of the current modulation curves
    pub async fn get_curves(&self) -> ModulationCurves {
        self.curves.read().await.clone()
    }

    /// Set new modulation curves (e.g., loaded from persistence or learned)
    pub async fn set_curves(&self, curves: ModulationCurves) {
        *self.curves.write().await = curves;
    }

    /// Get a clone of the current behavior thresholds
    pub async fn get_thresholds(&self) -> BehaviorThresholds {
        self.thresholds.read().await.clone()
    }

    /// Set new behavior thresholds (e.g., loaded from persistence or learned)
    pub async fn set_thresholds(&self, thresholds: BehaviorThresholds) {
        *self.thresholds.write().await = thresholds;
    }

    /// Check if the system needs social interaction (proactivity trigger)
    pub async fn needs_social_interaction(&self) -> bool {
        let state = self.state.read().await;
        let t = self.thresholds.read().await;
        state.fast.social_need > t.attention_social
    }

    /// Check if the system is stressed (may need calming)
    pub async fn is_stressed(&self) -> bool {
        let state = self.state.read().await;
        let t = self.thresholds.read().await;
        state.fast.stress > t.attention_stress
    }

    /// Check if energy is low (may need rest)
    pub async fn is_tired(&self) -> bool {
        let state = self.state.read().await;
        let t = self.thresholds.read().await;
        state.fast.energy < t.attention_energy
    }
}

impl Default for LimbicSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_limbic_system_creation() {
        let limbic = LimbicSystem::new();
        let marker = limbic.get_somatic_marker().await;
        assert!(marker.energy > 0.5);
    }

    #[tokio::test]
    async fn test_stimulus_processing() {
        let limbic = LimbicSystem::new();

        // Initial state
        let initial_stress = limbic.get_fast_state().await.stress;

        // Send negative stimulus
        let stimulus = Stimulus {
            valence: -0.8,
            intensity: 0.9,
            is_social: true,
            content: "I'm very angry at you!".to_string(),
            violated_values: vec![],
        };

        limbic.receive_stimulus(stimulus).await.unwrap();

        // Wait for processing
        sleep(Duration::from_millis(100)).await;

        // Stress should have increased
        let new_stress = limbic.get_fast_state().await.stress;
        assert!(new_stress > initial_stress);
    }

    #[tokio::test]
    async fn test_social_need() {
        let config = HeartbeatConfig {
            interval: Duration::from_millis(10),
        };
        let limbic = LimbicSystem::with_config(config, DefaultDynamics::default());

        // Should not need social interaction initially
        assert!(!limbic.needs_social_interaction().await);
    }

    #[tokio::test]
    async fn test_set_and_get_state() {
        let limbic = LimbicSystem::new();

        let mut custom_state = OrganismState::default();
        custom_state.fast.energy = 0.2;
        custom_state.fast.stress = 0.9;

        limbic.set_state(custom_state.clone()).await;

        let retrieved = limbic.get_state().await;
        assert!((retrieved.fast.energy - 0.2).abs() < 1e-6);
        assert!((retrieved.fast.stress - 0.9).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_is_stressed_threshold() {
        let limbic = LimbicSystem::new();

        // Default stress is low
        assert!(!limbic.is_stressed().await);

        // Set high stress
        let mut state = limbic.get_state().await;
        state.fast.stress = 0.8;
        limbic.set_state(state).await;
        assert!(limbic.is_stressed().await);
    }

    #[tokio::test]
    async fn test_is_tired_threshold() {
        let limbic = LimbicSystem::new();

        // Default energy is high
        assert!(!limbic.is_tired().await);

        // Set low energy
        let mut state = limbic.get_state().await;
        state.fast.energy = 0.1;
        limbic.set_state(state).await;
        assert!(limbic.is_tired().await);
    }

    #[tokio::test]
    async fn test_get_affect() {
        let limbic = LimbicSystem::new();
        let affect = limbic.get_affect().await;
        // Default affect should be near neutral
        assert!(affect.valence.abs() < 0.5);
        assert!(affect.arousal >= 0.0 && affect.arousal <= 1.0);
    }

    #[tokio::test]
    async fn test_subscribe_receives_updates() {
        let limbic = LimbicSystem::new();
        let mut rx = limbic.subscribe();

        // Set a distinct state
        let mut state = OrganismState::default();
        state.fast.energy = 0.1;
        limbic.set_state(state).await;

        // Subscriber should see the update
        rx.changed().await.unwrap();
        let marker = rx.borrow().clone();
        assert!((marker.energy - 0.1).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_modulation_vector_valid_ranges() {
        let limbic = LimbicSystem::new();
        let modulation = limbic.get_modulation_vector().await;

        assert!(modulation.max_tokens_factor > 0.0);
        assert!(modulation.temperature_delta.is_finite());
        assert!(modulation.context_budget_factor > 0.0);
        assert!(modulation.silence_inclination >= 0.0 && modulation.silence_inclination <= 1.0);
    }

    #[tokio::test]
    async fn test_curves_get_set() {
        let limbic = LimbicSystem::new();
        let original = limbic.get_curves().await;

        let mut modified = original.clone();
        modified.energy_to_max_tokens.0 = 99.0;
        limbic.set_curves(modified.clone()).await;

        let retrieved = limbic.get_curves().await;
        assert!((retrieved.energy_to_max_tokens.0 - 99.0).abs() < 1e-6);
    }
}
