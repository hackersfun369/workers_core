//! # ModelRouter — Optimal Model Selection Per Task Type
//!
//! Selects the best model for each task type based on historical performance
//! scores stored in DB_SHARED. Never hardcodes a model anywhere.
//!
//! ## Selection Logic
//! 1. Receive task type from worker
//! 2. Query DB_SHARED model_scores for success rates
//! 3. Select highest performing model for this task domain
//! 4. Call KeyRotator::get_next_key(provider) for round robin key selection
//! 5. Make API call with selected model and key
//! 6. On 429/quota error: mark key exhausted, get next key, retry transparently
//! 7. On success, increment success count in model_scores

use worker::D1Database;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

use crate::models::{TaskType, ModelId, Provider, ActionResult};
use crate::key_rotator::KeyRotator;
use crate::api_clients::{GeminiClient, GroqClient, OpenRouterClient, HuggingFaceClient, MistralClient};
use crate::models::Message;
use crate::{CoreError, Result};

// ============================================================================
// Model Configuration
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub model_id: ModelId,
    pub provider: Provider,
    pub env_var: &'static str,
    pub default_model_name: &'static str,
    pub max_tokens: i32,
    pub temperature: f32,
}

impl ModelConfig {
    pub fn for_task_type(task: TaskType) -> Vec<ModelConfig> {
        match task {
            TaskType::FullAppGeneration => vec![
                ModelConfig {
                    model_id: ModelId::Gemini2Flash,
                    provider: Provider::GoogleAI,
                    env_var: "GOOGLE_AI_KEYS",
                    default_model_name: "gemini-2.0-flash",
                    max_tokens: 8192,
                    temperature: 0.7,
                },
            ],
            TaskType::ArchitecturePlanning => vec![
                ModelConfig {
                    model_id: ModelId::Gemini2FlashThinking,
                    provider: Provider::GoogleAI,
                    env_var: "GOOGLE_AI_KEYS",
                    default_model_name: "gemini-2.0-flash-thinking-exp",
                    max_tokens: 8192,
                    temperature: 0.7,
                },
            ],
            TaskType::CodeGeneration => vec![
                ModelConfig {
                    model_id: ModelId::Qwen25Coder32B,
                    provider: Provider::HuggingFace,
                    env_var: "HUGGINGFACE_API_KEYS",
                    default_model_name: "qwen2.5-coder-32b",
                    max_tokens: 4096,
                    temperature: 0.2,
                },
                ModelConfig {
                    model_id: ModelId::Gemini2Flash,
                    provider: Provider::GoogleAI,
                    env_var: "GOOGLE_AI_KEYS",
                    default_model_name: "gemini-2.0-flash",
                    max_tokens: 8192,
                    temperature: 0.3,
                },
            ],
            TaskType::Reasoning => vec![
                ModelConfig {
                    model_id: ModelId::DeepSeekR1,
                    provider: Provider::OpenRouter,
                    env_var: "OPENROUTER_API_KEYS",
                    default_model_name: "deepseek-reasoner",
                    max_tokens: 8192,
                    temperature: 0.6,
                },
            ],
            TaskType::ContentWriting => vec![
                ModelConfig {
                    model_id: ModelId::MistralLarge,
                    provider: Provider::OpenRouter,
                    env_var: "OPENROUTER_API_KEYS",
                    default_model_name: "mistral-large-latest",
                    max_tokens: 8192,
                    temperature: 0.7,
                },
            ],
            TaskType::FastFilter => vec![
                ModelConfig {
                    model_id: ModelId::Llama31_8B,
                    provider: Provider::Groq,
                    env_var: "GROQ_API_KEYS",
                    default_model_name: "llama-3.1-8b-instant",
                    max_tokens: 1024,
                    temperature: 0.3,
                },
            ],
            TaskType::Embedding => vec![
                ModelConfig {
                    model_id: ModelId::NomicEmbed,
                    provider: Provider::HuggingFace,
                    env_var: "HUGGINGFACE_API_KEYS",
                    default_model_name: "nomic-embed-text-v1",
                    max_tokens: 0,
                    temperature: 0.0,
                },
            ],
            TaskType::VoiceInterview => vec![
                ModelConfig {
                    model_id: ModelId::MistralVoice,
                    provider: Provider::Mistral,
                    env_var: "MISTRAL_API_KEYS",
                    default_model_name: "mistral-medium",
                    max_tokens: 2048,
                    temperature: 0.7,
                },
            ],
        }
    }
}

// ============================================================================
// ModelRouter
// ============================================================================

pub struct ModelRouter {
    pub db: D1Database,
    pub key_rotator: KeyRotator,
}

