use crate::sqlite::SqliteMemory;
use mneme_core::{SocialGraph, Person};
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
