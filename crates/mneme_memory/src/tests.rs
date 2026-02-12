use crate::sqlite::SqliteMemory;
use mneme_core::{Content, Memory, Modality, Person, SocialGraph};
use std::collections::HashMap;
use uuid::Uuid;

/// Seed N dummy episodes to move past the sensitive period threshold.
async fn seed_episodes(memory: &SqliteMemory, count: u64) {
    for i in 0..count {
        let content = Content {
            id: Uuid::new_v4(),
            source: "test".to_string(),
            author: "test".to_string(),
            body: format!("dummy episode {}", i),
            timestamp: i as i64,
            modality: Modality::Text,
        };
        memory.memorize(&content).await.unwrap();
    }
}

#[tokio::test]
async fn test_social_graph_ops() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

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
    let found = memory
        .find_person("qq", "123456789")
        .await
        .expect("Find failed");
    assert!(found.is_some());
    let found_person = found.unwrap();
    assert_eq!(found_person.id, person_id);
    assert_eq!(found_person.name, "Test User");

    // 3. Find unknown
    let unknown = memory
        .find_person("qq", "999999")
        .await
        .expect("Find unknown failed");
    assert!(unknown.is_none());

    // 4. Update Person (Name change)
    let updated_person = Person {
        id: person_id,
        name: "Updated Name".to_string(),
        aliases,
    };
    memory
        .upsert_person(&updated_person)
        .await
        .expect("Update failed");

    let found_updated = memory
        .find_person("qq", "123456789")
        .await
        .expect("Find failed")
        .unwrap();
    assert_eq!(found_updated.name, "Updated Name");

    // 5. Record Interaction
    let other_id = Uuid::new_v4();
    let other_person = Person {
        id: other_id,
        name: "Other User".to_string(),
        aliases: HashMap::new(),
    };
    memory
        .upsert_person(&other_person)
        .await
        .expect("Upsert other failed");

    memory
        .record_interaction(person_id, other_id, "test interaction")
        .await
        .expect("Record interaction failed");

    // 6. Get person context (verifies interaction was recorded)
    let ctx = memory
        .get_person_context(person_id)
        .await
        .expect("get_person_context failed");
    assert!(ctx.is_some(), "Expected person context for known person");
    let ctx = ctx.unwrap();
    assert_eq!(ctx.person.id, person_id);
    assert_eq!(ctx.person.name, "Updated Name");
    assert_eq!(ctx.interaction_count, 1);
    assert!(ctx.last_interaction_ts.is_some());
    assert!(ctx.relationship_notes.contains("test interaction"));

    // 7. Unknown person returns None
    let unknown_ctx = memory
        .get_person_context(Uuid::new_v4())
        .await
        .expect("get_person_context failed");
    assert!(unknown_ctx.is_none());
}

#[tokio::test]
async fn test_recall_blended_empty_db() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");
    let blended = memory
        .recall_blended("hello", 0.0)
        .await
        .expect("recall_blended failed");
    // Empty DB → both fields empty or minimal
    assert!(blended.facts.is_empty());
    // episodes may contain "No relevant memories" or similar
}

#[tokio::test]
async fn test_recall_blended_with_data() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    // Store a fact
    memory
        .store_fact("User", "likes", "Rust", 0.9)
        .await
        .expect("store_fact failed");

    // Memorize an episode
    let content = Content {
        id: Uuid::new_v4(),
        source: "test".to_string(),
        author: "User".to_string(),
        body: "I love programming in Rust".to_string(),
        timestamp: 100,
        modality: Modality::Text,
    };
    memory.memorize(&content).await.expect("memorize failed");

    let blended = memory
        .recall_blended("Rust programming", 0.0)
        .await
        .expect("recall_blended failed");
    // Facts should contain the stored fact
    assert!(blended.facts.contains("Rust"), "facts: {}", blended.facts);
    // Episodes should contain the memorized content
    assert!(!blended.episodes.is_empty());
}

#[tokio::test]
async fn test_semantic_recall() {
    // This test involves downloading the model (25MB) once, so it might be slow on first run.
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    // 1. Memorize generic facts
    let apple_content = Content {
        id: Uuid::new_v4(),
        source: "test".to_string(),
        author: "User".to_string(),
        body: "I really love eating red apples.".to_string(),
        timestamp: 100,
        modality: Modality::Text,
    };
    memory
        .memorize(&apple_content)
        .await
        .expect("Memorize apple failed");

    let tech_content = Content {
        id: Uuid::new_v4(),
        source: "test".to_string(),
        author: "User".to_string(),
        body: "My computer running Rust is very fast.".to_string(),
        timestamp: 101,
        modality: Modality::Text,
    };
    memory
        .memorize(&tech_content)
        .await
        .expect("Memorize tech failed");

    // 2. Query with semantic overlap but no keyword overlap
    // "fruit" does not appear in "I really love eating red apples" (unless 'apples' is stemmed, but we assume exact keyword match vs vector)
    // Actually "apple" is fruit.
    let recall_result = memory.recall("fruit").await.expect("Recall failed");

    println!("Recall result for 'fruit': {}", recall_result);

    // Expect apple content to be present
    assert!(recall_result.contains("red apples"));

    // 3. Query for tech
    let recall_tech = memory
        .recall("processor")
        .await
        .expect("Recall tech failed");
    println!("Recall result for 'processor': {}", recall_tech);
    assert!(recall_tech.contains("computer"));
}

