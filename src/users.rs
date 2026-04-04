//! # UserManager — Account, Session, Credit, Referral Management

use chrono::Utc;
use worker::*;
use wasm_bindgen::JsValue;

use crate::models::{
    User, UserPlan, UserSession, DiscountCode, Referral, SupportTicket, UserStatus
};
use crate::{CoreError, Result};

pub struct UserManager {
    pub db: D1Database,
    pub kv: Option<KvStore>,
}

impl UserManager {
    pub fn new(db: D1Database) -> Self {
        Self { db, kv: None }
    }

    pub fn with_kv(mut self, kv: KvStore) -> Self {
        self.kv = Some(kv);
        self
    }

    pub async fn create_user(&self, name: String, email: String, phone: String) -> Result<User> {
        if !email.is_empty() {
            if let Some(existing) = self.find_user_by_email(&email).await? {
                return Ok(existing);
            }
        }

        if !phone.is_empty() {
            if let Some(existing) = self.find_user_by_phone(&phone).await? {
                return Ok(existing);
            }
        }

        let user = User::new(name, email.clone(), phone.clone());

        let stmt = self.db.prepare(
            "INSERT INTO users (user_id, name, email, phone, created_at, status, referral_code, referred_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
        );

        stmt.bind(&[
            JsValue::from_str(&user.user_id),
            JsValue::from_str(&user.name),
            JsValue::from_str(&user.email),
            JsValue::from_str(&user.phone),
            JsValue::from_f64(user.created_at as f64),
            JsValue::from_str(&format!("{:?}", user.status)),
            JsValue::from_str(&user.referral_code),
            JsValue::NULL,
        ])
        .map_err(|e| CoreError::D1Error(format!("Failed to bind create user: {}", e)))?
        .run()
        .await
        .map_err(|e| CoreError::D1Error(format!("Failed to create user: {}", e)))?;

        Ok(user)
    }

    pub async fn find_user_by_id(&self, user_id: &str) -> Result<Option<User>> {
        let stmt = self.db.prepare("SELECT * FROM users WHERE user_id = ?1");
        let result = stmt.bind(&[JsValue::from_str(user_id)])
            .map_err(|e| CoreError::D1Error(format!("Failed to bind user query: {}", e)))?
            .first::<User>(None)
            .await
            .map_err(|e| CoreError::D1Error(format!("Failed to query user: {}", e)))?;
        Ok(result)
    }

    pub async fn find_user_by_email(&self, email: &str) -> Result<Option<User>> {
        let stmt = self.db.prepare("SELECT * FROM users WHERE email = ?1");
        let result = stmt.bind(&[JsValue::from_str(email)])
            .map_err(|e| CoreError::D1Error(format!("Failed to bind email query: {}", e)))?
            .first::<User>(None)
            .await
            .map_err(|e| CoreError::D1Error(format!("Failed to query user by email: {}", e)))?;
        Ok(result)
    }

    pub async fn find_user_by_phone(&self, phone: &str) -> Result<Option<User>> {
        let stmt = self.db.prepare("SELECT * FROM users WHERE phone = ?1");
        let result = stmt.bind(&[JsValue::from_str(phone)])
            .map_err(|e| CoreError::D1Error(format!("Failed to bind phone query: {}", e)))?
            .first::<User>(None)
            .await
            .map_err(|e| CoreError::D1Error(format!("Failed to query user by phone: {}", e)))?;
        Ok(result)
    }

    pub async fn suspend_user(&self, user_id: &str) -> Result<()> {
        self.update_user_status(user_id, UserStatus::Suspended).await
    }

    pub async fn unsuspend_user(&self, user_id: &str) -> Result<()> {
        self.update_user_status(user_id, UserStatus::Active).await
    }

    pub async fn delete_user(&self, user_id: &str) -> Result<()> {
        self.update_user_status(user_id, UserStatus::Deleted).await
    }

    async fn update_user_status(&self, user_id: &str, status: UserStatus) -> Result<()> {
        let stmt = self.db.prepare("UPDATE users SET status = ?1 WHERE user_id = ?2");
        stmt.bind(&[
            JsValue::from_str(&format!("{:?}", status)),
            JsValue::from_str(user_id),
        ])
        .map_err(|e| CoreError::D1Error(format!("Failed to bind update status: {}", e)))?
        .run()
        .await
        .map_err(|e| CoreError::D1Error(format!("Failed to update user status: {}", e)))?;
        Ok(())
    }

