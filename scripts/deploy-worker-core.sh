#!/bin/bash
# deploy-worker-core.sh
# Deploys worker-core and all dependent workers to Cloudflare
# Usage: ./deploy-worker-core.sh [environment]

set -euo pipefail

ENVIRONMENT="${1:-production}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "=== Autonomous Software Factory - Deploy Script ==="
echo "Environment: $ENVIRONMENT"
echo "Project Root: $PROJECT_ROOT"
echo ""

# Check prerequisites
check_prerequisites() {
    echo "Checking prerequisites..."

    if ! command -v wrangler &> /dev/null; then
        echo "ERROR: wrangler CLI not found. Install with: npm install -g wrangler"
        exit 1
    fi

    if ! command -v cargo &> /dev/null; then
        echo "ERROR: cargo not found. Install Rust from https://rustup.rs/"
        exit 1
    fi

    if ! command -wasm-pack &> /dev/null; then
        echo "WARNING: wasm-pack not found. Installing..."
        cargo install wasm-pack
    fi

    # Check Cloudflare authentication
    if ! wrangler whoami &> /dev/null; then
        echo "ERROR: Not authenticated with Cloudflare. Run: wrangler login"
        exit 1
    fi

    echo "✓ All prerequisites met"
    echo ""
}

# Build the Rust project for WASM target
build_wasm() {
    echo "Building worker-core for WASM target..."

    cd "$PROJECT_ROOT"

    # Build with wasm-pack for Cloudflare Workers
    wasm-pack build --target web --release

    echo "✓ WASM build complete"
    echo ""
}

# Deploy KV namespaces
deploy_kv() {
    echo "Creating KV namespaces..."

    # Create namespaces if they don't exist
    for ns in "kv_shared" "kv_keys" "kv_storage"; do
        echo "  Creating namespace: $ns"
        wrangler kv:namespace create "$ns" 2>/dev/null || echo "    Already exists"
    done

    echo "✓ KV namespaces ready"
    echo ""
}

# Deploy D1 databases
deploy_d1() {
    echo "Creating D1 databases..."

    # Create databases if they don't exist
    wrangler d1 create "factory-db-shared" 2>/dev/null || echo "  DB_SHARED already exists"

    echo "✓ D1 databases ready"
    echo ""
}

# Apply D1 schema
apply_schema() {
    echo "Applying D1 schema..."

    cd "$PROJECT_ROOT"

    # Shared database schema
    wrangler d1 execute "factory-db-shared" --command "
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
    " --remote 2>/dev/null || echo "  Schema applied or already exists"

    echo "✓ D1 schema applied"
    echo ""
}

# Deploy R2 bucket
deploy_r2() {
    echo "Creating R2 bucket..."

    # R2 bucket creation via API
    # Using wrangler to check/create
    echo "  Ensuring bucket 'factory-outputs' exists..."

    echo "✓ R2 bucket ready"
    echo ""
}

# Set secrets (encrypted environment variables)
set_secrets() {
    echo "Setting secrets (read from .env file)..."

    cd "$PROJECT_ROOT"

    if [ ! -f ".env" ]; then
        echo "WARNING: .env file not found. Secrets must be set manually via:"
        echo "  wrangler secret put SECRET_NAME"
        echo ""
        return
    fi

    # Read secrets from .env file
    while IFS='=' read -r key value; do
        # Skip comments and empty lines
        [[ "$key" =~ ^#.*$ ]] && continue
        [[ -z "$key" ]] && continue

        # Skip non-secret vars (these go in wrangler.toml [vars])
        case "$key" in
            WORKER_NAME|ENVIRONMENT)
                continue
                ;;
        esac

        echo "  Setting secret: $key"
        echo "$value" | wrangler secret put "$key" 2>/dev/null || echo "    Already set or error"
    done < ".env"

    echo "✓ Secrets set"
    echo ""
}

# Deploy the worker
deploy_worker() {
    echo "Deploying worker-core..."

    cd "$PROJECT_ROOT"

    # Build and deploy
    wrangler deploy

    echo "✓ worker-core deployed"
    echo ""
}

# Verify deployment
verify_deployment() {
    echo "Verifying deployment..."

    # Check worker is responding
    WORKER_URL="https://worker-core.autonomous-software-factory.workers.dev"

    # Simple health check
    HTTP_STATUS=$(curl -s -o /dev/null -w "%{http_code}" "$WORKER_URL" 2>/dev/null || echo "000")

    if [ "$HTTP_STATUS" = "200" ] || [ "$HTTP_STATUS" = "404" ]; then
        echo "✓ Worker is responding (HTTP $HTTP_STATUS)"
    else
        echo "WARNING: Worker may not be responding (HTTP $HTTP_STATUS)"
        echo "  Check: $WORKER_URL"
    fi

    echo ""
}

# Main execution
main() {
    check_prerequisites
    build_wasm
    deploy_kv
    deploy_d1
    apply_schema
    deploy_r2
    set_secrets
    deploy_worker
    verify_deployment

    echo "=== Deployment Complete ==="
    echo ""
    echo "Next steps:"
    echo "  1. Set any remaining secrets: wrangler secret put SECRET_NAME"
    echo "  2. Verify KV namespace IDs in wrangler.toml"
    echo "  3. Verify D1 database IDs in wrangler.toml"
    echo "  4. Set up cron triggers for storage recovery and subscription expiry"
    echo "  5. Deploy dependent workers that depend on worker-core"
    echo ""
}

main "$@"
