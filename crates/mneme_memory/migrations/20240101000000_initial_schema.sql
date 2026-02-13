-- Mneme baseline schema: all tables consolidated into a single versioned migration.
-- Episodes with embedding + strength columns included from the start.

CREATE TABLE IF NOT EXISTS episodes (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    author TEXT NOT NULL,
    body TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    modality TEXT NOT NULL,
    embedding BLOB,
    strength REAL NOT NULL DEFAULT 0.5
);

CREATE TABLE IF NOT EXISTS facts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subject TEXT NOT NULL,
    predicate TEXT NOT NULL,
    object TEXT NOT NULL,
    confidence REAL NOT NULL,
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_facts_subject ON facts(subject);
CREATE INDEX IF NOT EXISTS idx_facts_predicate ON facts(predicate);

CREATE TABLE IF NOT EXISTS people (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS aliases (
    person_id TEXT NOT NULL,
    platform TEXT NOT NULL,
    platform_id TEXT NOT NULL,
    PRIMARY KEY (platform, platform_id),
    FOREIGN KEY(person_id) REFERENCES people(id)
);

CREATE TABLE IF NOT EXISTS relationships (
    source_id TEXT NOT NULL,
    target_id TEXT NOT NULL,
    context TEXT,
    timestamp INTEGER NOT NULL,
    FOREIGN KEY(source_id) REFERENCES people(id),
    FOREIGN KEY(target_id) REFERENCES people(id)
);

CREATE INDEX IF NOT EXISTS idx_relationships_source_target ON relationships(source_id, target_id);

-- Organism state (singleton)
CREATE TABLE IF NOT EXISTS organism_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    state_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Narrative chapters
CREATE TABLE IF NOT EXISTS narrative_chapters (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    period_start INTEGER NOT NULL,
    period_end INTEGER NOT NULL,
    emotional_tone REAL NOT NULL,
    themes_json TEXT NOT NULL,
    people_json TEXT NOT NULL,
    turning_points_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Feedback signals
CREATE TABLE IF NOT EXISTS feedback_signals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    signal_type TEXT NOT NULL,
    content TEXT NOT NULL,
    confidence REAL NOT NULL,
    emotional_context REAL NOT NULL,
    timestamp INTEGER NOT NULL,
    consolidated INTEGER NOT NULL DEFAULT 0
);

-- Organism state history (time series)
CREATE TABLE IF NOT EXISTS organism_state_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp INTEGER NOT NULL,
    state_json TEXT NOT NULL,
    trigger TEXT NOT NULL,
    diff_summary TEXT
);

CREATE INDEX IF NOT EXISTS idx_state_history_timestamp ON organism_state_history(timestamp);

-- Self-Knowledge (emergent self-model, ADR-002)
CREATE TABLE IF NOT EXISTS self_knowledge (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    domain TEXT NOT NULL,
    content TEXT NOT NULL,
    confidence REAL NOT NULL,
    source TEXT NOT NULL,
    source_episode_id TEXT,
    is_private INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_self_knowledge_domain ON self_knowledge(domain);

-- Token Usage Tracking (v0.4.0)
CREATE TABLE IF NOT EXISTS token_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    timestamp INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_token_usage_timestamp ON token_usage(timestamp);

-- Modulation samples (offline learning)
CREATE TABLE IF NOT EXISTS modulation_samples (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    energy REAL NOT NULL,
    stress REAL NOT NULL,
    arousal REAL NOT NULL,
    mood_bias REAL NOT NULL,
    social_need REAL NOT NULL,
    modulation_json TEXT NOT NULL,
    feedback_valence REAL NOT NULL,
    timestamp INTEGER NOT NULL,
    consumed INTEGER NOT NULL DEFAULT 0
);

-- Learned curves (singleton)
CREATE TABLE IF NOT EXISTS learned_curves (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    curves_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Learned thresholds (singleton)
CREATE TABLE IF NOT EXISTS learned_thresholds (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    thresholds_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Vector search index (sqlite-vec)
CREATE VIRTUAL TABLE IF NOT EXISTS vec_episodes USING vec0(
    episode_id TEXT PRIMARY KEY,
    embedding float[384]
);

-- Behavior Rules (ADR-004, v0.6.0)
CREATE TABLE IF NOT EXISTS behavior_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    priority INTEGER NOT NULL DEFAULT 0,
    enabled INTEGER NOT NULL DEFAULT 1,
    trigger_json TEXT NOT NULL,
    condition_json TEXT NOT NULL,
    action_json TEXT NOT NULL,
    cooldown_secs INTEGER,
    last_fired INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Goals (#22, v0.6.0)
CREATE TABLE IF NOT EXISTS goals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    goal_type TEXT NOT NULL,
    description TEXT NOT NULL,
    priority REAL NOT NULL DEFAULT 0.5,
    status TEXT NOT NULL DEFAULT 'active',
    progress REAL NOT NULL DEFAULT 0.0,
    created_at INTEGER NOT NULL,
    deadline INTEGER,
    parent_id INTEGER,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    FOREIGN KEY(parent_id) REFERENCES goals(id)
);
