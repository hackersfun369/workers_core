//! # API Clients — One Client Per External Service
//!
//! HTTP clients for all external services. All clients use gloo_net for WASM-compatible HTTP.

use serde::{Deserialize, Serialize};
use worker::*;

use crate::models::Message;
use crate::{CoreError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiUsage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<ApiUsage>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingResponse {
    pub embedding: Vec<f32>,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionResponse {
    pub ai_score: f64,
    pub flagged_sentences: Vec<FlaggedSentence>,
    pub overall_verdict: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlaggedSentence {
    pub text: String,
    pub ai_probability: f64,
    pub start_char: usize,
    pub end_char: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceResponse {
    pub audio_url: Option<String>,
    pub text: String,
    pub is_end_of_conversation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubRepo {
    pub id: u64,
    pub name: String,
    pub full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    pub language: Option<String>,
    pub stargazers_count: u64,
    pub forks_count: u64,
    pub updated_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubCommit {
    pub sha: String,
    pub commit: GitHubCommitDetails,
    pub author: Option<GitHubAuthor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubCommitDetails {
    pub message: String,
    pub author: GitHubAuthor,
    pub committer: GitHubAuthor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubAuthor {
    pub name: String,
    pub email: String,
    pub date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobListing {
    pub title: String,
    pub company: String,
    pub location: String,
    pub description: String,
    pub url: String,
    pub source: String,
    pub posted_date: Option<String>,
    pub skills_required: Vec<String>,
}

// ============================================================================
// Gemini Client
// ============================================================================

pub struct GeminiClient {
    api_key: String,
    base_url: String,
}

impl GeminiClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://generativelanguage.googleapis.com".to_string(),
        }
    }

    pub async fn generate_content(
        &self,
        model: &str,
        messages: &[Message],
        max_tokens: Option<i32>,
        temperature: Option<f32>,
    ) -> Result<LlmResponse> {
        let system_prompt = messages.iter()
            .find(|m| matches!(m.role, crate::models::Role::System))
            .map(|m| m.content.clone())
            .unwrap_or_default();

        let user_messages: String = messages.iter()
            .filter(|m| !matches!(m.role, crate::models::Role::System))
            .map(|m| format!("{:?}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");

        let full_content = if !system_prompt.is_empty() {
            format!("{}\n\n{}", system_prompt, user_messages)
        } else {
            user_messages
        };

        let mut body = serde_json::json!({
            "contents": [{
                "parts": [{"text": full_content}]
            }],
            "generationConfig": {
                "temperature": temperature.unwrap_or(0.7),
            }
        });

        if let Some(max) = max_tokens {
            body["generationConfig"]["maxOutputTokens"] = serde_json::Value::Number(serde_json::Number::from(max));
        }

        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            self.base_url, model, self.api_key
        );

        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| CoreError::HttpError(format!("Failed to serialize Gemini request: {}", e)))?;

        let response = gloo_net::http::Request::post(&url)
            .header("Content-Type", "application/json")
            .body(body_bytes)?
            .send()
            .await
            .map_err(|e| CoreError::HttpError(format!("Gemini request failed: {}", e)))?;

        let status = response.status();
        let resp_body = response.text().await
            .map_err(|e| CoreError::HttpError(format!("Failed to read Gemini response: {}", e)))?;

        if status != 200 {
            return Err(CoreError::HttpError(format!(
                "Gemini API error ({}): {}",
                status, resp_body
            )));
        }

        let json: serde_json::Value = serde_json::from_str(&resp_body)
            .map_err(|e| CoreError::HttpError(format!("Failed to parse Gemini response: {}", e)))?;

        let content = json["candidates"][0]["content"]["parts"][0]["text"].as_str()
            .ok_or_else(|| CoreError::HttpError("No content in Gemini response".to_string()))?
            .to_string();

        let usage = json["usageMetadata"].as_object().map(|u| ApiUsage {
            prompt_tokens: u.get("promptTokenCount").and_then(|v| v.as_i64()).unwrap_or(0),
            completion_tokens: u.get("candidatesTokenCount").and_then(|v| v.as_i64()).unwrap_or(0),
            total_tokens: u.get("totalTokenCount").and_then(|v| v.as_i64()).unwrap_or(0),
        });

        let finish_reason = json["candidates"][0]["finishReason"].as_str().map(String::from);

        Ok(LlmResponse {
            content,
            model: model.to_string(),
            usage,
            finish_reason,
        })
    }

    pub async fn flash(&self, prompt: &str) -> Result<LlmResponse> {
        self.generate_content("gemini-2.0-flash", &[Message::user(prompt)], Some(8192), Some(0.7)).await
    }

    pub async fn flash_thinking(&self, prompt: &str) -> Result<LlmResponse> {
        self.generate_content("gemini-2.0-flash-thinking-exp", &[Message::user(prompt)], Some(8192), Some(0.7)).await
    }
}

// ============================================================================
// Groq Client
// ============================================================================

pub struct GroqClient {
    api_key: String,
}

impl GroqClient {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    pub async fn generate(&self, prompt: &str, system_prompt: Option<&str>) -> Result<LlmResponse> {
        let mut messages = Vec::new();
        if let Some(sys) = system_prompt {
            messages.push(Message::system(sys));
        }
        messages.push(Message::user(prompt));

        let body = serde_json::json!({
            "model": "llama-3.1-8b-instant",
            "messages": messages.iter().map(|m| {
                let role = match m.role {
                    crate::models::Role::System => "system",
                    crate::models::Role::User => "user",
                    crate::models::Role::Assistant => "assistant",
                };
                serde_json::json!({"role": role, "content": m.content.clone()})
            }).collect::<Vec<_>>(),
            "temperature": 0.3,
            "max_tokens": 1024,
        });

        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| CoreError::HttpError(format!("Failed to serialize Groq request: {}", e)))?;

        let response = gloo_net::http::Request::post("https://api.groq.com/openai/v1/chat/completions")
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(body_bytes)?.send()
            .await
            .map_err(|e| CoreError::HttpError(format!("Groq request failed: {}", e)))?;

        let status = response.status();
        let resp_body = response.text().await
            .map_err(|e| CoreError::HttpError(format!("Failed to read Groq response: {}", e)))?;

        if status != 200 {
            return Err(CoreError::HttpError(format!("Groq API error ({}): {}", status, resp_body)));
        }

        let json: serde_json::Value = serde_json::from_str(&resp_body)
            .map_err(|e| CoreError::HttpError(format!("Failed to parse Groq response: {}", e)))?;

        let content = json["choices"][0]["message"]["content"].as_str()
            .ok_or_else(|| CoreError::HttpError("No content in Groq response".to_string()))?
            .to_string();

        let usage = json["usage"].as_object().map(|u| ApiUsage {
            prompt_tokens: u.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            completion_tokens: u.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            total_tokens: u.get("total_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
        });

        Ok(LlmResponse {
            content,
            model: "llama-3.1-8b-instant".to_string(),
            usage,
            finish_reason: json["choices"][0]["finish_reason"].as_str().map(String::from),
        })
    }
}

// ============================================================================
// OpenRouter Client
// ============================================================================

pub struct OpenRouterClient {
    api_key: String,
}

impl OpenRouterClient {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    pub async fn generate(
        &self,
        model: &str,
        messages: &[Message],
        max_tokens: Option<i32>,
        temperature: Option<f32>,
    ) -> Result<LlmResponse> {
        let mut body = serde_json::json!({
            "model": model,
            "messages": messages.iter().map(|m| {
                let role = match m.role {
                    crate::models::Role::System => "system",
                    crate::models::Role::User => "user",
                    crate::models::Role::Assistant => "assistant",
                };
                serde_json::json!({"role": role, "content": m.content.clone()})
            }).collect::<Vec<_>>(),
            "temperature": temperature.unwrap_or(0.7),
        });

        if let Some(max) = max_tokens {
            body["max_tokens"] = serde_json::Value::Number(serde_json::Number::from(max));
        }

        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| CoreError::HttpError(format!("Failed to serialize OpenRouter request: {}", e)))?;

        let response = gloo_net::http::Request::post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://autonomous-software-factory.com")
            .header("X-Title", "Autonomous Software Factory")
            .body(body_bytes)?.send()
            .await
            .map_err(|e| CoreError::HttpError(format!("OpenRouter request failed: {}", e)))?;

        let status = response.status();
        let resp_body = response.text().await
            .map_err(|e| CoreError::HttpError(format!("Failed to read OpenRouter response: {}", e)))?;

        if status != 200 {
            return Err(CoreError::HttpError(format!("OpenRouter API error ({}): {}", status, resp_body)));
        }

        let json: serde_json::Value = serde_json::from_str(&resp_body)
            .map_err(|e| CoreError::HttpError(format!("Failed to parse OpenRouter response: {}", e)))?;

        let content = json["choices"][0]["message"]["content"].as_str()
            .ok_or_else(|| CoreError::HttpError("No content in OpenRouter response".to_string()))?
            .to_string();

        let usage = json["usage"].as_object().map(|u| ApiUsage {
            prompt_tokens: u.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            completion_tokens: u.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            total_tokens: u.get("total_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
        });

        Ok(LlmResponse {
            content,
            model: model.to_string(),
            usage,
            finish_reason: json["choices"][0]["finish_reason"].as_str().map(String::from),
        })
    }
}

// ============================================================================
// HuggingFace Client
// ============================================================================

pub struct HuggingFaceClient {
    api_key: String,
}

impl HuggingFaceClient {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    pub async fn generate_code(&self, prompt: &str, max_tokens: Option<i32>) -> Result<LlmResponse> {
        let body = serde_json::json!({
            "inputs": prompt,
            "parameters": {
                "max_new_tokens": max_tokens.unwrap_or(4096),
                "temperature": 0.2,
                "return_full_text": false,
            }
        });

        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| CoreError::HttpError(format!("Failed to serialize HuggingFace request: {}", e)))?;

        let response = gloo_net::http::Request::post("https://api-inference.huggingface.co/models/Qwen/Qwen2.5-Coder-32B-Instruct")
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(body_bytes)?.send()
            .await
            .map_err(|e| CoreError::HttpError(format!("HuggingFace request failed: {}", e)))?;

        let status = response.status();
        let resp_body = response.text().await
            .map_err(|e| CoreError::HttpError(format!("Failed to read HuggingFace response: {}", e)))?;

        if status != 200 {
            return Err(CoreError::HttpError(format!("HuggingFace API error ({}): {}", status, resp_body)));
        }

        let json: serde_json::Value = serde_json::from_str(&resp_body)
            .map_err(|e| CoreError::HttpError(format!("Failed to parse HuggingFace response: {}", e)))?;

        let content = json[0]["generated_text"].as_str()
            .or_else(|| json["generated_text"].as_str())
            .unwrap_or("")
            .to_string();

        Ok(LlmResponse {
            content,
            model: "qwen2.5-coder-32b".to_string(),
            usage: None,
            finish_reason: None,
        })
    }

    pub async fn embed(&self, text: &str) -> Result<EmbeddingResponse> {
        let body = serde_json::json!({
            "inputs": text,
        });

        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| CoreError::HttpError(format!("Failed to serialize embedding request: {}", e)))?;

        let response = gloo_net::http::Request::post("https://api-inference.huggingface.co/models/nomic-ai/nomic-embed-text-v1")
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(body_bytes)?.send()
            .await
            .map_err(|e| CoreError::HttpError(format!("Embedding request failed: {}", e)))?;

        let status = response.status();
        let resp_body = response.text().await
            .map_err(|e| CoreError::HttpError(format!("Failed to read embedding response: {}", e)))?;

        if status != 200 {
            return Err(CoreError::HttpError(format!("Embedding API error ({}): {}", status, resp_body)));
        }

        let json: serde_json::Value = serde_json::from_str(&resp_body)
            .map_err(|e| CoreError::HttpError(format!("Failed to parse embedding response: {}", e)))?;

        let embedding = json.as_array()
            .and_then(|arr| arr[0].as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect())
            .unwrap_or_default();

        Ok(EmbeddingResponse {
            embedding,
            model: "nomic-embed-text-v1".to_string(),
        })
    }
}

// ============================================================================
// Mistral Client
// ============================================================================

pub struct MistralClient {
    api_key: String,
}

impl MistralClient {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    pub async fn voice_chat(
        &self,
        prompt: &str,
        system_prompt: &str,
        _voice_id: Option<&str>,
    ) -> Result<VoiceResponse> {
        let body = serde_json::json!({
            "model": "mistral-medium",
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": prompt}
            ],
            "voice": "default",
        });

        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| CoreError::HttpError(format!("Failed to serialize Mistral request: {}", e)))?;

        let response = gloo_net::http::Request::post("https://api.mistral.ai/v1/chat/completions")
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(body_bytes)?.send()
            .await
            .map_err(|e| CoreError::HttpError(format!("Mistral request failed: {}", e)))?;

        let status = response.status();
        let resp_body = response.text().await
            .map_err(|e| CoreError::HttpError(format!("Failed to read Mistral response: {}", e)))?;

        if status != 200 {
            return Err(CoreError::HttpError(format!("Mistral API error ({}): {}", status, resp_body)));
        }

        let json: serde_json::Value = serde_json::from_str(&resp_body)
            .map_err(|e| CoreError::HttpError(format!("Failed to parse Mistral response: {}", e)))?;

        let content = json["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string();

        Ok(VoiceResponse {
            audio_url: None,
            text: content,
            is_end_of_conversation: false,
        })
    }

    pub async fn generate_text(&self, prompt: &str, system_prompt: Option<&str>) -> Result<LlmResponse> {
        let mut messages = Vec::new();
        if let Some(sys) = system_prompt {
            messages.push(Message::system(sys));
        }
        messages.push(Message::user(prompt));

        let body = serde_json::json!({
            "model": "mistral-large-latest",
            "messages": messages.iter().map(|m| {
                let role = match m.role {
                    crate::models::Role::System => "system",
                    crate::models::Role::User => "user",
                    crate::models::Role::Assistant => "assistant",
                };
                serde_json::json!({"role": role, "content": m.content.clone()})
            }).collect::<Vec<_>>(),
            "temperature": 0.7,
        });

        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| CoreError::HttpError(format!("Failed to serialize Mistral Large request: {}", e)))?;

        let response = gloo_net::http::Request::post("https://api.mistral.ai/v1/chat/completions")
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(body_bytes)?.send()
            .await
            .map_err(|e| CoreError::HttpError(format!("Mistral Large request failed: {}", e)))?;

        let status = response.status();
        let resp_body = response.text().await
            .map_err(|e| CoreError::HttpError(format!("Failed to read Mistral Large response: {}", e)))?;

        if status != 200 {
            return Err(CoreError::HttpError(format!("Mistral Large API error ({}): {}", status, resp_body)));
        }

        let json: serde_json::Value = serde_json::from_str(&resp_body)
            .map_err(|e| CoreError::HttpError(format!("Failed to parse Mistral Large response: {}", e)))?;

        let content = json["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string();

        Ok(LlmResponse {
            content,
            model: "mistral-large-latest".to_string(),
            usage: None,
            finish_reason: json["choices"][0]["finish_reason"].as_str().map(String::from),
        })
    }
}

// ============================================================================
// GitHub Client
// ============================================================================

pub struct GitHubClient {
    token: String,
}

impl GitHubClient {
    pub fn new(token: String) -> Self {
        Self { token }
    }

    pub async fn list_repos(&self, username: &str) -> Result<Vec<GitHubRepo>> {
        let url = format!("https://api.github.com/users/{}/repos?per_page=100&sort=updated", username);

        let response = gloo_net::http::Request::get(&url)
            .header("Authorization", &format!("token {}", self.token))
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await
            .map_err(|e| CoreError::HttpError(format!("GitHub repos request failed: {}", e)))?;

        let status = response.status();
        let body = response.text().await
            .map_err(|e| CoreError::HttpError(format!("Failed to read GitHub repos response: {}", e)))?;

        if status != 200 {
            return Err(CoreError::HttpError(format!("GitHub API error ({}): {}", status, body)));
        }

        let repos: Vec<GitHubRepo> = serde_json::from_str(&body)
            .map_err(|e| CoreError::HttpError(format!("Failed to parse GitHub repos response: {}", e)))?;

        Ok(repos)
    }

    pub async fn list_commits(&self, owner: &str, repo: &str) -> Result<Vec<GitHubCommit>> {
        let url = format!("https://api.github.com/repos/{}/{}/commits?per_page=50", owner, repo);

        let response = gloo_net::http::Request::get(&url)
            .header("Authorization", &format!("token {}", self.token))
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await
            .map_err(|e| CoreError::HttpError(format!("GitHub commits request failed: {}", e)))?;

        let status = response.status();
        let body = response.text().await
            .map_err(|e| CoreError::HttpError(format!("Failed to read GitHub commits response: {}", e)))?;

        if status != 200 {
            return Err(CoreError::HttpError(format!("GitHub API error ({}): {}", status, body)));
        }

        let commits: Vec<GitHubCommit> = serde_json::from_str(&body)
            .map_err(|e| CoreError::HttpError(format!("Failed to parse GitHub commits response: {}", e)))?;

        Ok(commits)
    }

    pub async fn get_file_content(&self, owner: &str, repo: &str, path: &str, branch: &str) -> Result<String> {
        let url = format!("https://raw.githubusercontent.com/{}/{}/{}/{}", owner, repo, branch, path);

        let response = gloo_net::http::Request::get(&url)
            .header("Authorization", &format!("token {}", self.token))
            .send()
            .await
            .map_err(|e| CoreError::HttpError(format!("GitHub file content request failed: {}", e)))?;

        let body = response.text().await
            .map_err(|e| CoreError::HttpError(format!("Failed to read GitHub file content: {}", e)))?;

        if response.status() != 200 {
            return Err(CoreError::HttpError(format!(
                "GitHub file content error ({}): {}",
                response.status(),
                body
            )));
        }

        Ok(body)
    }

    pub async fn create_file(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
        content: &str,
        message: &str,
        branch: &str,
    ) -> Result<()> {
        let url = format!("https://api.github.com/repos/{}/{}/contents/{}", owner, repo, path);

        let body = serde_json::json!({
            "message": message,
            "content": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, content.as_bytes()),
            "branch": branch,
        });

        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| CoreError::HttpError(format!("Failed to serialize GitHub create file request: {}", e)))?;

        let response = gloo_net::http::Request::put(&url)
            .header("Authorization", &format!("token {}", self.token))
            .header("Accept", "application/vnd.github.v3+json")
            .header("Content-Type", "application/json")
            .body(body_bytes)?.send()
            .await
            .map_err(|e| CoreError::HttpError(format!("GitHub create file request failed: {}", e)))?;

        let status = response.status();
        if status != 201 && status != 200 {
            let body = response.text().await.unwrap_or_default();
            return Err(CoreError::HttpError(format!("GitHub create file error ({}): {}", status, body)));
        }

        Ok(())
    }

    pub async fn trigger_workflow(
        &self,
        owner: &str,
        repo: &str,
        workflow_id: &str,
        ref_name: &str,
        inputs: Option<serde_json::Value>,
    ) -> Result<()> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/actions/workflows/{}/dispatches",
            owner, repo, workflow_id
        );

        let mut body = serde_json::json!({
            "ref": ref_name,
        });

        if let Some(i) = inputs {
            body["inputs"] = i;
        }

        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| CoreError::HttpError(format!("Failed to serialize workflow trigger: {}", e)))?;

        let response = gloo_net::http::Request::post(&url)
            .header("Authorization", &format!("token {}", self.token))
            .header("Accept", "application/vnd.github.v3+json")
            .header("Content-Type", "application/json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .body(body_bytes)?.send()
            .await
            .map_err(|e| CoreError::HttpError(format!("Workflow trigger request failed: {}", e)))?;

        let status = response.status();
        if status != 204 {
            let body = response.text().await.unwrap_or_default();
            return Err(CoreError::HttpError(format!("Workflow trigger error ({}): {}", status, body)));
        }

        Ok(())
    }
}

