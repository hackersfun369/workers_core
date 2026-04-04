-- Migration: 0001_create_shared_tables
-- Created: 2026-04-04

-- User accounts
CREATE TABLE IF NOT EXISTS users (
    user_id          TEXT PRIMARY KEY,
    name             TEXT NOT NULL,
    email            TEXT NOT NULL,
    phone            TEXT NOT NULL,
    created_at       INTEGER NOT NULL,
    status           TEXT DEFAULT 'active',
    referral_code    TEXT UNIQUE NOT NULL,
    referred_by      TEXT
);

CREATE TABLE IF NOT EXISTS user_plans (
    plan_id           TEXT PRIMARY KEY,
    user_id           TEXT NOT NULL,
    worker_id         TEXT NOT NULL,
    plan_type         TEXT NOT NULL,
    total_credits     INTEGER NOT NULL,
    used_credits      INTEGER DEFAULT 0,
    remaining_credits INTEGER NOT NULL,
    plan_start        INTEGER NOT NULL,
    plan_expiry       INTEGER,
    status            TEXT DEFAULT 'active',
    created_at        INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS user_sessions (
    session_id        TEXT PRIMARY KEY,
    user_id           TEXT NOT NULL,
    worker_id         TEXT NOT NULL,
    plan_id           TEXT NOT NULL,
    started_at        INTEGER NOT NULL,
    completed_at      INTEGER,
    status            TEXT NOT NULL,
    output_path       TEXT,
    credits_consumed  INTEGER DEFAULT 1,
    feedback_score    INTEGER,
    feedback_text     TEXT
);

CREATE TABLE IF NOT EXISTS payments (
    payment_id        TEXT PRIMARY KEY,
    user_id           TEXT NOT NULL,
    plan_id           TEXT NOT NULL,
    amount            INTEGER NOT NULL,
    currency          TEXT DEFAULT 'INR',
    gpay_reference    TEXT,
    status            TEXT NOT NULL,
    created_at        INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS discount_codes (
    code              TEXT PRIMARY KEY,
    user_id           TEXT NOT NULL,
    discount_percent  INTEGER NOT NULL,
    created_at        INTEGER NOT NULL,
    expires_at        INTEGER NOT NULL,
    used              INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS referrals (
    referral_id       TEXT PRIMARY KEY,
    referrer_id       TEXT NOT NULL,
    referee_id        TEXT NOT NULL,
    converted         INTEGER DEFAULT 0,
    reward_issued     INTEGER DEFAULT 0,
    created_at        INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS support_tickets (
    ticket_id         TEXT PRIMARY KEY,
    user_id           TEXT NOT NULL,
    subject           TEXT NOT NULL,
    message           TEXT NOT NULL,
    status            TEXT DEFAULT 'open',
    response          TEXT,
    created_at        INTEGER NOT NULL,
    resolved_at       INTEGER
);

CREATE TABLE IF NOT EXISTS notifications (
    notification_id   TEXT PRIMARY KEY,
    user_id           TEXT NOT NULL,
    type              TEXT NOT NULL,
    channel           TEXT NOT NULL,
    content           TEXT NOT NULL,
    sent              INTEGER DEFAULT 0,
    sent_at           INTEGER,
    created_at        INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS model_scores (
    model_name        TEXT NOT NULL,
    task_type         TEXT NOT NULL,
    success_count     INTEGER DEFAULT 0,
    failure_count     INTEGER DEFAULT 0,
    timeout_count     INTEGER DEFAULT 0,
    last_updated      INTEGER NOT NULL,
    PRIMARY KEY (model_name, task_type)
);

CREATE TABLE IF NOT EXISTS audit_log (
    log_id            TEXT PRIMARY KEY,
    actor             TEXT NOT NULL,
    action            TEXT NOT NULL,
    target_type       TEXT NOT NULL,
    target_id         TEXT NOT NULL,
    before_value      TEXT,
    after_value       TEXT,
    timestamp         INTEGER NOT NULL
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_user_plans_user_status ON user_plans(user_id, status);
CREATE INDEX IF NOT EXISTS idx_user_sessions_user_id ON user_sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_payments_user_id ON payments(user_id);
CREATE INDEX IF NOT EXISTS idx_notifications_unsent ON notifications(sent);
CREATE INDEX IF NOT EXISTS idx_referrals_referrer ON referrals(referrer_id);
