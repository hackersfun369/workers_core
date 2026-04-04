use wasm_bindgen_test::*;
use worker_core::models::*;
use worker_core::abuse::{AbuseGuard, AbuseGuardResult, SubmissionResult};
use worker_core::model_router::ModelConfig;

wasm_bindgen_test_configure!(run_in_browser);

// ============================================================================
// Model Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_task_type_variants() {
    // Verify all 8 task types exist and serialize correctly
    let types = vec![
        TaskType::FullAppGeneration,
        TaskType::ArchitecturePlanning,
        TaskType::CodeGeneration,
        TaskType::Reasoning,
        TaskType::ContentWriting,
        TaskType::FastFilter,
        TaskType::Embedding,
        TaskType::VoiceInterview,
    ];

    assert_eq!(types.len(), 8);

    // Verify serialization
    for task_type in types {
        let json = serde_json::to_string(&task_type).unwrap();
        assert!(!json.is_empty());
    }
}

#[wasm_bindgen_test]
fn test_model_config_for_task() {
    for task_type in [
        TaskType::FullAppGeneration,
        TaskType::ArchitecturePlanning,
        TaskType::CodeGeneration,
        TaskType::Reasoning,
        TaskType::ContentWriting,
        TaskType::FastFilter,
        TaskType::Embedding,
        TaskType::VoiceInterview,
    ] {
        let configs = ModelConfig::for_task_type(task_type);
        assert!(!configs.is_empty(), "No config for {:?}", task_type);

        for config in &configs {
            assert!(!config.default_model_name.is_empty());
            assert!(config.max_tokens >= 0);
        }
    }
}

// ============================================================================
// SimHash Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_simhash_deterministic() {
    let text = "the quick brown fox jumps over the lazy dog";
    let hash1 = SimHash::from_text(text);
    let hash2 = SimHash::from_text(text);

    assert_eq!(hash1, hash2, "SimHash should be deterministic");
}

#[wasm_bindgen_test]
fn test_simhash_identical_distance_zero() {
    let text = "hello world test";
    let hash1 = SimHash::from_text(text);
    let hash2 = SimHash::from_text(text);

    assert_eq!(hash1.distance(&hash2), 0);
    assert!((hash1.similarity(&hash2) - 1.0).abs() < 0.001);
}

#[wasm_bindgen_test]
fn test_simhash_different_texts() {
    let text1 = "rust programming language is great";
    let text2 = "pizza is a popular food worldwide";

    let hash1 = SimHash::from_text(text1);
    let hash2 = SimHash::from_text(text2);

    // Completely different texts should have high distance
    assert!(hash1.distance(&hash2) > 20);
    assert!(hash1.similarity(&hash2) < 0.5);
}

#[wasm_bindgen_test]
fn test_simhash_similar_texts() {
    let text1 = "build a web application with rust and cloudflare workers";
    let text2 = "build a web app using rust and cloudflare workers";

    let hash1 = SimHash::from_text(text1);
    let hash2 = SimHash::from_text(text2);

    // Similar texts should have moderate similarity
    assert!(hash1.similarity(&hash2) > 0.6);
}

// ============================================================================
// Binary Embedding Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_binary_embedding_dimensions() {
    let floats = vec![0.5f32; 768];
    let binary = BinaryEmbedding::from_floats(&floats);

    assert_eq!(binary.dimensions, 768);
    assert_eq!(binary.bits.len(), 96); // 768 bits / 8 = 96 bytes
}

#[wasm_bindgen_test]
fn test_binary_embedding_serialization() {
    let floats: Vec<f32> = (0..768).map(|i| (i as f32 - 384.0) / 384.0).collect();
    let binary = BinaryEmbedding::from_floats(&floats);

    let bytes = binary.to_bytes();
    assert!(!bytes.is_empty());

    let restored = BinaryEmbedding::from_bytes(&bytes);
    assert!(restored.is_some());
    let restored = restored.unwrap();

    assert_eq!(restored.dimensions, binary.dimensions);
    assert_eq!(restored.bits, binary.bits);
}