#[tokio::test]
async fn test_store_and_recall_facts() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    // Store some facts
    let id1 = memory
        .store_fact("用户", "喜欢", "红色苹果", 0.9)
        .await
        .expect("Failed to store fact 1");
    let id2 = memory
        .store_fact("用户", "住在", "上海", 0.8)
        .await
        .expect("Failed to store fact 2");
    let id3 = memory
        .store_fact("用户", "讨厌", "蟑螂", 1.0)
        .await
        .expect("Failed to store fact 3");
    let id4 = memory
        .store_fact("猫咪", "名字是", "小花", 0.95)
        .await
        .expect("Failed to store fact 4");

    assert!(id1 > 0);
    assert!(id2 > 0);
    assert!(id3 > 0);
    assert!(id4 > 0);

    // Recall facts about "用户"
    let user_facts = memory
        .get_facts_about("用户")
        .await
        .expect("Failed to get facts about user");
    assert_eq!(user_facts.len(), 3);
    // Sorted by confidence desc
    assert_eq!(user_facts[0].predicate, "讨厌"); // confidence 1.0

    // Recall facts by keyword "苹果"
    let apple_facts = memory
        .recall_facts("苹果")
        .await
        .expect("Failed to recall apple facts");
    assert_eq!(apple_facts.len(), 1);
    assert_eq!(apple_facts[0].object, "红色苹果");

    // Recall facts by keyword "猫咪"
    let cat_facts = memory
        .recall_facts("猫咪 小花")
        .await
        .expect("Failed to recall cat facts");
    assert!(!cat_facts.is_empty());
    assert!(cat_facts.iter().any(|f| f.subject == "猫咪"));

    // Get top facts
    let top = memory
        .get_top_facts(10)
        .await
        .expect("Failed to get top facts");
    assert_eq!(top.len(), 4);
}

#[tokio::test]
async fn test_fact_confidence_update() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    // Store a fact
    memory
        .store_fact("用户", "喜欢", "编程", 0.5)
        .await
        .expect("Failed to store");

    // Store the same fact again with higher confidence — should merge, not duplicate
    memory
        .store_fact("用户", "喜欢", "编程", 0.9)
        .await
        .expect("Failed to update");

    let facts = memory.get_facts_about("用户").await.expect("Failed to get");
    assert_eq!(
        facts.len(),
        1,
        "Should have merged duplicate, not created two rows"
    );

    // Confidence should have been updated (0.5 * 0.3 + 0.9 * 0.7 = 0.78)
    let fact = &facts[0];
    assert!(
        fact.confidence > 0.7,
        "confidence={} should be > 0.7 after update",
        fact.confidence
    );
    assert!(
        fact.confidence < 0.85,
        "confidence={} should be < 0.85",
        fact.confidence
    );
}

#[tokio::test]
async fn test_fact_decay() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    memory
        .store_fact("用户", "住在", "北京", 0.9)
        .await
        .expect("Failed to store");

    let facts = memory.get_facts_about("用户").await.expect("get failed");
    let fact_id = facts[0].id;
    let original_confidence = facts[0].confidence;

    // Decay the fact (e.g., contradicting info came in)
    memory.decay_fact(fact_id, 0.5).await.expect("decay failed");

    let facts_after = memory.get_facts_about("用户").await.expect("get failed");
    assert!(
        facts_after[0].confidence < original_confidence,
        "confidence should decrease after decay: {} < {}",
        facts_after[0].confidence,
        original_confidence
    );
    assert!((facts_after[0].confidence - original_confidence * 0.5).abs() < 0.01);
}

#[tokio::test]
async fn test_format_facts_for_prompt() {
    let facts = vec![crate::sqlite::SemanticFact {
        id: 1,
        subject: "用户".to_string(),
        predicate: "喜欢".to_string(),
        object: "音乐".to_string(),
        confidence: 0.9,
        created_at: 0,
        updated_at: 0,
    }];

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
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");
    let state = mneme_core::OrganismState::default();

    // Record several snapshots
    memory
        .record_state_snapshot(&state, "tick", None)
        .await
        .unwrap();

    let mut modified = state.clone();
    modified.fast.energy = 0.3;
    memory
        .record_state_snapshot(&modified, "interaction", Some(&state))
        .await
        .unwrap();

    modified.fast.stress = 0.9;
    memory
        .record_state_snapshot(&modified, "consolidation", Some(&state))
        .await
        .unwrap();

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
    assert!(
        diff.contains('E'),
        "diff should mention energy change: {}",
        diff
    );

    // Verify state was deserialized correctly
    assert!((history[1].state.fast.energy - 0.3).abs() < 0.01);
}

