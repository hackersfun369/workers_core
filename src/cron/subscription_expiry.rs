//! # Subscription Expiry Cron
//!
//! Runs every hour. Checks for expired plans and marks them as expired.
//! Sends notifications before expiry and on expiry day.
//!
//! ## Process
//! 1. Check user_plans for plan_expiry < now()
//! 2. Set status = 'expired'
//! 3. Send notification via email and WhatsApp
//! 4. Check for plans expiring in 3 days and send reminder

use worker::D1Database;
use chrono::Utc;
use wasm_bindgen::JsValue;

use crate::notifications::NotificationManager;
use crate::models::{NotificationType};
use crate::users::UserManager;
use crate::{CoreError, Result};

pub struct SubscriptionExpiryCron {
    pub db: D1Database,
    pub notification_manager: NotificationManager,
    pub user_manager: UserManager,
}

impl SubscriptionExpiryCron {
    pub fn new(
        db: D1Database,
        notification_manager: NotificationManager,
        user_manager: UserManager,
    ) -> Self {
        Self {
            db,
            notification_manager,
            user_manager,
        }
    }

    /// Run the expiry check.
    pub async fn run(&self) -> Result<ExpiryReport> {
        let mut report = ExpiryReport {
            checked_at: Utc::now().timestamp(),
            plans_expired: 0,
            reminders_sent: 0,
            errors: Vec::new(),
        };

        // Step 1: Expire overdue plans
        let now = Utc::now().timestamp();

        let expire_stmt = self.db.prepare(
            "UPDATE user_plans SET status = 'expired'
             WHERE plan_expiry IS NOT NULL AND plan_expiry < ?1 AND status = 'active'"
        );

        let result = expire_stmt.bind(&[JsValue::from_f64(now as f64)])
            .map_err(|e| CoreError::D1Error(format!("Failed to bind expiry check: {}", e)))?
            .run()
            .await
            .map_err(|e| CoreError::D1Error(format!("Failed to expire plans: {}", e)))?;

        report.plans_expired = result.meta().ok().flatten().and_then(|m| m.changes).unwrap_or(0) as u32;

        if report.plans_expired > 0 {
            tracing::info!("Expired {} plans", report.plans_expired);

            // Send expiry notifications to affected users
            let affected_users = self.db.prepare(
                "SELECT DISTINCT user_id FROM user_plans WHERE status = 'expired'
                 AND plan_expiry < ?1"
            );

            if let Ok(users_result) = affected_users
                .bind(&[JsValue::from_f64(now as f64)])
                .map_err(|e| CoreError::D1Error(format!("Failed to bind affected users: {}", e)))?
                .all()
                .await
            {
                #[derive(serde::Deserialize)]
                struct UserId {
                    user_id: String,
                }

                if let Ok(users) = users_result.results::<UserId>() {
                    for user in users {
                        if let Err(e) = self.notification_manager
                            .queue_both(
                                &user.user_id,
                                NotificationType::PlanExpired,
                                "Your plan has expired. Please renew from the billing page.".to_string(),
                            )
                            .await
                        {
                            report.errors.push(format!("Failed to notify user {}: {}", user.user_id, e));
                        }
                    }
                }
            }
        }

        // Step 2: Send 3-day reminders for plans expiring soon
        let three_days_later = now + (3 * 86400);

        let reminder_stmt = self.db.prepare(
            "SELECT user_id FROM user_plans
             WHERE plan_expiry IS NOT NULL
             AND plan_expiry > ?1
             AND plan_expiry <= ?2
             AND status = 'active'"
        );

        if let Ok(reminder_result) = reminder_stmt
            .bind(&[
                JsValue::from_f64(now as f64),
                JsValue::from_f64(three_days_later as f64),
            ])
            .map_err(|e| CoreError::D1Error(format!("Failed to bind reminder query: {}", e)))?
            .all()
            .await
        {
            #[derive(serde::Deserialize)]
            struct ReminderUser {
                user_id: String,
            }

            if let Ok(users) = reminder_result.results::<ReminderUser>() {
                for user in users {
                    if let Err(e) = self.notification_manager
                        .queue_both(
                            &user.user_id,
                            NotificationType::PlanExpiringSoon,
                            "Your plan expires in 3 days. Renew now to avoid interruption.".to_string(),
                        )
                        .await
                    {
                        report.errors.push(format!("Failed to send reminder to {}: {}", user.user_id, e));
                    }
                    report.reminders_sent += 1;
                }
            }
        }

        if report.reminders_sent > 0 {
            tracing::info!("Sent {} plan expiry reminders", report.reminders_sent);
        }

        Ok(report)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExpiryReport {
    pub checked_at: i64,
    pub plans_expired: u32,
    pub reminders_sent: u32,
    pub errors: Vec<String>,
}
