use tauri::State;

use crate::databases::{
    repositories::gmail_account_repository::{
        GmailAccount,
        GmailAccountRepository,
    },
    Database,
};

#[tauri::command]
pub fn get_gmail_accounts(
    database: State<'_, Database>,
) -> Result<Vec<GmailAccount>, String> {
    let connection = database
        .connection
        .lock()
        .map_err(|error| format!("Database lock failed: {error}"))?;

    GmailAccountRepository::find_all(&connection)
        .map_err(|error| format!("Failed to retrieve Gmail accounts: {error}"))
}

#[tauri::command]
pub fn update_sync_start_date(
    database: State<'_, Database>,
    account_id: i64,
    start_date: String,
) -> Result<bool, String> {
    let connection = database
        .connection
        .lock()
        .map_err(|error| format!("Database lock failed: {error}"))?;

    GmailAccountRepository::update_sync_start_date(
        &connection,
        account_id,
        &start_date,
    )
    .map_err(|error| format!("Failed to update sync start date: {error}"))
}