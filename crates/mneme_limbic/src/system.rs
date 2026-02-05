//! Core Limbic System implementation
//!
//! The LimbicSystem is the central coordinator for System 1. It:
//! - Maintains the OrganismState
//! - Runs continuous state evolution (heartbeat)
//! - Processes incoming stimuli
//! - Provides state snapshots for System 2

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, mpsc, watch};
use mneme_core::{
    OrganismState, SensoryInput, DefaultDynamics, Dynamics,
    Affect, FastState,
};
use crate::somatic::SomaticMarker;
use crate::surprise::SurpriseDetector;
use crate::heartbeat::HeartbeatConfig;

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
        self.stimulus_tx.send(stimulus).await
            .map_err(|e| anyhow::anyhow!("Failed to send stimulus: {}", e))
    }

    /// Get the current somatic marker (for System 2 context injection)
    pub async fn get_somatic_marker(&self) -> SomaticMarker {
        self.state_watch_rx.borrow().clone()
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

    /// Check if the system needs social interaction (proactivity trigger)
    pub async fn needs_social_interaction(&self) -> bool {
        let state = self.state.read().await;
        state.fast.social_need > 0.7
    }

    /// Check if the system is stressed (may need calming)
    pub async fn is_stressed(&self) -> bool {
        let state = self.state.read().await;
        state.fast.stress > 0.7
    }

    /// Check if energy is low (may need rest)
    pub async fn is_tired(&self) -> bool {
        let state = self.state.read().await;
        state.fast.energy < 0.3
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
}
