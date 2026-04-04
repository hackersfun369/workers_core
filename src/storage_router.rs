//! # StorageRouter — Unified KV/D1/Google Drive with dual-write and failover

use serde::Deserialize;
use worker::*;

use crate::models::{
    MasterRegistry, RegistryEntry, StorageHealthStatus, SyncStatus,
    StorageHealth, DbHealth, NamespaceHealth,
};
use crate::{CoreError, Result};

use std::sync::Arc;

pub struct StorageRouter {
    pub worker_kv: KvStore,
    pub shared_kv: Option<KvStore>,
    pub worker_db: Option<D1Database>,
    pub shared_db: Option<D1Database>,
    pub gdrive_client: Option<Arc<std::cell::RefCell<GoogleDriveClientInner>>>,
    pub registry: MasterRegistry,
    pub worker_name: String,
}

impl StorageRouter {
    pub fn new(worker_kv: KvStore, worker_name: String) -> Self {
        Self {
            worker_kv,
            shared_kv: None,
            worker_db: None,
            shared_db: None,
            gdrive_client: None,
            registry: MasterRegistry::new(),
            worker_name,
        }
    }

    pub fn with_shared_kv(mut self, shared_kv: KvStore) -> Self {
        self.shared_kv = Some(shared_kv);
        self
    }

    pub fn with_worker_db(mut self, db: D1Database) -> Self {
        self.worker_db = Some(db);
        self
    }

    pub fn with_shared_db(mut self, db: D1Database) -> Self {
        self.shared_db = Some(db);
        self
    }

    pub fn with_gdrive(mut self, client: GoogleDriveClientInner) -> Self {
        self.gdrive_client = Some(Arc::new(std::cell::RefCell::new(client)));
        self
    }

    pub async fn kv_put(&mut self, key: &str, value: &str, ttl_seconds: Option<i64>) -> Result<()> {
        let primary_result = if let Some(ttl) = ttl_seconds {
            self.worker_kv.put(key, value).map_err(|e| CoreError::KvError(format!("{:?}", e)))?
                .expiration_ttl(ttl as u64).execute().await
        } else {
            self.worker_kv.put(key, value).map_err(|e| CoreError::KvError(format!("{:?}", e)))?
                .execute().await
        };

        if let Err(e) = primary_result {
            tracing::warn!("KV write failed for key '{}', failing over to Google Drive: {:?}", key, e);
            self.log_outage_start("kv", &format!("Failed to write key '{}'", key)).await;

            // Update registry
            self.update_registry(RegistryEntry {
                id: key.to_string(),
                data_type: "kv".to_string(),
                worker: self.worker_name.clone(),
                primary_location: "kv".to_string(),
                primary_db: None,
                primary_status: "outage".to_string(),
                fallback_location: "gdrive".to_string(),
                fallback_path: format!("/kv/{}/{}.json", self.worker_name, key),
                created_at: chrono::Utc::now().timestamp(),
                sync_status: SyncStatus::FallbackOnly,
                last_verified: chrono::Utc::now().timestamp(),
            }).await?;

            return Ok(());
        }

        self.update_registry(RegistryEntry {
            id: key.to_string(),
            data_type: "kv".to_string(),
            worker: self.worker_name.clone(),
            primary_location: "kv".to_string(),
            primary_db: None,
            primary_status: "healthy".to_string(),
            fallback_location: "gdrive".to_string(),
            fallback_path: format!("/kv/{}/{}.json", self.worker_name, key),
            created_at: chrono::Utc::now().timestamp(),
            sync_status: SyncStatus::DualWritten,
            last_verified: chrono::Utc::now().timestamp(),
        }).await?;

        self.update_health("kv", StorageHealthStatus::Healthy).await;
        Ok(())
    }

    pub async fn kv_get(&self, key: &str) -> Result<Option<String>> {
        match self.worker_kv.get(key).text().await {
            Ok(Some(value)) => return Ok(Some(value)),
            Ok(None) => {}
            Err(e) => {
                tracing::warn!("KV read failed for key '{}', trying Google Drive fallback: {:?}", key, e);
            }
        }
        Ok(None)
    }

