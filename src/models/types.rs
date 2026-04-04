use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::enums::*;

// ============================================================================
// User Account
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub user_id: String,
    pub name: String,
    pub email: String,
    pub phone: String,
    pub created_at: i64,
    pub status: UserStatus,
    pub referral_code: String,
    pub referred_by: Option<String>,
}

impl User {
    pub fn new(name: String, email: String, phone: String) -> Self {
        let user_id = Uuid::new_v4().to_string();
        let referral_code = format!("REF{}", &user_id[..8].to_uppercase());
        Self {
            user_id,
            name,
            email,
            phone,
            created_at: Utc::now().timestamp(),
            status: UserStatus::Active,
            referral_code,
            referred_by: None,
        }
    }
}

// ============================================================================
// User Plan
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPlan {
    pub plan_id: String,
    pub user_id: String,
    pub worker_id: String,
    pub plan_type: String,
    pub total_credits: i64,
    pub used_credits: i64,
    pub remaining_credits: i64,
    pub plan_start: i64,
    pub plan_expiry: Option<i64>,
    pub status: String,
    pub created_at: i64,
}

impl UserPlan {
    pub fn new(user_id: String, worker_id: String, plan_type: String, total_credits: i64, expiry_days: Option<i64>) -> Self {
        let now = Utc::now().timestamp();
        let plan_id = Uuid::new_v4().to_string();
        let plan_expiry = expiry_days.map(|days| now + days * 86400);
        Self {
            plan_id,
            user_id,
            worker_id,
            plan_type,
            total_credits,
            used_credits: 0,
            remaining_credits: total_credits,
            plan_start: now,
            plan_expiry,
            status: "active".to_string(),
            created_at: now,
        }
    }

    pub fn is_expired(&self) -> bool {
        if let Some(expiry) = self.plan_expiry {
            return Utc::now().timestamp() > expiry;
        }
        false
    }

    pub fn has_credits(&self) -> bool {
        self.remaining_credits > 0 && self.status == "active"
    }

    pub fn consume_credit(&mut self) -> bool {
        if self.has_credits() {
            self.remaining_credits -= 1;
            self.used_credits += 1;
            true
        } else {
            false
        }
    }
}

// ============================================================================
// User Session
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSession {
    pub session_id: String,
    pub user_id: String,
    pub worker_id: String,
    pub plan_id: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub status: String,
    pub output_path: Option<String>,
    pub credits_consumed: i64,
    pub feedback_score: Option<i32>,
    pub feedback_text: Option<String>,
}

impl UserSession {
    pub fn new(user_id: String, worker_id: String, plan_id: String) -> Self {
        Self {
            session_id: Uuid::new_v4().to_string(),
            user_id,
            worker_id,
            plan_id,
            started_at: Utc::now().timestamp(),
            completed_at: None,
            status: "pending".to_string(),
            output_path: None,
            credits_consumed: 1,
            feedback_score: None,
            feedback_text: None,
        }
    }

    pub fn complete(&mut self, output_path: String) {
        self.completed_at = Some(Utc::now().timestamp());
        self.status = "completed".to_string();
        self.output_path = Some(output_path);
    }

    pub fn fail(&mut self, reason: &str) {
        self.completed_at = Some(Utc::now().timestamp());
        self.status = format!("failed:{}", reason);
    }
}

// ============================================================================
// Payment Record
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payment {
    pub payment_id: String,
    pub user_id: String,
    pub plan_id: String,
    pub amount: i64, // in paise (₹499 = 49900)
    pub currency: String,
    pub gpay_reference: Option<String>,
    pub status: PaymentStatus,
    pub created_at: i64,
}

impl Payment {
    pub fn new(user_id: String, plan_id: String, amount: i64) -> Self {
        Self {
            payment_id: Uuid::new_v4().to_string(),
            user_id,
            plan_id,
            amount,
            currency: "INR".to_string(),
            gpay_reference: None,
            status: PaymentStatus::Pending,
            created_at: Utc::now().timestamp(),
        }
    }

    pub fn success(mut self, gpay_reference: String) -> Self {
        self.status = PaymentStatus::Success;
        self.gpay_reference = Some(gpay_reference);
        self
    }

    pub fn failed(mut self) -> Self {
        self.status = PaymentStatus::Failed;
        self
    }
}

// ============================================================================
// Discount Code
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscountCode {
    pub code: String,
    pub user_id: String,
    pub discount_percent: i32,
    pub created_at: i64,
    pub expires_at: i64,
    pub used: i32,
}

