use tauri::{AppHandle, Manager, State};
use crate::{databases::Database, services::settings::{self, MonitoringSettings, StorageInfo}};

#[tauri::command]
pub fn get_monitoring_settings(database: State<'_, Database>) -> Result<MonitoringSettings, String> { settings::get_monitoring_settings(database.inner()) }

#[tauri::command]
pub fn set_monitoring_settings(database: State<'_, Database>, enabled: bool, interval_seconds: u32) -> Result<MonitoringSettings, String> { settings::set_monitoring_settings(database.inner(), enabled, interval_seconds) }

#[tauri::command]
pub fn get_storage_info(app: AppHandle, database: State<'_, Database>) -> Result<StorageInfo, String> {
    let data_dir = crate::storage_paths::app_data_dir(&app)?;
    settings::get_storage_info(database.inner(), &data_dir)
}

#[tauri::command]
pub fn open_storage_directory(app: AppHandle) -> Result<(), String> {
    let path = crate::storage_paths::app_data_dir(&app)?;
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| format!("Gagal membuka folder penyimpanan: {e}"))
}