// ============================================================================
// Mailgun Client
// ============================================================================

pub struct MailgunClient {
    api_key: String,
    domain: String,
}

impl MailgunClient {
    pub fn new(api_key: String, domain: String) -> Self {
        Self { api_key, domain }
    }

    pub async fn send_email(
        &self,
        to: &str,
        subject: &str,
        body_html: &str,
        from_name: Option<&str>,
    ) -> Result<()> {
        let from = format!("{} <noreply@{}>", from_name.unwrap_or("Autonomous Software Factory"), self.domain);

        let form = form_urlencoded::Serializer::new(String::new())
            .append_pair("from", &from)
            .append_pair("to", to)
            .append_pair("subject", subject)
            .append_pair("html", body_html)
            .finish();

        let url = format!("https://api.mailgun.net/v3/{}/messages", self.domain);

        let response = gloo_net::http::Request::post(&url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form)?.send()
            .await
            .map_err(|e| CoreError::HttpError(format!("Mailgun request failed: {}", e)))?;

        let status = response.status();
        if status != 200 {
            let body = response.text().await.unwrap_or_default();
            return Err(CoreError::HttpError(format!("Mailgun API error ({}): {}", status, body)));
        }

        tracing::info!("Email sent to {}: {}", to, subject);
        Ok(())
    }
}