#[wasm_bindgen_test]
fn test_binary_embedding_similarity_identical() {
    let floats = vec![1.0f32; 768];
    let b1 = BinaryEmbedding::from_floats(&floats);
    let b2 = BinaryEmbedding::from_floats(&floats);

    let sim = b1.similarity(&b2);
    assert!((sim - 1.0).abs() < 0.001);
}

#[wasm_bindgen_test]
fn test_binary_embedding_similarity_opposite() {
    let floats1 = vec![1.0f32; 768];
    let floats2 = vec![-1.0f32; 768];
    let b1 = BinaryEmbedding::from_floats(&floats1);
    let b2 = BinaryEmbedding::from_floats(&floats2);

    let sim = b1.similarity(&b2);
    assert!(sim < 0.1); // Should be close to 0
}

// ============================================================================
// Count-Min Sketch Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_count_min_sketch_basic() {
    let mut sketch = CountMinSketch::new();

    sketch.add("timeout_error");
    sketch.add("timeout_error");
    sketch.add("timeout_error");

    assert!(sketch.estimate("timeout_error") >= 3);
}

#[wasm_bindgen_test]
fn test_count_min_sketch_never_underestimates() {
    let mut sketch = CountMinSketch::new();

    sketch.add("error_a");
    sketch.add("error_a");
    sketch.add("error_b");

    // CMS never underestimates
    assert!(sketch.estimate("error_a") >= 2);
    assert!(sketch.estimate("error_b") >= 1);
}

#[wasm_bindgen_test]
fn test_count_min_sketch_zero_count() {
    let sketch = CountMinSketch::new();
    assert_eq!(sketch.estimate("nonexistent"), 0);
}

#[wasm_bindgen_test]
fn test_count_min_sketch_overflow() {
    let mut sketch = CountMinSketch::new();

    // Add 300 times (max counter is 255)
    for _ in 0..300 {
        sketch.add("frequent_error");
    }

    // Should cap at 255
    assert!(sketch.estimate("frequent_error") <= 255);
    assert!(sketch.estimate("frequent_error") > 0);
}

// ============================================================================
// MemoryMapEntry Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_memory_map_entry_creation() {
    let embedding = vec![0.5f32; 768];
    let entry = MemoryMapEntry::new(
        "test input for generating a web application",
        &embedding,
        Some("timeout_no_response"),
        Some("retry_with_backoff"),
        "gemini-2.0-flash",
        "full_app_generation",
        2, // failure
        1234567890,
        "generate_all_at_once",
        6, // Worker 6
    );

    assert_eq!(entry.timestamp, 1234567890);
    assert_eq!(entry.worker_id, 6);
    assert_eq!(entry.outcome, 2); // failure
    assert_ne!(entry.simhash.0, 0); // Should have a non-zero hash
}

// ============================================================================
// MemoryMap Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_memory_map_add_and_find_similar() {
    let mut map = MemoryMap::new("test-worker".to_string(), 100);
    let embedding = vec![0.5f32; 768];

    // Add several entries
    for i in 0..5 {
        map.add_entry(MemoryMapEntry::new(
            &format!("create a web application with feature {}", i),
            &embedding,
            None,
            None,
            "gemini-2.0-flash",
            "full_app_generation",
            1, // success
            1234567890 + i,
            "generate_all",
            6,
        ));
    }

    // Query with similar input
    let results = map.find_similar(
        "create a web application with feature 2",
        &embedding,
        3,
    );

    assert!(!results.is_empty());
    assert!(results.len() <= 3);
}