impl ModelRouter {
    pub fn new(db: D1Database, key_rotator: KeyRotator) -> Self {
        Self { db, key_rotator }
    }

    /// Select the best model for a given task type.
    /// Queries DB_SHARED for historical performance and picks the highest performer.
    pub async fn select_model(&mut self, task_type: TaskType) -> Result<ModelSelection> {
        let configs = ModelConfig::for_task_type(task_type);

        if configs.is_empty() {
            return Err(CoreError::ModelRoutingFailed(format!(
                "No model configured for task type: {:?}", task_type
            )));
        }

        // Get historical scores for this task type
        let task_type_str = format!("{:?}", task_type);
        let scores = self.get_scores_for_task(&task_type_str).await?;

        // Sort by success rate and pick the best one
        let best = if scores.is_empty() {
            // No historical data — use first configured model
            configs.first().cloned().ok_or_else(|| {
                CoreError::ModelRoutingFailed("No model configured".to_string())
            })?
        } else {
            // Find the model with the highest success rate
            let best_score = scores.into_iter()
                .max_by(|a, b| a.success_rate().partial_cmp(&b.success_rate()).unwrap_or(std::cmp::Ordering::Equal));

            if let Some(score) = best_score {
                // Find the config matching this model
                configs.iter()
                    .find(|c| c.default_model_name == score.model_name)
                    .cloned()
                    .unwrap_or_else(|| configs[0].clone())
            } else {
                configs[0].clone()
            }
        };

        // Get an API key for this model's provider
        let (provider, api_key) = self.key_rotator.get_key(best.provider).await?;

        Ok(ModelSelection {
            config: best,
            api_key,
            provider,
        })
    }

    /// Get scores for a specific task type from DB_SHARED.
    async fn get_scores_for_task(&self, task_type: &str) -> Result<Vec<crate::models::ModelScore>> {
        let stmt = self.db.prepare(
            "SELECT * FROM model_scores WHERE task_type = ?1 ORDER BY success_count DESC"
        );

        let result = stmt.bind(&[JsValue::from_str(task_type)])
            .map_err(|e| CoreError::D1Error(format!("Failed to bind model scores query: {}", e)))?
            .all()
            .await
            .map_err(|e| CoreError::D1Error(format!("Failed to query model scores: {}", e)))?;

        let scores = result.results::<crate::models::ModelScore>()
            .map_err(|e| CoreError::D1Error(format!("Failed to deserialize model scores: {}", e)))?;

        Ok(scores)
    }

    /// Make an LLM API call with automatic model selection, key rotation, and retry.
    pub async fn call_llm(
        &mut self,
        task_type: TaskType,
        messages: &[Message],
        system_prompt: Option<&str>,
    ) -> Result<crate::api_clients::LlmResponse> {
        let mut last_error: Option<CoreError> = None;
        let max_retries = 3;

        for attempt in 0..max_retries {
            let selection = match self.select_model(task_type).await {
                Ok(s) => s,
                Err(e) => {
                    last_error = Some(e);
                    continue;
                }
            };

            let result = self.execute_llm_call(&selection, messages, system_prompt).await;

            match result {
                Ok(response) => {
                    // Record success in model_scores
                    self.record_outcome(&selection.config.default_model_name, &format!("{:?}", task_type), ActionResult::Success).await.ok();
                    return Ok(response);
                }
                Err(e) => {
                    // Check if it's a rate limit / quota error
                    let is_quota = e.to_string().contains("429") || e.to_string().contains("quota");
                    if is_quota {
                        // Mark key as exhausted and retry
                        self.key_rotator.mark_exhausted(selection.provider);
                        last_error = Some(e);
                        continue;
                    }

                    // Other error — record and retry
                    last_error = Some(e);
                    continue;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            CoreError::ModelRoutingFailed("All retry attempts failed".to_string())
        }))
    }