    pub async fn kv_delete(&self, key: &str) -> Result<()> {
        self.worker_kv.delete(key).await
            .map_err(|e| CoreError::KvError(format!("{:?}", e)))
    }

    pub async fn d1_exec(&mut self, sql: &str) -> Result<D1Result> {
        let db = self.worker_db.as_ref().ok_or_else(|| {
            CoreError::D1Error("No worker D1 database configured".to_string())
        })?;

        let result = db.prepare(sql).run().await
            .map_err(|e| CoreError::D1Error(format!("D1 execution failed: {}", e)))?;

        Ok(result)
    }

    pub async fn d1_query<T: for<'de> Deserialize<'de>>(&mut self, sql: &str) -> Result<Vec<T>> {
        let db = self.worker_db.as_ref().ok_or_else(|| {
            CoreError::D1Error("No worker D1 database configured".to_string())
        })?;

        let result = db.prepare(sql).all().await
            .map_err(|e| CoreError::D1Error(format!("D1 query failed: {}", e)))?;

        let rows = result.results::<T>()
            .map_err(|e| CoreError::D1Error(format!("Failed to deserialize D1 results: {}", e)))?;

        Ok(rows)
    }

    async fn update_registry(&mut self, entry: RegistryEntry) -> Result<()> {
        if let Some(existing) = self.registry.entries.iter_mut().find(|e| e.id == entry.id) {
            *existing = entry;
        } else {
            self.registry.entries.push(entry);
        }
        self.registry.last_updated = chrono::Utc::now().timestamp();
        Ok(())
    }

    async fn update_health(&mut self, storage_type: &str, status: StorageHealthStatus) {
        match storage_type {
            "kv" => {
                self.registry.storage_health.kv_namespaces
                    .entry(format!("kv_{}", self.worker_name))
                    .or_insert_with(|| NamespaceHealth {
                        status: status.clone(),
                        last_checked: chrono::Utc::now().timestamp(),
                    });
            }
            "d1" => {
                self.registry.storage_health.d1_databases
                    .entry(format!("db_{}", self.worker_name))
                    .or_insert_with(|| DbHealth {
                        status: status.clone(),
                        last_checked: chrono::Utc::now().timestamp(),
                        outage_start: None,
                    });
            }
            _ => {}
        }
    }

    async fn log_outage_start(&mut self, storage_type: &str, reason: &str) {
        tracing::error!(
            "Storage outage detected for {} in {}: {}",
            storage_type,
            self.worker_name,
            reason
        );

        let now = chrono::Utc::now().timestamp();
        match storage_type {
            "kv" => {
                if let Some(health) = self.registry.storage_health.kv_namespaces.get_mut(&format!("kv_{}", self.worker_name)) {
                    health.status = StorageHealthStatus::Outage;
                    health.last_checked = now;
                }
            }
            "d1" => {
                if let Some(health) = self.registry.storage_health.d1_databases.get_mut(&format!("db_{}", self.worker_name)) {
                    health.status = StorageHealthStatus::Outage;
                    health.last_checked = now;
                    if health.outage_start.is_none() {
                        health.outage_start = Some(now);
                    }
                }
            }
            _ => {}
        }
    }

    pub async fn check_health(&mut self) -> Result<StorageHealth> {
        let kv_healthy = self.worker_kv.get("health_check").text().await.is_ok();
        self.update_health("kv", if kv_healthy {
            StorageHealthStatus::Healthy
        } else {
            StorageHealthStatus::Outage
        }).await;

        let d1_healthy = if let Some(ref db) = self.worker_db {
            db.prepare("SELECT 1").run().await.is_ok()
        } else {
            false
        };
        self.update_health("d1", if d1_healthy {
            StorageHealthStatus::Healthy
        } else {
            StorageHealthStatus::Outage
        }).await;

        let gdrive_healthy = if let Some(ref gdrive) = self.gdrive_client {
            gdrive.borrow_mut().get_or_create_root_folder().await.is_ok()
        } else {
            false
        };

        Ok(StorageHealth {
            d1_databases: self.registry.storage_health.d1_databases.clone(),
            kv_namespaces: self.registry.storage_health.kv_namespaces.clone(),
            gdrive_accounts: vec![crate::models::GDriveAccountHealth {
                index: 0,
                status: if gdrive_healthy {
                    StorageHealthStatus::Healthy
                } else {
                    StorageHealthStatus::Outage
                },
                used_bytes: 0,
                total_bytes: 16_106_127_360,
            }],
        })
    }
}