    pub async fn send_otp(&self, phone_or_email: &str) -> Result<String> {
        let otp = format!("{:06}", fastrand::u32(0..999999));

        if let Some(ref kv) = self.kv {
            let key = format!("otp:{}", phone_or_email);
            kv.put(&key, &otp)?
                .expiration_ttl(300)
                .execute()
                .await
                .map_err(|e| CoreError::KvError(format!("Failed to store OTP: {:?}", e)))?;
        }

        tracing::info!("OTP generated for {}: ******", phone_or_email);
        Ok(otp)
    }

    pub async fn verify_otp(&self, phone_or_email: &str, otp: &str) -> Result<bool> {
        if let Some(ref kv) = self.kv {
            let key = format!("otp:{}", phone_or_email);
            match kv.get(&key).text().await {
                Ok(Some(stored_otp)) => {
                    let valid = stored_otp == otp;
                    kv.delete(&key).await.ok();
                    return Ok(valid);
                }
                Ok(None) => return Ok(false),
                Err(_) => return Ok(false),
            }
        }
        Ok(false)
    }

    pub async fn get_active_plan(&self, user_id: &str) -> Result<Option<UserPlan>> {
        let stmt = self.db.prepare(
            "SELECT * FROM user_plans WHERE user_id = ?1 AND status = 'active' ORDER BY created_at DESC LIMIT 1"
        );

        let result = stmt.bind(&[JsValue::from_str(user_id)])
            .map_err(|e| CoreError::D1Error(format!("Failed to bind active plan query: {}", e)))?
            .first::<UserPlan>(None)
            .await
            .map_err(|e| CoreError::D1Error(format!("Failed to query active plan: {}", e)))?;

        if let Some(mut plan) = result {
            if plan.is_expired() {
                self.expire_plan(&plan.plan_id).await?;
                return Ok(None);
            }
            return Ok(Some(plan));
        }

        Ok(None)
    }

