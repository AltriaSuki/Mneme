-- Track files created by Mneme's shell tool (ownership tracking).
-- When a self-created artifact is lost, the grief response is amplified.
CREATE TABLE IF NOT EXISTS created_artifacts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER
);
