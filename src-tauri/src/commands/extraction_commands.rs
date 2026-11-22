use tauri::{AppHandle, State};

use crate::{
    databases::Database,
    services::extraction::pipeline::{
        correct_transfer_note, get_extraction_status_summary as read_extraction_status_summary, list_extraction_transactions,
        retry_transaction_extraction, run_pending_extraction, mark_transactions_copied as mark_copied_records, ExtractionStatusSummary,
        ExtractionSummary, ExtractionTransactionRecord,
    },
    services::ocr::TesseractOptions,
};


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