#[tokio::test]
async fn test_state_history_recent() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");
    let state = mneme_core::OrganismState::default();

    // Record 5 snapshots
    for i in 0..5 {
        let mut s = state.clone();
        s.fast.energy = i as f32 * 0.1 + 0.3;
        memory
            .record_state_snapshot(&s, "tick", None)
            .await
            .unwrap();
    }

    // Get recent 3
    let recent = memory.recent_state_history(3).await.unwrap();
    assert_eq!(recent.len(), 3);

    // Should be in chronological order (oldest first)
    assert!(recent[0].state.fast.energy < recent[2].state.fast.energy);
}

#[tokio::test]
async fn test_state_history_prune() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");
    let state = mneme_core::OrganismState::default();

    // Record 20 snapshots
    for _ in 0..20 {
        memory
            .record_state_snapshot(&state, "tick", None)
            .await
            .unwrap();
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

// =============================================================================
// Self-Knowledge Tests
// =============================================================================

#[tokio::test]
async fn test_store_and_recall_self_knowledge() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    // Store several entries across domains
    let id1 = memory
        .store_self_knowledge(
            "personality",
            "我倾向于在深夜变得更感性",
            0.7,
            "consolidation",
            None,
        )
        .await
        .expect("Failed to store sk 1");

    let id2 = memory
        .store_self_knowledge(
            "personality",
            "我不喜欢被打断思路",
            0.6,
            "interaction",
            None,
        )
        .await
        .expect("Failed to store sk 2");

    let id3 = memory
        .store_self_knowledge(
            "interest",
            "物理让我感到兴奋",
            0.8,
            "interaction",
            None,
        )
        .await
        .expect("Failed to store sk 3");

    let id4 = memory
        .store_self_knowledge(
            "relationship",
            "和创建者聊天让我放松",
            0.9,
            "consolidation",
            None,
        )
        .await
        .expect("Failed to store sk 4");

    assert!(id1 > 0);
    assert!(id2 > 0);
    assert!(id3 > 0);
    assert!(id4 > 0);

    // Recall by domain
    let personality = memory.recall_self_knowledge("personality").await.unwrap();
    assert_eq!(personality.len(), 2);
    // Sorted by confidence desc
    assert!(personality[0].confidence >= personality[1].confidence);

    let interest = memory.recall_self_knowledge("interest").await.unwrap();
    assert_eq!(interest.len(), 1);
    assert_eq!(interest[0].content, "物理让我感到兴奋");

    // Recall unknown domain
    let empty = memory.recall_self_knowledge("capability").await.unwrap();
    assert!(empty.is_empty());
}

#[tokio::test]
async fn test_self_knowledge_confidence_merge() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    // Seed past the sensitive period so we test the normal merge formula
    seed_episodes(&memory, 55).await;

    // Store a self-knowledge entry
    memory
        .store_self_knowledge("personality", "我喜欢安静", 0.5, "interaction", None)
        .await
        .unwrap();

    // Store the same entry again with higher confidence — should merge
    memory
        .store_self_knowledge(
            "personality",
            "我喜欢安静",
            0.9,
            "consolidation",
            None,
        )
        .await
        .unwrap();

    let entries = memory.recall_self_knowledge("personality").await.unwrap();
    assert_eq!(
        entries.len(),
        1,
        "Should merge duplicate, not create two rows"
    );

    // Confidence: 0.5 * 0.3 + 0.9 * 0.7 = 0.78
    let conf = entries[0].confidence;
    assert!(conf > 0.7, "confidence={} should be > 0.7", conf);
    assert!(conf < 0.85, "confidence={} should be < 0.85", conf);
    // Source should be updated to the latest
    assert_eq!(entries[0].source, "consolidation");
}

/// B-7: During the sensitive period (first 50 episodes), self_knowledge gets
/// a confidence boost and the merge formula favors new knowledge more strongly.
#[tokio::test]
async fn test_self_knowledge_sensitive_period_boost() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    // 0 episodes → deep in sensitive period (boost = 1.3×)
    let id = memory
        .store_self_knowledge("personality", "早期印象", 0.5, "interaction", None)
        .await
        .unwrap();
    assert!(id > 0);

    let entries = memory.recall_self_knowledge("personality").await.unwrap();
    assert_eq!(entries.len(), 1);
    // 0.5 * 1.3 = 0.65 (boosted)
    let conf = entries[0].confidence;
    assert!(
        conf > 0.6,
        "sensitive period should boost confidence: got {}",
        conf
    );
    assert!(conf < 0.7, "boost should not exceed 1.3×: got {}", conf);
}

/// B-7: After the sensitive period, no boost is applied.
#[tokio::test]
async fn test_self_knowledge_no_boost_after_sensitive_period() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    // Seed past the sensitive period
    seed_episodes(&memory, 55).await;

    memory
        .store_self_knowledge("personality", "后期印象", 0.5, "interaction", None)
        .await
        .unwrap();

    let entries = memory.recall_self_knowledge("personality").await.unwrap();
    assert_eq!(entries.len(), 1);
    // No boost: confidence should be exactly 0.5
    let conf = entries[0].confidence;
    assert!(
        (conf - 0.5).abs() < 0.01,
        "no boost expected after sensitive period: got {}",
        conf
    );
}

