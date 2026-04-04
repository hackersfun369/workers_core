//! # Mistake Memory — Outcome Recording and Embedding Search

use worker::D1Database;
use chrono::Utc;
use wasm_bindgen::JsValue;

use crate::models::{OutcomeRecord, ActionResult, ModelScore, MemoryMapEntry, MemoryMap};
use crate::api_clients::HuggingFaceClient;
use crate::{CoreError, Result};

pub struct MistakeMemory {
    pub worker_db: D1Database,
    pub shared_db: D1Database,
    pub embedding_client: HuggingFaceClient,
    pub memory_map: MemoryMap,
    pub worker_name: String,
}

impl MistakeMemory {
    pub fn new(
        worker_db: D1Database,
        shared_db: D1Database,
        embedding_client: HuggingFaceClient,
        worker_name: String,
    ) -> Self {
        Self {
            worker_db,
            shared_db,
            embedding_client,
            memory_map: MemoryMap::new(worker_name.clone(), 500),
            worker_name,
        }
    }

    pub async fn find_similar_past_outcomes(
        &self,
        input_text: &str,
        top_k: usize,
    ) -> Result<SimilarOutcomes> {
        let embedding_response = self.embedding_client.embed(input_text).await
            .map_err(|e| CoreError::HttpError(format!("Failed to generate embedding: {}", e)))?;

        let embedding = &embedding_response.embedding;

        let map_results = self.memory_map.find_similar(input_text, embedding, top_k);

        let db_results = self.query_d1_similar(embedding, top_k).await?;

        let mut failures = Vec::new();
        let mut successes = Vec::new();

        for (similarity, entry) in &map_results {
            let info = PastOutcomeInfo {
                similarity: *similarity,
                model_used: entry.model_code.to_string(),
                prompt_strategy: entry.strategy_hash.to_string(),
                failure_reason: if entry.outcome == 2 {
                    Some(entry.failure_code.to_string())
                } else {
                    None
                },
                success_strategy: if entry.outcome == 1 {
                    Some(entry.success_code.to_string())
                } else {
                    None
                },
            };

            match entry.outcome {
                1 => successes.push(info),
                2 => failures.push(info),
                _ => {}
            }
        }

        for record in db_results {
            let info = PastOutcomeInfo {
                similarity: 0.0,
                model_used: record.model_used.clone(),
                prompt_strategy: record.prompt_strategy.clone(),
                failure_reason: record.failure_reason.clone(),
                success_strategy: if record.result == ActionResult::Success {
                    Some(record.prompt_strategy.clone())
                } else {
                    None
                },
            };

            match record.result {
                ActionResult::Failure => failures.push(info),
                ActionResult::Success => successes.push(info),
                _ => {}
            }
        }

        Ok(SimilarOutcomes {
            failures,
            successes,
        })
    }

    async fn query_d1_similar(&self, _embedding: &[f32], limit: usize) -> Result<Vec<OutcomeRecord>> {
        let sql = format!(
            "SELECT * FROM outcomes WHERE worker_name = ?1 ORDER BY timestamp DESC LIMIT {}",
            limit
        );

        let stmt = self.worker_db.prepare(&sql);
        let result = stmt.bind(&[JsValue::from_str(&self.worker_name)])
            .map_err(|e| CoreError::D1Error(format!("Failed to bind outcome query: {}", e)))?
            .all()
            .await
            .map_err(|e| CoreError::D1Error(format!("Failed to query outcomes: {}", e)))?;

        let records = result.results::<OutcomeRecord>()
            .map_err(|e| CoreError::D1Error(format!("Failed to deserialize outcomes: {}", e)))?;

        Ok(records)
    }

    pub async fn record_outcome(
        &mut self,
        action_type: &str,
        input_text: &str,
        model_used: &str,
        prompt_strategy: &str,
        result: ActionResult,
        failure_reason: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().timestamp();
        let id = uuid::Uuid::new_v4().to_string();

        let input_fingerprint = Self::fingerprint(input_text);

        let embedding = match self.embedding_client.embed(input_text).await {
            Ok(resp) => resp.embedding,
            Err(e) => {
                tracing::warn!("Failed to generate embedding for outcome: {}", e);
                vec![0.0; 768]
            }
        };

        let map_entry = MemoryMapEntry::new(
            input_text,
            &embedding,
            failure_reason,
            if result == ActionResult::Success { Some(prompt_strategy) } else { None },
            model_used,
            action_type,
            match result {
                ActionResult::Success => 1,
                ActionResult::Failure => 2,
                ActionResult::Timeout => 3,
            },
            now,
            prompt_strategy,
            0,
        );

        let archived = self.memory_map.add_entry(map_entry);

        if let Some(old_entry) = archived {
            if let Some(_record_id) = &old_entry.full_record_id {
                // Already in D1
            }
        }

        let record = OutcomeRecord {
            id: id.clone(),
            worker_name: self.worker_name.clone(),
            action_type: action_type.to_string(),
            input_fingerprint,
            input_embedding: crate::models::BinaryEmbedding::from_floats(&embedding).to_bytes(),
            model_used: model_used.to_string(),
            prompt_strategy: prompt_strategy.to_string(),
            result: result.clone(),
            failure_reason: failure_reason.map(String::from),
            timestamp: now,
        };

        let stmt = self.worker_db.prepare(
            "INSERT INTO outcomes (id, worker_name, action_type, input_fingerprint, input_embedding,
             model_used, prompt_strategy, result, failure_reason, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
        );

        stmt.bind(&[
            JsValue::from_str(&record.id),
            JsValue::from_str(&record.worker_name),
            JsValue::from_str(&record.action_type),
            JsValue::from_str(&record.input_fingerprint),
            JsValue::from_str(&base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &record.input_embedding)),
            JsValue::from_str(&record.model_used),
            JsValue::from_str(&record.prompt_strategy),
            JsValue::from_str(&format!("{:?}", record.result)),
            JsValue::from_str(record.failure_reason.as_deref().unwrap_or("")),
            JsValue::from_f64(record.timestamp as f64),
        ])
        .map_err(|e| CoreError::D1Error(format!("Failed to bind outcome record: {}", e)))?
        .run()
        .await
        .map_err(|e| CoreError::D1Error(format!("Failed to record outcome: {}", e)))?;

