use crate::{
    databases::{Database, repositories::gmail_account_repository::GmailAccountRepository},
    gmail_sync::{SyncCoordinator, service::run_automatic_sync_for_account},
};
use std::time::Duration;
use tauri::{AppHandle, Emitter};


/// Starts the app-lifetime Gmail monitoring loop. The first cycle runs immediately,
/// then subsequent cycles use the persisted monitoring interval.
pub fn start(app: AppHandle, database: Database, coordinator: SyncCoordinator) {
    tauri::async_runtime::spawn(async move {
        let mut last_cycle: Option<tokio::time::Instant> = None;
        loop {
            match crate::services::settings::get_monitoring_settings(&database) {
                Ok(config) if config.enabled => {
                    if last_cycle.map(|last| last.elapsed() >= Duration::from_secs(config.interval_seconds as u64)).unwrap_or(true) {
                        let _ = app.emit("flip://monitoring-started", serde_json::json!({ "intervalSeconds": config.interval_seconds }));
                        run_cycle(&app, &database, &coordinator, config.interval_seconds).await;
                        last_cycle = Some(tokio::time::Instant::now());
                    }
                }
                Ok(config) => {
                    let _ = app.emit("flip://monitoring-paused", serde_json::json!({ "intervalSeconds": config.interval_seconds }));
                }
                Err(error) => emit_monitoring_error(&app, None, error),
            }
            // A short wake interval makes changes to Settings responsive while
            // preventing the sync worker from spinning or running duplicate cycles.
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}


async fn run_cycle(app: &AppHandle, database: &Database, coordinator: &SyncCoordinator, interval_seconds: u32) {
    let accounts = match database.connection.lock() {
        Ok(connection) => match GmailAccountRepository::find_all(&connection) {
            Ok(accounts) => accounts,
            Err(error) => {
                emit_monitoring_error(app, None, format!("Failed to read accounts: {error}"));
                return;
            },
        },
        Err(error) => {
            emit_monitoring_error(app, None, format!("Database lock failed: {error}"));
            return;
        },
    };

    let checked_accounts = accounts.iter().filter(|account| account.account_status == "connected").count();
    for account in accounts.into_iter().filter(|account| account.account_status == "connected") {
        let _ = app.emit("flip://monitoring-account", serde_json::json!({
            "accountId": account.id,
            "email": account.email_address,
            "status": "checking"
        }));
        match run_automatic_sync_for_account(app, database, coordinator, account.id).await {
            Ok(Some(summary)) => {
                let _ = app.emit("flip://monitoring-account", serde_json::json!({
                    "accountId": account.id,
                    "email": account.email_address,
                    "status": "completed",
                    "processedMessages": summary.processed_messages,
                    "skippedMessages": summary.skipped_messages,
                    "attachmentCount": summary.attachment_count
                }));
            }
            Ok(None) => {
                let _ = app.emit("flip://monitoring-account", serde_json::json!({
                    "accountId": account.id,
                    "email": account.email_address,
                    "status": "skipped"
                }));
            }
            Err(error) => emit_monitoring_error(app, Some(account.id), error),
        }
    }
    let _ = app.emit("flip://monitoring-cycle-complete", serde_json::json!({
        "checkedAccounts": checked_accounts,
        "intervalSeconds": interval_seconds
    }));
}


fn emit_monitoring_error(app: &AppHandle, account_id: Option<i64>, error: String) {
    let _ = app.emit("flip://monitoring-error", serde_json::json!({
        "accountId": account_id,
        "error": error
    }));
}