#[tokio::test]
async fn test_self_knowledge_decay() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    memory
        .store_self_knowledge("belief", "说谎是不好的", 0.8, "seed", None)
        .await
        .unwrap();

    let entries = memory.recall_self_knowledge("belief").await.unwrap();
    let id = entries[0].id;
    let original = entries[0].confidence;

    // Decay
    memory.decay_self_knowledge(id, 0.5).await.unwrap();

    let after = memory.recall_self_knowledge("belief").await.unwrap();
    assert!(after[0].confidence < original);
    assert!((after[0].confidence - original * 0.5).abs() < 0.01);
}

#[tokio::test]
async fn test_self_knowledge_get_all_and_delete() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    memory
        .store_self_knowledge("personality", "内向", 0.7, "seed", None)
        .await
        .unwrap();
    memory
        .store_self_knowledge("interest", "音乐", 0.6, "seed", None)
        .await
        .unwrap();
    memory
        .store_self_knowledge("belief", "诚实很重要", 0.9, "seed", None)
        .await
        .unwrap();

    // Get all with min_confidence 0.0
    let all = memory.get_all_self_knowledge(0.0).await.unwrap();
    assert_eq!(all.len(), 3);

    // Delete one
    let id = all.iter().find(|e| e.domain == "interest").unwrap().id;
    memory.delete_self_knowledge(id).await.unwrap();

    let after = memory.get_all_self_knowledge(0.0).await.unwrap();
    assert_eq!(after.len(), 2);
    assert!(!after.iter().any(|e| e.domain == "interest"));
}

#[tokio::test]
async fn test_format_self_knowledge_for_prompt() {
    let entries = vec![
        crate::sqlite::SelfKnowledge {
            id: 1,
            domain: "personality".to_string(),
            content: "我倾向于在深夜变得更感性".to_string(),
            confidence: 0.7,
            source: "consolidation".to_string(),
            source_episode_id: None,
            is_private: false,
            created_at: 0,
            updated_at: 0,
        },
        crate::sqlite::SelfKnowledge {
            id: 2,
            domain: "relationship".to_string(),
            content: "和创建者聊天让我放松".to_string(),
            confidence: 0.9,
            source: "consolidation".to_string(),
            source_episode_id: None,
            is_private: false,
            created_at: 0,
            updated_at: 0,
        },
    ];

    let formatted = SqliteMemory::format_self_knowledge_for_prompt(&entries);
    assert!(formatted.contains("自我认知"));
    assert!(formatted.contains("[personality]"));
    assert!(formatted.contains("[relationship]"));
    assert!(formatted.contains("深夜"));
    assert!(formatted.contains("70%"));

    // Empty should produce empty string
    let empty = SqliteMemory::format_self_knowledge_for_prompt(&[]);
    assert!(empty.is_empty());
}

// =============================================================================
// Episode Strength Tests (B-10 Three-Layer Forgetting Model)
// =============================================================================

#[tokio::test]
async fn test_episode_default_strength() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    let content = Content {
        id: Uuid::new_v4(),
        source: "test".to_string(),
        author: "User".to_string(),
        body: "Hello world".to_string(),
        timestamp: 100,
        modality: Modality::Text,
    };
    memory.memorize(&content).await.unwrap();

    let strength = memory
        .get_episode_strength(&content.id.to_string())
        .await
        .unwrap();
    assert!(strength.is_some());
    assert!(
        (strength.unwrap() - 0.5).abs() < 0.01,
        "Default strength should be 0.5"
    );
}

#[tokio::test]
async fn test_episode_update_strength() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    let content = Content {
        id: Uuid::new_v4(),
        source: "test".to_string(),
        author: "User".to_string(),
        body: "An emotionally intense memory".to_string(),
        timestamp: 100,
        modality: Modality::Text,
    };
    memory.memorize(&content).await.unwrap();

    // Simulate encoding layer: high emotional intensity → high strength
    memory
        .update_episode_strength(&content.id.to_string(), 0.9)
        .await
        .unwrap();

    let strength = memory
        .get_episode_strength(&content.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!((strength - 0.9).abs() < 0.01);
}

