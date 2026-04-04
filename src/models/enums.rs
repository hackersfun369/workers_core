use serde::{Deserialize, Serialize};
use strum_macros::{Display, EnumIter, EnumString};

// ============================================================================
// Task Types — 8 domains for ModelRouter
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumIter)]
pub enum TaskType {
    #[strum(serialize = "full_app_generation")]
    FullAppGeneration,

    #[strum(serialize = "architecture_planning")]
    ArchitecturePlanning,

    #[strum(serialize = "code_generation")]
    CodeGeneration,

    #[strum(serialize = "reasoning")]
    Reasoning,

    #[strum(serialize = "content_writing")]
    ContentWriting,

    #[strum(serialize = "fast_filter")]
    FastFilter,

    #[strum(serialize = "embedding")]
    Embedding,

    #[strum(serialize = "voice_interview")]
    VoiceInterview,
}

// ============================================================================
// Model Identifiers
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
pub enum ModelId {
    // Gemini (Google AI Studio)
    #[strum(serialize = "gemini-2.0-flash")]
    Gemini2Flash,

    #[strum(serialize = "gemini-2.0-flash-thinking")]
    Gemini2FlashThinking,

    // Qwen2.5 Coder (Hugging Face)
    #[strum(serialize = "qwen2.5-coder-32b")]
    Qwen25Coder32B,

    // DeepSeek R1 (OpenRouter)
    #[strum(serialize = "deepseek-r1")]
    DeepSeekR1,

    // Mistral Large (OpenRouter)
    #[strum(serialize = "mistral-large")]
    MistralLarge,

    // Llama 3.1 8B (Groq)
    #[strum(serialize = "llama-3.1-8b")]
    Llama31_8B,

    // Nomic Embed (Hugging Face)
    #[strum(serialize = "nomic-embed")]
    NomicEmbed,

    // Mistral Voice (Mistral API)
    #[strum(serialize = "mistral-voice")]
    MistralVoice,
}

// ============================================================================
// Provider Identifiers for KeyRotator
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
pub enum Provider {
    #[strum(serialize = "google_ai")]
    GoogleAI,

    #[strum(serialize = "openrouter")]
    OpenRouter,

    #[strum(serialize = "huggingface")]
    HuggingFace,

    #[strum(serialize = "groq")]
    Groq,

    #[strum(serialize = "mistral")]
    Mistral,

    #[strum(serialize = "github")]
    GitHub,

    #[strum(serialize = "google_drive")]
    GoogleDrive,

    #[strum(serialize = "mailgun")]
    Mailgun,

    #[strum(serialize = "whatsapp")]
    WhatsApp,

    #[strum(serialize = "scispace")]
    SciSpace,

    #[strum(serialize = "cloudflare")]
    Cloudflare,
}

// ============================================================================
// Conversation Roles
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
}

// ============================================================================
// Conversation Patterns
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConversationPattern {
    /// Worker 5: Voice interviews — genuine back-and-forth, turn order matters
    History,

    /// Workers 1, 6, 7, 8: Multi-step workflows — structured context injection
    Pipeline,
}

// ============================================================================
// Message for ConversationManager
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn system(content: &str) -> Self {
        Self {
            role: Role::System,
            content: content.to_string(),
        }
    }

    pub fn user(content: &str) -> Self {
        Self {
            role: Role::User,
            content: content.to_string(),
        }
    }

    pub fn assistant(content: &str) -> Self {
        Self {
            role: Role::Assistant,
            content: content.to_string(),
        }
    }
}

// ============================================================================
// Worker Identification
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
pub enum WorkerId {
    #[strum(serialize = "worker-job-readiness")]
    Worker1JobReadiness,

    #[strum(serialize = "worker-ai-humanizer")]
    Worker2AiHumanizer,

    #[strum(serialize = "worker-voice-interview-coach")]
    Worker5VoiceInterviewCoach,

    #[strum(serialize = "worker-web-builder")]
    Worker6WebBuilder,

    #[strum(serialize = "worker-mobile-builder")]
    Worker7MobileBuilder,

    #[strum(serialize = "worker-api-builder")]
    Worker8ApiBuilder,
}

// ============================================================================
// Job Status
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    AwaitingApproval,
    Approved,
    Rejected,
    Cancelled,
}

// ============================================================================
// Action Result
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionResult {
    Success,
    Failure,
    Timeout,
}

// ============================================================================
// Payment Status
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaymentStatus {
    Pending,
    Success,
    Failed,
    Refunded,
}

// ============================================================================
// User Status
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserStatus {
    Active,
    Suspended,
    Deleted,
}

// ============================================================================
// Notification Channel
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Display)]
pub enum NotificationChannel {
    #[strum(serialize = "email")]
    Email,

    #[strum(serialize = "whatsapp")]
    WhatsApp,
}

// ============================================================================
// Notification Type
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Display)]
pub enum NotificationType {
    AccountCreated,
    PaymentSuccess,
    SessionCompleted,
    CreditLow,
    PlanExpiringSoon,
    PlanExpired,
    PaymentFailed,
    SubscriptionCancelled,
    ReferralConverted,
    NewFeatureLaunched,
    SupportTicketResponse,
    AccountSuspended,
    DiscountCodeGenerated,
}

// ============================================================================
// Storage Health Status
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Display)]
pub enum StorageHealthStatus {
    #[strum(serialize = "healthy")]
    Healthy,

    #[strum(serialize = "outage")]
    Outage,

    #[strum(serialize = "recovering")]
    Recovering,
}

// ============================================================================
// Sync Status for registry entries
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncStatus {
    DualWritten,
    FallbackOnly,
    Synced,
}

// ============================================================================
// Worker Cron schedule
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronSchedule {
    pub worker_id: WorkerId,
    pub cron_expression: String,
    pub on_demand: bool,
}

impl CronSchedule {
    pub fn on_demand(worker_id: WorkerId) -> Self {
        Self {
            worker_id,
            cron_expression: "* * * * *".to_string(),
            on_demand: true,
        }
    }

    pub fn periodic(worker_id: WorkerId, expression: &str) -> Self {
        Self {
            worker_id,
            cron_expression: expression.to_string(),
            on_demand: false,
        }
    }
}