// ============================================================================
// WhatsApp Client
// ============================================================================

pub struct WhatsAppClient {
    api_token: String,
}

impl WhatsAppClient {
    pub fn new(api_token: String) -> Self {
        Self { api_token }
    }

    pub async fn send_message(&self, phone_number: &str, message: &str) -> Result<()> {
        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "to": phone_number,
            "type": "text",
            "text": {
                "body": message
            }
        });

        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| CoreError::HttpError(format!("Failed to serialize WhatsApp request: {}", e)))?;

        let response = gloo_net::http::Request::post("https://graph.facebook.com/v18.0/me/messages")
            .header("Authorization", &format!("Bearer {}", self.api_token))
            .header("Content-Type", "application/json")
            .body(body_bytes)?.send()
            .await
            .map_err(|e| CoreError::HttpError(format!("WhatsApp request failed: {}", e)))?;

        let status = response.status();
        if status != 200 {
            let resp_body = response.text().await.unwrap_or_default();
            return Err(CoreError::HttpError(format!("WhatsApp API error ({}): {}", status, resp_body)));
        }

        tracing::info!("WhatsApp message sent to {}", phone_number);
        Ok(())
    }
}

// ============================================================================
// SciSpace Client
// ============================================================================

pub struct SciSpaceClient {
    api_key: String,
}