#[wasm_bindgen_test]
fn test_memory_map_capacity_management() {
    let mut map = MemoryMap::new("test-worker".to_string(), 3);
    let embedding = vec![0.5f32; 768];

    // Add 3 entries (at capacity)
    for i in 0..3 {
        map.add_entry(MemoryMapEntry::new(
            &format!("input {}", i),
            &embedding,
            None,
            None,
            "model",
            "task",
            1,
            1234567890 + i,
            "strategy",
            2,
        ));
    }
    assert_eq!(map.entries.len(), 3);

    // Add 4th → oldest should be archived
    let archived = map.add_entry(MemoryMapEntry::new(
        "input 3",
        &embedding,
        None,
        None,
        "model",
        "task",
        1,
        1234567893,
        "strategy",
        2,
    ));

    assert!(archived.is_some());
    assert_eq!(map.entries.len(), 3);
}

#[wasm_bindgen_test]
fn test_memory_map_serialization() {
    let map = MemoryMap::new("test-worker".to_string(), 100);

    let bytes = map.to_serialized().unwrap();
    assert!(!bytes.is_empty());

    let restored = MemoryMap::from_serialized(&bytes).unwrap();
    assert_eq!(restored.worker_id, map.worker_id);
    assert_eq!(restored.max_entries, map.max_entries);
}

#[wasm_bindgen_test]
fn test_memory_map_top_patterns() {
    let mut map = MemoryMap::new("test-worker".to_string(), 100);
    let embedding = vec![0.5f32; 768];

    // Add some failures
    for _ in 0..5 {
        map.add_entry(MemoryMapEntry::new(
            "test input",
            &embedding,
            Some("timeout_error"),
            None,
            "model_a",
            "code_generation",
            2, // failure
            1234567890,
            "strategy_a",
            6,
        ));
    }

    // Add some successes
    for _ in 0..3 {
        map.add_entry(MemoryMapEntry::new(
            "test input",
            &embedding,
            None,
            Some("retry_with_backoff"),
            "model_b",
            "code_generation",
            1, // success
            1234567890,
            "strategy_b",
            6,
        ));
    }

    let top_failures = map.top_failure_reasons(5);
    assert!(!top_failures.is_empty());
}

// ============================================================================
// User Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_user_creation() {
    let user = User::new(
        "Test User".to_string(),
        "test@example.com".to_string(),
        "+919876543210".to_string(),
    );

    assert_eq!(user.name, "Test User");
    assert_eq!(user.email, "test@example.com");
    assert_eq!(user.phone, "+919876543210");
    assert!(user.referral_code.starts_with("REF"));
    assert_eq!(user.status, UserStatus::Active);
    assert!(user.created_at > 0);
}

#[wasm_bindgen_test]
fn test_user_plan_creation() {
    let plan = UserPlan::new(
        "user_123".to_string(),
        "worker-6".to_string(),
        "single_analysis".to_string(),
        1,
        Some(30),
    );

    assert_eq!(plan.user_id, "user_123");
    assert_eq!(plan.total_credits, 1);
    assert_eq!(plan.remaining_credits, 1);
    assert!(plan.has_credits());
    assert_eq!(plan.status, "active");
}

#[wasm_bindgen_test]
fn test_user_plan_credit_consumption() {
    let mut plan = UserPlan::new(
        "user_123".to_string(),
        "worker-6".to_string(),
        "3_session_pack".to_string(),
        3,
        Some(30),
    );

    assert!(plan.consume_credit());
    assert_eq!(plan.remaining_credits, 2);
    assert_eq!(plan.used_credits, 1);

    assert!(plan.consume_credit());
    assert_eq!(plan.remaining_credits, 1);

    assert!(plan.consume_credit());
    assert_eq!(plan.remaining_credits, 0);

    // No more credits
    assert!(!plan.consume_credit());
}

#[wasm_bindgen_test]
fn test_user_plan_expiry() {
    // Create an already-expired plan
    let past_timestamp = chrono::Utc::now().timestamp() - 86400; // 1 day ago
    let mut plan = UserPlan::new(
        "user_123".to_string(),
        "worker-6".to_string(),
        "single_analysis".to_string(),
        1,
        Some(0), // expired immediately
    );
    // Manually set to past for testing
    plan.plan_start = past_timestamp;
    plan.plan_expiry = Some(past_timestamp);

    assert!(plan.is_expired());
    assert!(!plan.has_credits()); // Even with credits, expired plans have no access
}

