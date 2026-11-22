use tauri::State;
use crate::databases::{Database, repositories::gmail_history_repository::{GmailHistoryEmail, GmailHistoryRepository}};


#[tauri::command]
pub fn get_gmail_history(database: State<'_, Database>, account_id: Option<i64>) -> Result<Vec<GmailHistoryEmail>, String> {
    let connection = database.connection.lock().map_err(|error| format!("Database lock failed: {error}"))?;
    GmailHistoryRepository::find_all(&connection, account_id)
        .map_err(|error| format!("Failed to retrieve Gmail history: {error}"))
}


#[tauri::command]
pub fn get_gmail_message_body(
    database: State<'_, Database>,
    account_id: i64,
    message_id: String,
) -> Result<String, String> {
    if message_id.trim().is_empty() {
        return Err("Message ID wajib diisi.".into());
    }
    let connection = database.connection.lock().map_err(|error| format!("Database lock failed: {error}"))?;
    let payload_json: String = connection
        .query_row(
            "SELECT payload_json FROM gmail_messages WHERE gmail_account_id=?1 AND gmail_message_id=?2",
            (account_id, message_id.trim()),
            |row| row.get(0),
        )
        .map_err(|error| format!("Email tidak ditemukan atau gagal membaca pesan: {error}"))?;
    let payload: serde_json::Value = serde_json::from_str(&payload_json)
        .map_err(|error| format!("Payload email tersimpan tidak valid: {error}"))?;
    let body = crate::services::extraction::pipeline::decoded_email_body(&payload);
    if body.trim().is_empty() {
        return Err("Email ini tidak memiliki body pesan teks yang tersimpan.".into());
    }
    Ok(body)
}
