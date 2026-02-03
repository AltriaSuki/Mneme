use crate::sqlite::SqliteMemory;
use mneme_core::{SocialGraph, Person, Memory, Content, Modality};
use uuid::Uuid;
use std::collections::HashMap;
use std::time::SystemTime;

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
