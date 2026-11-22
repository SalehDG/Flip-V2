use tauri::{AppHandle, Manager, State};
use crate::{databases::Database, services::settings::{self, MonitoringSettings, StorageInfo}};

#[tauri::command]
pub fn get_monitoring_settings(database: State<'_, Database>) -> Result<MonitoringSettings, String> { settings::get_monitoring_settings(database.inner()) }

#[tauri::command]
pub fn set_monitoring_settings(database: State<'_, Database>, enabled: bool, interval_seconds: u32) -> Result<MonitoringSettings, String> { settings::set_monitoring_settings(database.inner(), enabled, interval_seconds) }

#[tauri::command]
pub fn get_storage_info(app: AppHandle, database: State<'_, Database>) -> Result<StorageInfo, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| format!("Gagal membaca direktori data: {e}"))?;
    settings::get_storage_info(database.inner(), &data_dir)
}