        self.update_model_score(model_used, action_type, &result).await?;

        tracing::info!(
            "Outcome recorded: {} for {} using {} -> {:?}",
            id, action_type, model_used, result
        );

        Ok(())
    }

    async fn update_model_score(
        &self,
        model_name: &str,
        task_type: &str,
        result: &ActionResult,
    ) -> Result<()> {
        let now = Utc::now().timestamp();

        let update_stmt = self.shared_db.prepare(
            "UPDATE model_scores SET
                success_count = success_count + ?1,
                failure_count = failure_count + ?2,
                timeout_count = timeout_count + ?3,
                last_updated = ?4
             WHERE model_name = ?5 AND task_type = ?6"
        );

        let (success, failure, timeout) = match result {
            ActionResult::Success => (1, 0, 0),
            ActionResult::Failure => (0, 1, 0),
            ActionResult::Timeout => (0, 0, 1),
        };

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

        let changes = if let Ok(r) = &update_result {
            r.meta().ok().flatten().and_then(|m| m.changes).unwrap_or(0)
        } else {
            0
        };
        if changes == 0 {
            let insert_stmt = self.shared_db.prepare(
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

    pub async fn get_model_scores(&self, task_type: &str) -> Result<Vec<ModelScore>> {
        let stmt = self.shared_db.prepare(
            "SELECT * FROM model_scores WHERE task_type = ?1 ORDER BY success_count DESC"
        );

        let result = stmt.bind(&[JsValue::from_str(task_type)])
            .map_err(|e| CoreError::D1Error(format!("Failed to bind model scores query: {}", e)))?
            .all()
            .await
            .map_err(|e| CoreError::D1Error(format!("Failed to query model scores: {}", e)))?;

        let scores = result.results::<ModelScore>()
            .map_err(|e| CoreError::D1Error(format!("Failed to deserialize model scores: {}", e)))?;

        Ok(scores)
    }

    pub async fn get_best_model(&self, task_type: &str) -> Result<Option<ModelScore>> {
        let scores = self.get_model_scores(task_type).await?;
        Ok(scores.into_iter()
            .max_by(|a, b| a.success_rate().partial_cmp(&b.success_rate()).unwrap_or(std::cmp::Ordering::Equal)))
    }

    fn fingerprint(text: &str) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(text.as_bytes());
        hasher.finalize().to_hex().to_string()[..16].to_string()
    }

    pub fn get_memory_map(&self) -> &MemoryMap {
        &self.memory_map
    }

    pub fn get_memory_map_mut(&mut self) -> &mut MemoryMap {
        &mut self.memory_map
    }
}

#[derive(Debug, Clone)]
pub struct SimilarOutcomes {
    pub failures: Vec<PastOutcomeInfo>,
    pub successes: Vec<PastOutcomeInfo>,
}

impl SimilarOutcomes {
    pub fn format_negative_examples(&self) -> String {
        if self.failures.is_empty() {
            return "No similar failures found.".to_string();
        }

        self.failures.iter().map(|f| {
            format!(
                "FAILURE (similarity: {:.2}): Model={}, Strategy={}, Reason={}",
                f.similarity,
                f.model_used,
                f.prompt_strategy,
                f.failure_reason.as_deref().unwrap_or("unknown")
            )
        }).collect::<Vec<_>>().join("\n")
    }

    pub fn format_positive_examples(&self) -> String {
        if self.successes.is_empty() {
            return "No similar successes found.".to_string();
        }

        self.successes.iter().map(|s| {
            format!(
                "SUCCESS (similarity: {:.2}): Model={}, Strategy={}",
                s.similarity,
                s.model_used,
                s.prompt_strategy,
            )
        }).collect::<Vec<_>>().join("\n")
    }

    pub fn format_for_prompt(&self) -> String {
        format!(
            "## Past Similar Failures (AVOID THESE)\n{}\n\n\
             ## Past Similar Successes (LEARN FROM THESE)\n{}",
            self.format_negative_examples(),
            self.format_positive_examples()
        )
    }
}

#[derive(Debug, Clone)]
pub struct PastOutcomeInfo {
    pub similarity: f64,
    pub model_used: String,
    pub prompt_strategy: String,
    pub failure_reason: Option<String>,
    pub success_strategy: Option<String>,
}