impl DiscountCode {
    pub fn new(user_id: String, discount_percent: i32, valid_days: i64) -> Self {
        let now = Utc::now().timestamp();
        let code = format!("DISC{}", Uuid::new_v4().as_simple().to_string().chars().take(8).collect::<String>().to_uppercase());
        Self {
            code,
            user_id,
            discount_percent,
            created_at: now,
            expires_at: now + valid_days * 86400,
            used: 0,
        }
    }

    pub fn is_valid(&self) -> bool {
        Utc::now().timestamp() < self.expires_at && self.used == 0
    }
}

// ============================================================================
// Referral Record
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Referral {
    pub referral_id: String,
    pub referrer_id: String,
    pub referee_id: String,
    pub converted: i32,
    pub reward_issued: i32,
    pub created_at: i64,
}

impl Referral {
    pub fn new(referrer_id: String, referee_id: String) -> Self {
        Self {
            referral_id: Uuid::new_v4().to_string(),
            referrer_id,
            referee_id,
            converted: 0,
            reward_issued: 0,
            created_at: Utc::now().timestamp(),
        }
    }
}

// ============================================================================
// Support Ticket
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupportTicket {
    pub ticket_id: String,
    pub user_id: String,
    pub subject: String,
    pub message: String,
    pub status: String,
    pub response: Option<String>,
    pub created_at: i64,
    pub resolved_at: Option<i64>,
}

impl SupportTicket {
    pub fn new(user_id: String, subject: String, message: String) -> Self {
        Self {
            ticket_id: Uuid::new_v4().to_string(),
            user_id,
            subject,
            message,
            status: "open".to_string(),
            response: None,
            created_at: Utc::now().timestamp(),
            resolved_at: None,
        }
    }

    pub fn resolve(&mut self, response: String) {
        self.response = Some(response);
        self.status = "resolved".to_string();
        self.resolved_at = Some(Utc::now().timestamp());
    }
}

// ============================================================================
// Notification Queue Entry
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub notification_id: String,
    pub user_id: String,
    pub r#type: NotificationType,
    pub channel: NotificationChannel,
    pub content: String,
    pub sent: i32,
    pub sent_at: Option<i64>,
    pub created_at: i64,
}

impl Notification {
    pub fn new(user_id: String, r#type: NotificationType, channel: NotificationChannel, content: String) -> Self {
        Self {
            notification_id: Uuid::new_v4().to_string(),
            user_id,
            r#type,
            channel,
            content,
            sent: 0,
            sent_at: None,
            created_at: Utc::now().timestamp(),
        }
    }
}

// ============================================================================
// Model Score Record
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelScore {
    pub model_name: String,
    pub task_type: String,
    pub success_count: i64,
    pub failure_count: i64,
    pub timeout_count: i64,
    pub last_updated: i64,
}

impl ModelScore {
    pub fn success_rate(&self) -> f64 {
        let total = self.success_count + self.failure_count + self.timeout_count;
        if total == 0 {
            return 0.5; // Default unknown model to middle performance
        }
        self.success_count as f64 / total as f64
    }

    pub fn record_outcome(&mut self, result: &ActionResult) {
        match result {
            ActionResult::Success => self.success_count += 1,
            ActionResult::Failure => self.failure_count += 1,
            ActionResult::Timeout => self.timeout_count += 1,
        }
        self.last_updated = Utc::now().timestamp();
    }
}

// ============================================================================
// Audit Log Entry
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    pub log_id: String,
    pub actor: String, // "admin" or "system"
    pub action: String,
    pub target_type: String,
    pub target_id: String,
    pub before_value: Option<String>,
    pub after_value: Option<String>,
    pub timestamp: i64,
}

impl AuditLogEntry {
    pub fn new(actor: String, action: String, target_type: String, target_id: String) -> Self {
        Self {
            log_id: Uuid::new_v4().to_string(),
            actor,
            action,
            target_type,
            target_id,
            before_value: None,
            after_value: None,
            timestamp: Utc::now().timestamp(),
        }
    }

    pub fn with_diff(mut self, before: Option<String>, after: Option<String>) -> Self {
        self.before_value = before;
        self.after_value = after;
        self
    }
}

// ============================================================================
// Outcome Record (per-worker D1 mistake memory)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeRecord {
    pub id: String,
    pub worker_name: String,
    pub action_type: String,
    pub input_fingerprint: String,
    pub input_embedding: Vec<u8>, // binary embedding
    pub model_used: String,
    pub prompt_strategy: String,
    pub result: ActionResult,
    pub failure_reason: Option<String>,
    pub timestamp: i64,
}

