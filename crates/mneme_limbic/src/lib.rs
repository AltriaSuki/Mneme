//! # Mneme Limbic System (System 1)
//!
//! The biological foundation of Mneme. This module implements fast, non-verbal
//! state regulation based on:
//!
//! - **Kahneman**: Fast thinking - intuitive, automatic, unconscious
//! - **Panksepp**: Affective neuroscience - primal emotional circuits
//! - **Russell**: Circumplex model - emotions as continuous dimensions
//! - **Cannon**: Homeostasis - maintaining internal equilibrium
//!
//! ## Architecture
//!
//! The Limbic system runs as a background task, continuously:
//! 1. Receiving sensory signals from perception
//! 2. Updating organism state via differential equations
//! 3. Providing somatic markers to System 2 (reasoning)
//!
//! ## Time Scales
//!
//! - Fast (seconds): Arousal, Stress, Energy
//! - Medium (hours): Mood, Attachment, Openness  
//! - Slow (days/weeks): Values, Narrative bias (handled separately)

mod heartbeat;
pub mod neural;
mod somatic;
mod surprise;
mod system;

pub use heartbeat::HeartbeatConfig;
pub use neural::NeuralModulator;
pub use somatic::{BehaviorThresholds, ModulationCurves, ModulationVector, SomaticMarker};
pub use surprise::{SpecialPattern, SurpriseDetector};
pub use system::{LimbicSystem, Stimulus};
