use tauri::{AppHandle, State};
use rusqlite::OptionalExtension;

use crate::{
    databases::Database,
    services::extraction::pipeline::{
        correct_transfer_note, correct_transaction_fields, list_processing_logs, get_extraction_status_summary as read_extraction_status_summary, list_extraction_transactions,
        retry_transaction_extraction, run_pending_extraction, mark_transactions_copied as mark_copied_records, ExtractionStatusSummary,
        ExtractionSummary, ExtractionTransactionRecord, ProcessingLogRecord,
    },
    services::extraction::merge::merge_duplicate_group,
    services::ocr::TesseractOptions,
};



#[tauri::command]
pub fn get_transaction_invoice_data_url(
    database: State<'_, Database>,
    transaction_id: i64,
) -> Result<Option<String>, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use std::fs;

    let path: Option<String> = {
        let connection = database.connection.lock().map_err(|e| format!("Database lock gagal: {e}"))?;
        connection.query_row("SELECT png_path FROM transactions WHERE id=?1", [transaction_id], |row| row.get(0))
            .optional().map_err(|e| format!("Gagal mengambil path invoice: {e}"))?
    };
    let Some(path) = path.filter(|p| !p.trim().is_empty()) else { return Ok(None); };
    let bytes = fs::read(&path).map_err(|e| format!("Invoice tidak dapat dibaca ({path}): {e}"))?;
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") { "image/png" }
        else if bytes.starts_with(b"\xff\xd8\xff") { "image/jpeg" }
        else { return Err("Format invoice tidak dikenali sebagai PNG/JPEG".into()); };
    Ok(Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes))))
}

#[tauri::command]
pub async fn run_transaction_extraction(
    app: AppHandle,
    database: State<'_, Database>,
) -> Result<ExtractionSummary, String> {
    let database = database.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_pending_extraction(&database, &TesseractOptions::default(), Some(&app))
    })
    .await
    .map_err(|error| format!("Extraction worker gagal: {error}"))?
}


#[tauri::command]
pub async fn retry_transaction_extraction_command(
    app: AppHandle,
    database: State<'_, Database>,
    transaction_id: i64,
) -> Result<String, String> {
    let database = database.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        retry_transaction_extraction(&database, transaction_id, &TesseractOptions::default(), Some(&app))
    })
    .await
    .map_err(|error| format!("Extraction retry worker gagal: {error}"))?
}


#[tauri::command]
pub fn correct_transaction_transfer_note(
    database: State<'_, Database>,
    transaction_id: i64,
    transfer_note: String,
) -> Result<String, String> {
    correct_transfer_note(database.inner(), transaction_id, &transfer_note)
}



#[tauri::command]
pub fn save_transaction_correction(
    database: State<'_, Database>, transaction_id: i64, transaction_date: String,
    recipient_name: String, flip_transaction_id: String, amount: i64, transfer_note: String,
) -> Result<String, String> {
    correct_transaction_fields(database.inner(), transaction_id, &transaction_date, &recipient_name, &flip_transaction_id, amount, &transfer_note)
}


#[tauri::command]
pub fn get_processing_logs(database: State<'_, Database>, limit: Option<u32>) -> Result<Vec<ProcessingLogRecord>, String> {
    list_processing_logs(database.inner(), limit.unwrap_or(200))
}


#[tauri::command]
pub fn get_extraction_transactions(
    database: State<'_, Database>,
    limit: Option<u32>,
    offset: Option<u32>,
    status: Option<String>,
) -> Result<Vec<ExtractionTransactionRecord>, String> {
    list_extraction_transactions(
        database.inner(),
        limit.unwrap_or(100),
        offset.unwrap_or(0),
        status.as_deref(),
    )
}


#[tauri::command]
pub fn get_extraction_status_summary(
    database: State<'_, Database>,
) -> Result<ExtractionStatusSummary, String> {
    read_extraction_status_summary(database.inner())
}


#[tauri::command]
pub fn mark_transactions_copied(database: State<'_, Database>, transaction_ids: Vec<i64>) -> Result<usize, String> {
    mark_copied_records(database.inner(), &transaction_ids)
}


#[tauri::command]
pub fn merge_duplicate_transactions(database: State<'_, Database>, transaction_id: i64) -> Result<i64, String> {
    merge_duplicate_group(database.inner(), transaction_id)
}
