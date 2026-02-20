-- #84: Add pinned column for autonomous memory management.
-- Pinned episodes are exempt from strength decay.
ALTER TABLE episodes ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
