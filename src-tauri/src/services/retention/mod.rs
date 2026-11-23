use serde::Serialize;
use std::{collections::HashSet, fs};
use crate::databases::Database;

const DEFAULT_DAYS: u32 = 7;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionSettings { pub retention_days: u32 }
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionPreview { pub retention_days: u32, pub eligible_count: usize, pub oldest_expired_at: Option<String>, pub png_file_count: usize }
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionRunSummary { pub deleted_transactions: usize, pub deleted_png_files: usize, pub failed_png_deletions: usize }

fn db_error(e: rusqlite::Error) -> String { format!("Database error: {e}") }
fn lock_error(e: impl std::fmt::Display) -> String { format!("Database lock error: {e}") }

pub fn get_settings(db: &Database) -> Result<RetentionSettings, String> {
    let conn = db.connection.lock().map_err(lock_error)?;
    let days: u32 = conn.query_row("SELECT COALESCE((SELECT CAST(setting_value AS INTEGER) FROM app_settings WHERE setting_key='retention_period'), ?1)", [DEFAULT_DAYS], |r| r.get(0)).map_err(db_error)?;
    Ok(RetentionSettings { retention_days: days.clamp(1, 365) })
}


pub fn set_settings(db: &Database, days: u32) -> Result<RetentionSettings, String> {
    if !(1..=365).contains(&days) { return Err("Masa retensi harus 1 sampai 365 hari.".into()); }
    let conn = db.connection.lock().map_err(lock_error)?;
    conn.execute("INSERT INTO app_settings(setting_key,setting_value,updated_at) VALUES('retention_period',?1,datetime('now')) ON CONFLICT(setting_key) DO UPDATE SET setting_value=excluded.setting_value,updated_at=datetime('now')", [days.to_string()]).map_err(db_error)?;
    conn.execute("UPDATE transactions SET retention_until=datetime(first_copied_at, '+' || ?1 || ' days'),updated_at=datetime('now') WHERE first_copied_at IS NOT NULL AND status IN ('Completed','Needs Review')", [days]).map_err(db_error)?;
    Ok(RetentionSettings { retention_days: days })
}

pub fn preview(db: &Database) -> Result<RetentionPreview, String> {
    let conn = db.connection.lock().map_err(lock_error)?;
    let days: u32 = conn.query_row("SELECT COALESCE((SELECT CAST(setting_value AS INTEGER) FROM app_settings WHERE setting_key='retention_period'), ?1)", [DEFAULT_DAYS], |r| r.get(0)).map_err(db_error)?;
    let eligible_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE first_copied_at IS NOT NULL AND status IN ('Completed','Needs Review') AND COALESCE(retention_until,datetime(first_copied_at, '+' || ?1 || ' days')) <= datetime('now')",
        [days], |r| r.get::<_, i64>(0),
    ).map_err(db_error)?;
    let oldest_expired_at: Option<String> = conn.query_row(
        "SELECT MIN(COALESCE(retention_until,datetime(first_copied_at, '+' || ?1 || ' days'))) FROM transactions WHERE first_copied_at IS NOT NULL AND status IN ('Completed','Needs Review') AND COALESCE(retention_until,datetime(first_copied_at, '+' || ?1 || ' days')) <= datetime('now')",
        [days], |r| r.get::<_, Option<String>>(0),
    ).map_err(db_error)?;
    let png_file_count: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT png_path) FROM transactions WHERE first_copied_at IS NOT NULL AND status IN ('Completed','Needs Review') AND COALESCE(retention_until,datetime(first_copied_at, '+' || ?1 || ' days')) <= datetime('now') AND png_path IS NOT NULL AND png_path <> ''",
        [days], |r| r.get::<_, i64>(0),
    ).map_err(db_error)?;
    Ok(RetentionPreview { retention_days: days, eligible_count: eligible_count.max(0) as usize, oldest_expired_at, png_file_count: png_file_count.max(0) as usize })
}

pub fn run_cleanup(db: &Database) -> Result<RetentionRunSummary, String> {
    let mut conn = db.connection.lock().map_err(lock_error)?;
    let days: u32 = conn.query_row("SELECT COALESCE((SELECT CAST(setting_value AS INTEGER) FROM app_settings WHERE setting_key='retention_period'), ?1)", [DEFAULT_DAYS], |r| r.get(0)).map_err(db_error)?;
    let tx = conn.transaction().map_err(db_error)?;
    let mut stmt = tx.prepare("SELECT id,png_path FROM transactions WHERE first_copied_at IS NOT NULL AND status IN ('Completed','Needs Review') AND COALESCE(retention_until,datetime(first_copied_at, '+' || ?1 || ' days')) <= datetime('now')").map_err(db_error)?;
    let expired: Vec<(i64, Option<String>)> = stmt.query_map([days], |r| Ok((r.get(0)?, r.get(1)?))).map_err(db_error)?.collect::<Result<_,_>>().map_err(db_error)?;
    drop(stmt);
    let ids: Vec<i64> = expired.iter().map(|(id,_)| *id).collect();
    for id in &ids { tx.execute("DELETE FROM transactions WHERE id=?1", [id]).map_err(db_error)?; }
    tx.commit().map_err(db_error)?;

    // Remove PNGs only if no surviving transaction references the same path.
    let candidates: HashSet<String> = expired.into_iter().filter_map(|(_,p)| p).filter(|p| !p.trim().is_empty()).collect();
    let mut deleted_files = 0usize; let mut failed_files = 0usize;
    for path in candidates {
        let still_referenced: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM transactions WHERE png_path=?1)", [&path], |r| r.get(0)).map_err(db_error)?;
        if still_referenced { continue; }
        if std::path::Path::new(&path).extension().and_then(|ext| ext.to_str()).map(|ext| ext.eq_ignore_ascii_case("png")).unwrap_or(false) {
            match fs::remove_file(&path) { Ok(()) => deleted_files += 1, Err(e) if e.kind()==std::io::ErrorKind::NotFound => {}, Err(_) => failed_files += 1 }
        } else { failed_files += 1; }
    }
    Ok(RetentionRunSummary { deleted_transactions: ids.len(), deleted_png_files: deleted_files, failed_png_deletions: failed_files })
}
