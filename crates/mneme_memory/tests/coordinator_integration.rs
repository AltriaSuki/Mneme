//! Integration tests for OrganismCoordinator
//!
//! Uses tempfile::TempDir for isolated SQLite databases.

use std::sync::Arc;
use mneme_limbic::LimbicSystem;
use mneme_memory::{
    OrganismCoordinator, OrganismConfig, LifecycleState,
    SqliteMemory, SignalType,
    GoalManager, Goal, GoalType, GoalStatus,
    RuleEngine, RuleContext, RuleTrigger,
};

async fn setup_coordinator(dir: &tempfile::TempDir) -> (Arc<OrganismCoordinator>, Arc<SqliteMemory>) {
    let db_path = dir.path().join("test.db");
    let db = Arc::new(SqliteMemory::new(db_path.to_str().unwrap()).await.unwrap());
    let limbic = Arc::new(LimbicSystem::new());
    let mut config = OrganismConfig::default();
    config.sleep_config.allow_manual_trigger = true;
    let coord = Arc::new(
        OrganismCoordinator::with_persistence(limbic, config, db.clone()).await.unwrap()
    );
    (coord, db)
}

/// Test 1: Concurrent process_interaction + trigger_sleep don't lose state
#[tokio::test]
async fn test_concurrent_interaction_and_sleep() {
    let dir = tempfile::TempDir::new().unwrap();
    let (coord, _db) = setup_coordinator(&dir).await;

    // Populate episodes via interactions (public API)
    for i in 0..15 {
        let _ = coord.process_interaction("user", &format!("msg {}", i), 1.0).await;
    }

    let c1 = coord.clone();
    let h1 = tokio::spawn(async move {
        c1.process_interaction("user", "hello world", 1.0).await
    });

    let c2 = coord.clone();
    let h2 = tokio::spawn(async move {
        c2.trigger_sleep().await
    });

    let (r1, r2) = tokio::join!(h1, h2);
    assert!(r1.unwrap().is_ok());
    assert!(r2.unwrap().is_ok());

    // State should be consistent
    let state_arc = coord.state();
    let state = state_arc.read().await;
    assert!(state.fast.energy >= 0.0);
    assert!(state.fast.energy <= 1.0);
}

/// Test 2: Lifecycle state transitions Awake → Sleeping → Awake
#[tokio::test]
async fn test_lifecycle_transitions() {
    let dir = tempfile::TempDir::new().unwrap();
    let (coord, _db) = setup_coordinator(&dir).await;

    assert_eq!(coord.lifecycle_state().await, LifecycleState::Awake);

    // Populate episodes via interactions
    for i in 0..15 {
        let _ = coord.process_interaction("user", &format!("msg {}", i), 1.0).await;
    }

    // trigger_sleep transitions: Awake → Sleeping → Awake
    let result = coord.trigger_sleep().await.unwrap();
    assert!(result.performed);
    assert_eq!(coord.lifecycle_state().await, LifecycleState::Awake);
}

/// Test 3: Feedback persistence round-trip (write → restart → read)
#[tokio::test]
async fn test_feedback_persistence_roundtrip() {
    let dir = tempfile::TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");

    // Phase 1: write feedback
    {
        let db = Arc::new(SqliteMemory::new(db_path.to_str().unwrap()).await.unwrap());
        let limbic = Arc::new(LimbicSystem::new());
        let config = OrganismConfig::default();
        let coord = OrganismCoordinator::with_persistence(limbic, config, db.clone()).await.unwrap();

        coord.record_feedback(
            SignalType::UserEmotionalFeedback,
            "用户很开心".to_string(),
            0.9,
            0.7,
        ).await;

        // Verify it was persisted to DB
        let pending = db.load_pending_feedback().await.unwrap();
        assert!(!pending.is_empty(), "Feedback should be persisted to DB");
    }

    // Phase 2: reload and verify feedback was loaded from DB
    {
        let db = Arc::new(SqliteMemory::new(db_path.to_str().unwrap()).await.unwrap());
        let pending = db.load_pending_feedback().await.unwrap();
        assert!(!pending.is_empty(), "Feedback should survive restart");
    }
}

