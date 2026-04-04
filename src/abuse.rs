//! # AbuseGuard — Rate Limiting, Deduplication, Auto-Suspension

use worker::KvStore;
use chrono::Utc;
use blake3::Hasher;

use crate::{CoreError, Result};

pub struct AbuseGuard {
    pub kv: KvStore,
    pub worker_id: String,
    pub max_per_hour: u32,
    pub max_per_day: u32,
    pub suspend_threshold: u32,
    pub suspend_duration: i64,
    pub dedup_window: i64,
}

impl AbuseGuard {
    pub fn new(kv: KvStore, worker_id: String) -> Self {
        Self {
            kv,
            worker_id,
            max_per_hour: 5,
            max_per_day: 50,
            suspend_threshold: 20,
            suspend_duration: 86400,
            dedup_window: 86400,
        }
    }

    pub fn with_max_per_hour(mut self, max: u32) -> Self {
        self.max_per_hour = max;
        self
    }

    pub fn with_max_per_day(mut self, max: u32) -> Self {
        self.max_per_day = max;
        self
    }

    pub fn with_suspend_threshold(mut self, threshold: u32) -> Self {
        self.suspend_threshold = threshold;
        self
    }

    pub fn with_suspend_duration(mut self, duration: i64) -> Self {
        self.suspend_duration = duration;
        self
    }

    pub async fn check(&self, user_id: &str) -> Result<AbuseGuardResult> {
        if self.is_suspended(user_id).await? {
            return Err(CoreError::AbuseGuard(format!(
                "User {} is suspended for worker {}",
                user_id, self.worker_id
            )));
        }

        let hourly_count = self.get_hourly_count(user_id).await?;
        if hourly_count >= self.max_per_hour {
            return Err(CoreError::RateLimitError(format!(
                "Hourly rate limit exceeded for user {} on worker {}",
                user_id, self.worker_id
            )));
        }

        let daily_count = self.get_daily_count(user_id).await?;
        if daily_count >= self.max_per_day {
            return Err(CoreError::RateLimitError(format!(
                "Daily rate limit exceeded for user {} on worker {}",
                user_id, self.worker_id
            )));
        }

        Ok(AbuseGuardResult {
            hourly_count,
            daily_count,
            is_suspended: false,
        })
    }

    pub async fn record_submission(&self, user_id: &str) -> Result<()> {
        let now = Utc::now().timestamp();

        let hourly_key = format!("abuse:{}:{}:hourly", self.worker_id, user_id);
        let hourly_count = self.increment_counter(&hourly_key, 3600).await?;

        let daily_key = format!("abuse:{}:{}:daily", self.worker_id, user_id);
        self.increment_counter(&daily_key, 86400).await?;

        if hourly_count >= self.suspend_threshold {
            let suspend_key = format!("abuse:{}:{}:suspended", self.worker_id, user_id);
            self.kv.put(&suspend_key, &now.to_string())?
                .expiration_ttl(self.suspend_duration as u64)
                .execute()
                .await
                .map_err(|e| CoreError::KvError(format!("Failed to set suspension flag: {:?}", e)))?;

            tracing::warn!(
                "User {} auto-suspended on worker {} ({} submissions in 1 hour)",
                user_id, self.worker_id, hourly_count
            );
        }

        Ok(())
    }

    pub async fn is_duplicate(&self, user_id: &str, content: &str) -> Result<bool> {
        let hash = Self::hash_content(content);
        let dedup_key = format!("abuse:{}:{}:dedup:{}", self.worker_id, user_id, hash);

        match self.kv.get(&dedup_key).text().await {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(e) => Err(CoreError::KvError(format!("Failed to check dedup: {:?}", e))),
        }
    }

    pub async fn mark_seen(&self, user_id: &str, content: &str) -> Result<()> {
        let hash = Self::hash_content(content);
        let dedup_key = format!("abuse:{}:{}:dedup:{}", self.worker_id, user_id, hash);

        let now = Utc::now().timestamp();
        self.kv.put(&dedup_key, &now.to_string())?
            .expiration_ttl(self.dedup_window as u64)
            .execute()
            .await
            .map_err(|e| CoreError::KvError(format!("Failed to mark document as seen: {:?}", e)))?;

        Ok(())
    }

