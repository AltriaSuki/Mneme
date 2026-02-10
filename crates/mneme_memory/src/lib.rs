pub mod sqlite;
pub mod embedding;
pub mod feedback_buffer;
pub mod narrative;
pub mod consolidation;
pub mod coordinator;
pub mod learning;
pub mod dream;

pub use sqlite::SqliteMemory;
pub use sqlite::SemanticFact;
pub use sqlite::SelfKnowledge;
pub use sqlite::StateSnapshot;
pub use feedback_buffer::{FeedbackBuffer, FeedbackSignal, SignalType, ConsolidatedPattern, StateUpdates};
pub use narrative::{NarrativeWeaver, NarrativeChapter, EpisodeDigest, CrisisEvent};
pub use consolidation::{SleepConsolidator, SleepConfig, ConsolidationResult, SelfReflector, SelfKnowledgeCandidate};
pub use coordinator::{OrganismCoordinator, OrganismConfig, LifecycleState, InteractionResult, ActionEvaluation};
pub use learning::{CurveLearner, ModulationSample};
pub use dream::{DreamGenerator, DreamEpisode};
pub use sqlite::DreamSeed;

#[cfg(test)]
mod tests;
