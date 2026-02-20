-- Learned neural modulator weights (singleton)
CREATE TABLE IF NOT EXISTS learned_neural (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    neural_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
