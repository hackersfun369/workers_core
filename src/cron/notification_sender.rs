//! # Notification Sender Cron
//!
//! Runs every 5 minutes. Processes the notification queue and sends
//! pending notifications via email and WhatsApp.

use worker::D1Database;
use chrono::Utc;

use crate::notifications::NotificationManager;
use crate::{CoreError, Result};

pub struct NotificationSenderCron {
    pub notification_manager: NotificationManager,
}

impl NotificationSenderCron {
    pub fn new(notification_manager: NotificationManager) -> Self {
        Self { notification_manager }
    }

    /// Process the notification queue.
    pub async fn run(&self) -> Result<SenderReport> {
        let report = self.notification_manager.process_queue().await
            .map_err(|e| CoreError::Internal(format!("Notification processing failed: {}", e)))?;

        let sender_report = SenderReport {
            processed_at: Utc::now().timestamp(),
            sent: report.sent,
            failed: report.failed,
        };

        if sender_report.sent > 0 || sender_report.failed > 0 {
            tracing::info!(
                "Notification sender cron: {} sent, {} failed",
                sender_report.sent, sender_report.failed
            );
        }

        Ok(sender_report)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SenderReport {
    pub processed_at: i64,
    pub sent: u32,
    pub failed: u32,
}