impl SciSpaceClient {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    pub async fn analyze_text(&self, text: &str) -> Result<DetectionResponse> {
        let body = serde_json::json!({
            "text": text,
        });

        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| CoreError::HttpError(format!("Failed to serialize SciSpace request: {}", e)))?;

        let response = gloo_net::http::Request::post("https://typeset.io/api/ai-detector/")
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(body_bytes)?.send()
            .await
            .map_err(|e| CoreError::HttpError(format!("SciSpace request failed: {}", e)))?;

        let status = response.status();
        let resp_body = response.text().await
            .map_err(|e| CoreError::HttpError(format!("Failed to read SciSpace response: {}", e)))?;

        if status != 200 {
            return Err(CoreError::HttpError(format!("SciSpace API error ({}): {}", status, resp_body)));
        }

        let json: serde_json::Value = serde_json::from_str(&resp_body)
            .map_err(|e| CoreError::HttpError(format!("Failed to parse SciSpace response: {}", e)))?;

        let ai_score = json["ai_score"].as_f64().unwrap_or(0.0);
        let overall_verdict = json["verdict"].as_str().unwrap_or("unknown").to_string();

        Ok(DetectionResponse {
            ai_score,
            flagged_sentences: Vec::new(),
            overall_verdict,
        })
    }
}

