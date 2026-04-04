# worker-core

**Shared Rust crate for the Autonomous Software Factory.**

Every worker imports this as a Git dependency in `Cargo.toml`.
Update once, all workers get it on next deployment automatically.

---

## Architecture

```
worker-core/
├── src/
│   ├── lib.rs                      # Root — re-exports all components
│   ├── models/                     # Shared data types, enums, MemoryMap
│   │   ├── enums.rs                # TaskType, ModelId, Provider, Role, etc.
│   │   ├── types.rs                # User, Payment, Session, Job, etc.
│   │   └── memory_map.rs           # Compressed mistake memory store (~204 bytes/entry)
│   ├── model_router.rs             # Model selection via D1 performance scores
│   ├── key_rotator/                # API key pool rotation
│   │   └── rotator.rs              # Round-robin, cooldown, recovery, fallback
│   ├── storage_router/             # Unified KV/D1/Google Drive
│   │   ├── router.rs               # Dual-write, failover, recovery
│   │   └── google_drive.rs         # Real OAuth2 service account + file ops
│   ├── conversation/               # Stateless API memory layer
│   │   └── manager.rs              # History and Pipeline patterns
│   ├── api_clients/                # HTTP clients for all external services
│   │   └── clients.rs              # Gemini, Groq, OpenRouter, HF, Mistral, GitHub, etc.
│   ├── payments/                   # Google Pay webhook processing
│   │   └── google_pay.rs           # RSA-SHA256 signature verification
│   ├── users/                      # Account and session management
│   │   └── manager.rs              # OTP, credits, referrals, discounts
│   ├── notifications/              # Email and WhatsApp notifications
│   │   └── manager.rs              # Queue-based notification sender
│   ├── abuse/                      # Rate limiting and deduplication
│   │   └── guard.rs                # Rolling window counters, auto-suspend
│   ├── mistake_memory/             # Outcome recording and semantic search
│   │   └── memory.rs               # Embedding search, model score tracking
│   ├── cron/                       # Scheduled job handlers
│   │   ├── storage_recovery.rs     # Failover sync, health checks
│   │   ├── subscription_expiry.rs  # Plan expiry + reminders
│   │   └── notification_sender.rs  # Queue processor
│   └── mcp/                        # MCP HTTP transport
│       └── http_transport.rs       # JSON-RPC endpoint for external clients
├── scripts/
│   ├── deploy-worker-core.sh       # Linux/Mac deployment script
│   ├── deploy-worker-core.bat      # Windows deployment script
│   ├── schema.sql                  # DB_SHARED D1 schema
│   └── worker-schema.sql           # Per-worker D1 schema (outcomes table)
├── tests/
│   └── integration_test.rs         # Comprehensive WASM tests
├── Cargo.toml
├── wrangler.toml
└── .github/workflows/deploy.yml    # GitHub Actions CI/CD
```

---

## Components

### MemoryMap — Compressed Mistake Memory Store

Stores large amounts of information in a tiny fixed-size structure (~204 bytes/entry)
while preserving maximum retrieval fidelity. Uses four layered techniques:

| Technique | Size | Purpose |
|-----------|------|---------|
| **SimHash (64-bit)** | 8 bytes | Near-duplicate detection. Hamming distance ≤ 3 = nearly identical. |
| **Binary Embedding** | 96 bytes | 768-dim float vector compressed to 768 bits. Cosine similarity via popcount. |
| **Count-Min Sketch** | 32 KB | Probabilistic frequency tracking. Fixed-size regardless of entry count. |
| **Compressed Trie** | ~4 KB | Shared prefix collapse for failure reasons. Varint encoding. |

**10-25× compression** vs raw JSON (2-5KB → ~204 bytes) with near-lossless retrieval.

### Model Router — 8 Task Types

| Task Type | Model | Provider | Tier |
|-----------|-------|----------|------|
| FullAppGeneration | Gemini 2.0 Flash | Google AI | Free |
| ArchitecturePlanning | Gemini 2.0 Flash Thinking | Google AI | Free |
| CodeGeneration | Qwen2.5 Coder 32B | Hugging Face | Free |
| Reasoning | DeepSeek R1 | OpenRouter | Free |
| ContentWriting | Mistral Large | OpenRouter | Free |
| FastFilter | Llama 3.1 8B | Groq | Free |
| Embedding | Nomic Embed | Hugging Face | Free |
| VoiceInterview | Mistral Voice | Mistral | Free |

### Key Rotation

- Round-robin across 1..N keys per provider
- Auto-skip on 429/quota errors
- Cooldown recovery via KV timestamps
- Cross-provider fallback when all keys exhausted
- Workers never see a key failure

### Storage Router — Three Layers