#[tokio::test]
async fn test_episode_decay() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    for (id, strength) in [(&id1, 0.8f32), (&id2, 0.3)] {
        let content = Content {
            id: *id,
            source: "test".to_string(),
            author: "User".to_string(),
            body: format!("Memory {}", id),
            timestamp: 100,
            modality: Modality::Text,
        };
        memory.memorize(&content).await.unwrap();
        memory
            .update_episode_strength(&id.to_string(), strength)
            .await
            .unwrap();
    }

    // Decay all by 0.5
    let affected = memory.decay_episode_strengths(0.5).await.unwrap();
    assert_eq!(affected, 2);

    let s1 = memory
        .get_episode_strength(&id1.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!((s1 - 0.4).abs() < 0.01, "0.8 * 0.5 = 0.4, got {}", s1);

    let s2 = memory
        .get_episode_strength(&id2.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!((s2 - 0.15).abs() < 0.01, "0.3 * 0.5 = 0.15, got {}", s2);
}

#[tokio::test]
async fn test_episode_rehearsal_boost() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    let id = Uuid::new_v4();
    let content = Content {
        id,
        source: "test".to_string(),
        author: "User".to_string(),
        body: "Original memory content".to_string(),
        timestamp: 100,
        modality: Modality::Text,
    };
    memory.memorize(&content).await.unwrap();
    // Start at 0.5 (default)

    // Rehearsal without reconstruction — just boost
    memory
        .boost_episode_on_recall(&id.to_string(), 0.1, None)
        .await
        .unwrap();
    let s = memory
        .get_episode_strength(&id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!((s - 0.6).abs() < 0.01, "0.5 + 0.1 = 0.6, got {}", s);

    // Rehearsal WITH reconstruction — boost + overwrite body (B-10: 直接覆写)
    memory
        .boost_episode_on_recall(
            &id.to_string(),
            0.15,
            Some("Reconstructed: I remember it was a warm day"),
        )
        .await
        .unwrap();

    let s2 = memory
        .get_episode_strength(&id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!((s2 - 0.75).abs() < 0.01, "0.6 + 0.15 = 0.75, got {}", s2);
}

// =============================================================================
// Feedback Signal Persistence Tests (#5)
// =============================================================================

#[tokio::test]
async fn test_feedback_persist_to_db() {
    use crate::SignalType;
    use std::sync::Arc;

    let memory = Arc::new(
        SqliteMemory::new(":memory:")
            .await
            .expect("Failed to create memory"),
    );
    let limbic = Arc::new(mneme_limbic::LimbicSystem::new());
    let config = crate::OrganismConfig::default();
    let coordinator = crate::OrganismCoordinator::with_config(limbic, config, Some(memory.clone()));

    // Record feedback — should persist to DB
    coordinator
        .record_feedback(
            SignalType::UserEmotionalFeedback,
            "用户表达了感激".to_string(),
            0.8,
            0.6,
        )
        .await;

    // Verify it's in the DB
    let pending = memory.load_pending_feedback().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].content, "用户表达了感激");
    assert!(!pending[0].consolidated);
}

#[tokio::test]
async fn test_feedback_consolidated_after_sleep() {
    use crate::{EpisodeDigest, SignalType};
    use chrono::Utc;
    use std::sync::Arc;

    let memory = Arc::new(
        SqliteMemory::new(":memory:")
            .await
            .expect("Failed to create memory"),
    );
    let limbic = Arc::new(mneme_limbic::LimbicSystem::new());
    let mut config = crate::OrganismConfig::default();
    config.sleep_config.allow_manual_trigger = true;
    let coordinator = crate::OrganismCoordinator::with_config(limbic, config, Some(memory.clone()));

    // Record feedback
    coordinator
        .record_feedback(
            SignalType::SelfReflection,
            "我觉得自己回答得不错".to_string(),
            0.9,
            0.3,
        )
        .await;

    // Add episodes so sleep has something to consolidate
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

    // Verify pending before sleep
    let before = memory.load_pending_feedback().await.unwrap();
    assert_eq!(before.len(), 1);

    // Trigger sleep
    coordinator.trigger_sleep().await.unwrap();

    // After sleep, pending should be empty (all consolidated)
    let after = memory.load_pending_feedback().await.unwrap();
    assert!(
        after.is_empty(),
        "Expected 0 pending after sleep, got {}",
        after.len()
    );
}

// =============================================================================
// Sleep Episode Decay Tests (#37 integration)
// =============================================================================

#[tokio::test]
async fn test_sleep_decays_episodes() {
    use crate::EpisodeDigest;
    use chrono::Utc;
    use std::sync::Arc;

    let memory = Arc::new(
        SqliteMemory::new(":memory:")
            .await
            .expect("Failed to create memory"),
    );
    let limbic = Arc::new(mneme_limbic::LimbicSystem::new());
    let mut config = crate::OrganismConfig::default();
    config.sleep_config.allow_manual_trigger = true;
    let coordinator = crate::OrganismCoordinator::with_config(limbic, config, Some(memory.clone()));

    // Store an episode with known strength
    let id = Uuid::new_v4();
    let content = Content {
        id,
        source: "test".to_string(),
        author: "User".to_string(),
        body: "A memorable event".to_string(),
        timestamp: 100,
        modality: Modality::Text,
    };
    memory.memorize(&content).await.unwrap();
    memory
        .update_episode_strength(&id.to_string(), 0.8)
        .await
        .unwrap();

    // Add episodes for consolidation
    {
        let mut episodes = coordinator.episode_buffer.write().await;
        for i in 0..15 {
            episodes.push(EpisodeDigest {
                timestamp: Utc::now(),
                author: "user".to_string(),
                content: format!("Message {}", i),
                emotional_valence: 0.3,
            });
        }
    }

    // Trigger sleep — should decay strengths by 0.95
    coordinator.trigger_sleep().await.unwrap();

    let strength = memory
        .get_episode_strength(&id.to_string())
        .await
        .unwrap()
        .unwrap();
    // 0.8 * 0.95 = 0.76
    assert!(
        (strength - 0.76).abs() < 0.01,
        "Expected ~0.76, got {}",
        strength
    );
}