#[wasm_bindgen_test]
fn test_discount_code_validity() {
    let code = DiscountCode::new(
        "user_123".to_string(),
        10,
        30, // 30 days validity
    );

    assert!(code.is_valid());
    assert_eq!(code.discount_percent, 10);
    assert!(code.code.starts_with("DISC"));
}

// ============================================================================
// Payment Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_payment_creation() {
    let payment = Payment::new(
        "user_123".to_string(),
        "plan_456".to_string(),
        49900, // ₹499 in paise
    );

    assert_eq!(payment.user_id, "user_123");
    assert_eq!(payment.amount, 49900);
    assert_eq!(payment.currency, "INR");
    assert_eq!(payment.status, PaymentStatus::Pending);
}

#[wasm_bindgen_test]
fn test_payment_success() {
    let payment = Payment::new(
        "user_123".to_string(),
        "plan_456".to_string(),
        49900,
    );

    let successful = payment.success("gpay_txn_123".to_string());

    assert_eq!(successful.status, PaymentStatus::Success);
    assert_eq!(successful.gpay_reference, Some("gpay_txn_123".to_string()));
}

// ============================================================================
// Session Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_session_creation() {
    let session = UserSession::new(
        "user_123".to_string(),
        "worker-6".to_string(),
        "plan_456".to_string(),
    );

    assert_eq!(session.user_id, "user_123");
    assert_eq!(session.worker_id, "worker-6");
    assert_eq!(session.status, "pending");
    assert!(session.session_id.len() > 0);
}

#[wasm_bindgen_test]
fn test_session_completion() {
    let mut session = UserSession::new(
        "user_123".to_string(),
        "worker-6".to_string(),
        "plan_456".to_string(),
    );

    session.complete("/gdrive/output/path".to_string());

    assert_eq!(session.status, "completed");
    assert_eq!(session.output_path, Some("/gdrive/output/path".to_string()));
    assert!(session.completed_at.is_some());
}

#[wasm_bindgen_test]
fn test_session_failure() {
    let mut session = UserSession::new(
        "user_123".to_string(),
        "worker-6".to_string(),
        "plan_456".to_string(),
    );

    session.fail("timeout_error");

    assert!(session.status.contains("failed"));
    assert!(session.completed_at.is_some());
}

// ============================================================================
// Audit Log Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_audit_log_creation() {
    let log = AuditLogEntry::new(
        "admin".to_string(),
        "update_plan".to_string(),
        "user_plan".to_string(),
        "plan_123".to_string(),
    );

    assert_eq!(log.actor, "admin");
    assert_eq!(log.action, "update_plan");
    assert_eq!(log.target_id, "plan_123");
    assert!(log.timestamp > 0);
}

#[wasm_bindgen_test]
fn test_audit_log_with_diff() {
    let log = AuditLogEntry::new(
        "admin".to_string(),
        "update_plan".to_string(),
        "user_plan".to_string(),
        "plan_123".to_string(),
    )
    .with_diff(
        Some("active".to_string()),
        Some("expired".to_string()),
    );

    assert_eq!(log.before_value, Some("active".to_string()));
    assert_eq!(log.after_value, Some("expired".to_string()));
}

// ============================================================================
// Model Score Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_model_score_success_rate() {
    let mut score = ModelScore {
        model_name: "gemini-2.0-flash".to_string(),
        task_type: "code_generation".to_string(),
        success_count: 80,
        failure_count: 15,
        timeout_count: 5,
        last_updated: chrono::Utc::now().timestamp(),
    };

    let rate = score.success_rate();
    assert!((rate - 0.8).abs() < 0.001); // 80/100 = 0.8

    score.record_outcome(&ActionResult::Success);
    assert!((score.success_rate() - 81.0 / 101.0).abs() < 0.001);
}

