use crate::{
    databases::{Database, repositories::gmail_account_repository::GmailAccountRepository},
    gmail_sync::{SyncCoordinator, service::run_automatic_sync_for_account},
};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const POLLING_INTERVAL: Duration = Duration::from_secs(60);


/// Starts the app-lifetime Gmail monitoring loop. The first cycle runs immediately,
/// then subsequent cycles are spaced at the configured 60-second interval.
pub fn start(app: AppHandle, database: Database, coordinator: SyncCoordinator) {
    let _ = app.emit("flip://monitoring-started", serde_json::json!({
        "intervalSeconds": POLLING_INTERVAL.as_secs()
    }));
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(POLLING_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            run_cycle(&app, &database, &coordinator).await;
        }
    });
}


async fn run_cycle(app: &AppHandle, database: &Database, coordinator: &SyncCoordinator) {
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
        "intervalSeconds": POLLING_INTERVAL.as_secs()
    }));
}


fn emit_monitoring_error(app: &AppHandle, account_id: Option<i64>, error: String) {
    let _ = app.emit("flip://monitoring-error", serde_json::json!({
        "accountId": account_id,
        "error": error
    }));
}


