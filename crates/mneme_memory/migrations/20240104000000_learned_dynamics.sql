-- Learned dynamics parameters (singleton, like learned_curves/learned_neural)
CREATE TABLE IF NOT EXISTS learned_dynamics (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    dynamics_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
