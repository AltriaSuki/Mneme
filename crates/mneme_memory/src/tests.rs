use crate::sqlite::SqliteMemory;
use mneme_core::{SocialGraph, Person, Memory, Content, Modality};
use uuid::Uuid;
use std::collections::HashMap;

#[tokio::test]
async fn test_social_graph_ops() {
    let memory = SqliteMemory::new(":memory:").await.expect("Failed to create memory");
    
    // 1. Create Person
    let person_id = Uuid::new_v4();
    let mut aliases = HashMap::new();
    aliases.insert("qq".to_string(), "123456789".to_string());
    
    let person = Person {
        id: person_id,
        name: "Test User".to_string(),
        aliases: aliases.clone(),
    };
    
    memory.upsert_person(&person).await.expect("Upsert failed");
    
    // 2. Find by alias
    let found = memory.find_person("qq", "123456789").await.expect("Find failed");
    assert!(found.is_some());
    let found_person = found.unwrap();
    assert_eq!(found_person.id, person_id);
    assert_eq!(found_person.name, "Test User");
    
    // 3. Find unknown
    let unknown = memory.find_person("qq", "999999").await.expect("Find unknown failed");
    assert!(unknown.is_none());
    
    // 4. Update Person (Name change)
    let updated_person = Person {
        id: person_id,
        name: "Updated Name".to_string(),
        aliases,
    };
    memory.upsert_person(&updated_person).await.expect("Update failed");
    
    let found_updated = memory.find_person("qq", "123456789").await.expect("Find failed").unwrap();
    assert_eq!(found_updated.name, "Updated Name");
    
    // 5. Record Interaction
    let other_id = Uuid::new_v4();
    let other_person = Person {
        id: other_id,
        name: "Other User".to_string(),
        aliases: HashMap::new(),
    };
    memory.upsert_person(&other_person).await.expect("Upsert other failed");
    
    memory.record_interaction(person_id, other_id, "test interaction").await.expect("Record interaction failed");
    
    // We can't query interactions yet as there is no API for it, but if it didn't error, the FK checks passed.
}

#[tokio::test]
async fn test_semantic_recall() {
    // This test involves downloading the model (25MB) once, so it might be slow on first run.
    let memory = SqliteMemory::new(":memory:").await.expect("Failed to create memory");

    // 1. Memorize generic facts
    let apple_content = Content {
        id: Uuid::new_v4(),
        source: "test".to_string(),
        author: "User".to_string(),
        body: "I really love eating red apples.".to_string(),
        timestamp: 100,
        modality: Modality::Text,
    };
    memory.memorize(&apple_content).await.expect("Memorize apple failed");

    let tech_content = Content {
        id: Uuid::new_v4(),
        source: "test".to_string(),
        author: "User".to_string(),
        body: "My computer running Rust is very fast.".to_string(),
        timestamp: 101,
        modality: Modality::Text,
    };
    memory.memorize(&tech_content).await.expect("Memorize tech failed");

    // 2. Query with semantic overlap but no keyword overlap
    // "fruit" does not appear in "I really love eating red apples" (unless 'apples' is stemmed, but we assume exact keyword match vs vector)
    // Actually "apple" is fruit. 
    let recall_result = memory.recall("fruit").await.expect("Recall failed");
    
    println!("Recall result for 'fruit': {}", recall_result);
    
    // Expect apple content to be present
    assert!(recall_result.contains("red apples"));
    
    // 3. Query for tech
    let recall_tech = memory.recall("processor").await.expect("Recall tech failed");
    println!("Recall result for 'processor': {}", recall_tech);
    assert!(recall_tech.contains("computer"));
}

#[tokio::test]
async fn test_store_and_recall_facts() {
    let memory = SqliteMemory::new(":memory:").await.expect("Failed to create memory");
    
    // Store some facts
    let id1 = memory.store_fact("用户", "喜欢", "红色苹果", 0.9)
        .await.expect("Failed to store fact 1");
    let id2 = memory.store_fact("用户", "住在", "上海", 0.8)
        .await.expect("Failed to store fact 2");
    let id3 = memory.store_fact("用户", "讨厌", "蟑螂", 1.0)
        .await.expect("Failed to store fact 3");
    let id4 = memory.store_fact("猫咪", "名字是", "小花", 0.95)
        .await.expect("Failed to store fact 4");
    
    assert!(id1 > 0);
    assert!(id2 > 0);
    assert!(id3 > 0);
    assert!(id4 > 0);
    
    // Recall facts about "用户"
    let user_facts = memory.get_facts_about("用户").await.expect("Failed to get facts about user");
    assert_eq!(user_facts.len(), 3);
    // Sorted by confidence desc
    assert_eq!(user_facts[0].predicate, "讨厌"); // confidence 1.0
    
    // Recall facts by keyword "苹果"
    let apple_facts = memory.recall_facts("苹果").await.expect("Failed to recall apple facts");
    assert_eq!(apple_facts.len(), 1);
    assert_eq!(apple_facts[0].object, "红色苹果");
    
    // Recall facts by keyword "猫咪"
    let cat_facts = memory.recall_facts("猫咪 小花").await.expect("Failed to recall cat facts");
    assert!(!cat_facts.is_empty());
    assert!(cat_facts.iter().any(|f| f.subject == "猫咪"));
    
    // Get top facts
    let top = memory.get_top_facts(10).await.expect("Failed to get top facts");
    assert_eq!(top.len(), 4);
}

