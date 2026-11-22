use crate::databases::Database;
use serde::Serialize;
use std::{fs, path::{Path, PathBuf}};

const DEFAULT_INTERVAL: u32 = 60;
const MIN_INTERVAL: u32 = 30;
const MAX_INTERVAL: u32 = 3600;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitoringSettings { pub enabled: bool, pub interval_seconds: u32 }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo { pub data_directory: String, pub database_bytes: u64, pub png_bytes: u64, pub png_files: u64, pub total_bytes: u64 }

fn db_error(e: rusqlite::Error) -> String { format!("Database error: {e}") }
fn lock_error(e: impl std::fmt::Display) -> String { format!("Database lock error: {e}") }

fn read_setting(db: &Database, key: &str, default: &str) -> Result<String, String> {
    let conn = db.connection.lock().map_err(lock_error)?;
    conn.query_row("SELECT setting_value FROM app_settings WHERE setting_key=?1", [key], |r| r.get(0))
        .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(default.to_owned()) } else { Err(e) })
        .map_err(db_error)
}

fn write_setting(db: &Database, key: &str, value: &str) -> Result<(), String> {
    let conn = db.connection.lock().map_err(lock_error)?;
    conn.execute("INSERT INTO app_settings(setting_key,setting_value,updated_at) VALUES(?1,?2,datetime('now')) ON CONFLICT(setting_key) DO UPDATE SET setting_value=excluded.setting_value,updated_at=datetime('now')", [key, value]).map_err(db_error)?;
    Ok(())
}


pub fn get_monitoring_settings(db: &Database) -> Result<MonitoringSettings, String> {
    let enabled = match read_setting(db, "monitoring_enabled", "1")?.as_str() { "0" | "false" => false, _ => true };
    let interval = read_setting(db, "monitoring_interval_seconds", &DEFAULT_INTERVAL.to_string())?.parse::<u32>().unwrap_or(DEFAULT_INTERVAL).clamp(MIN_INTERVAL, MAX_INTERVAL);
    Ok(MonitoringSettings { enabled, interval_seconds: interval })
}


pub fn set_monitoring_settings(db: &Database, enabled: bool, interval_seconds: u32) -> Result<MonitoringSettings, String> {
    if !(MIN_INTERVAL..=MAX_INTERVAL).contains(&interval_seconds) {
        return Err(format!("Interval monitoring harus antara {MIN_INTERVAL} dan {MAX_INTERVAL} detik."));
    }
    write_setting(db, "monitoring_enabled", if enabled { "1" } else { "0" })?;
    write_setting(db, "monitoring_interval_seconds", &interval_seconds.to_string())?;
    Ok(MonitoringSettings { enabled, interval_seconds })
}


pub fn get_storage_info(db: &Database, app_data_dir: &Path) -> Result<StorageInfo, String> {
    let database_bytes = fs::metadata(app_data_dir.join("flip-v2.sqlite3")).map(|m| m.len()).unwrap_or(0);
    let png_paths: Vec<PathBuf> = {
        let conn = db.connection.lock().map_err(lock_error)?;
        let mut stmt = conn.prepare("SELECT DISTINCT png_path FROM transactions WHERE png_path IS NOT NULL AND png_path <> ''").map_err(db_error)?;
        stmt.query_map([], |row| row.get::<_, String>(0).map(PathBuf::from)).map_err(db_error)?.collect::<Result<Vec<_>, _>>().map_err(db_error)?
    };
    let mut png_bytes = 0u64;
    let mut png_files = 0u64;
    for path in png_paths {
        if let Ok(metadata) = fs::metadata(&path) {
            if metadata.is_file() && Path::new(&path).extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("png")).unwrap_or(false) {
                png_bytes = png_bytes.saturating_add(metadata.len());
                png_files = png_files.saturating_add(1);
            }
        }
    }
    Ok(StorageInfo { data_directory: app_data_dir.to_string_lossy().into_owned(), database_bytes, png_bytes, png_files, total_bytes: database_bytes.saturating_add(png_bytes) })
}
