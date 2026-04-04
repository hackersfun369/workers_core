//! # GooglePayWebhook — Payment Confirmation Listener

use rsa::pkcs8::DecodePublicKey;
use rsa::pkcs1v15::VerifyingKey;
use rsa::signature::Verifier;
use sha2::Sha256;
use serde::{Deserialize, Serialize};
use worker::*;
use wasm_bindgen::JsValue;

use crate::models::{Payment, PaymentStatus, User, UserPlan, Notification, NotificationType, NotificationChannel};
use crate::{CoreError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GPayWebhookRequest {
    pub transaction_id: String,
    pub amount: String,
    pub currency: String,
    pub timestamp: i64,
    pub signature: String,
    pub merchant_id: String,
    pub user_data: GPayUserData,
    pub raw_payload: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GPayUserData {
    pub user_id: Option<String>,
    pub plan_id: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub name: Option<String>,
    pub worker_id: Option<String>,
    pub referral_code: Option<String>,
}

pub fn verify_signature(
    public_key_pem: &str,
    payload: &str,
    signature_base64: &str,
) -> Result<()> {
    let signature_bytes = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        signature_base64,
    )
    .map_err(|e| CoreError::SignatureError(format!("Failed to decode signature: {}", e)))?;

    let verifying_key = VerifyingKey::<Sha256>::from_public_key_pem(public_key_pem)
        .map_err(|e| CoreError::SignatureError(format!("Failed to parse public key: {}", e)))?;

    // Parse the DER-encoded signature
    let signature = rsa::pkcs1v15::Signature::try_from(signature_bytes.as_slice())
        .map_err(|e| CoreError::SignatureError(format!("Failed to parse signature: {}", e)))?;

    verifying_key.verify(payload.as_bytes(), &signature)
        .map_err(|e| CoreError::SignatureError(format!("Signature verification failed: {}", e)))?;

    Ok(())
}

pub async fn verify_webhook_signature(
    request: &GPayWebhookRequest,
    public_key_pem: &str,
) -> Result<()> {
    let signed_data = format!(
        "{}|{}|{}|{}",
        request.merchant_id,
        request.transaction_id,
        request.amount,
        request.timestamp
    );

    verify_signature(public_key_pem, &signed_data, &request.signature)
}

pub struct GooglePayWebhook {
    pub db: D1Database,
    pub kv: Option<KvStore>,
    pub mailgun_api_key: Option<String>,
    pub mailgun_domain: Option<String>,
    pub whatsapp_token: Option<String>,
    pub public_key_pem: String,
    pub merchant_id: String,
}

impl GooglePayWebhook {
    pub async fn process(&self, request: GPayWebhookRequest) -> Result<WebhookResult> {
        verify_webhook_signature(&request, &self.public_key_pem).await?;

        if request.merchant_id != self.merchant_id {
            return Err(CoreError::PaymentError(format!(
                "Merchant ID mismatch: expected {}, got {}",
                self.merchant_id, request.merchant_id
            )));
        }

        if let Some(ref kv) = self.kv {
            let dedup_key = format!("payment:processed:{}", request.transaction_id);
            if kv.get(&dedup_key).text().await.ok().flatten().is_some() {
                tracing::info!(
                    "Duplicate webhook detected for transaction: {}",
                    request.transaction_id
                );
                return Ok(WebhookResult::Duplicate);
            }

            kv.put(&dedup_key, "1")?
                .expiration_ttl(86400)
                .execute()
                .await
                .ok();
        }

        let amount_paise: i64 = request.amount.parse()
            .map_err(|e| CoreError::PaymentError(format!("Invalid amount: {}", e)))?;

        let user_id = request.user_data.user_id.clone()
            .ok_or_else(|| CoreError::PaymentError("No user_id in webhook data".to_string()))?;

        let existing_user = self.find_user(&user_id).await?;

        if existing_user.is_none() {
            let name = request.user_data.name.clone().unwrap_or_else(|| "User".to_string());
            let email = request.user_data.email.clone().unwrap_or_default();
            let phone = request.user_data.phone.clone().unwrap_or_default();

            if email.is_empty() && phone.is_empty() {
                return Err(CoreError::PaymentError(
                    "Either email or phone is required for account creation".to_string(),
                ));
            }

            let user = User::new(name, email, phone);
            self.create_user(&user).await?;

            tracing::info!("New user account created: {} ({})", user.name, user.user_id);

            self.queue_notification(&user.user_id, NotificationType::AccountCreated,
                format!("Welcome to Autonomous Software Factory, {}! Visit your portal to get started.", user.name)).await?;
        }

        let plan_id = request.user_data.plan_id.clone()
            .ok_or_else(|| CoreError::PaymentError("No plan_id in webhook data".to_string()))?;

        let payment = Payment::new(user_id.clone(), plan_id.clone(), amount_paise)
            .success(request.transaction_id.clone());

        self.record_payment(&payment).await?;

        self.activate_plan(&user_id, &plan_id, amount_paise, &request).await?;

        self.queue_notification(&user_id, NotificationType::PaymentSuccess,
            format!("Payment of Rs.{:.2} successful. Your plan is now active!", amount_paise as f64 / 100.0)).await?;

        tracing::info!(
            "Payment processed: {} for user {}, plan {}",
            request.transaction_id, user_id, plan_id
        );

        Ok(WebhookResult::Success {
            user_id,
            plan_id,
            amount: amount_paise,
            transaction_id: request.transaction_id,
        })
    }

    async fn find_user(&self, user_id: &str) -> Result<Option<User>> {
        let stmt = self.db.prepare("SELECT * FROM users WHERE user_id = ?1");
        let result = stmt.bind(&[JsValue::from_str(user_id)])
            .map_err(|e| CoreError::D1Error(format!("Failed to bind user query: {}", e)))?
            .first::<User>(None)
            .await
            .map_err(|e| CoreError::D1Error(format!("Failed to query user: {}", e)))?;

        Ok(result)
    }

    async fn create_user(&self, user: &User) -> Result<()> {
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

        Ok(())
    }

    async fn record_payment(&self, payment: &Payment) -> Result<()> {
        let stmt = self.db.prepare(
            "INSERT INTO payments (payment_id, user_id, plan_id, amount, currency, gpay_reference, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
        );

        stmt.bind(&[
            JsValue::from_str(&payment.payment_id),
            JsValue::from_str(&payment.user_id),
            JsValue::from_str(&payment.plan_id),
            JsValue::from_f64(payment.amount as f64),
            JsValue::from_str(&payment.currency),
            JsValue::from_str(payment.gpay_reference.as_deref().unwrap_or("")),
            JsValue::from_str(&format!("{:?}", payment.status)),
            JsValue::from_f64(payment.created_at as f64),
        ])
        .map_err(|e| CoreError::D1Error(format!("Failed to bind payment: {}", e)))?
        .run()
        .await
        .map_err(|e| CoreError::D1Error(format!("Failed to record payment: {}", e)))?;

        Ok(())
    }

    async fn activate_plan(
        &self,
        user_id: &str,
        plan_id: &str,
        amount_paise: i64,
        request: &GPayWebhookRequest,
    ) -> Result<()> {
        let worker_id = request.user_data.worker_id.clone().unwrap_or("worker-6".to_string());
        let (plan_type, credits) = Self::amount_to_plan(amount_paise);

        let plan = UserPlan::new(
            user_id.to_string(),
            worker_id,
            plan_type.clone(),
            credits,
            Some(30),
        );

        let stmt = self.db.prepare(
            "INSERT INTO user_plans (plan_id, user_id, worker_id, plan_type, total_credits, used_credits,
             remaining_credits, plan_start, plan_expiry, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"
        );

        let plan_expiry_json = plan.plan_expiry
            .map(|v| JsValue::from_f64(v as f64))
            .unwrap_or(JsValue::NULL);

        stmt.bind(&[
            JsValue::from_str(&plan.plan_id),
            JsValue::from_str(&plan.user_id),
            JsValue::from_str(&plan.worker_id),
            JsValue::from_str(&plan.plan_type),
            JsValue::from_f64(plan.total_credits as f64),
            JsValue::from_f64(plan.used_credits as f64),
            JsValue::from_f64(plan.remaining_credits as f64),
            JsValue::from_f64(plan.plan_start as f64),
            plan_expiry_json,
            JsValue::from_str(&plan.status),
            JsValue::from_f64(plan.created_at as f64),
        ])
        .map_err(|e| CoreError::D1Error(format!("Failed to bind plan activation: {}", e)))?
        .run()
        .await
        .map_err(|e| CoreError::D1Error(format!("Failed to activate plan: {}", e)))?;

        tracing::info!(
            "Plan activated: {} for user {} ({} credits, {} days)",
            plan_type, plan.user_id, credits, 30
        );

        Ok(())
    }

    fn amount_to_plan(amount_paise: i64) -> (String, i64) {
        let rupees = amount_paise / 100;

        match rupees {
            149..=299 => ("pay_per_doc".to_string(), 1),
            499 => ("single_analysis".to_string(), 1),
            999 => ("monthly_unlimited".to_string(), 99999),
            1299 => ("3_session_pack".to_string(), 3),
            2399 => ("6_session_pack".to_string(), 6),
            3999 => ("semester_pack".to_string(), 50),
            _ => ("custom".to_string(), 1),
        }
    }

    async fn queue_notification(
        &self,
        user_id: &str,
        notification_type: NotificationType,
        content: String,
    ) -> Result<()> {
        for channel in [NotificationChannel::Email, NotificationChannel::WhatsApp] {
            let notification = Notification::new(
                user_id.to_string(),
                notification_type.clone(),
                channel,
                content.clone(),
            );

            let stmt = self.db.prepare(
                "INSERT INTO notifications (notification_id, user_id, type, channel, content, sent, sent_at, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
            );

            stmt.bind(&[
                JsValue::from_str(&notification.notification_id),
                JsValue::from_str(&notification.user_id),
                JsValue::from_str(&format!("{:?}", notification.r#type)),
                JsValue::from_str(&format!("{:?}", notification.channel)),
                JsValue::from_str(&notification.content),
                JsValue::from_f64(notification.sent as f64),
                JsValue::NULL,
                JsValue::from_f64(notification.created_at as f64),
            ])
            .map_err(|e| CoreError::D1Error(format!("Failed to bind notification: {}", e)))?
            .run()
            .await
            .map_err(|e| CoreError::D1Error(format!("Failed to queue notification: {}", e)))?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum WebhookResult {
    Success {
        user_id: String,
        plan_id: String,
        amount: i64,
        transaction_id: String,
    },
    Duplicate,
}
