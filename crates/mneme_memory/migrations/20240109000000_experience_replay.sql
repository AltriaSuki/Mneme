-- Phase II Step 3: Add boredom column for complete StateFeatures in experience replay
ALTER TABLE modulation_samples ADD COLUMN boredom REAL NOT NULL DEFAULT 0.0;