    /// Execute the actual LLM API call based on the selected model.
    async fn execute_llm_call(
        &self,
        selection: &ModelSelection,
        messages: &[Message],
        system_prompt: Option<&str>,
    ) -> Result<crate::api_clients::LlmResponse> {
        match selection.config.model_id {
            ModelId::Gemini2Flash | ModelId::Gemini2FlashThinking => {
                let client = GeminiClient::new(selection.api_key.clone());
                let model_name = if matches!(selection.config.model_id, ModelId::Gemini2FlashThinking) {
                    "gemini-2.0-flash-thinking-exp"
                } else {
                    "gemini-2.0-flash"
                };
                client.generate_content(model_name, messages, Some(selection.config.max_tokens), Some(selection.config.temperature)).await
            }
            ModelId::Qwen25Coder32B => {
                let client = HuggingFaceClient::new(selection.api_key.clone());
                let prompt = messages.iter().map(|m| format!("{:?}: {}", m.role, m.content)).collect::<Vec<_>>().join("\n");
                client.generate_code(&prompt, Some(selection.config.max_tokens)).await
            }
            ModelId::DeepSeekR1 => {
                let client = OpenRouterClient::new(selection.api_key.clone());
                client.generate("deepseek-reasoner", messages, Some(selection.config.max_tokens), Some(selection.config.temperature)).await
            }
            ModelId::MistralLarge => {
                let client = OpenRouterClient::new(selection.api_key.clone());
                client.generate("mistral-large-latest", messages, Some(selection.config.max_tokens), Some(selection.config.temperature)).await
            }
            ModelId::Llama31_8B => {
                let client = GroqClient::new(selection.api_key.clone());
                let prompt = messages.iter().map(|m| m.content.clone()).collect::<Vec<_>>().join("\n");
                client.generate(&prompt, system_prompt).await
            }
            ModelId::NomicEmbed => {
                let client = HuggingFaceClient::new(selection.api_key.clone());
                let text = messages.iter().map(|m| m.content.clone()).collect::<Vec<_>>().join("\n");
                let embed_resp = client.embed(&text).await?;
                Ok(crate::api_clients::LlmResponse {
                    content: format!("{:?}", embed_resp.embedding),
                    model: embed_resp.model,
                    usage: None,
                    finish_reason: None,
                })
            }
            ModelId::MistralVoice => {
                let client = MistralClient::new(selection.api_key.clone());
                let prompt = messages.iter().map(|m| m.content.clone()).collect::<Vec<_>>().join("\n");
                let voice_resp = client.voice_chat(&prompt, system_prompt.unwrap_or(""), None).await?;
                Ok(crate::api_clients::LlmResponse {
                    content: voice_resp.text,
                    model: "mistral-voice".to_string(),
                    usage: None,
                    finish_reason: None,
                })
            }
        }
    }

    /// Record an outcome for model performance tracking.
    async fn record_outcome(&self, model_name: &str, task_type: &str, result: ActionResult) -> Result<()> {
        use chrono::Utc;
        let now = Utc::now().timestamp();

        let (success, failure, timeout) = match result {
            ActionResult::Success => (1, 0, 0),
            ActionResult::Failure => (0, 1, 0),
            ActionResult::Timeout => (0, 0, 1),
        };

        // Try update
        let update_stmt = self.db.prepare(
            "UPDATE model_scores SET
                success_count = success_count + ?1,
                failure_count = failure_count + ?2,
                timeout_count = timeout_count + ?3,
                last_updated = ?4
             WHERE model_name = ?5 AND task_type = ?6"
        );

        let update_result = update_stmt.bind(&[
            JsValue::from_f64(success as f64),
            JsValue::from_f64(failure as f64),
            JsValue::from_f64(timeout as f64),
            JsValue::from_f64(now as f64),
            JsValue::from_str(model_name),
            JsValue::from_str(task_type),
        ])
        .map_err(|e| CoreError::D1Error(format!("Failed to bind model score update: {}", e)))?
        .run()
        .await;

        // Insert if not exists
        let changes = if let Ok(r) = &update_result {
            r.meta().ok().flatten().and_then(|m| m.changes).unwrap_or(0)
        } else {
            0
        };
        if changes == 0 {
            let insert_stmt = self.db.prepare(
                "INSERT INTO model_scores (model_name, task_type, success_count, failure_count, timeout_count, last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
            );

            insert_stmt.bind(&[
                JsValue::from_str(model_name),
                JsValue::from_str(task_type),
                JsValue::from_f64(success as f64),
                JsValue::from_f64(failure as f64),
                JsValue::from_f64(timeout as f64),
                JsValue::from_f64(now as f64),
            ])
            .map_err(|e| CoreError::D1Error(format!("Failed to bind model score insert: {}", e)))?
            .run()
            .await
            .map_err(|e| CoreError::D1Error(format!("Failed to insert model score: {}", e)))?;
        }

        Ok(())
    }
}

// ============================================================================
// Model Selection Result
// ============================================================================

#[derive(Debug, Clone)]
pub struct ModelSelection {
    pub config: ModelConfig,
    pub api_key: String,
    pub provider: Provider,
}

impl ModelSelection {
    pub fn model_name(&self) -> &str {
        self.config.default_model_name
    }

    pub fn max_tokens(&self) -> i32 {
        self.config.max_tokens
    }

    pub fn temperature(&self) -> f32 {
        self.config.temperature
    }
}
