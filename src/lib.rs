//! # worker-core
//!
//! Shared Rust crate for the Autonomous Software Factory.
//! Every worker imports this as a Git dependency in Cargo.toml.
//! Update once, all workers get it on next deployment automatically.
//!
//! ## Components
//! - `models` - All shared data types, enums, and the MemoryMap
//! - `model_router` - Selects optimal model per task type via D1 performance scores
//! - `key_rotator` - Dynamic API key pool rotation with cooldown/recovery
//! - `storage_router` - Unified KV/D1/Google Drive with dual-write and failover
//! - `conversation` - ConversationManager for History and Pipeline patterns
//! - `api_clients` - HTTP clients for all external services
//! - `payments` - GooglePayWebhook with RSA-SHA256 verification
//! - `users` - UserManager for accounts, OTP, sessions, credits, referrals
//! - `notifications` - NotificationManager via email and WhatsApp
//! - `abuse` - AbuseGuard rate limiting and deduplication
//! - `mistake_memory` - Mistake recording, embedding search, outcome tracking
//! - `cron` - Storage recovery, subscription expiry, notification sender
//! - `mcp` - MCP HTTP transport endpoint

pub mod models;
pub mod model_router;
pub mod key_rotator;
pub mod storage_router;
pub mod conversation;
pub mod api_clients;
pub mod payments;
pub mod users;
pub mod notifications;
pub mod abuse;
pub mod mistake_memory;
pub mod cron;
pub mod mcp;

/// Re-exports for convenience
pub use models::*;
pub use model_router::ModelRouter;
pub use key_rotator::KeyRotator;
pub use storage_router::StorageRouter;
pub use conversation::ConversationManager;
pub use payments::GooglePayWebhook;
pub use users::UserManager;
pub use notifications::NotificationManager;
pub use abuse::AbuseGuard;
pub use mistake_memory::MistakeMemory;
pub use mcp::McpHttpHandler;

/// WorkerCore error type
pub mod error {
    use thiserror::Error;

    #[derive(Error, Debug)]
    pub enum CoreError {
        #[error("KV operation failed: {0}")]
        KvError(String),

        #[error("D1 operation failed: {0}")]
        D1Error(String),

        #[error("R2 operation failed: {0}")]
        R2Error(String),

        #[error("HTTP request failed: {0}")]
        HttpError(String),

        #[error("Serialization failed: {0}")]
        SerializationError(String),

        #[error("Authentication failed: {0}")]
        AuthError(String),

        #[error("Payment verification failed: {0}")]
        PaymentError(String),

        #[error("Rate limit exceeded: {0}")]
        RateLimitError(String),

        #[error("Storage failover active: primary down, using fallback")]
        StorageFailover(String),

        #[error("All API keys exhausted for provider: {0}")]
        KeysExhausted(String),

        #[error("Model routing failed: no model available for task: {0}")]
        ModelRoutingFailed(String),

        #[error("Google Drive operation failed: {0}")]
        GoogleDriveError(String),

        #[error("RSA signature verification failed: {0}")]
        SignatureError(String),

        #[error("User not found: {0}")]
        UserNotFound(String),

        #[error("Abuse guard triggered: {0}")]
        AbuseGuard(String),

        #[error("Internal error: {0}")]
        Internal(String),

        #[error("Timeout: {0}")]
        Timeout(String),

        #[error("Invalid input: {0}")]
        InvalidInput(String),

        #[error("Concurrency lock conflict: {0}")]
        LockConflict(String),

        #[error("Notification send failed: {0}")]
        NotificationError(String),
    }

    pub type Result<T> = std::result::Result<T, CoreError>;
}

pub use error::{CoreError, Result};

// Implement From<worker::KvError> for CoreError
impl From<worker::KvError> for CoreError {
    fn from(e: worker::KvError) -> Self {
        CoreError::KvError(format!("{:?}", e))
    }
}

// Implement From<worker::Error> for CoreError
impl From<worker::Error> for CoreError {
    fn from(e: worker::Error) -> Self {
        CoreError::Internal(format!("worker error: {:?}", e))
    }
}

// Implement From<gloo_net::Error> for CoreError
impl From<gloo_net::Error> for CoreError {
    fn from(e: gloo_net::Error) -> Self {
        CoreError::HttpError(format!("{:?}", e))
    }
}

/// Initialize the core library
pub fn init() {
    console_error_panic_hook::set_once();
}
