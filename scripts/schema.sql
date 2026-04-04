-- D1 Schema Migrations for Autonomous Software Factory
-- Apply these migrations to set up all required tables.
-- Run: wrangler d1 execute factory-db-shared --file=scripts/schema.sql

-- ============================================================================
-- DB_SHARED Schema (shared across all workers)
-- ============================================================================

-- User accounts
CREATE TABLE IF NOT EXISTS users (
    user_id          TEXT PRIMARY KEY,
    name             TEXT NOT NULL,
    email            TEXT NOT NULL,
    phone            TEXT NOT NULL,
    created_at       INTEGER NOT NULL,
    status           TEXT DEFAULT 'active',  -- active / suspended / deleted
    referral_code    TEXT UNIQUE NOT NULL,
    referred_by      TEXT
);

-- Subscription plans and credit balances
CREATE TABLE IF NOT EXISTS user_plans (
    plan_id           TEXT PRIMARY KEY,
    user_id           TEXT NOT NULL,
    worker_id         TEXT NOT NULL,
    plan_type         TEXT NOT NULL,
    total_credits     INTEGER NOT NULL,
    used_credits      INTEGER DEFAULT 0,
    remaining_credits INTEGER NOT NULL,
    plan_start        INTEGER NOT NULL,
    plan_expiry       INTEGER,               -- null for unlimited monthly
    status            TEXT DEFAULT 'active', -- active / expired / cancelled
    created_at        INTEGER NOT NULL
);

-- Session and job history per user
CREATE TABLE IF NOT EXISTS user_sessions (
    session_id        TEXT PRIMARY KEY,
    user_id           TEXT NOT NULL,
    worker_id         TEXT NOT NULL,
    plan_id           TEXT NOT NULL,
    started_at        INTEGER NOT NULL,
    completed_at      INTEGER,
    status            TEXT NOT NULL,         -- pending / processing / completed / failed
    output_path       TEXT,                  -- Google Drive path to output
    credits_consumed  INTEGER DEFAULT 1,
    feedback_score    INTEGER,               -- 1-5 user rating
    feedback_text     TEXT
);

-- Payment records
CREATE TABLE IF NOT EXISTS payments (
    payment_id        TEXT PRIMARY KEY,
    user_id           TEXT NOT NULL,
    plan_id           TEXT NOT NULL,
    amount            INTEGER NOT NULL,      -- in paise (₹499 = 49900)
    currency          TEXT DEFAULT 'INR',
    gpay_reference    TEXT,
    status            TEXT NOT NULL,         -- pending / success / failed / refunded
    created_at        INTEGER NOT NULL
);

-- Discount codes
CREATE TABLE IF NOT EXISTS discount_codes (
    code              TEXT PRIMARY KEY,
    user_id           TEXT NOT NULL,
    discount_percent  INTEGER NOT NULL,
    created_at        INTEGER NOT NULL,
    expires_at        INTEGER NOT NULL,
    used              INTEGER DEFAULT 0
);

-- Referral tracking
CREATE TABLE IF NOT EXISTS referrals (
    referral_id       TEXT PRIMARY KEY,
    referrer_id       TEXT NOT NULL,
    referee_id        TEXT NOT NULL,
    converted         INTEGER DEFAULT 0,
    reward_issued     INTEGER DEFAULT 0,
    created_at        INTEGER NOT NULL
);

-- Support tickets
CREATE TABLE IF NOT EXISTS support_tickets (
    ticket_id         TEXT PRIMARY KEY,
    user_id           TEXT NOT NULL,
    subject           TEXT NOT NULL,
    message           TEXT NOT NULL,
    status            TEXT DEFAULT 'open',   -- open / in_progress / resolved
    response          TEXT,
    created_at        INTEGER NOT NULL,
    resolved_at       INTEGER
);

-- Notification queue
CREATE TABLE IF NOT EXISTS notifications (
    notification_id   TEXT PRIMARY KEY,
    user_id           TEXT NOT NULL,
    type              TEXT NOT NULL,
    channel           TEXT NOT NULL,         -- email / whatsapp
    content           TEXT NOT NULL,
    sent              INTEGER DEFAULT 0,
    sent_at           INTEGER,
    created_at        INTEGER NOT NULL
);

-- Model performance scores (cross-worker)
CREATE TABLE IF NOT EXISTS model_scores (
    model_name        TEXT NOT NULL,
    task_type         TEXT NOT NULL,
    success_count     INTEGER DEFAULT 0,
    failure_count     INTEGER DEFAULT 0,
    timeout_count     INTEGER DEFAULT 0,
    last_updated      INTEGER NOT NULL,
    PRIMARY KEY (model_name, task_type)
);

-- Audit log for all dashboard actions
CREATE TABLE IF NOT EXISTS audit_log (
    log_id            TEXT PRIMARY KEY,
    actor             TEXT NOT NULL,         -- admin or system
    action            TEXT NOT NULL,
    target_type       TEXT NOT NULL,
    target_id         TEXT NOT NULL,
    before_value      TEXT,
    after_value       TEXT,
    timestamp         INTEGER NOT NULL
);

-- ============================================================================
-- Indexes for common queries
-- ============================================================================

CREATE INDEX IF NOT EXISTS idx_user_plans_user_status ON user_plans(user_id, status);
CREATE INDEX IF NOT EXISTS idx_user_sessions_user_id ON user_sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_payments_user_id ON payments(user_id);
CREATE INDEX IF NOT EXISTS idx_notifications_unsent ON notifications(sent);
CREATE INDEX IF NOT EXISTS idx_referrals_referrer ON referrals(referrer_id);
