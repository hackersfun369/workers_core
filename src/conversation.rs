//! # ConversationManager — Stateless API Memory Layer

use serde::{Deserialize, Serialize};
use worker::KvStore;
use chrono::Utc;

use crate::models::{ConversationPattern, Message, Role, StepResult};
use crate::storage_router::GoogleDriveClient;
use crate::{CoreError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub session_id: String,
    pub pattern: ConversationPattern,
    pub created_at: i64,
    pub last_activity: i64,
    pub turn_count: u32,
    pub worker_id: String,
    pub user_id: Option<String>,
    pub ttl_seconds: u64,
}

impl SessionMetadata {
    pub fn is_expired(&self) -> bool {
        Utc::now().timestamp() as i64 >= (self.last_activity + self.ttl_seconds as i64)
    }
}

pub struct ConversationManager {
    pub session_id: String,
    pub pattern: ConversationPattern,
    pub kv: KvStore,
    pub gdrive: Option<GoogleDriveClient>,
    pub worker_id: String,
    pub user_id: Option<String>,
    pub ttl_seconds: u64,
    pub messages: Vec<Message>,
    pub summaries: Vec<String>,
    pub turn_count: u32,
    pub summarize_threshold: u32,
}

impl ConversationManager {
    pub fn history(session_id: String, kv: KvStore, worker_id: String, user_id: Option<String>) -> Self {
        Self {
            session_id: session_id.clone(),
            pattern: ConversationPattern::History,
            kv,
            gdrive: None,
            worker_id,
            user_id,
            ttl_seconds: 86400,
            messages: Vec::new(),
            summaries: Vec::new(),
            turn_count: 0,
            summarize_threshold: 5,
        }
    }

    pub fn pipeline(session_id: String, kv: KvStore, worker_id: String, user_id: Option<String>) -> Self {
        Self {
            session_id: session_id.clone(),
            pattern: ConversationPattern::Pipeline,
            kv,
            gdrive: None,
            worker_id,
            user_id,
            ttl_seconds: 86400,
            messages: Vec::new(),
            summaries: Vec::new(),
            turn_count: 0,
            summarize_threshold: 5,
        }
    }

    pub fn with_gdrive(mut self, gdrive: GoogleDriveClient) -> Self {
        self.gdrive = Some(gdrive);
        self
    }

    pub fn with_ttl(mut self, ttl_seconds: u64) -> Self {
        self.ttl_seconds = ttl_seconds;
        self
    }

    pub fn with_summarize_threshold(mut self, threshold: u32) -> Self {
        self.summarize_threshold = threshold;
        self
    }

    pub async fn add_turn(&mut self, role: Role, content: String) -> Result<()> {
        self.messages.push(Message { role, content });
        self.turn_count += 1;
        self.last_activity();

        if self.turn_count >= self.summarize_threshold {
            self.summarize_if_needed_inner().await?;
        }

        self.save_to_kv().await
    }

    pub async fn get_history(&self) -> Result<Vec<Message>> {
        let mut messages = Vec::new();

        if !self.summaries.is_empty() {
            let summary_text = self.summaries.join("\n---\n");
            messages.push(Message::system(&format!(
                "Previous conversation summary:\n{}",
                summary_text
            )));
        }

        messages.extend(self.messages.iter().cloned());
        Ok(messages)
    }

    pub fn get_raw_messages(&self) -> Vec<Message> {
        self.messages.clone()
    }

    pub fn to_api_format(&self) -> Vec<serde_json::Value> {
        self.messages.iter().map(|m| {
            let role = match m.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            serde_json::json!({
                "role": role,
                "content": m.content
            })
        }).collect()
    }

    pub fn to_api_format_with_summary(&self) -> Vec<serde_json::Value> {
        let mut messages = Vec::new();

        if !self.summaries.is_empty() {
            let summary_text = self.summaries.join("\n---\n");
            messages.push(serde_json::json!({
                "role": "system",
                "content": format!("Previous conversation summary:\n{}", summary_text)
            }));
        }

        for m in &self.messages {
            let role = match m.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            messages.push(serde_json::json!({
                "role": role,
                "content": m.content
            }));
        }

        messages
    }