// ============================================================================
// Step Result (for ConversationManager Pipeline pattern)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub step: u8,
    pub result: String,
    pub metadata: Option<serde_json::Value>,
}

// ============================================================================
// GooglePay Webhook Payload
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GooglePayWebhookPayload {
    pub merchant_id: String,
    pub transaction_id: String,
    pub amount: String,
    pub currency: String,
    pub timestamp: i64,
    pub signature: String,
    pub user_data: serde_json::Value,
}

// ============================================================================
// Master Registry Entry
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub id: String,
    pub data_type: String,
    pub worker: String,
    pub primary_location: String,
    pub primary_db: Option<String>,
    pub primary_status: String,
    pub fallback_location: String,
    pub fallback_path: String,
    pub created_at: i64,
    pub sync_status: SyncStatus,
    pub last_verified: i64,
}

// ============================================================================
// Master Registry Full Structure
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasterRegistry {
    pub last_updated: i64,
    pub storage_health: StorageHealth,
    pub entries: Vec<RegistryEntry>,
}

impl MasterRegistry {
    pub fn new() -> Self {
        Self {
            last_updated: Utc::now().timestamp(),
            storage_health: StorageHealth::default(),
            entries: Vec::new(),
        }
    }
}

// ============================================================================
// Storage Health
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageHealth {
    pub d1_databases: std::collections::HashMap<String, DbHealth>,
    pub kv_namespaces: std::collections::HashMap<String, NamespaceHealth>,
    pub gdrive_accounts: Vec<GDriveAccountHealth>,
}

impl Default for StorageHealth {
    fn default() -> Self {
        Self {
            d1_databases: std::collections::HashMap::new(),
            kv_namespaces: std::collections::HashMap::new(),
            gdrive_accounts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbHealth {
    pub status: StorageHealthStatus,
    pub last_checked: i64,
    pub outage_start: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceHealth {
    pub status: StorageHealthStatus,
    pub last_checked: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GDriveAccountHealth {
    pub index: usize,
    pub status: StorageHealthStatus,
    pub used_bytes: u64,
    pub total_bytes: u64, // 15GB = 16106127360
}

// ============================================================================
// Google Drive Service Account Credentials
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GDriveCredentials {
    pub r#type: String,
    pub project_id: String,
    pub private_key_id: String,
    pub private_key: String,
    pub client_email: String,
    pub client_id: String,
    pub auth_uri: String,
    pub token_uri: String,
    pub auth_provider_x509_cert_url: String,
    pub client_x509_cert_url: String,
}

impl GDriveCredentials {
    pub fn from_base64(encoded: &str) -> crate::Result<Self> {
        let decoded = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            encoded,
        )
        .map_err(|e| crate::CoreError::SerializationError(format!("Failed to decode GDrive credentials: {}", e)))?;

        serde_json::from_slice(&decoded)
            .map_err(|e| crate::CoreError::SerializationError(format!("Failed to parse GDrive credentials: {}", e)))
    }

    pub fn to_base64(&self) -> crate::Result<String> {
        let json = serde_json::to_string(self)
            .map_err(|e| crate::CoreError::SerializationError(format!("Failed to serialize GDrive credentials: {}", e)))?;

        Ok(base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            json.as_bytes(),
        ))
    }
}

// ============================================================================
// Google Drive File Metadata
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GDriveFile {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub size: Option<u64>,
    pub created_time: String,
    pub modified_time: String,
    pub parents: Vec<String>,
    pub web_view_link: Option<String>,
    pub download_url: Option<String>,
}

// ============================================================================
// Job Record (per-worker)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub job_id: String,
    pub user_id: String,
    pub worker_id: String,
    pub status: JobStatus,
    pub requirements: serde_json::Value,
    pub output: Option<serde_json::Value>,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub error: Option<String>,
    pub feedback: Option<String>,
    pub step_results: Vec<StepResult>,
}

impl JobRecord {
    pub fn new(user_id: String, worker_id: String, requirements: serde_json::Value) -> Self {
        let now = Utc::now().timestamp();
        Self {
            job_id: Uuid::new_v4().to_string(),
            user_id,
            worker_id,
            status: JobStatus::Pending,
            requirements,
            output: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
            error: None,
            feedback: None,
            step_results: Vec::new(),
        }
    }

    pub fn add_step_result(&mut self, step: StepResult) {
        self.step_results.push(step);
        self.updated_at = Utc::now().timestamp();
    }

    pub fn get_step_result(&self, step: u8) -> Option<&StepResult> {
        self.step_results.iter().find(|s| s.step == step)
    }
}