#[wasm_bindgen_test]
fn test_model_score_record_outcome() {
    let mut score = ModelScore {
        model_name: "gemini-2.0-flash".to_string(),
        task_type: "code_generation".to_string(),
        success_count: 0,
        failure_count: 0,
        timeout_count: 0,
        last_updated: 0,
    };

    score.record_outcome(&ActionResult::Success);
    assert_eq!(score.success_count, 1);

    score.record_outcome(&ActionResult::Failure);
    assert_eq!(score.failure_count, 1);

    score.record_outcome(&ActionResult::Timeout);
    assert_eq!(score.timeout_count, 1);
    assert!(score.last_updated > 0);
}

// ============================================================================
// GDrive Credentials Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_gdrive_credentials_roundtrip() {
    let creds = GDriveCredentials {
        r#type: "service_account".to_string(),
        project_id: "test-project".to_string(),
        private_key_id: "key123".to_string(),
        private_key: "-----BEGIN PRIVATE KEY-----\ntest\n-----END PRIVATE KEY-----\n".to_string(),
        client_email: "test@test-project.iam.gserviceaccount.com".to_string(),
        client_id: "123456".to_string(),
        auth_uri: "https://accounts.google.com/o/oauth2/auth".to_string(),
        token_uri: "https://oauth2.googleapis.com/token".to_string(),
        auth_provider_x509_cert_url: "https://www.googleapis.com/oauth2/v1/certs".to_string(),
        client_x509_cert_url: "https://www.googleapis.com/robot/v1/metadata/x509/test%40test-project.iam.gserviceaccount.com".to_string(),
    };

    let encoded = creds.to_base64().unwrap();
    assert!(!encoded.is_empty());

    let decoded = GDriveCredentials::from_base64(&encoded).unwrap();
    assert_eq!(decoded.client_email, creds.client_email);
    assert_eq!(decoded.project_id, creds.project_id);
}

// ============================================================================
// AbuseGuard Tests
// ============================================================================

// Note: AbuseGuard requires KV store, so we test the logic components separately

#[wasm_bindgen_test]
fn test_submission_result_variants() {
    // Test that the enum variants work correctly
    use worker_core::abuse::SubmissionResult;

    // These are just type checks, actual behavior needs KV
    let _ = SubmissionResult::New(AbuseGuardResult {
        hourly_count: 1,
        daily_count: 5,
        is_suspended: false,
    });

    let _ = SubmissionResult::Duplicate(AbuseGuardResult {
        hourly_count: 1,
        daily_count: 5,
        is_suspended: false,
    });
}

// ============================================================================
// Pattern Trie Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_pattern_trie_insert_and_lookup() {
    let mut trie = PatternTrie::new();

    let code1 = trie.insert("timeout_no_response");
    assert!(code1 > 0);

    let code2 = trie.insert("timeout_slow_response");
    assert!(code2 > 0);
    assert_ne!(code1, code2);

    // Lookup should return same code
    assert_eq!(trie.lookup("timeout_no_response"), code1);

    // Unknown pattern returns 0
    assert_eq!(trie.lookup("unknown_pattern"), 0);
}

#[wasm_bindgen_test]
fn test_pattern_trie_decode() {
    let mut trie = PatternTrie::new();

    let code = trie.insert("rate_limit_exceeded");
    let decoded = trie.decode(code);

    assert!(decoded.is_some());
    assert_eq!(decoded.unwrap(), "rate_limit_exceeded");
}

// ============================================================================
// Notification Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_notification_creation() {
    let notification = Notification::new(
        "user_123".to_string(),
        NotificationType::PaymentSuccess,
        NotificationChannel::Email,
        "Your payment was successful!".to_string(),
    );

    assert_eq!(notification.user_id, "user_123");
    assert_eq!(notification.content, "Your payment was successful!");
    assert_eq!(notification.sent, 0);
    assert!(notification.sent_at.is_none());
    assert!(notification.created_at > 0);
}