/// Test 4: State persistence round-trip
#[tokio::test]
async fn test_state_persistence_roundtrip() {
    let dir = tempfile::TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");

    // Phase 1: modify state and save
    {
        let db = Arc::new(SqliteMemory::new(db_path.to_str().unwrap()).await.unwrap());
        let limbic = Arc::new(LimbicSystem::new());
        let config = OrganismConfig::default();
        let coord = OrganismCoordinator::with_persistence(limbic, config, db).await.unwrap();

        let state_arc = coord.state();
        {
            let mut state = state_arc.write().await;
            state.fast.energy = 0.42;
            state.medium.mood_bias = -0.3;
        }
        coord.save_state().await;
    }

    // Phase 2: reload and verify state
    {
        let db = Arc::new(SqliteMemory::new(db_path.to_str().unwrap()).await.unwrap());
        let limbic = Arc::new(LimbicSystem::new());
        let config = OrganismConfig::default();
        let coord = OrganismCoordinator::with_persistence(limbic, config, db).await.unwrap();

        let state_arc = coord.state();
        let state = state_arc.read().await;
        assert!((state.fast.energy - 0.42).abs() < 0.01, "Energy should persist");
        assert!((state.medium.mood_bias - (-0.3)).abs() < 0.01, "Mood bias should persist");
    }
}

/// Test 5: Rule engine loads from DB end-to-end
#[tokio::test]
async fn test_rule_engine_loads() {
    let dir = tempfile::TempDir::new().unwrap();
    let (coord, _db) = setup_coordinator(&dir).await;

    // Rule engine should be initialized from seed rules
    assert!(coord.rule_engine().is_some(), "Rule engine should be loaded");
}

/// Test 6: GoalManager CRUD round-trip against real SQLite
#[tokio::test]
async fn test_goal_manager_roundtrip() {
    let dir = tempfile::TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Arc::new(SqliteMemory::new(db_path.to_str().unwrap()).await.unwrap());
    let gm = GoalManager::new(db.clone());

    // Initially no active goals
    let goals = gm.active_goals().await.unwrap();
    assert!(goals.is_empty());

    // Create a goal
    let goal = Goal {
        id: 0,
        goal_type: GoalType::Social,
        description: "和创建者聊聊".into(),
        priority: 0.8,
        status: GoalStatus::Active,
        progress: 0.0,
        created_at: 1000,
        deadline: None,
        parent_id: None,
        metadata: serde_json::json!({"source": "test"}),
    };
    let goal_id = gm.create_goal(&goal).await.unwrap();
    assert!(goal_id > 0);

    // Load and verify
    let goals = gm.active_goals().await.unwrap();
    assert_eq!(goals.len(), 1);
    assert_eq!(goals[0].description, "和创建者聊聊");
    assert_eq!(goals[0].goal_type, GoalType::Social);

    // Update progress
    gm.update_progress(goal_id, 0.5).await.unwrap();
    let goals = gm.active_goals().await.unwrap();
    assert!((goals[0].progress - 0.5).abs() < 0.01);

    // Complete via progress = 1.0
    gm.update_progress(goal_id, 1.0).await.unwrap();
    let goals = gm.active_goals().await.unwrap();
    assert!(goals.is_empty(), "Completed goal should not appear in active list");
}

/// Test 7: RuleEngine load + evaluate + last_fired persistence
#[tokio::test]
async fn test_rule_engine_evaluate_and_persist() {
    let dir = tempfile::TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Arc::new(SqliteMemory::new(db_path.to_str().unwrap()).await.unwrap());

    // Seed rules into DB
    let seed = mneme_memory::rules::seed_rules();
    db.seed_behavior_rules(&seed).await.unwrap();

    // Load engine from DB
    let mut engine = RuleEngine::load(db.clone()).await.unwrap();
    let rules = engine.rules();
    assert!(rules.len() >= 3, "Should have at least 3 seed rules");

    // Evaluate with OnTick trigger + low energy → low_energy_silence should fire
    let mut state = mneme_core::OrganismState::default();
    state.fast.energy = 0.1; // Below 0.2 threshold
    let ctx = RuleContext {
        trigger_type: RuleTrigger::OnTick,
        state,
        current_hour: 14,
        interaction_count: 5,
        lifecycle: LifecycleState::Awake,
        now: 2000000,
        message_text: None,
    };
    let results = engine.evaluate(&ctx).await;
    let fired_names: Vec<&str> = results.iter()
        .filter_map(|(id, _)| engine.rules().iter().find(|r| r.id == *id).map(|r| r.name.as_str()))
        .collect();
    assert!(fired_names.contains(&"low_energy_silence"), "low_energy_silence should fire when energy < 0.2");

    // Reload engine from DB and verify last_fired was persisted
    let engine2 = RuleEngine::load(db).await.unwrap();
    let rule = engine2.rules().iter().find(|r| r.name == "low_energy_silence").unwrap();
    assert_eq!(rule.last_fired, Some(2000000), "last_fired should be persisted to DB");
}