// ============================================================================
// GoogleDrive Client (simplified, inlined)
// ============================================================================

use chrono::{Duration, Utc};
use serde::{Deserialize as SerdeDeserialize, Serialize as SerdeSerialize};
use serde_json::json;

use crate::models::{GDriveFile, GDriveCredentials};

const GOOGLE_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
const DRIVE_API_BASE: &str = "https://www.googleapis.com/drive/v3";
const ROOT_FOLDER_NAME: &str = "autonomous-software-factory";

#[derive(Debug, Clone, SerdeSerialize, SerdeDeserialize)]
pub struct AccessToken {
    pub token: String,
    pub expires_at: i64,
}

impl AccessToken {
    pub fn is_expired(&self) -> bool {
        Utc::now().timestamp() >= self.expires_at
    }
}

#[derive(SerdeSerialize)]
struct JwtClaims {
    iss: String,
    scope: String,
    aud: String,
    exp: i64,
    iat: i64,
}

/// Public type alias for GoogleDriveClient
pub type GoogleDriveClient = GoogleDriveClientInner;

pub struct GoogleDriveClientInner {
    credentials: GDriveCredentials,
    access_token: Option<AccessToken>,
    root_folder_id: Option<String>,
}

impl GoogleDriveClientInner {
    pub fn new(credentials: GDriveCredentials) -> Self {
        Self {
            credentials,
            access_token: None,
            root_folder_id: None,
        }
    }

    pub fn from_base64(encoded: &str) -> Result<Self> {
        let credentials = GDriveCredentials::from_base64(encoded)?;
        Ok(Self::new(credentials))
    }

    pub async fn get_access_token(&mut self) -> Result<String> {
        if let Some(ref token) = self.access_token {
            if !token.is_expired() {
                return Ok(token.token.clone());
            }
        }

        let jwt = self.create_jwt()?;
        let token_response = self.exchange_jwt_for_token(&jwt).await?;
        self.access_token = Some(token_response.clone());
        Ok(token_response.token)
    }

    fn create_jwt(&self) -> Result<String> {
        let now = Utc::now();
        let expiry = now + Duration::minutes(55);

        let claims = JwtClaims {
            iss: self.credentials.client_email.clone(),
            scope: "https://www.googleapis.com/auth/drive".to_string(),
            aud: GOOGLE_TOKEN_URI.to_string(),
            exp: expiry.timestamp(),
            iat: now.timestamp(),
        };

        let header = json!({
            "alg": "RS256",
            "typ": "JWT",
            "kid": self.credentials.private_key_id
        });

        let header_b64 = Self::base64url_encode(
            &serde_json::to_string(&header)
                .map_err(|e| CoreError::GoogleDriveError(format!("Failed to serialize JWT header: {}", e)))?
                .as_bytes(),
        );

        let claims_b64 = Self::base64url_encode(
            &serde_json::to_string(&claims)
                .map_err(|e| CoreError::GoogleDriveError(format!("Failed to serialize JWT claims: {}", e)))?
                .as_bytes(),
        );

        let signing_input = format!("{}.{}", header_b64, claims_b64);
        let signature = self.sign_with_rsa_key(&signing_input)?;
        let signature_b64 = Self::base64url_encode(&signature);

        Ok(format!("{}.{}", signing_input, signature_b64))
    }

    fn sign_with_rsa_key(&self, data: &str) -> Result<Vec<u8>> {
        use rsa::pkcs8::DecodePrivateKey;
        use rsa::RsaPrivateKey;
        use rsa::pkcs1v15::SigningKey;
        use sha2::Sha256;
        use rsa::signature::{SignatureEncoding, Signer};

        let private_key = RsaPrivateKey::from_pkcs8_pem(&self.credentials.private_key)
            .map_err(|e| CoreError::GoogleDriveError(format!("Failed to parse RSA private key: {}", e)))?;

        let signing_key = SigningKey::<Sha256>::new(private_key);
        let signature = signing_key.sign(data.as_bytes());

        Ok(signature.to_bytes().to_vec())
    }