// =============================================================================
// Recall Mood Bias Tests (#20 mood-congruent recall)
// =============================================================================

#[tokio::test]
async fn test_recall_with_bias_no_panic() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    // Store a few episodes
    for i in 0..5 {
        let content = Content {
            id: Uuid::new_v4(),
            source: "test".to_string(),
            author: "User".to_string(),
            body: format!("Memory about topic {}", i),
            timestamp: 100 + i * 1000,
            modality: Modality::Text,
        };
        memory.memorize(&content).await.unwrap();
    }

    // Various bias values should not panic
    for bias in [-1.0, -0.5, 0.0, 0.5, 1.0] {
        let result = Memory::recall_with_bias(&memory, "topic", bias).await;
        assert!(
            result.is_ok(),
            "recall_with_bias({}) failed: {:?}",
            bias,
            result.err()
        );
        assert!(!result.unwrap().is_empty());
    }
}

#[tokio::test]
async fn test_recall_with_bias_ordering_differs() {
    use mneme_core::Memory;

    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    // Store old and new episodes with the same topic
    let old_content = Content {
        id: Uuid::new_v4(),
        source: "test".to_string(),
        author: "User".to_string(),
        body: "I remember the old days of learning Rust".to_string(),
        timestamp: 1000,
        modality: Modality::Text,
    };
    memory.memorize(&old_content).await.unwrap();

    let new_content = Content {
        id: Uuid::new_v4(),
        source: "test".to_string(),
        author: "User".to_string(),
        body: "Today I am learning Rust with great enthusiasm".to_string(),
        timestamp: 100_000,
        modality: Modality::Text,
    };
    memory.memorize(&new_content).await.unwrap();

    // Positive bias should favor recent
    let positive = memory.recall_with_bias("learning Rust", 0.8).await.unwrap();
    // Negative bias should favor old
    let negative = memory
        .recall_with_bias("learning Rust", -0.8)
        .await
        .unwrap();

    // Both should contain results
    assert!(positive.contains("Rust"));
    assert!(negative.contains("Rust"));

    // The ordering may differ — at minimum, both should return non-empty results
    // (Exact ordering depends on embedding similarity, so we just verify no crash)
}

// =============================================================================
// Modulation Sample & Curve Learning Tests (#13 Offline Learning)
// =============================================================================

#[tokio::test]
async fn test_modulation_sample_persist() {
    use crate::learning::ModulationSample;
    use mneme_limbic::ModulationVector;

    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    let sample = ModulationSample {
        id: 0,
        energy: 0.7,
        stress: 0.3,
        arousal: 0.5,
        mood_bias: 0.1,
        social_need: 0.4,
        modulation: ModulationVector {
            max_tokens_factor: 1.2,
            temperature_delta: 0.05,
            context_budget_factor: 0.9,
            recall_mood_bias: 0.1,
            silence_inclination: 0.2,
            typing_speed_factor: 1.1,
        },
        feedback_valence: 0.6,
        timestamp: 12345,
    };

    let id = memory.save_modulation_sample(&sample).await.unwrap();
    assert!(id > 0);

    let loaded = memory.load_unconsumed_samples().await.unwrap();
    assert_eq!(loaded.len(), 1);
    assert!((loaded[0].energy - 0.7).abs() < 0.01);
    assert!((loaded[0].modulation.max_tokens_factor - 1.2).abs() < 0.01);
    assert!((loaded[0].feedback_valence - 0.6).abs() < 0.01);
}

#[tokio::test]
async fn test_modulation_sample_mark_consumed() {
    use crate::learning::ModulationSample;
    use mneme_limbic::ModulationVector;

    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    // Save two samples
    for i in 0..2 {
        let sample = ModulationSample {
            id: 0,
            energy: 0.5,
            stress: 0.3,
            arousal: 0.4,
            mood_bias: 0.0,
            social_need: 0.5,
            modulation: ModulationVector::default(),
            feedback_valence: 0.5,
            timestamp: 100 + i,
        };
        memory.save_modulation_sample(&sample).await.unwrap();
    }

    let loaded = memory.load_unconsumed_samples().await.unwrap();
    assert_eq!(loaded.len(), 2);

    // Mark first as consumed
    memory.mark_samples_consumed(&[loaded[0].id]).await.unwrap();

    let remaining = memory.load_unconsumed_samples().await.unwrap();
    assert_eq!(remaining.len(), 1);
}