// ============================================================================
// Job Board Client
// ============================================================================

pub struct JobBoardClient {}

impl JobBoardClient {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn fetch_wellfound_jobs(&self, query: &str, location: &str) -> Result<Vec<JobListing>> {
        let url = format!(
            "https://api.wellfound.com/api/search/jobs?query={}&location={}",
            urlencoding::encode(query),
            urlencoding::encode(location)
        );

        let response = gloo_net::http::Request::get(&url)
            .send()
            .await
            .map_err(|e| CoreError::HttpError(format!("Wellfound request failed: {}", e)))?;

        let body = response.text().await
            .map_err(|e| CoreError::HttpError(format!("Failed to read Wellfound response: {}", e)))?;

        if response.status() != 200 {
            return Err(CoreError::HttpError(format!(
                "Wellfound API error ({}): {}",
                response.status(),
                body
            )));
        }

        let json: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| CoreError::HttpError(format!("Failed to parse Wellfound response: {}", e)))?;

        let jobs = json["jobs"].as_array()
            .map(|arr| arr.iter().filter_map(|j| {
                Some(JobListing {
                    title: j["title"].as_str()?.to_string(),
                    company: j["company"].as_object()
                        .and_then(|c| c.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("Unknown")
                        .to_string(),
                    location: j["location"].as_str().unwrap_or("Remote").to_string(),
                    description: j["content"].as_str().unwrap_or("").to_string(),
                    url: j["url"].as_str().unwrap_or("").to_string(),
                    source: "wellfound".to_string(),
                    posted_date: j["published_at"].as_str().map(String::from),
                    skills_required: Vec::new(),
                })
            }).collect())
            .unwrap_or_default();

        Ok(jobs)
    }

