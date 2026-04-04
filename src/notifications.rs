//! # NotificationManager — Email and WhatsApp Notifications

use worker::*;
use wasm_bindgen::JsValue;

use crate::models::{Notification, NotificationType, NotificationChannel};
use crate::api_clients::{MailgunClient, WhatsAppClient};
use crate::{CoreError, Result};

pub struct NotificationManager {
    pub db: D1Database,
    pub mailgun: Option<MailgunClient>,
    pub whatsapp: Option<WhatsAppClient>,
}

impl NotificationManager {
    pub fn new(db: D1Database) -> Self {
        Self {
            db,
            mailgun: None,
            whatsapp: None,
        }
    }

    pub fn with_mailgun(mut self, api_key: String, domain: String) -> Self {
        self.mailgun = Some(MailgunClient::new(api_key, domain));
        self
    }

    pub fn with_whatsapp(mut self, token: String) -> Self {
        self.whatsapp = Some(WhatsAppClient::new(token));
        self
    }

    pub async fn queue(
        &self,
        user_id: &str,
        notification_type: NotificationType,
        channel: NotificationChannel,
        content: String,
    ) -> Result<String> {
        let notification = Notification::new(user_id.to_string(), notification_type, channel, content);

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
        .map_err(|e| CoreError::D1Error(format!("Failed to bind notification queue: {}", e)))?
        .run()
        .await
        .map_err(|e| CoreError::D1Error(format!("Failed to queue notification: {}", e)))?;

        Ok(notification.notification_id)
    }

    pub async fn queue_both(
        &self,
        user_id: &str,
        notification_type: NotificationType,
        content: String,
    ) -> Result<(String, String)> {
        let email_id = self.queue(user_id, notification_type.clone(), NotificationChannel::Email, content.clone()).await?;
        let whatsapp_id = self.queue(user_id, notification_type, NotificationChannel::WhatsApp, content).await?;
        Ok((email_id, whatsapp_id))
    }

    pub async fn process_queue(&self) -> Result<QueueResult> {
        let mut sent = 0u32;
        let mut failed = 0u32;

        let stmt = self.db.prepare("SELECT * FROM notifications WHERE sent = 0 ORDER BY created_at ASC LIMIT 100");
        let result = stmt.all().await
            .map_err(|e| CoreError::D1Error(format!("Failed to query notification queue: {}", e)))?;

        let notifications: Vec<Notification> = result.results::<Notification>()
            .map_err(|e| CoreError::D1Error(format!("Failed to deserialize notifications: {}", e)))?;

        for notification in notifications {
            match self.send_notification(&notification).await {
                Ok(_) => {
                    let now = chrono::Utc::now().timestamp();
                    let update = self.db.prepare(
                        "UPDATE notifications SET sent = 1, sent_at = ?1 WHERE notification_id = ?2"
                    );
                    update.bind(&[
                        JsValue::from_f64(now as f64),
                        JsValue::from_str(&notification.notification_id),
                    ])
                    .map_err(|e| CoreError::D1Error(format!("Failed to bind sent update: {}", e)))?
                    .run()
                    .await
                    .map_err(|e| CoreError::D1Error(format!("Failed to mark notification sent: {}", e)))?;

                    sent += 1;
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to send notification {}: {}",
                        notification.notification_id, e
                    );
                    failed += 1;
                }
            }
        }

        Ok(QueueResult { sent, failed })
    }

    async fn send_notification(&self, notification: &Notification) -> Result<()> {
        let user = self.get_user_contact(&notification.user_id).await?;
        if user.is_none() {
            return Err(CoreError::UserNotFound(format!(
                "User not found for notification: {}", notification.user_id
            )));
        }
        let user = user.unwrap();

        match &notification.channel {
            NotificationChannel::Email => {
                if user.email.is_empty() {
                    return Err(CoreError::NotificationError(
                        "No email address for user".to_string()
                    ));
                }

                if let Some(ref mailgun) = self.mailgun {
                    mailgun.send_email(
                        &user.email,
                        &format!("{:?}", notification.r#type),
                        &self.format_email_body(&notification.content),
                        Some("Autonomous Software Factory"),
                    ).await?;
                }
            }
            NotificationChannel::WhatsApp => {
                if user.phone.is_empty() {
                    return Err(CoreError::NotificationError(
                        "No phone number for user".to_string()
                    ));
                }

                if let Some(ref whatsapp) = self.whatsapp {
                    whatsapp.send_message(&user.phone, &notification.content).await?;
                }
            }
        }

        tracing::info!(
            "Notification {:?} sent to {} via {:?}",
            notification.r#type, notification.user_id, notification.channel
        );

        Ok(())
    }

    async fn get_user_contact(&self, user_id: &str) -> Result<Option<UserContact>> {
        let stmt = self.db.prepare("SELECT email, phone FROM users WHERE user_id = ?1");
        let result = stmt.bind(&[JsValue::from_str(user_id)])
            .map_err(|e| CoreError::D1Error(format!("Failed to bind user contact query: {}", e)))?
            .first::<UserContact>(None)
            .await
            .map_err(|e| CoreError::D1Error(format!("Failed to query user contact: {}", e)))?;
        Ok(result)
    }

    fn format_email_body(&self, content: &str) -> String {
        format!(
            r#"<!DOCTYPE html>
<html>
<head><meta charset="UTF-8"><title>Autonomous Software Factory</title></head>
<body style="font-family: Arial, sans-serif; max-width: 600px; margin: 0 auto; padding: 20px;">
    <div style="background: #1a1a2e; color: #eee; padding: 20px; border-radius: 8px;">
        <h2 style="margin: 0 0 16px;">Autonomous Software Factory</h2>
        <p style="font-size: 16px; line-height: 1.6;">{}</p>
    </div>
    <p style="color: #888; font-size: 12px; margin-top: 16px;">
        This is an automated message from Autonomous Software Factory.
    </p>
</body>
</html>"#,
            content
        )
    }
}

#[derive(Debug, Clone)]
pub struct QueueResult {
    pub sent: u32,
    pub failed: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserContact {
    pub email: String,
    pub phone: String,
}
