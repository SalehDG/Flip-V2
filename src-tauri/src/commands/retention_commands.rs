use tauri::State;
use crate::{databases::Database, services::retention::{self, RetentionSettings, RetentionPreview, RetentionRunSummary}};

#[tauri::command]
pub fn get_retention_settings(database: State<'_, Database>) -> Result<RetentionSettings, String> { retention::get_settings(database.inner()) }
#[tauri::command]
pub fn set_retention_settings(database: State<'_, Database>, retention_days: u32) -> Result<RetentionSettings, String> { retention::set_settings(database.inner(), retention_days) }
#[tauri::command]
pub fn preview_retention_cleanup(database: State<'_, Database>) -> Result<RetentionPreview, String> { retention::preview(database.inner()) }
#[tauri::command]
pub async fn run_retention_cleanup(database: State<'_, Database>) -> Result<RetentionRunSummary, String> {
    let database = database.inner().clone();
    tauri::async_runtime::spawn_blocking(move || retention::run_cleanup(&database)).await.map_err(|e| format!("Retention worker gagal: {e}"))?
}