    pub async fn fetch_naukri_jobs(&self, query: &str, location: &str) -> Result<Vec<JobListing>> {
        tracing::info!("Naukri jobs search for: {} in {}", query, location);
        Ok(Vec::new())
    }

    pub async fn fetch_linkedin_jobs(&self, query: &str, location: &str) -> Result<Vec<JobListing>> {
        tracing::info!("LinkedIn jobs search for: {} in {}", query, location);
        Ok(Vec::new())
    }
}

// ============================================================================
// Cloudflare Client
// ============================================================================

pub struct CloudflareClient {
    api_token: String,
    account_id: String,
}

impl CloudflareClient {
    pub fn new(api_token: String, account_id: String) -> Self {
        Self { api_token, account_id }
    }

    pub async fn deploy_worker(
        &self,
        script_name: &str,
        script_content: &str,
        bindings: Option<serde_json::Value>,
    ) -> Result<()> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/workers/scripts/{}",
            self.account_id, script_name
        );

        let mut form_data = Vec::new();
        let boundary = "cloudflare_worker_upload";

        form_data.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        form_data.extend_from_slice(b"Content-Disposition: form-data; name=\"metadata\"\r\n\r\n");

        let mut metadata = serde_json::json!({
            "main_module": "index.js",
        });

        if let Some(b) = bindings {
            metadata["bindings"] = b;
        }

        form_data.extend_from_slice(serde_json::to_string(&metadata)
            .map_err(|e| CoreError::HttpError(format!("Failed to serialize metadata: {}", e)))?
            .as_bytes());

        form_data.extend_from_slice(format!("\r\n--{}\r\n", boundary).as_bytes());
        form_data.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"index.js\"; filename=\"index.js\"\r\n\
                 Content-Type: application/javascript\r\n\r\n"
            ).as_bytes()
        );
        form_data.extend_from_slice(script_content.as_bytes());
        form_data.extend_from_slice(format!("\r\n--{}--\r\n", boundary).as_bytes());

        let response = gloo_net::http::Request::put(&url)
            .header("Authorization", &format!("Bearer {}", self.api_token))
            .header("Content-Type", &format!("multipart/form-data; boundary={}", boundary))
            .body(form_data)?.send()
            .await
            .map_err(|e| CoreError::HttpError(format!("Deploy worker request failed: {}", e)))?;

        let status = response.status();
        let resp_body = response.text().await.unwrap_or_default();

        if status != 200 {
            return Err(CoreError::HttpError(format!("Cloudflare deploy error ({}): {}", status, resp_body)));
        }

        Ok(())
    }

    pub async fn deploy_pages(&self, project_name: &str, _branch: &str, content: Vec<u8>) -> Result<()> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/pages/projects/{}/deployments",
            self.account_id, project_name
        );

        let response = gloo_net::http::Request::post(&url)
            .header("Authorization", &format!("Bearer {}", self.api_token))
            .header("Content-Type", "application/octet-stream")
            .body(content)?.send()
            .await
            .map_err(|e| CoreError::HttpError(format!("Pages deploy request failed: {}", e)))?;

        let status = response.status();
        if status != 200 {
            let body = response.text().await.unwrap_or_default();
            return Err(CoreError::HttpError(format!("Pages deploy error ({}): {}", status, body)));
        }

        Ok(())
    }

    pub async fn create_d1_database(&self, name: &str) -> Result<String> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/d1/database",
            self.account_id
        );

        let body = serde_json::json!({ "name": name });
        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| CoreError::HttpError(format!("Failed to serialize D1 create request: {}", e)))?;

        let response = gloo_net::http::Request::post(&url)
            .header("Authorization", &format!("Bearer {}", self.api_token))
            .header("Content-Type", "application/json")
            .body(body_bytes)?.send()
            .await
            .map_err(|e| CoreError::HttpError(format!("Create D1 request failed: {}", e)))?;

        let status = response.status();
        let resp_body = response.text().await.unwrap_or_default();

        if status != 200 {
            return Err(CoreError::HttpError(format!("Create D1 error ({}): {}", status, resp_body)));
        }

        let json: serde_json::Value = serde_json::from_str(&resp_body)
            .map_err(|e| CoreError::HttpError(format!("Failed to parse D1 create response: {}", e)))?;

        let database_id = json["result"]["uuid"].as_str()
            .ok_or_else(|| CoreError::HttpError("No database ID in response".to_string()))?
            .to_string();

        Ok(database_id)
    }

    pub async fn d1_execute(&self, database_id: &str, sql: &str) -> Result<serde_json::Value> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/d1/database/{}/query",
            self.account_id, database_id
        );

        let body = serde_json::json!({ "sql": sql });
        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| CoreError::HttpError(format!("Failed to serialize D1 query: {}", e)))?;

        let response = gloo_net::http::Request::post(&url)
            .header("Authorization", &format!("Bearer {}", self.api_token))
            .header("Content-Type", "application/json")
            .body(body_bytes)?.send()
            .await
            .map_err(|e| CoreError::HttpError(format!("D1 query failed: {}", e)))?;

        let resp_body = response.text().await
            .map_err(|e| CoreError::HttpError(format!("Failed to read D1 response: {}", e)))?;

        if response.status() != 200 {
            return Err(CoreError::HttpError(format!(
                "D1 query error ({}): {}",
                response.status(),
                resp_body
            )));
        }

        let json: serde_json::Value = serde_json::from_str(&resp_body)
            .map_err(|e| CoreError::HttpError(format!("Failed to parse D1 response: {}", e)))?;

        Ok(json)
    }
}