#[tokio::test]
async fn test_fact_confidence_update() {
    let memory = SqliteMemory::new(":memory:").await.expect("Failed to create memory");
    
    // Store a fact
    memory.store_fact("用户", "喜欢", "编程", 0.5)
        .await.expect("Failed to store");
    
    // Store the same fact again with higher confidence — should merge, not duplicate
    memory.store_fact("用户", "喜欢", "编程", 0.9)
        .await.expect("Failed to update");
    
    let facts = memory.get_facts_about("用户").await.expect("Failed to get");
    assert_eq!(facts.len(), 1, "Should have merged duplicate, not created two rows");
    
    // Confidence should have been updated (0.5 * 0.3 + 0.9 * 0.7 = 0.78)
    let fact = &facts[0];
    assert!(fact.confidence > 0.7, "confidence={} should be > 0.7 after update", fact.confidence);
    assert!(fact.confidence < 0.85, "confidence={} should be < 0.85", fact.confidence);
}

#[tokio::test]
async fn test_fact_decay() {
    let memory = SqliteMemory::new(":memory:").await.expect("Failed to create memory");
    
    memory.store_fact("用户", "住在", "北京", 0.9)
        .await.expect("Failed to store");
    
    let facts = memory.get_facts_about("用户").await.expect("get failed");
    let fact_id = facts[0].id;
    let original_confidence = facts[0].confidence;
    
    // Decay the fact (e.g., contradicting info came in)
    memory.decay_fact(fact_id, 0.5).await.expect("decay failed");
    
    let facts_after = memory.get_facts_about("用户").await.expect("get failed");
    assert!(facts_after[0].confidence < original_confidence, 
        "confidence should decrease after decay: {} < {}", facts_after[0].confidence, original_confidence);
    assert!((facts_after[0].confidence - original_confidence * 0.5).abs() < 0.01);
}

#[tokio::test]
async fn test_format_facts_for_prompt() {
    let facts = vec![
        crate::sqlite::SemanticFact {
            id: 1,
            subject: "用户".to_string(),
            predicate: "喜欢".to_string(),
            object: "音乐".to_string(),
            confidence: 0.9,
            created_at: 0,
            updated_at: 0,
        },
    ];
    
    let formatted = SqliteMemory::format_facts_for_prompt(&facts);
    assert!(formatted.contains("KNOWN FACTS"));
    assert!(formatted.contains("用户 喜欢 音乐"));
    assert!(formatted.contains("90%"));
    
    // Empty facts should produce empty string
    let empty = SqliteMemory::format_facts_for_prompt(&[]);
    assert!(empty.is_empty());
}

// =============================================================================
// State History Integration Tests
// =============================================================================

#[tokio::test]
async fn test_state_history_record_and_query() {
    let memory = SqliteMemory::new(":memory:").await.expect("Failed to create memory");
    let state = mneme_core::OrganismState::default();

    // Record several snapshots
    memory.record_state_snapshot(&state, "tick", None).await.unwrap();

    let mut modified = state.clone();
    modified.fast.energy = 0.3;
    memory.record_state_snapshot(&modified, "interaction", Some(&state)).await.unwrap();

    modified.fast.stress = 0.9;
    memory.record_state_snapshot(&modified, "consolidation", Some(&state)).await.unwrap();

    // Query all history
    let history = memory.query_state_history(0, i64::MAX, 100).await.unwrap();
    assert_eq!(history.len(), 3);

    // Check triggers are correct
    assert_eq!(history[0].trigger, "tick");
    assert_eq!(history[1].trigger, "interaction");
    assert_eq!(history[2].trigger, "consolidation");

    // First snapshot has no diff (no prev_state)
    assert!(history[0].diff_summary.is_none());

    // Second snapshot should have a diff mentioning energy
    let diff = history[1].diff_summary.as_ref().unwrap();
    assert!(diff.contains('E'), "diff should mention energy change: {}", diff);

    // Verify state was deserialized correctly
    assert!((history[1].state.fast.energy - 0.3).abs() < 0.01);
}

#[tokio::test]
async fn test_state_history_recent() {
    let memory = SqliteMemory::new(":memory:").await.expect("Failed to create memory");
    let state = mneme_core::OrganismState::default();

    // Record 5 snapshots
    for i in 0..5 {
        let mut s = state.clone();
        s.fast.energy = i as f32 * 0.1 + 0.3;
        memory.record_state_snapshot(&s, "tick", None).await.unwrap();
    }

    // Get recent 3
    let recent = memory.recent_state_history(3).await.unwrap();
    assert_eq!(recent.len(), 3);

    // Should be in chronological order (oldest first)
    assert!(recent[0].state.fast.energy < recent[2].state.fast.energy);
}

#[tokio::test]
async fn test_state_history_prune() {
    let memory = SqliteMemory::new(":memory:").await.expect("Failed to create memory");
    let state = mneme_core::OrganismState::default();

    // Record 20 snapshots
    for _ in 0..20 {
        memory.record_state_snapshot(&state, "tick", None).await.unwrap();
    }

    // Verify all 20 exist
    let all = memory.query_state_history(0, i64::MAX, 100).await.unwrap();
    assert_eq!(all.len(), 20);

    // Prune to keep only 10
    let pruned = memory.prune_state_history(10, i64::MAX).await.unwrap();
    assert_eq!(pruned, 10);

    let remaining = memory.query_state_history(0, i64::MAX, 100).await.unwrap();
    assert_eq!(remaining.len(), 10);
}