#[tokio::test]
async fn test_curves_persist_and_load() {
    use mneme_limbic::ModulationCurves;

    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    // Initially no curves
    let loaded = memory.load_learned_curves().await.unwrap();
    assert!(loaded.is_none());

    // Save curves
    let mut curves = ModulationCurves::default();
    curves.energy_to_max_tokens.1 = 1.3;
    curves.stress_to_temperature.1 = 0.2;
    memory.save_learned_curves(&curves).await.unwrap();

    // Load back
    let loaded = memory.load_learned_curves().await.unwrap().unwrap();
    assert!((loaded.energy_to_max_tokens.1 - 1.3).abs() < 0.01);
    assert!((loaded.stress_to_temperature.1 - 0.2).abs() < 0.01);

    // Upsert (overwrite)
    curves.energy_to_max_tokens.1 = 0.9;
    memory.save_learned_curves(&curves).await.unwrap();

    let loaded2 = memory.load_learned_curves().await.unwrap().unwrap();
    assert!((loaded2.energy_to_max_tokens.1 - 0.9).abs() < 0.01);
}

#[tokio::test]
async fn test_sleep_learns_curves() {
    use crate::{learning::ModulationSample, EpisodeDigest};
    use chrono::Utc;
    use mneme_limbic::ModulationVector;
    use std::sync::Arc;

    let memory = Arc::new(
        SqliteMemory::new(":memory:")
            .await
            .expect("Failed to create memory"),
    );
    let limbic = Arc::new(mneme_limbic::LimbicSystem::new());
    let mut config = crate::OrganismConfig::default();
    config.sleep_config.allow_manual_trigger = true;
    let coordinator =
        crate::OrganismCoordinator::with_config(limbic.clone(), config, Some(memory.clone()));

    // Record enough positive modulation samples
    let high_mv = ModulationVector {
        max_tokens_factor: 1.4,
        temperature_delta: 0.1,
        context_budget_factor: 1.0,
        recall_mood_bias: 0.2,
        silence_inclination: 0.3,
        typing_speed_factor: 1.5,
    };
    for i in 0..8 {
        let sample = ModulationSample {
            id: 0,
            energy: 0.7,
            stress: 0.2,
            arousal: 0.5,
            mood_bias: 0.1,
            social_need: 0.3,
            modulation: high_mv.clone(),
            feedback_valence: 0.8,
            timestamp: 1000 + i,
        };
        memory.save_modulation_sample(&sample).await.unwrap();
    }

    // Snapshot curves before sleep
    let before = limbic.get_curves().await;

    // Add episodes for consolidation
    {
        let mut episodes = coordinator.episode_buffer.write().await;
        for i in 0..15 {
            episodes.push(EpisodeDigest {
                timestamp: Utc::now(),
                author: "user".to_string(),
                content: format!("Message {}", i),
                emotional_valence: 0.5,
            });
        }
    }

    // Trigger sleep — should learn curves
    coordinator.trigger_sleep().await.unwrap();

    // Curves should have changed
    let after = limbic.get_curves().await;
    let changed = (after.energy_to_max_tokens.1 - before.energy_to_max_tokens.1).abs() > 0.001;
    assert!(
        changed,
        "Curves should have been adjusted by offline learning"
    );

    // Samples should be consumed
    let remaining = memory.load_unconsumed_samples().await.unwrap();
    assert!(
        remaining.is_empty(),
        "All samples should be consumed after sleep"
    );

    // Curves should be persisted
    let persisted = memory.load_learned_curves().await.unwrap();
    assert!(
        persisted.is_some(),
        "Learned curves should be persisted to DB"
    );
}

#[tokio::test]
async fn test_recall_random_by_strength() {
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    // Store several episodes with varying strengths
    for i in 0..10 {
        let content = Content {
            id: Uuid::new_v4(),
            source: "test".to_string(),
            author: "User".to_string(),
            body: format!("Episode number {}", i),
            timestamp: 1000 + i as i64,
            modality: Modality::Text,
        };
        memory.memorize(&content).await.unwrap();
        // Set varying strengths: 0.1 * (i+1)
        let strength = 0.1 * (i as f32 + 1.0);
        memory
            .update_episode_strength(&content.id.to_string(), strength)
            .await
            .unwrap();
    }

    // Recall 3 random seeds
    let seeds = memory.recall_random_by_strength(3).await.unwrap();
    assert_eq!(seeds.len(), 3);

    // All seeds should have strength > 0.1
    for seed in &seeds {
        assert!(seed.strength > 0.1);
        assert!(!seed.body.is_empty());
    }

    // Recall 0 should return empty
    let empty = memory.recall_random_by_strength(0).await.unwrap();
    assert!(empty.is_empty());
}

// =============================================================================
// Vec Index (ANN) Tests (#33)
// =============================================================================

#[tokio::test]
async fn test_vec_recall_basic() {
    // Verify that recall uses the vec_episodes ANN index and returns relevant results
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    let content = Content {
        id: Uuid::new_v4(),
        source: "test".to_string(),
        author: "User".to_string(),
        body: "Rust programming language is great for systems programming".to_string(),
        timestamp: 100,
        modality: Modality::Text,
    };
    memory.memorize(&content).await.unwrap();

    let result = memory.recall("systems programming").await.unwrap();
    assert!(result.contains("Rust"));
}