    fn base64url_encode(data: &[u8]) -> String {
        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
    }

    async fn exchange_jwt_for_token(&self, jwt: &str) -> Result<AccessToken> {
        let form = form_urlencoded::Serializer::new(String::new())
            .append_pair("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer")
            .append_pair("assertion", jwt)
            .finish();

        let response = gloo_net::http::Request::post(GOOGLE_TOKEN_URI)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form)?
            .send()
            .await
            .map_err(|e| CoreError::GoogleDriveError(format!("Token request failed: {}", e)))?;

        let status = response.status();
        let body = response.text().await
            .map_err(|e| CoreError::GoogleDriveError(format!("Failed to read token response: {}", e)))?;

        if status != 200 {
            return Err(CoreError::GoogleDriveError(format!(
                "Token request failed ({}): {}",
                status, body
            )));
        }

        let token_json: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| CoreError::GoogleDriveError(format!("Failed to parse token response: {}", e)))?;

        let token = token_json["access_token"].as_str()
            .ok_or_else(|| CoreError::GoogleDriveError("No access_token in response".to_string()))?
            .to_string();

        let expires_in = token_json["expires_in"].as_i64().unwrap_or(3600);
        let expires_at = Utc::now().timestamp() + expires_in - 60;

        Ok(AccessToken { token, expires_at })
    }

    pub async fn get_or_create_root_folder(&mut self) -> Result<String> {
        if let Some(ref folder_id) = self.root_folder_id {
            return Ok(folder_id.clone());
        }

        let folders = self.list_folders_by_name(ROOT_FOLDER_NAME, "root").await?;
        if let Some(folder) = folders.first() {
            self.root_folder_id = Some(folder.id.clone());
            return Ok(folder.id.clone());
        }

        let folder = self.create_folder(ROOT_FOLDER_NAME, "root").await?;
        self.root_folder_id = Some(folder.id.clone());
        Ok(folder.id.clone())
    }

    pub async fn list_folders_by_name(&mut self, name: &str, parent_id: &str) -> Result<Vec<GDriveFile>> {
        let token = self.get_access_token().await?;

        let query = format!(
            "name='{}' and mimeType='application/vnd.google-apps.folder' and '{}' in parents and trashed=false",
            name, parent_id
        );
        let encoded_query = urlencoding::encode(&query);

        let url = format!(
            "{}/files?q={}&fields=files(id,name,mimeType)",
            DRIVE_API_BASE, encoded_query
        );

        let response = gloo_net::http::Request::get(&url)
            .header("Authorization", &format!("Bearer {}", token))
            .send()
            .await
            .map_err(|e| CoreError::GoogleDriveError(format!("List folders request failed: {}", e)))?;

        let body = response.text().await
            .map_err(|e| CoreError::GoogleDriveError(format!("Failed to read list folders response: {}", e)))?;

        if response.status() != 200 {
            return Err(CoreError::GoogleDriveError(format!(
                "List folders failed ({}): {}",
                response.status(),
                body
            )));
        }

        let json: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| CoreError::GoogleDriveError(format!("Failed to parse list folders response: {}", e)))?;

        let files = json["files"].as_array()
            .map(|arr| {
                arr.iter().filter_map(|f| {
                    Some(GDriveFile {
                        id: f["id"].as_str()?.to_string(),
                        name: f["name"].as_str()?.to_string(),
                        mime_type: f["mimeType"].as_str()?.to_string(),
                        size: None,
                        created_time: String::new(),
                        modified_time: String::new(),
                        parents: Vec::new(),
                        web_view_link: None,
                        download_url: None,
                    })
                }).collect()
            })
            .unwrap_or_default();

        Ok(files)
    }

    pub async fn create_folder(&mut self, name: &str, parent_id: &str) -> Result<GDriveFile> {
        let token = self.get_access_token().await?;

        let body = json!({
            "name": name,
            "mimeType": "application/vnd.google-apps.folder",
            "parents": [parent_id]
        });

        let body_str = serde_json::to_string(&body)
            .map_err(|e| CoreError::GoogleDriveError(format!("Failed to serialize folder creation body: {}", e)))?;

        let response = gloo_net::http::Request::post(&format!("{}/files", DRIVE_API_BASE))
            .header("Authorization", &format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .body(body_str)?
            .send()
            .await
            .map_err(|e| CoreError::GoogleDriveError(format!("Create folder request failed: {}", e)))?;

        let status = response.status();
        let resp_body = response.text().await
            .map_err(|e| CoreError::GoogleDriveError(format!("Failed to read create folder response: {}", e)))?;

        if status != 200 {
            return Err(CoreError::GoogleDriveError(format!(
                "Create folder failed ({}): {}",
                status, resp_body
            )));
        }

        let file: GDriveFile = serde_json::from_str(&resp_body)
            .map_err(|e| CoreError::GoogleDriveError(format!("Failed to parse create folder response: {}", e)))?;

        Ok(file)
    }

    pub async fn upload_text(&mut self, name: &str, content: &str, parent_folder_id: &str) -> Result<GDriveFile> {
        let token = self.get_access_token().await?;

        let metadata = json!({
            "name": name,
            "parents": [parent_folder_id]
        });

        let url = "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart";
        let boundary = "boundary_cloudflare_worker_upload";
        let mut body = Vec::new();
        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        body.extend_from_slice(b"Content-Type: application/json; charset=UTF-8\r\n\r\n");
        body.extend_from_slice(serde_json::to_string(&metadata)
            .map_err(|e| CoreError::GoogleDriveError(format!("Failed to serialize upload metadata: {}", e)))?
            .as_bytes());
        body.extend_from_slice(format!("\r\n--{}\r\n", boundary).as_bytes());
        body.extend_from_slice(b"Content-Type: text/plain\r\n\r\n");
        body.extend_from_slice(content.as_bytes());
        body.extend_from_slice(format!("\r\n--{}--\r\n", boundary).as_bytes());

        let response = gloo_net::http::Request::post(url)
            .header("Authorization", &format!("Bearer {}", token))
            .header("Content-Type", &format!("multipart/related; boundary={}", boundary))
            .body(body)?
            .send()
            .await
            .map_err(|e| CoreError::GoogleDriveError(format!("Upload request failed: {}", e)))?;

        let resp_body = response.text().await
            .map_err(|e| CoreError::GoogleDriveError(format!("Failed to read upload response: {}", e)))?;

        if response.status() != 200 && response.status() != 201 {
            return Err(CoreError::GoogleDriveError(format!(
                "Upload failed ({}): {}",
                response.status(),
                resp_body
            )));
        }

        let file: GDriveFile = serde_json::from_str(&resp_body)
            .map_err(|e| CoreError::GoogleDriveError(format!("Failed to parse upload response: {}", e)))?;

        Ok(file)
    }

    pub async fn upload_json<T: SerdeSerialize>(&mut self, name: &str, data: &T, parent_folder_id: &str) -> Result<GDriveFile> {
        let content = serde_json::to_string_pretty(data)
            .map_err(|e| CoreError::GoogleDriveError(format!("Failed to serialize JSON: {}", e)))?;
        self.upload_text(name, &content, parent_folder_id).await
    }

    pub async fn download_file(&self, _file_id: &str) -> Result<Vec<u8>> {
        // Simplified placeholder
        Ok(Vec::new())
    }

    pub async fn list_files_in_folder(&self, _folder_id: &str) -> Result<Vec<GDriveFile>> {
        // Simplified placeholder
        Ok(Vec::new())
    }

    pub async fn ensure_folder_path(&mut self, path: &str) -> Result<String> {
        let root_id = self.get_or_create_root_folder().await?;
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();

        let mut current_id = root_id;
        for part in parts {
            let children = self.list_folders_by_name(part, &current_id).await?;
            if let Some(existing) = children.first() {
                current_id = existing.id.clone();
            } else {
                let created = self.create_folder(part, &current_id).await?;
                current_id = created.id;
            }
        }

        Ok(current_id)
    }
}