| Layer | Technology | Purpose |
|-------|-----------|---------|
| Hot | Cloudflare KV | Active sessions, rate limits, key cooldown |
| Warm | Cloudflare D1 | Outcomes, users, payments, sessions |
| Cold | Google Drive | Archives, fallback, registry |

**Dual-write** on every operation. **Auto-failover** on primary outage.
**Auto-recovery** when primary returns (cron every 15 min).

### ConversationManager

Two patterns for stateless LLM APIs:

1. **History** (Worker 5): Full conversation with summarization after N turns
2. **Pipeline** (Workers 1, 6, 7, 8): Structured context injection per step

Active sessions in KV (TTL 24h). Completed sessions archived to D1 + Google Drive.

### GooglePay Webhook

- **RSA-SHA256** signature verification (pure Rust, WASM-compatible via `rsa` crate)
- Idempotent processing (24h dedup via KV)
- Auto-creates user account on first payment
- Activates plan, queues notifications

### AbuseGuard

- Rolling window rate limiting (per user, per worker)
- Blake3 hash deduplication (24h window)
- Auto-suspension on threshold breach (20 submissions/hour → 24h ban)
- Configurable limits per worker

---

## Setup

### Prerequisites

```bash
# Rust (latest stable)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# wasm-pack
cargo install wasm-pack

# wrangler CLI
npm install -g wrangler

# Cloudflare login
wrangler login
```

### Environment Variables

Set secrets via `wrangler secret put`:

```bash
wrangler secret put GOOGLE_AI_KEYS       # comma-separated API keys
wrangler secret put OPENROUTER_API_KEYS
wrangler secret put HUGGINGFACE_API_KEYS
wrangler secret put GROQ_API_KEYS
wrangler secret put MISTRAL_API_KEYS
wrangler secret put GITHUB_TOKENS
wrangler secret put GDRIVE_CREDENTIALS   # base64-encoded service account JSON
wrangler secret put GPAY_MERCHANT_ID
wrangler secret put GPAY_PUBLIC_KEY
wrangler secret put MAILGUN_API_KEY
wrangler secret put MAILGUN_DOMAIN
wrangler secret put WHATSAPP_API_TOKEN
wrangler secret put CLOUDFLARE_API_TOKEN
wrangler secret put ADMIN_AUTH_TOKEN     # MCP endpoint authentication
```

### Deploy

```bash
# Linux/Mac
./scripts/deploy-worker-core.sh

# Windows
.\scripts\deploy-worker-core.bat

# Or directly
wrangler deploy
```

---

## Usage in Dependent Workers

Add to worker's `Cargo.toml`:

```toml
[dependencies]
worker-core = { git = "https://github.com/your-org/autonomous-software-factory", branch = "main" }
```

Example usage:

```rust
use worker_core::*;
use worker_core::models::*;

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response, worker::Error> {
    // Initialize
    init();

    // Create StorageRouter
    let storage = StorageRouter::new(
        env.kv("KV_WORKER_6")?,
        "worker-web-builder".to_string(),
    )
    .with_worker_db(env.d1("DB_WORKER_6")?)
    .with_shared_db(env.d1("DB_SHARED")?);

    // Create KeyRotator
    let key_rotator = KeyRotator::new(env.kv("KV_KEYS")?, &env).await?;

    // Create ModelRouter
    let model_router = ModelRouter::new(env.d1("DB_SHARED")?, key_rotator);

    // Make an LLM call with automatic model selection
    let response = model_router.call_llm(
        TaskType::CodeGeneration,
        &[Message::user("Build a todo app")],
        Some("You are a web application generator"),
    ).await?;

    // Record outcome for mistake memory
    // ...

    Response::ok("Done")
}
```

---

## MCP HTTP Endpoint

The MCP (Model Context Protocol) endpoint allows external clients to interact
with worker-core services via JSON-RPC over HTTP.

**Endpoint:** `POST /mcp`
**Auth:** `Bearer <ADMIN_AUTH_TOKEN>`
**Content-Type:** `application/json`

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/list",
  "params": {}
}
```

**Available tools:**
- `get_model_scores` — Get model performance scores for a task type
- `check_storage_health` — Check all storage layer health
- `get_key_pool_status` — Get API key pool status

---

## Testing

```bash
# Build for WASM
wasm-pack build --target web --release

# Run tests
cargo test
```

---

## Cron Schedules

| Cron | Frequency | Handler |
|------|-----------|---------|
| Storage Recovery | Every 15 min | `cron::StorageRecoveryCron` |
| Subscription Expiry | Every hour | `cron::SubscriptionExpiryCron` |
| Notification Sender | Every 5 min | `cron::NotificationSenderCron` |
| Key Recovery | Every 15 min | `KeyRotator::run_recovery_check()` |

Configure in each worker's `wrangler.toml`:

```toml
[triggers]
crons = ["*/15 * * * *", "0 * * * *"]
```

---

## License

MIT