// ============================================================================
// Support Ticket Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_support_ticket_resolution() {
    let mut ticket = SupportTicket::new(
        "user_123".to_string(),
        "Cannot access my account".to_string(),
        "I am unable to login to my account.".to_string(),
    );

    assert_eq!(ticket.status, "open");
    assert!(ticket.response.is_none());

    ticket.resolve("Please reset your password using the forgot password link.".to_string());

    assert_eq!(ticket.status, "resolved");
    assert!(ticket.response.is_some());
    assert!(ticket.resolved_at.is_some());
}

// ============================================================================
// Referral Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_referral_creation() {
    let referral = Referral::new(
        "referrer_123".to_string(),
        "referee_456".to_string(),
    );

    assert_eq!( referral.referrer_id, "referrer_123");
    assert_eq!( referral.referee_id, "referee_456");
    assert_eq!(referral.converted, 0);
    assert_eq!(referral.reward_issued, 0);
}

// ============================================================================
// Job Record Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_job_record_creation() {
    let job = JobRecord::new(
        "user_123".to_string(),
        "worker-6".to_string(),
        serde_json::json!({"description": "Build a todo app"}),
    );

    assert_eq!(job.user_id, "user_123");
    assert_eq!(job.worker_id, "worker-6");
    assert_eq!(job.status, JobStatus::Pending);
    assert!(job.output.is_none());
    assert!(job.step_results.is_empty());
}

#[wasm_bindgen_test]
fn test_job_record_step_results() {
    let mut job = JobRecord::new(
        "user_123".to_string(),
        "worker-6".to_string(),
        serde_json::json!({"description": "Build a todo app"}),
    );

    job.add_step_result(StepResult {
        step: 1,
        result: "Architecture planned".to_string(),
        metadata: Some(serde_json::json!({"components": 5})),
    });

    job.add_step_result(StepResult {
        step: 2,
        result: "Code generated".to_string(),
        metadata: None,
    });

    assert_eq!(job.step_results.len(), 2);

    let step1 = job.get_step_result(1);
    assert!(step1.is_some());
    assert_eq!(step1.unwrap().result, "Architecture planned");

    let step3 = job.get_step_result(3);
    assert!(step3.is_none());
}

// ============================================================================
// Registry Entry Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_registry_entry_creation() {
    let entry = RegistryEntry {
        id: "payment_123".to_string(),
        data_type: "payment".to_string(),
        worker: "worker-ai-humanizer".to_string(),
        primary_location: "d1".to_string(),
        primary_db: Some("DB_SHARED".to_string()),
        primary_status: "healthy".to_string(),
        fallback_location: "gdrive".to_string(),
        fallback_path: "/payments/worker-2/payment_123.json".to_string(),
        created_at: 1234567890,
        sync_status: SyncStatus::DualWritten,
        last_verified: 1234567890,
    };

    assert_eq!(entry.id, "payment_123");
    assert_eq!(entry.sync_status, SyncStatus::DualWritten);
}

// ============================================================================
// Master Registry Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_master_registry_creation() {
    let registry = MasterRegistry::new();
    assert!(registry.last_updated > 0);
    assert!(registry.entries.is_empty());
    assert!(registry.storage_health.d1_databases.is_empty());
    assert!(registry.storage_health.kv_namespaces.is_empty());
}

// ============================================================================
// Cron Schedule Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_cron_schedule_creation() {
    let on_demand = CronSchedule::on_demand(WorkerId::Worker6WebBuilder);
    assert!(on_demand.on_demand);

    let periodic = CronSchedule::periodic(WorkerId::Worker1JobReadiness, "*/15 * * * *");
    assert!(!periodic.on_demand);
    assert_eq!(periodic.cron_expression, "*/15 * * * *");
}