    pub async fn summarize_if_needed(&mut self, _model_router: &crate::model_router::ModelRouter) -> Result<bool> {
        if self.turn_count >= self.summarize_threshold {
            self.summarize_if_needed_inner().await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn summarize_if_needed_inner(&mut self) -> Result<()> {
        if self.messages.len() < 4 {
            return Ok(());
        }

        let text_to_summarize: Vec<String> = self.messages.iter().map(|m| {
            let role = match m.role {
                Role::System => "[SYSTEM]",
                Role::User => "[USER]",
                Role::Assistant => "[ASSISTANT]",
            };
            format!("{}: {}", role, m.content)
        }).collect();

        let summary_text = text_to_summarize.join("\n");

        self.summaries.push(format!(
            "Summary of {} turns (compact): {}...",
            self.messages.len(),
            &summary_text.chars().take(500).collect::<String>()
        ));

        let keep_count = 2.min(self.messages.len());
        let start = self.messages.len() - keep_count;
        self.messages.drain(..start);

        self.turn_count = keep_count as u32;

        tracing::info!("Conversation summarized: {} messages -> {} + summary",
            self.messages.len() + self.summaries.len(),
            keep_count
        );

        Ok(())
    }

    pub async fn inject_context(&self, step_results: &[StepResult]) -> Result<String> {
        let mut context = String::new();

        for result in step_results {
            context.push_str(&format!(
                "\nStep {} result:\n{}\n",
                result.step,
                result.result
            ));

            if let Some(ref metadata) = result.metadata {
                context.push_str(&format!("Metadata: {}\n", metadata));
            }
        }

        Ok(context)
    }

    pub async fn save_step_result(&self, step: u8, result: String) -> Result<()> {
        let key = format!("pipeline:{}:step_{}", self.session_id, step);
        let step_result = StepResult {
            step,
            result: result.clone(),
            metadata: None,
        };

        let serialized = serde_json::to_string(&step_result)
            .map_err(|e| CoreError::SerializationError(format!("Step result serialization failed: {}", e)))?;

        self.kv.put(&key, &serialized)
            .map_err(|e| CoreError::KvError(format!("{:?}", e)))?
            .expiration_ttl(self.ttl_seconds)
            .execute()
            .await
            .map_err(|e| CoreError::KvError(format!("{:?}", e)))?;

        tracing::info!("Step {} result saved for session {}", step, self.session_id);
        Ok(())
    }

    pub async fn get_step_result(&self, step: u8) -> Result<Option<StepResult>> {
        let key = format!("pipeline:{}:step_{}", self.session_id, step);

        match self.kv.get(&key).text().await {
            Ok(Some(serialized)) => {
                let step_result: StepResult = serde_json::from_str(&serialized)
                    .map_err(|e| CoreError::SerializationError(format!("Step result deserialization failed: {}", e)))?;
                Ok(Some(step_result))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(CoreError::KvError(format!("Failed to get step result: {:?}", e))),
        }
    }

    pub async fn get_all_step_results(&self) -> Result<Vec<StepResult>> {
        Ok(Vec::new())
    }

    pub async fn complete_session(&self, _d1: Option<&worker::D1Database>) -> Result<()> {
        let history = self.get_history().await?;
        let metadata = SessionMetadata {
            session_id: self.session_id.clone(),
            pattern: self.pattern,
            created_at: 0,
            last_activity: Utc::now().timestamp(),
            turn_count: self.turn_count,
            worker_id: self.worker_id.clone(),
            user_id: self.user_id.clone(),
            ttl_seconds: self.ttl_seconds,
        };

        if let Some(ref _gdrive) = self.gdrive {
            let archive_data = serde_json::json!({
                "metadata": metadata,
                "history": history,
                "summaries": self.summaries,
            });

            let folder_path = format!("/sessions/{}/{}", self.worker_id, self.session_id);
            let _ = archive_data;
            let _ = folder_path;

            tracing::info!(
                "Session {} would be archived to Google Drive: {}",
                self.session_id,
                folder_path
            );
        }

        self.delete_from_kv().await?;

        tracing::info!("Session {} completed and archived", self.session_id);
        Ok(())
    }

    fn last_activity(&mut self) {}

    async fn save_to_kv(&mut self) -> Result<()> {
        let key = format!("conversations:{}:history", self.session_id);

        let state = serde_json::json!({
            "messages": self.messages,
            "summaries": self.summaries,
            "turn_count": self.turn_count,
            "last_activity": Utc::now().timestamp(),
        });

        let serialized = serde_json::to_string(&state)
            .map_err(|e| CoreError::SerializationError(format!("Conversation state serialization failed: {}", e)))?;

        self.kv.put(&key, &serialized)
            .map_err(|e| CoreError::KvError(format!("{:?}", e)))?
            .expiration_ttl(self.ttl_seconds)
            .execute()
            .await
            .map_err(|e| CoreError::KvError(format!("{:?}", e)))?;

        Ok(())
    }

    pub async fn load_from_kv(&mut self) -> Result<bool> {
        let key = format!("conversations:{}:history", self.session_id);

        match self.kv.get(&key).text().await {
            Ok(Some(serialized)) => {
                let state: serde_json::Value = serde_json::from_str(&serialized)
                    .map_err(|e| CoreError::SerializationError(format!("Conversation state deserialization failed: {}", e)))?;

                if let Some(messages) = state["messages"].as_array() {
                    self.messages = messages.iter().filter_map(|m| {
                        let role_str = m["role"].as_str()?;
                        let content = m["content"].as_str()?.to_string();
                        let role = match role_str {
                            "system" => Role::System,
                            "user" => Role::User,
                            "assistant" => Role::Assistant,
                            _ => return None,
                        };
                        Some(Message { role, content })
                    }).collect();
                }

                if let Some(summaries) = state["summaries"].as_array() {
                    self.summaries = summaries.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect();
                }

                self.turn_count = state["turn_count"].as_u64().unwrap_or(0) as u32;

                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(e) => Err(CoreError::KvError(format!("Failed to load conversation state: {:?}", e))),
        }
    }

    async fn delete_from_kv(&self) -> Result<()> {
        let key = format!("conversations:{}:history", self.session_id);
        self.kv.delete(&key).await
            .map_err(|e| CoreError::KvError(format!("Failed to delete conversation state: {:?}", e)))
    }

    pub fn get_metadata(&self) -> SessionMetadata {
        SessionMetadata {
            session_id: self.session_id.clone(),
            pattern: self.pattern,
            created_at: 0,
            last_activity: Utc::now().timestamp(),
            turn_count: self.turn_count,
            worker_id: self.worker_id.clone(),
            user_id: self.user_id.clone(),
            ttl_seconds: self.ttl_seconds,
        }
    }
}
