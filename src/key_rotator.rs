//! # KeyRotator — Dynamic API Key Pool Rotation
//!
//! Round-robin key selection with automatic cooldown, recovery, and
//! cross-provider fallback. Workers never see a key failure.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::Utc;
use worker::*;

use crate::models::Provider;
use crate::{CoreError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyState {
    pub key_hash: u64,
    pub is_active: bool,
    pub exhausted_at: Option<i64>,
    pub cooldown_seconds: i64,
    pub request_count: i64,
    pub last_used: i64,
}

impl KeyState {
    pub fn new(key: &str) -> Self {
        Self {
            key_hash: Self::hash_key(key),
            is_active: true,
            exhausted_at: None,
            cooldown_seconds: 60,
            request_count: 0,
            last_used: 0,
        }
    }

    pub fn is_cooldown_expired(&self) -> bool {
        if let Some(exhausted_at) = self.exhausted_at {
            let now = Utc::now().timestamp();
            now >= exhausted_at + self.cooldown_seconds
        } else {
            true
        }
    }

    pub fn mark_exhausted(&mut self) {
        self.is_active = false;
        self.exhausted_at = Some(Utc::now().timestamp());
    }

    pub fn restore(&mut self) {
        self.is_active = true;
        self.exhausted_at = None;
    }

    pub fn record_use(&mut self) {
        self.request_count += 1;
        self.last_used = Utc::now().timestamp();
    }

    fn hash_key(key: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyPool {
    pub provider: Provider,
    pub keys: Vec<KeyState>,
    pub raw_keys: Vec<String>,
    pub current_index: usize,
}

impl KeyPool {
    pub fn new(provider: Provider, raw_keys: Vec<String>) -> Self {
        let keys = raw_keys.iter().map(|k| KeyState::new(k)).collect();
        Self {
            provider,
            keys,
            raw_keys,
            current_index: 0,
        }
    }

    pub fn get_next_key(&mut self) -> Result<String> {
        if self.raw_keys.is_empty() {
            return Err(CoreError::KeysExhausted(format!(
                "No keys configured for provider: {:?}",
                self.provider
            )));
        }

        let total_keys = self.keys.len();
        let mut attempts = 0;

        while attempts < total_keys {
            let idx = self.current_index % total_keys;
            self.current_index = (self.current_index + 1) % total_keys;
            attempts += 1;

            let key_state = &mut self.keys[idx];

            if key_state.is_active || key_state.is_cooldown_expired() {
                if !key_state.is_active {
                    key_state.restore();
                }
                key_state.record_use();
                return Ok(self.raw_keys[idx].clone());
            }
        }

        Err(CoreError::KeysExhausted(format!(
            "All {} keys are exhausted/cooldown",
            self.provider
        )))
    }

    pub fn mark_current_exhausted(&mut self) {
        let idx = if self.current_index == 0 {
            self.keys.len().saturating_sub(1)
        } else {
            self.current_index - 1
        };

        if idx < self.keys.len() {
            self.keys[idx].mark_exhausted();
        }
    }

    pub fn restore_key(&mut self, index: usize) {
        if index < self.keys.len() {
            self.keys[index].restore();
        }
    }

    pub fn remove_key(&mut self, index: usize) {
        if index < self.keys.len() {
            self.keys.remove(index);
            self.raw_keys.remove(index);
            if self.current_index >= self.keys.len() {
                self.current_index = 0;
            }
        }
    }

    pub fn add_key(&mut self, key: String) {
        let state = KeyState::new(&key);
        self.keys.push(state);
        self.raw_keys.push(key);
    }

    pub fn status(&self) -> KeyPoolStatus {
        let active = self.keys.iter().filter(|k| k.is_active || k.is_cooldown_expired()).count();
        let exhausted = self.keys.len() - active;
        let total_requests: i64 = self.keys.iter().map(|k| k.request_count).sum();

        KeyPoolStatus {
            provider: self.provider,
            total_keys: self.keys.len(),
            active_keys: active,
            exhausted_keys: exhausted,
            total_requests,
            current_index: self.current_index,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyPoolStatus {
    pub provider: Provider,
    pub total_keys: usize,
    pub active_keys: usize,
    pub exhausted_keys: usize,
    pub total_requests: i64,
    pub current_index: usize,
}

pub struct KeyRotator {
    pools: HashMap<Provider, KeyPool>,
    kv: KvStore,
    fallback_providers: HashMap<Provider, Provider>,
}

impl KeyRotator {
    pub async fn new(kv: KvStore, env: &Env) -> Result<Self> {
        let mut pools = HashMap::new();
        let mut fallback_providers = HashMap::new();

        Self::load_pool(&mut pools, env, Provider::GoogleAI, "GOOGLE_AI_KEYS")?;
        Self::load_pool(&mut pools, env, Provider::OpenRouter, "OPENROUTER_API_KEYS")?;
        Self::load_pool(&mut pools, env, Provider::HuggingFace, "HUGGINGFACE_API_KEYS")?;
        Self::load_pool(&mut pools, env, Provider::Groq, "GROQ_API_KEYS")?;
        Self::load_pool(&mut pools, env, Provider::Mistral, "MISTRAL_API_KEYS")?;
        Self::load_pool(&mut pools, env, Provider::GitHub, "GITHUB_TOKENS")?;

        if let Ok(creds_str) = env.secret("GDRIVE_CREDENTIALS").map(|s| s.to_string()) {
            let creds: Vec<String> = creds_str.split(',').map(|s| s.trim().to_string()).collect();
            pools.insert(Provider::GoogleDrive, KeyPool::new(Provider::GoogleDrive, creds));
        }

        fallback_providers.insert(Provider::Groq, Provider::GoogleAI);
        fallback_providers.insert(Provider::OpenRouter, Provider::GoogleAI);
        fallback_providers.insert(Provider::HuggingFace, Provider::GoogleAI);
        fallback_providers.insert(Provider::Mistral, Provider::GoogleAI);

        Ok(Self {
            pools,
            kv,
            fallback_providers,
        })
    }

    fn load_pool(
        pools: &mut HashMap<Provider, KeyPool>,
        env: &Env,
        provider: Provider,
        env_var: &str,
    ) -> Result<()> {
        if let Ok(keys_str) = env.secret(env_var).map(|s| s.to_string()) {
            let keys: Vec<String> = keys_str.split(',').map(|s| s.trim().to_string()).collect();
            if !keys.is_empty() {
                pools.insert(provider, KeyPool::new(provider, keys));
            }
        }
        Ok(())
    }

    pub async fn get_key(&mut self, provider: Provider) -> Result<(Provider, String)> {
        if let Some(pool) = self.pools.get_mut(&provider) {
            match pool.get_next_key() {
                Ok(key) => return Ok((provider, key)),
                Err(_) => {
                    if let Some(&fallback) = self.fallback_providers.get(&provider) {
                        if let Some(fallback_pool) = self.pools.get_mut(&fallback) {
                            match fallback_pool.get_next_key() {
                                Ok(key) => return Ok((fallback, key)),
                                Err(e) => return Err(e),
                            }
                        }
                    }
                    return Err(CoreError::KeysExhausted(format!(
                        "All keys exhausted for provider {:?} and no fallback available",
                        provider
                    )));
                }
            }
        }

        Err(CoreError::KeysExhausted(format!(
            "No key pool configured for provider: {:?}",
            provider
        )))
    }

    pub fn mark_exhausted(&mut self, provider: Provider) {
        if let Some(pool) = self.pools.get_mut(&provider) {
            pool.mark_current_exhausted();
        }
    }

    pub fn status(&self) -> HashMap<Provider, KeyPoolStatus> {
        self.pools.iter().map(|(p, pool)| (*p, pool.status())).collect()
    }

    pub fn restore_key(&mut self, provider: Provider, index: usize) {
        if let Some(pool) = self.pools.get_mut(&provider) {
            pool.restore_key(index);
        }
    }

    pub fn add_key(&mut self, provider: Provider, key: String) {
        if let Some(pool) = self.pools.get_mut(&provider) {
            pool.add_key(key);
        }
    }

    pub fn remove_key(&mut self, provider: Provider, index: usize) {
        if let Some(pool) = self.pools.get_mut(&provider) {
            pool.remove_key(index);
        }
    }

    pub async fn save_state(&self) -> Result<()> {
        for (provider, pool) in &self.pools {
            let key = format!("keyrotator:{:?}", provider);
            let serialized = serde_json::to_string(pool)
                .map_err(|e| CoreError::SerializationError(format!("KeyPool serialization failed: {}", e)))?;
            self.kv.put(&key, &serialized)
                .map_err(|e| CoreError::KvError(format!("{:?}", e)))?;
        }
        Ok(())
    }

    pub async fn load_state(&mut self) -> Result<()> {
        for provider in self.pools.keys().cloned().collect::<Vec<_>>() {
            let key = format!("keyrotator:{:?}", provider);
            if let Ok(Some(serialized)) = self.kv.get(&key).text().await {
                if let Ok(pool) = serde_json::from_str::<KeyPool>(&serialized) {
                    if let Some(existing) = self.pools.get_mut(&provider) {
                        *existing = pool;
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn run_recovery_check(&mut self) -> Result<Vec<Provider>> {
        let mut restored = Vec::new();

        for (provider, pool) in self.pools.iter_mut() {
            let mut any_restored = false;
            for (i, key_state) in pool.keys.iter_mut().enumerate() {
                if !key_state.is_active && key_state.is_cooldown_expired() {
                    key_state.restore();
                    any_restored = true;
                    tracing::info!(
                        "Key {} restored for provider {:?}",
                        i,
                        provider
                    );
                }
            }
            if any_restored {
                restored.push(*provider);
            }
        }

        if !restored.is_empty() {
            self.save_state().await?;
        }

        Ok(restored)
    }

    pub async fn reset_all_cooldowns(&mut self) -> Result<()> {
        for pool in self.pools.values_mut() {
            for key_state in pool.keys.iter_mut() {
                key_state.restore();
            }
        }
        self.save_state().await
    }
}