    pub async fn check_and_record(&self, user_id: &str, content: &str) -> Result<SubmissionResult> {
        let result = self.check(user_id).await?;

        if self.is_duplicate(user_id, content).await? {
            return Ok(SubmissionResult::Duplicate(result));
        }

        self.record_submission(user_id).await?;
        self.mark_seen(user_id, content).await?;

        Ok(SubmissionResult::New(result))
    }

    pub async fn suspend_user(&self, user_id: &str, duration: Option<i64>) -> Result<()> {
        let now = Utc::now().timestamp();
        let duration = duration.unwrap_or(self.suspend_duration);
        let suspend_key = format!("abuse:{}:{}:suspended", self.worker_id, user_id);

        self.kv.put(&suspend_key, &now.to_string())?
            .expiration_ttl(duration as u64)
            .execute()
            .await
            .map_err(|e| CoreError::KvError(format!("Failed to suspend user: {:?}", e)))?;

        tracing::warn!("User {} manually suspended on worker {}", user_id, self.worker_id);
        Ok(())
    }

    pub async fn unsuspend_user(&self, user_id: &str) -> Result<()> {
        let suspend_key = format!("abuse:{}:{}:suspended", self.worker_id, user_id);
        self.kv.delete(&suspend_key).await
            .map_err(|e| CoreError::KvError(format!("Failed to unsuspend user: {:?}", e)))?;

        tracing::info!("User {} manually unsuspended on worker {}", user_id, self.worker_id);
        Ok(())
    }

    pub async fn reset_limits(&self, user_id: &str) -> Result<()> {
        let hourly_key = format!("abuse:{}:{}:hourly", self.worker_id, user_id);
        let daily_key = format!("abuse:{}:{}:daily", self.worker_id, user_id);

        self.kv.delete(&hourly_key).await.ok();
        self.kv.delete(&daily_key).await.ok();

        Ok(())
    }

    pub async fn get_hourly_count(&self, user_id: &str) -> Result<u32> {
        let key = format!("abuse:{}:{}:hourly", self.worker_id, user_id);
        match self.kv.get(&key).text().await {
            Ok(Some(val)) => Ok(val.parse().unwrap_or(0)),
            _ => Ok(0),
        }
    }

    pub async fn get_daily_count(&self, user_id: &str) -> Result<u32> {
        let key = format!("abuse:{}:{}:daily", self.worker_id, user_id);
        match self.kv.get(&key).text().await {
            Ok(Some(val)) => Ok(val.parse().unwrap_or(0)),
            _ => Ok(0),
        }
    }

    async fn is_suspended(&self, user_id: &str) -> Result<bool> {
        let key = format!("abuse:{}:{}:suspended", self.worker_id, user_id);
        match self.kv.get(&key).text().await {
            Ok(Some(_)) => Ok(true),
            _ => Ok(false),
        }
    }

    async fn increment_counter(&self, key: &str, ttl: u64) -> Result<u32> {
        match self.kv.get(key).text().await {
            Ok(Some(val)) => {
                let count: u32 = val.parse().unwrap_or(0);
                let new_count = count + 1;
                self.kv.put(key, &new_count.to_string())?
                    .expiration_ttl(ttl)
                    .execute()
                    .await
                    .map_err(|e| CoreError::KvError(format!("Failed to increment counter: {:?}", e)))?;
                Ok(new_count)
            }
            _ => {
                self.kv.put(key, "1")?
                    .expiration_ttl(ttl)
                    .execute()
                    .await
                    .map_err(|e| CoreError::KvError(format!("Failed to create counter: {:?}", e)))?;
                Ok(1)
            }
        }
    }

    fn hash_content(content: &str) -> String {
        let mut hasher = Hasher::new();
        hasher.update(content.as_bytes());
        hasher.finalize().to_hex().to_string()[..16].to_string()
    }
}

#[derive(Debug, Clone)]
pub struct AbuseGuardResult {
    pub hourly_count: u32,
    pub daily_count: u32,
    pub is_suspended: bool,
}

#[derive(Debug, Clone)]
pub enum SubmissionResult {
    New(AbuseGuardResult),
    Duplicate(AbuseGuardResult),
}

impl SubmissionResult {
    pub fn is_new(&self) -> bool {
        matches!(self, SubmissionResult::New(_))
    }

    pub fn is_duplicate(&self) -> bool {
        matches!(self, SubmissionResult::Duplicate(_))
    }

    pub fn abuse_guard_result(&self) -> AbuseGuardResult {
        match self {
            SubmissionResult::New(result) => result.clone(),
            SubmissionResult::Duplicate(result) => result.clone(),
        }
    }
}
