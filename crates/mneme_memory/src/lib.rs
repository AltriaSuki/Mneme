pub mod consolidation;
pub mod coordinator;
pub mod dream;
pub mod embedding;
pub mod feedback_buffer;
pub mod goals;
pub mod learning;
pub mod narrative;
pub mod rules;
pub mod sqlite;

pub use consolidation::{
    ConsolidationResult, SelfKnowledgeCandidate, SelfReflector, SleepConfig, SleepConsolidator,
};
pub use coordinator::{
    ActionEvaluation, InteractionResult, LifecycleState, OrganismConfig, OrganismCoordinator,
};
pub use dream::{DreamEpisode, DreamGenerator};
pub use feedback_buffer::{
    ConsolidatedPattern, FeedbackBuffer, FeedbackSignal, SignalType, StateUpdates,
};
pub use goals::{Goal, GoalManager, GoalStatus, GoalTriggerEvaluator, GoalType};
pub use learning::{CurveLearner, ModulationSample};
pub use narrative::{CrisisEvent, EpisodeDigest, NarrativeChapter, NarrativeWeaver};
pub use rules::{BehaviorRule, RuleAction, RuleCondition, RuleContext, RuleEngine, RuleTrigger};
pub use sqlite::DreamSeed;
pub use sqlite::SelfKnowledge;
pub use sqlite::SemanticFact;
pub use sqlite::SqliteMemory;
pub use sqlite::StateSnapshot;

#[cfg(test)]
mod tests;
