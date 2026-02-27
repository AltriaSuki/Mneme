-- Recent messages sliding window for repetition detection (Sisyphus/boredom dynamics).
-- Persisted across process restarts so single-shot CLI mode accumulates similarity.

CREATE TABLE IF NOT EXISTS recent_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);
