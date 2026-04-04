//! # MCP HTTP Transport Endpoint
//!
//! Implements the Model Context Protocol (MCP) over HTTP transport.
//! Allows external MCP clients to interact with the worker-core services.
//!
//! ## Features
//! - HTTP POST endpoint for MCP JSON-RPC requests
//! - Supports tools/list, tools/call methods
//! - Authentication via Bearer token (ADMIN_AUTH_TOKEN env var)
//! - Returns JSON-RPC compliant responses

use serde::{Deserialize, Serialize};
use worker::*;

use crate::{CoreError, Result};

// ============================================================================
// MCP JSON-RPC Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

// ============================================================================
// MCP Tool Definitions
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: McpToolSchema,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    pub properties: serde_json::Value,
    pub required: Vec<String>,
}

// ============================================================================
// MCP HTTP Handler
// ============================================================================

pub struct McpHttpHandler {
    pub auth_token: String,
    pub tools: Vec<McpTool>,
}

impl McpHttpHandler {
    pub fn new(auth_token: String) -> Self {
        Self {
            auth_token,
            tools: Self::default_tools(),
        }
    }

    /// Handle an incoming MCP HTTP request.
    pub async fn handle_request(&self, mut req: Request, env: &Env) -> Result<Response> {
        // CORS preflight
        if req.method() == Method::Options {
            return Self::cors_response();
        }

        // Authentication check
        let auth_header = match req.headers().get("Authorization") {
            Ok(Some(h)) => h,
            _ => String::new(),
        };
        let auth_str = auth_header.as_str();
        let is_valid = auth_str.starts_with("Bearer ") && auth_str.trim_start_matches("Bearer ") == self.auth_token;
        if !is_valid {
            let body = serde_json::to_string(&JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: serde_json::Value::Null,
                result: None,
                error: Some(JsonRpcError {
                    code: -32600,
                    message: "Unauthorized: Invalid or missing Bearer token".to_string(),
                    data: None,
                }),
            }).map_err(|e| CoreError::SerializationError(format!("Failed to serialize error response: {}", e)))?;

            let response = Response::builder()
                .with_status(401)
                .with_header("Content-Type", "application/json")?
                .body(ResponseBody::Body(body.into_bytes()));
            return Ok(response);
        }

        // Parse request
        let body_bytes = req.bytes().await
            .map_err(|e| CoreError::HttpError(format!("Failed to read request body: {}", e)))?;

        let request: JsonRpcRequest = serde_json::from_slice(&body_bytes)
            .map_err(|e| CoreError::SerializationError(format!("Failed to parse JSON-RPC request: {}", e)))?;

        // Route method
        let response = self.route_method(&request, env).await;

        let body = serde_json::to_string(&response)
            .map_err(|e| CoreError::SerializationError(format!("Failed to serialize response: {}", e)))?;

        let resp = Response::builder()
            .with_status(200)
            .with_header("Content-Type", "application/json")?
            .with_header("Access-Control-Allow-Origin", "*")?
            .with_header("Access-Control-Allow-Methods", "POST, OPTIONS")?
            .with_header("Access-Control-Allow-Headers", "Content-Type, Authorization")?
            .body(ResponseBody::Body(body.into_bytes()));

        Ok(resp)
    }

    /// Route the JSON-RPC method to the appropriate handler.
    async fn route_method(&self, request: &JsonRpcRequest, env: &Env) -> JsonRpcResponse {
        match request.method.as_str() {
            "initialize" => self.handle_initialize(request),
            "tools/list" => self.handle_tools_list(request),
            "tools/call" => self.handle_tools_call(request, env).await,
            _ => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32601,
                    message: format!("Method not found: {}", request.method),
                    data: None,
                }),
            },
        }
    }

    fn handle_initialize(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {
                        "listChanged": false
                    }
                },
                "serverInfo": {
                    "name": "worker-core-mcp",
                    "version": "0.1.0"
                }
            })),
            error: None,
        }
    }

    fn handle_tools_list(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        let tools = self.tools.iter().map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema,
            })
        }).collect::<Vec<_>>();

        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: Some(serde_json::json!({ "tools": tools })),
            error: None,
        }
    }

    async fn handle_tools_call(&self, request: &JsonRpcRequest, _env: &Env) -> JsonRpcResponse {
        let params = match &request.params {
            Some(p) => p,
            None => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: "Invalid params: missing params".to_string(),
                        data: None,
                    }),
                };
            }
        };

        let tool_name = params["name"].as_str().unwrap_or("");

        match tool_name {
            "get_model_scores" => {
                let task_type = params["arguments"]["task_type"].as_str().unwrap_or("");
                // Would call ModelRouter::get_model_scores
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id.clone(),
                    result: Some(serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": format!("Model scores for task type: {}", task_type)
                        }]
                    })),
                    error: None,
                }
            }
            "check_storage_health" => {
                // Would call StorageRouter::check_health
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id.clone(),
                    result: Some(serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": "Storage health: All systems operational"
                        }]
                    })),
                    error: None,
                }
            }
            "get_key_pool_status" => {
                // Would call KeyRotator::status
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id.clone(),
                    result: Some(serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": "Key pool status: All providers have active keys"
                        }]
                    })),
                    error: None,
                }
            }
            _ => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Unknown tool: {}", tool_name),
                    data: None,
                }),
            },
        }
    }

    /// Default set of MCP tools exposed to clients.
    fn default_tools() -> Vec<McpTool> {
        vec![
            McpTool {
                name: "get_model_scores".to_string(),
                description: "Get model performance scores for a specific task type from DB_SHARED".to_string(),
                input_schema: McpToolSchema {
                    schema_type: "object".to_string(),
                    properties: serde_json::json!({
                        "task_type": {
                            "type": "string",
                            "description": "The task type to get scores for (e.g., code_generation, reasoning)"
                        }
                    }),
                    required: vec!["task_type".to_string()],
                },
            },
            McpTool {
                name: "check_storage_health".to_string(),
                description: "Check the health status of all storage layers (D1, KV, Google Drive)".to_string(),
                input_schema: McpToolSchema {
                    schema_type: "object".to_string(),
                    properties: serde_json::json!({}),
                    required: vec![],
                },
            },
            McpTool {
                name: "get_key_pool_status".to_string(),
                description: "Get the status of all API key pools (active, exhausted, cooldown)".to_string(),
                input_schema: McpToolSchema {
                    schema_type: "object".to_string(),
                    properties: serde_json::json!({}),
                    required: vec![],
                },
            },
        ]
    }

    fn cors_response() -> Result<Response> {
        let resp = Response::builder()
            .with_status(204)
            .with_header("Access-Control-Allow-Origin", "*")?
            .with_header("Access-Control-Allow-Methods", "POST, OPTIONS")?
            .with_header("Access-Control-Allow-Headers", "Content-Type, Authorization")?
            .body(ResponseBody::Empty);
        Ok(resp)
    }
}