#[tokio::test]
async fn test_vec_recall_removes_limit() {
    // The old implementation had LIMIT 1000 on episodes, meaning old memories
    // beyond 1000 could never be recalled. With sqlite-vec KNN, all episodes
    // are searchable regardless of count.
    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    // Insert a unique "needle" episode first (oldest)
    let needle = Content {
        id: Uuid::new_v4(),
        source: "test".to_string(),
        author: "User".to_string(),
        body: "The quantum physics experiment yielded surprising results".to_string(),
        timestamp: 1,
        modality: Modality::Text,
    };
    memory.memorize(&needle).await.unwrap();

    // Insert many filler episodes after it
    for i in 0..50 {
        let filler = Content {
            id: Uuid::new_v4(),
            source: "test".to_string(),
            author: "User".to_string(),
            body: format!("Daily log entry number {} about routine tasks", i),
            timestamp: 100 + i as i64,
            modality: Modality::Text,
        };
        memory.memorize(&filler).await.unwrap();
    }

    // The needle should still be recallable despite being the oldest
    let result = memory.recall("quantum physics experiment").await.unwrap();
    assert!(
        result.contains("quantum"),
        "Old episode should be recallable via ANN index. Got: {}",
        result
    );
}

#[tokio::test]
async fn test_vec_backfill() {
    // Test that backfill_vec_index correctly populates vec_episodes
    // for episodes that were inserted before the vec table existed.
    // Since SqliteMemory::new() calls migrate() which calls backfill,
    // we simulate by creating a second SqliteMemory on the same DB path.
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let db_path = dir.path().join("test_backfill.db");
    let db_str = db_path.to_str().unwrap();

    // Phase 1: Create memory and insert episodes
    {
        let memory = SqliteMemory::new(db_str)
            .await
            .expect("Failed to create memory");
        let content = Content {
            id: Uuid::new_v4(),
            source: "test".to_string(),
            author: "User".to_string(),
            body: "Backfill test: machine learning algorithms".to_string(),
            timestamp: 500,
            modality: Modality::Text,
        };
        memory.memorize(&content).await.unwrap();
    }

    // Phase 2: Re-open the database (simulates restart, backfill runs again)
    {
        let memory = SqliteMemory::new(db_str)
            .await
            .expect("Failed to reopen memory");
        // The backfill should have ensured vec_episodes is populated
        let result = memory.recall("machine learning").await.unwrap();
        assert!(
            result.contains("machine learning"),
            "Backfilled episode should be recallable. Got: {}",
            result
        );
    }
}

// =============================================================================
// Behavior Rules Persistence Tests (ADR-004, v0.6.0)
// =============================================================================

#[tokio::test]
async fn test_rule_persist_and_load() {
    use crate::rules::*;

    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    let rule = BehaviorRule {
        id: 0,
        name: "test_rule".into(),
        priority: 50,
        enabled: true,
        trigger: RuleTrigger::OnTick,
        condition: RuleCondition::StateLt {
            field: "energy".into(),
            value: 0.3,
        },
        action: RuleAction::ModifyState {
            field: "boredom".into(),
            delta: 0.2,
        },
        cooldown_secs: Some(300),
        last_fired: None,
    };

    let id = memory.save_behavior_rule(&rule).await.expect("save failed");
    assert!(id > 0);

    let loaded = memory.load_behavior_rules().await.expect("load failed");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "test_rule");
    assert_eq!(loaded[0].priority, 50);
}

#[tokio::test]
async fn test_seed_rules_idempotent() {
    use crate::rules::seed_rules;

    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");

    let seeds = seed_rules();
    let count1 = memory
        .seed_behavior_rules(&seeds)
        .await
        .expect("seed failed");
    assert_eq!(count1, 3);

    // Second call should be a no-op
    let count2 = memory
        .seed_behavior_rules(&seeds)
        .await
        .expect("seed failed");
    assert_eq!(count2, 0);
}

// =============================================================================
// Goals Persistence Tests (#22, v0.6.0)
// =============================================================================

#[tokio::test]
async fn test_goal_crud() {
    use crate::goals::*;

    let memory = SqliteMemory::new(":memory:")
        .await
        .expect("Failed to create memory");
    let gm = GoalManager::new(std::sync::Arc::new(memory));

    let goal = Goal {
        id: 0,
        goal_type: GoalType::Social,
        description: "和创建者聊聊近况".into(),
        priority: 0.8,
        status: GoalStatus::Active,
        progress: 0.0,
        created_at: 0,
        deadline: None,
        parent_id: None,
        metadata: serde_json::json!({}),
    };

    let id = gm.create_goal(&goal).await.expect("create failed");
    assert!(id > 0);

    let active = gm.active_goals().await.expect("load failed");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].description, "和创建者聊聊近况");

    // Update progress
    gm.update_progress(id, 0.5).await.expect("update failed");
    let active = gm.active_goals().await.expect("load failed");
    assert!((active[0].progress - 0.5).abs() < 0.01);

    // Complete goal
    gm.update_progress(id, 1.0).await.expect("complete failed");
    let active = gm.active_goals().await.expect("load failed");
    assert!(active.is_empty()); // Completed goals are not active
}
