use tauri::State;
use crate::databases::{Database, repositories::gmail_history_repository::{GmailHistoryEmail, GmailHistoryRepository}};


#[tauri::command]
pub fn get_gmail_history(database: State<'_, Database>, account_id: Option<i64>) -> Result<Vec<GmailHistoryEmail>, String> {
    let connection = database.connection.lock().map_err(|error| format!("Database lock failed: {error}"))?;
    GmailHistoryRepository::find_all(&connection, account_id)
        .map_err(|error| format!("Failed to retrieve Gmail history: {error}"))
}
