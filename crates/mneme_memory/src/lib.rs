pub mod sqlite;
pub mod embedding;
pub mod feedback_buffer;
pub mod narrative;
pub mod consolidation;
pub mod coordinator;

pub use sqlite::SqliteMemory;
pub use feedback_buffer::{FeedbackBuffer, FeedbackSignal, SignalType, ConsolidatedPattern, StateUpdates};
pub use narrative::{NarrativeWeaver, NarrativeChapter, EpisodeDigest, CrisisEvent};
pub use consolidation::{SleepConsolidator, SleepConfig, ConsolidationResult};
pub use coordinator::{OrganismCoordinator, OrganismConfig, LifecycleState, InteractionResult, ActionEvaluation};

#[cfg(test)]
mod tests;
