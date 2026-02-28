-- Learned LTC weights (singleton, persists Hebbian w_rec updates)
CREATE TABLE IF NOT EXISTS learned_ltc (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    ltc_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