// ============================================================================
// Message Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_message_helpers() {
    let system = Message::system("You are a helpful assistant");
    assert!(matches!(system.role, Role::System));

    let user = Message::user("Hello!");
    assert!(matches!(user.role, Role::User));

    let assistant = Message::assistant("Hi there!");
    assert!(matches!(assistant.role, Role::Assistant));
}

// ============================================================================
// Error Type Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_error_display() {
    use worker_core::CoreError;

    let err = CoreError::KvError("connection refused".to_string());
    assert!(err.to_string().contains("KV operation failed"));

    let err = CoreError::PaymentError("invalid amount".to_string());
    assert!(err.to_string().contains("Payment verification failed"));

    let err = CoreError::KeysExhausted("all groq keys".to_string());
    assert!(err.to_string().contains("All API keys exhausted"));
}

// ============================================================================
// GooglePayWebhook Payload Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_gpay_webhook_payload_serialization() {
    let payload = GooglePayWebhookPayload {
        merchant_id: "merchant_123".to_string(),
        transaction_id: "txn_456".to_string(),
        amount: "49900".to_string(),
        currency: "INR".to_string(),
        timestamp: 1234567890,
        signature: "base64_signature_here".to_string(),
        user_data: serde_json::json!({
            "user_id": "user_789",
            "plan_id": "plan_abc",
        }),
    };

    let json = serde_json::to_string(&payload).unwrap();
    assert!(json.contains("merchant_123"));
    assert!(json.contains("txn_456"));

    let decoded: GooglePayWebhookPayload = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.merchant_id, "merchant_123");
}

// ============================================================================
// Storage Health Tests
// ============================================================================

#[wasm_bindgen_test]
fn test_storage_health_defaults() {
    let health = StorageHealth::default();
    assert!(health.d1_databases.is_empty());
    assert!(health.kv_namespaces.is_empty());
    assert!(health.gdrive_accounts.is_empty());
}

// ============================================================================
// Comprehensive Integration-style Test
// ============================================================================

#[wasm_bindgen_test]
fn test_memory_map_end_to_end() {
    // Create a MemoryMap with realistic data
    let mut map = MemoryMap::new("worker-6-web-builder".to_string(), 50);

    // Simulate a series of job outcomes
    let outcomes = vec![
        ("build todo app with react", "gemini-2.0-flash", ActionResult::Success, None, Some("full_context")),
        ("build todo app with react and dark mode", "gemini-2.0-flash", ActionResult::Failure, Some("token_limit_exceeded"), None),
        ("build e-commerce frontend", "qwen2.5-coder-32b", ActionResult::Success, None, Some("step_by_step")),
        ("build chat application", "gemini-2.0-flash", ActionResult::Timeout, None, None),
        ("build portfolio website", "gemini-2.0-flash", ActionResult::Success, None, Some("full_context")),
    ];

    for (i, (input, model, result, failure_reason, strategy)) in outcomes.iter().enumerate() {
        let embedding: Vec<f32> = vec![0.5 + (i as f32 * 0.05); 768];

        map.add_entry(MemoryMapEntry::new(
            input,
            &embedding,
            failure_reason.as_deref(),
            strategy.as_deref(),
            model,
            "full_app_generation",
            match result {
                ActionResult::Success => 1,
                ActionResult::Failure => 2,
                ActionResult::Timeout => 3,
            },
            1234567890 + i as i64,
            strategy.unwrap_or("default"),
            6,
        ));
    }

    // Verify entries were added
    assert_eq!(map.entries.len(), 5);

    // Find similar to a new input
    let results = map.find_similar(
        "build todo app with react and authentication",
        &vec![0.55; 768], // Similar embedding
        2,
    );

    assert!(!results.is_empty());
    // First result should be one of the "todo app" entries
    assert!(results.len() <= 2);

    // Check failure reasons
    let top_failures = map.top_failure_reasons(5);
    assert!(!top_failures.is_empty());
}