    pub async fn consume_credit(&self, user_id: &str) -> Result<bool> {
        let plan = self.get_active_plan(user_id).await?;
        match plan {
            Some(mut plan) if plan.has_credits() => {
                plan.consume_credit();

                let stmt = self.db.prepare(
                    "UPDATE user_plans SET used_credits = ?1, remaining_credits = ?2 WHERE plan_id = ?3"
                );

                stmt.bind(&[
                    JsValue::from_f64(plan.used_credits as f64),
                    JsValue::from_f64(plan.remaining_credits as f64),
                    JsValue::from_str(&plan.plan_id),
                ])
                .map_err(|e| CoreError::D1Error(format!("Failed to bind credit consumption: {}", e)))?
                .run()
                .await
                .map_err(|e| CoreError::D1Error(format!("Failed to consume credit: {}", e)))?;

                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub async fn expire_plan(&self, plan_id: &str) -> Result<()> {
        let stmt = self.db.prepare("UPDATE user_plans SET status = 'expired' WHERE plan_id = ?1");
        stmt.bind(&[JsValue::from_str(plan_id)])
            .map_err(|e| CoreError::D1Error(format!("Failed to bind expire plan: {}", e)))?
            .run()
            .await
            .map_err(|e| CoreError::D1Error(format!("Failed to expire plan: {}", e)))?;
        Ok(())
    }

    pub async fn create_session(&self, user_id: &str, worker_id: &str, plan_id: &str) -> Result<UserSession> {
        let session = UserSession::new(user_id.to_string(), worker_id.to_string(), plan_id.to_string());

        let stmt = self.db.prepare(
            "INSERT INTO user_sessions (session_id, user_id, worker_id, plan_id, started_at, completed_at,
             status, output_path, credits_consumed, feedback_score, feedback_text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"
        );

        stmt.bind(&[
            JsValue::from_str(&session.session_id),
            JsValue::from_str(&session.user_id),
            JsValue::from_str(&session.worker_id),
            JsValue::from_str(&session.plan_id),
            JsValue::from_f64(session.started_at as f64),
            JsValue::NULL,
            JsValue::from_str(&session.status),
            JsValue::NULL,
            JsValue::from_f64(session.credits_consumed as f64),
            JsValue::NULL,
            JsValue::NULL,
        ])
        .map_err(|e| CoreError::D1Error(format!("Failed to bind session creation: {}", e)))?
        .run()
        .await
        .map_err(|e| CoreError::D1Error(format!("Failed to create session: {}", e)))?;

        Ok(session)
    }

    pub async fn complete_session(&self, session_id: &str, output_path: &str) -> Result<()> {
        let stmt = self.db.prepare(
            "UPDATE user_sessions SET completed_at = ?1, status = 'completed', output_path = ?2
             WHERE session_id = ?3"
        );

        let now = Utc::now().timestamp();
        stmt.bind(&[
            JsValue::from_f64(now as f64),
            JsValue::from_str(output_path),
            JsValue::from_str(session_id),
        ])
        .map_err(|e| CoreError::D1Error(format!("Failed to bind session completion: {}", e)))?
        .run()
        .await
        .map_err(|e| CoreError::D1Error(format!("Failed to complete session: {}", e)))?;

        Ok(())
    }

    pub async fn get_user_sessions(&self, user_id: &str, worker_id: Option<&str>) -> Result<Vec<UserSession>> {
        let sql = if let Some(wid) = worker_id {
            "SELECT * FROM user_sessions WHERE user_id = ?1 AND worker_id = ?2 ORDER BY started_at DESC"
        } else {
            "SELECT * FROM user_sessions WHERE user_id = ?1 ORDER BY started_at DESC"
        };

        let stmt = self.db.prepare(sql);
        let bound = if let Some(wid) = worker_id {
            stmt.bind(&[
                JsValue::from_str(user_id),
                JsValue::from_str(wid),
            ]).map_err(|e| CoreError::D1Error(format!("Failed to bind sessions: {}", e)))?
        } else {
            stmt.bind(&[JsValue::from_str(user_id)])
                .map_err(|e| CoreError::D1Error(format!("Failed to bind sessions: {}", e)))?
        };

        let result = bound.all().await
            .map_err(|e| CoreError::D1Error(format!("Failed to query sessions: {}", e)))?;

        let sessions = result.results::<UserSession>()
            .map_err(|e| CoreError::D1Error(format!("Failed to deserialize sessions: {}", e)))?;

        Ok(sessions)
    }

    pub async fn generate_referral_code(&self, user_id: &str) -> Result<String> {
        let user = self.find_user_by_id(user_id).await?;
        match user {
            Some(u) => Ok(u.referral_code),
            None => Err(CoreError::UserNotFound(format!("User not found: {}", user_id))),
        }
    }

    pub async fn apply_referral(&self, new_user_id: &str, referrer_code: &str) -> Result<()> {
        let stmt = self.db.prepare("SELECT user_id FROM users WHERE referral_code = ?1");
        let referrer = stmt.bind(&[JsValue::from_str(referrer_code)])
            .map_err(|e| CoreError::D1Error(format!("Failed to bind referral lookup: {}", e)))?
            .first::<serde_json::Value>(None)
            .await
            .map_err(|e| CoreError::D1Error(format!("Failed to lookup referrer: {}", e)))?;

        if let Some(referrer_data) = referrer {
            let referrer_id = referrer_data["user_id"].as_str().unwrap_or("").to_string();

            let referral = Referral::new(referrer_id.clone(), new_user_id.to_string());

            let insert = self.db.prepare(
                "INSERT INTO referrals (referral_id, referrer_id, referee_id, converted, reward_issued, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
            );

            insert.bind(&[
                JsValue::from_str(&referral.referral_id),
                JsValue::from_str(&referral.referrer_id),
                JsValue::from_str(&referral.referee_id),
                JsValue::from_f64(referral.converted as f64),
                JsValue::from_f64(referral.reward_issued as f64),
                JsValue::from_f64(referral.created_at as f64),
            ])
            .map_err(|e| CoreError::D1Error(format!("Failed to bind referral: {}", e)))?
            .run()
            .await
            .map_err(|e| CoreError::D1Error(format!("Failed to create referral: {}", e)))?;

            let update = self.db.prepare("UPDATE users SET referred_by = ?1 WHERE user_id = ?2");
            update.bind(&[
                JsValue::from_str(&referrer_id),
                JsValue::from_str(new_user_id),
            ])
            .map_err(|e| CoreError::D1Error(format!("Failed to bind referred_by update: {}", e)))?
            .run()
            .await
            .map_err(|e| CoreError::D1Error(format!("Failed to update referred_by: {}", e)))?;
        }

        Ok(())
    }

    pub async fn generate_discount_code(&self, user_id: &str, discount_percent: i32, valid_days: i64) -> Result<DiscountCode> {
        let code = DiscountCode::new(user_id.to_string(), discount_percent, valid_days);

        let stmt = self.db.prepare(
            "INSERT INTO discount_codes (code, user_id, discount_percent, created_at, expires_at, used)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
        );

        stmt.bind(&[
            JsValue::from_str(&code.code),
            JsValue::from_str(&code.user_id),
            JsValue::from_f64(code.discount_percent as f64),
            JsValue::from_f64(code.created_at as f64),
            JsValue::from_f64(code.expires_at as f64),
            JsValue::from_f64(code.used as f64),
        ])
        .map_err(|e| CoreError::D1Error(format!("Failed to bind discount code: {}", e)))?
        .run()
        .await
        .map_err(|e| CoreError::D1Error(format!("Failed to create discount code: {}", e)))?;

        Ok(code)
    }

    pub async fn use_discount_code(&self, code: &str) -> Result<Option<i32>> {
        let stmt = self.db.prepare("SELECT * FROM discount_codes WHERE code = ?1");
        let record = stmt.bind(&[JsValue::from_str(code)])
            .map_err(|e| CoreError::D1Error(format!("Failed to bind discount code lookup: {}", e)))?
            .first::<DiscountCode>(None)
            .await
            .map_err(|e| CoreError::D1Error(format!("Failed to lookup discount code: {}", e)))?;

        if let Some(dc) = record {
            if dc.is_valid() {
                let update = self.db.prepare("UPDATE discount_codes SET used = 1 WHERE code = ?1");
                update.bind(&[JsValue::from_str(code)])
                    .map_err(|e| CoreError::D1Error(format!("Failed to bind discount code use: {}", e)))?
                    .run()
                    .await
                    .map_err(|e| CoreError::D1Error(format!("Failed to mark discount code used: {}", e)))?;

                return Ok(Some(dc.discount_percent));
            }
        }

        Ok(None)
    }

    pub async fn create_ticket(&self, user_id: &str, subject: String, message: String) -> Result<SupportTicket> {
        let ticket = SupportTicket::new(user_id.to_string(), subject.clone(), message.clone());

        let stmt = self.db.prepare(
            "INSERT INTO support_tickets (ticket_id, user_id, subject, message, status, response, created_at, resolved_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
        );

        stmt.bind(&[
            JsValue::from_str(&ticket.ticket_id),
            JsValue::from_str(&ticket.user_id),
            JsValue::from_str(&ticket.subject),
            JsValue::from_str(&ticket.message),
            JsValue::from_str(&ticket.status),
            JsValue::NULL,
            JsValue::from_f64(ticket.created_at as f64),
            JsValue::NULL,
        ])
        .map_err(|e| CoreError::D1Error(format!("Failed to bind ticket creation: {}", e)))?
        .run()
        .await
        .map_err(|e| CoreError::D1Error(format!("Failed to create ticket: {}", e)))?;

        Ok(ticket)
    }

    pub async fn resolve_ticket(&self, ticket_id: &str, response: String) -> Result<()> {
        let stmt = self.db.prepare("UPDATE support_tickets SET response = ?1, status = 'resolved', resolved_at = ?2 WHERE ticket_id = ?3");
        let now = Utc::now().timestamp();
        stmt.bind(&[
            JsValue::from_str(&response),
            JsValue::from_f64(now as f64),
            JsValue::from_str(ticket_id),
        ])
        .map_err(|e| CoreError::D1Error(format!("Failed to bind ticket resolution: {}", e)))?
        .run()
        .await
        .map_err(|e| CoreError::D1Error(format!("Failed to resolve ticket: {}", e)))?;
        Ok(())
    }
}
