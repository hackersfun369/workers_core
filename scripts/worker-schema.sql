-- Per-Worker D1 Schema (outcomes table for mistake memory)
-- Each worker has its own D1 database with this schema.
-- Run: wrangler d1 execute DB_WORKER_X --file=scripts/worker-schema.sql

-- Mistake memory outcomes table
CREATE TABLE IF NOT EXISTS outcomes (
    id                TEXT PRIMARY KEY,
    worker_name       TEXT NOT NULL,
    action_type       TEXT NOT NULL,
    input_fingerprint TEXT NOT NULL,
    input_embedding   BLOB,
    model_used        TEXT NOT NULL,
    prompt_strategy   TEXT NOT NULL,
    result            TEXT NOT NULL,     -- success / failure / timeout
    failure_reason    TEXT,
    timestamp         INTEGER NOT NULL
);

-- Index for similarity search by fingerprint
CREATE INDEX IF NOT EXISTS idx_outcomes_fingerprint ON outcomes(input_fingerprint);

-- Index for chronological queries
CREATE INDEX IF NOT EXISTS idx_outcomes_timestamp ON outcomes(timestamp);

-- Index for model performance queries
CREATE INDEX IF NOT EXISTS idx_outcomes_model_result ON outcomes(model_used, result);

-- Job records (per-worker)
CREATE TABLE IF NOT EXISTS jobs (
    job_id            TEXT PRIMARY KEY,
    user_id           TEXT NOT NULL,
    worker_id         TEXT NOT NULL,
    status            TEXT NOT NULL,       -- pending / processing / completed / failed
    requirements      TEXT NOT NULL,       -- JSON
    output            TEXT,                -- JSON
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL,
    completed_at      INTEGER,
    error             TEXT,
    feedback          TEXT,
    step_results      TEXT                 -- JSON array of StepResult
);

-- Index for job queries
CREATE INDEX IF NOT EXISTS idx_jobs_user_status ON jobs(user_id, status);
CREATE INDEX IF NOT EXISTS idx_jobs_worker_status ON jobs(worker_id, status);
