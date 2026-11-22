//! Transaction extraction orchestration.
//! Database locks are held only for short SQL operations; OCR runs without the
//! database mutex so Gmail sync and other commands are not blocked by Tesseract.

use std::{fs::File, io::Read};

use base64::{engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD}, Engine};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::{databases::Database, services::ocr::{run_tesseract, OcrExtractionStatus, TesseractOptions}};
use super::email_parser::parse_flip_email;


const MAX_AUTOMATIC_ATTEMPTS: i64 = 3;
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const JPEG_SIGNATURE: &[u8; 3] = b"\xFF\xD8\xFF";
const EXTRACTION_EVENT: &str = "flip://extraction-progress";
const EXTRACTION_SUMMARY_EVENT: &str = "flip://extraction-summary";



#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionSummary {
    pub processed: usize,
    pub completed: usize,
    pub needs_review: usize,
    pub duplicate_review: usize,
    pub failed: usize,
    pub skipped: usize,
}


#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionProgressEvent {
    pub transaction_id: i64,
    pub status: String,
    pub message: Option<String>,
}


#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionTransactionRecord {
    pub id: i64,
    pub gmail_account_id: i64,
    pub account_email: String,
    pub gmail_message_id: String,
    pub flip_transaction_id: Option<String>,
    pub transaction_date: Option<String>,
    pub recipient_name: Option<String>,
    pub amount: Option<i64>,
    pub transfer_note: Option<String>,
    pub ocr_raw_result: Option<String>,
    pub status: String,
    pub review_reason: Option<String>,
    pub last_error: Option<String>,
    pub png_path: Option<String>,
    pub attempt_count: i64,
    pub last_attempt_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}


#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionStatusSummary {
    pub detected: i64,
    pub parsed: i64,
    pub ocr_processing: i64,
    pub completed: i64,
    pub needs_review: i64,
    pub duplicate_review: i64,
    pub failed: i64,
}


#[derive(Debug)]
struct PendingItem {
    id: i64,
    account_id: i64,
    message_id: String,
    subject: Option<String>,
    payload_json: String,
}



/// Runs the automatic queue: Detected rows and Failed rows below the retry cap.
/// Needs Review is intentionally excluded from automatic retries to avoid repeatedly
/// OCR-ing deterministic/missing data. Manual retry is exposed separately.
pub fn run_pending_extraction(
    database: &Database,
    options: &TesseractOptions,
    app: Option<&AppHandle>,
) -> Result<ExtractionSummary, String> {
    let pending_ids = {
        let connection = database.connection.lock().map_err(lock_err)?;
        let mut statement = connection.prepare(
            "SELECT id FROM transactions
             WHERE (status='Detected' AND attempt_count < ?1)
                OR (status='Failed' AND attempt_count < ?1)
             ORDER BY id ASC",
        ).map_err(db_err)?;
        let rows = statement.query_map([MAX_AUTOMATIC_ATTEMPTS], |row| row.get::<_, i64>(0)).map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?
    };

    let mut summary = ExtractionSummary::default();
    for id in pending_ids {
        match process_one(database, id, options, true, app) {
            Ok(Some(status)) => {
                summary.processed += 1;
                count_status(&mut summary, &status);
            }
            Ok(None) => summary.skipped += 1,
            Err(error) => {
                // Preserve the transaction and retry metadata; technical failures
                // are retried on later sync/startup only up to the automatic cap.
                set_terminal_state(database, id, "Failed", Some(&error), None, None, None, None, None, None)?;
                log_processing(database, id, "extraction", "Failed", Some(&error))?;
                emit_progress(app, id, "Failed", Some(&error));
                summary.processed += 1;
                summary.failed += 1;
            }
        }
    }
    if let Some(app) = app {
        let _ = app.emit(EXTRACTION_SUMMARY_EVENT, &summary);
    }
    Ok(summary)
}


/// Re-queues transactions interrupted while in Parsed or OCR Processing.
/// This is idempotent and should be run once during application startup.
pub fn recover_interrupted_extractions(database: &Database) -> Result<usize, String> {
    let connection = database.connection.lock().map_err(lock_err)?;
    let affected = connection.execute(
        "UPDATE transactions
         SET status='Detected',
             review_reason='Proses extraction sebelumnya terhenti. Transaksi dijadwalkan ulang.',
             last_error='Interrupted extraction recovered',
             updated_at=CURRENT_TIMESTAMP
         WHERE status IN ('Parsed','OCR Processing')",
        [],
    ).map_err(db_err)?;
    if affected > 0 {
        connection.execute(
            "INSERT INTO processing_logs (process_type, status, error_message)
             VALUES ('extraction-recovery', 'Recovered', ?1)",
            [format!("{} transaction(s) recovered from interrupted state", affected)],
        ).map_err(db_err)?;
    }
    Ok(affected)
}


/// Manually retries a single transaction regardless of the automatic retry cap.
pub fn retry_transaction_extraction(
    database: &Database,
    transaction_id: i64,
    options: &TesseractOptions,
    app: Option<&AppHandle>,
) -> Result<String, String> {
    {
        let connection = database.connection.lock().map_err(lock_err)?;
        let current_status: Option<String> = connection.query_row(
            "SELECT status FROM transactions WHERE id=?1", [transaction_id], |row| row.get(0),
        ).optional().map_err(db_err)?;
        let status = current_status.ok_or_else(|| format!("Transaction {transaction_id} tidak ditemukan."))?;
        if !matches!(status.as_str(), "Detected" | "Failed" | "Needs Review" | "Duplicate Review") {
            return Err(format!("Transaction tidak dapat di-retry dari status {status}."));
        }
    }
    match process_one(database, transaction_id, options, false, app) {
        Ok(Some(status)) => Ok(status),
        Ok(None) => Err(format!("Transaction {transaction_id} sedang diproses oleh worker lain.")),
        Err(error) => {
            set_terminal_state(database, transaction_id, "Failed", Some(&error), None, None, None, None, None, Some(&error))?;
            log_processing(database, transaction_id, "manual-retry", "Failed", Some(&error))?;
            emit_progress(app, transaction_id, "Failed", Some(&error));
            Err(error)
        }
    }
}

/// Stores a manual transfer-note correction and updates the transaction status.
/// The correction is committed together with its audit record.
pub fn correct_transfer_note(database: &Database, transaction_id: i64, new_note: &str) -> Result<String, String> {
    let cleaned = new_note.trim();
    if cleaned.is_empty() {
        return Err("Catatan Transfer tidak boleh kosong.".into());
    }

    let mut connection = database.connection.lock().map_err(lock_err)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_err)?;
    let row: Option<(Option<String>, Option<String>, Option<String>, Option<i64>, String)> = transaction.query_row(
        "SELECT transfer_note, flip_transaction_id, transaction_date, amount, status
         FROM transactions WHERE id=?1",
        [transaction_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    ).optional().map_err(db_err)?;
    let (old_note, flip_id, date, amount, current_status) = row.ok_or_else(|| format!("Transaction {transaction_id} tidak ditemukan."))?;
    if !matches!(current_status.as_str(), "Completed" | "Needs Review" | "Duplicate Review") {
        return Err(format!("Transfer Note tidak dapat dikoreksi saat status {current_status}."));
    }
    let recipient: Option<String> = transaction.query_row(
        "SELECT recipient_name FROM transactions WHERE id=?1", [transaction_id], |row| row.get(0),
    ).map_err(db_err)?;

    transaction.execute(
        "INSERT INTO correction_history (transaction_id, field_name, old_value, new_value)
         VALUES (?1, 'transfer_note', ?2, ?3)",
        params![transaction_id, old_note, cleaned],
    ).map_err(db_err)?;

    let duplicate = if let Some(id) = flip_id.as_deref() {
        transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM transactions WHERE flip_transaction_id=?1 AND id<>?2)",
            params![id, transaction_id], |row| row.get::<_, bool>(0),
        ).map_err(db_err)?
    } else { false };

    let missing_fields = flip_id.is_none() || date.is_none() || recipient.as_deref().unwrap_or_default().trim().is_empty() || amount.is_none();
    if duplicate {
        mark_existing_duplicate_rows(&transaction, flip_id.as_deref().unwrap_or_default(), transaction_id).map_err(db_err)?;
    }
    let (status, reason) = if duplicate {
        ("Duplicate Review", Some("Flip Transaction ID terdeteksi duplikat."))
    } else if missing_fields {
        ("Needs Review", Some("Field transaksi wajib belum lengkap; Catatan Transfer telah dikoreksi."))
    } else {
        ("Completed", None)
    };
    transaction.execute(
        "UPDATE transactions SET transfer_note=?1, status=?2, review_reason=?3,
            last_error=NULL, updated_at=CURRENT_TIMESTAMP WHERE id=?4",
        params![cleaned, status, reason, transaction_id],
    ).map_err(db_err)?;
    transaction.execute(
        "INSERT INTO processing_logs (transaction_id, process_type, status, error_message)
         VALUES (?1, 'manual-correction', ?2, ?3)",
        params![transaction_id, status, reason],
    ).map_err(db_err)?;
    transaction.commit().map_err(db_err)?;
    Ok(status.to_string())
}


pub fn list_extraction_transactions(
    database: &Database,
    limit: u32,
    offset: u32,
    status_filter: Option<&str>,
) -> Result<Vec<ExtractionTransactionRecord>, String> {
    let connection = database.connection.lock().map_err(lock_err)?;
    let limit = limit.clamp(1, 500);
    let offset = i64::from(offset);
    let mut statement = connection.prepare(
        "SELECT t.id, t.gmail_account_id, a.email_address, t.gmail_message_id,
                t.flip_transaction_id, t.transaction_date, t.recipient_name, t.amount,
                t.transfer_note, t.ocr_raw_result, t.status, t.review_reason, t.last_error,
                t.png_path, t.attempt_count, t.last_attempt_at, t.created_at, t.updated_at
         FROM transactions t JOIN gmail_accounts a ON a.id=t.gmail_account_id
         WHERE (?1 IS NULL OR t.status=?1)
         ORDER BY t.created_at DESC, t.id DESC LIMIT ?2 OFFSET ?3",
    ).map_err(db_err)?;
    let rows = statement.query_map(params![status_filter, i64::from(limit), offset], |row| Ok(ExtractionTransactionRecord {
        id: row.get(0)?, gmail_account_id: row.get(1)?, account_email: row.get(2)?, gmail_message_id: row.get(3)?,
        flip_transaction_id: row.get(4)?, transaction_date: row.get(5)?, recipient_name: row.get(6)?, amount: row.get(7)?,
        transfer_note: row.get(8)?, ocr_raw_result: row.get(9)?, status: row.get(10)?, review_reason: row.get(11)?,
        last_error: row.get(12)?, png_path: row.get(13)?, attempt_count: row.get(14)?, last_attempt_at: row.get(15)?,
        created_at: row.get(16)?, updated_at: row.get(17)?,
    })).map_err(db_err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
}


pub fn get_extraction_status_summary(database: &Database) -> Result<ExtractionStatusSummary, String> {
    let connection = database.connection.lock().map_err(lock_err)?;
    let mut statement = connection.prepare(
        "SELECT status, COUNT(*) FROM transactions GROUP BY status",
    ).map_err(db_err)?;
    let mut summary = ExtractionStatusSummary { detected:0, parsed:0, ocr_processing:0, completed:0, needs_review:0, duplicate_review:0, failed:0 };
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))).map_err(db_err)?;
    for row in rows {
        let (status, count) = row.map_err(db_err)?;
        match status.as_str() {
            "Detected" => summary.detected = count,
            "Parsed" => summary.parsed = count,
            "OCR Processing" => summary.ocr_processing = count,
            "Completed" => summary.completed = count,
            "Needs Review" => summary.needs_review = count,
            "Duplicate Review" => summary.duplicate_review = count,
            "Failed" => summary.failed = count,
            _ => {}
        }
    }
    Ok(summary)
}


fn process_one(
    database: &Database,
    transaction_id: i64,
    options: &TesseractOptions,
    automatic: bool,
    app: Option<&AppHandle>,
) -> Result<Option<String>, String> {
    let item = claim_transaction(database, transaction_id, automatic)?;
    let Some(item) = item else { return Ok(None); };
    emit_progress(app, transaction_id, "Parsed", None);

    let payload: Value = match serde_json::from_str(&item.payload_json) {
        Ok(payload) => payload,
        Err(error) => {
            let reason = format!("Payload email tidak valid: {error}");
            let status = finish_review(database, &item, None, None, None, None, None, Some(&reason), None, app)?;
            return Ok(Some(status));
        }
    };
    let body = decoded_email_body(&payload);
    let parsed = parse_flip_email(item.subject.as_deref(), &body);
    save_parsed_fields(database, item.id, &parsed)?;
    if !parsed.review_reasons.is_empty() {
        let reason = parsed.review_reasons.join(" ");
        let status = finish_review(database, &item, parsed.transaction_id.as_deref(), parsed.transaction_date.as_deref(), parsed.recipient_name.as_deref(), parsed.amount, None, Some(&reason), None, app)?;
        return Ok(Some(status));
    }

    let attachments = local_png_attachments(database, item.account_id, &item.message_id)?;
    if attachments.len() != 1 {
        let reason = if attachments.is_empty() { "Lampiran gambar invoice (PNG/JPEG) tidak ditemukan." } else { "Lebih dari satu attachment gambar invoice ditemukan; perlu pemeriksaan manual." };
        let status = finish_review(database, &item, parsed.transaction_id.as_deref(), parsed.transaction_date.as_deref(), parsed.recipient_name.as_deref(), parsed.amount, None, Some(reason), None, app)?;
        return Ok(Some(status));
    }
    let image_path = &attachments[0];
    if let Err(error) = validate_image_file(image_path) {
        let reason = format!("Attachment gambar tidak valid atau tidak dapat dibaca: {error}");
        let status = finish_review(database, &item, parsed.transaction_id.as_deref(), parsed.transaction_date.as_deref(), parsed.recipient_name.as_deref(), parsed.amount, Some(image_path), Some(&reason), Some(&reason), app)?;
        return Ok(Some(status));
    }

    set_processing_status(database, transaction_id, "OCR Processing")?;
    emit_progress(app, transaction_id, "OCR Processing", None);
    let ocr = match run_tesseract(image_path, options) {
        Ok(result) => result,
        Err(error) => {
            let reason = format!("OCR gagal: {error}");
            let status = finish_review(database, &item, parsed.transaction_id.as_deref(), parsed.transaction_date.as_deref(), parsed.recipient_name.as_deref(), parsed.amount, Some(image_path), Some(&reason), Some(&reason), app)?;
            return Ok(Some(status));
        }
    };
    if ocr.status != OcrExtractionStatus::Completed || ocr.transfer_note.as_deref().unwrap_or_default().trim().is_empty() {
        let reason = ocr.review_reason.as_deref().unwrap_or("Catatan Transfer kosong atau tidak terbaca.");
        let status = finish_review(database, &item, parsed.transaction_id.as_deref(), parsed.transaction_date.as_deref(), parsed.recipient_name.as_deref(), parsed.amount, Some(image_path), Some(reason), None, app)?;
        persist_ocr_raw(database, transaction_id, &ocr.raw_text)?;
        return Ok(Some(status));
    }

    finalize_with_duplicate_check(database, &item, &parsed, &ocr.transfer_note, &ocr.raw_text, image_path, app)
}


fn claim_transaction(database: &Database, id: i64, automatic: bool) -> Result<Option<PendingItem>, String> {
    let mut connection = database.connection.lock().map_err(lock_err)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_err)?;
    let row: Option<(i64, String, Option<String>, String)> = transaction.query_row(
        "SELECT t.gmail_account_id, t.gmail_message_id, m.subject, m.payload_json
         FROM transactions t JOIN gmail_messages m
          ON m.gmail_account_id=t.gmail_account_id AND m.gmail_message_id=t.gmail_message_id
         WHERE t.id=?1 AND (
            (?2=1 AND ((t.status='Detected' AND t.attempt_count < ?3) OR (t.status='Failed' AND t.attempt_count < ?3)))
            OR (?2=0 AND t.status IN ('Detected','Failed','Needs Review','Duplicate Review'))
         )",
        params![id, automatic, MAX_AUTOMATIC_ATTEMPTS],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).optional().map_err(db_err)?;
    let Some((account_id, message_id, subject, payload_json)) = row else { transaction.commit().map_err(db_err)?; return Ok(None); };
    transaction.execute(
        "UPDATE transactions SET status='Parsed',
            attempt_count=CASE WHEN ?1=1 THEN attempt_count+1 ELSE 0 END,
            flip_transaction_id=NULL, transaction_date=NULL, recipient_name=NULL,
            amount=NULL, transfer_note=NULL, ocr_raw_result=NULL, png_path=NULL,
            last_attempt_at=CURRENT_TIMESTAMP, review_reason=NULL, last_error=NULL,
            updated_at=CURRENT_TIMESTAMP WHERE id=?2",
        params![automatic, id],
    ).map_err(db_err)?;
    transaction.execute(
        "INSERT INTO processing_logs (gmail_account_id, transaction_id, process_type, status)
         VALUES (?1, ?2, 'extraction', 'Started')",
        params![account_id, id],
    ).map_err(db_err)?;
    transaction.commit().map_err(db_err)?;
    Ok(Some(PendingItem { id, account_id, message_id, subject, payload_json }))
}


fn save_parsed_fields(database: &Database, id: i64, parsed: &super::email_parser::EmailTransactionFields) -> Result<(), String> {
    let connection = database.connection.lock().map_err(lock_err)?;
    connection.execute(
        "UPDATE transactions SET flip_transaction_id=?1, transaction_date=?2,
            recipient_name=?3, amount=?4, updated_at=CURRENT_TIMESTAMP WHERE id=?5",
        params![parsed.transaction_id, parsed.transaction_date, parsed.recipient_name, parsed.amount, id],
    ).map_err(db_err)?;
    Ok(())
}


fn validate_image_file(path: &str) -> Result<(), String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut signature = [0u8; 8];
    let read = file.read(&mut signature).map_err(|error| error.to_string())?;
    if read >= PNG_SIGNATURE.len() && &signature == PNG_SIGNATURE {
        return Ok(());
    }
    if read >= JPEG_SIGNATURE.len() && &signature[..JPEG_SIGNATURE.len()] == JPEG_SIGNATURE {
        return Ok(());
    }
    Err("signature file bukan PNG atau JPEG yang valid".into())
}


fn local_png_attachments(database: &Database, account_id: i64, message_id: &str) -> Result<Vec<String>, String> {
    let connection = database.connection.lock().map_err(lock_err)?;
    let mut statement = connection.prepare(
        "SELECT local_path FROM gmail_attachments
         WHERE gmail_account_id=?1 AND gmail_message_id=?2
           AND detected_format IN ('png','jpeg')
         ORDER BY id ASC",
    ).map_err(db_err)?;
    let rows = statement.query_map(params![account_id, message_id], |row| row.get::<_, String>(0)).map_err(db_err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
}


fn set_processing_status(database: &Database, id: i64, status: &str) -> Result<(), String> {
    let connection = database.connection.lock().map_err(lock_err)?;
    connection.execute(
        "UPDATE transactions SET status=?1, updated_at=CURRENT_TIMESTAMP WHERE id=?2",
        params![status, id],
    ).map_err(db_err)?;
    Ok(())
}


fn finish_review(
    database: &Database,
    item: &PendingItem,
    flip_id: Option<&str>,
    date: Option<&str>,
    recipient: Option<&str>,
    amount: Option<i64>,
    png_path: Option<&str>,
    reason: Option<&str>,
    error: Option<&str>,
    app: Option<&AppHandle>,
) -> Result<String, String> {
    let mut connection = database.connection.lock().map_err(lock_err)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_err)?;
    let duplicate = if let Some(id) = flip_id {
        transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM transactions WHERE flip_transaction_id=?1 AND id<>?2)",
            params![id, item.id],
            |row| row.get::<_, bool>(0),
        ).map_err(db_err)?
    } else {
        false
    };
    if duplicate {
        mark_existing_duplicate_rows(&transaction, flip_id.unwrap_or_default(), item.id).map_err(db_err)?;
    }
    let status = if duplicate { "Duplicate Review" } else { "Needs Review" };
    let final_reason = if duplicate {
        Some(match reason {
            Some(reason) if !reason.trim().is_empty() => format!("{reason} Flip Transaction ID terdeteksi pada Message lain."),
            _ => "Flip Transaction ID terdeteksi pada Message lain.".to_string(),
        })
    } else {
        reason.map(|value| value.to_string())
    };
    transaction.execute(
        "UPDATE transactions SET status=?1, review_reason=?2,
            flip_transaction_id=COALESCE(?3, flip_transaction_id),
            transaction_date=COALESCE(?4, transaction_date), recipient_name=COALESCE(?5, recipient_name),
            amount=COALESCE(?6, amount), png_path=COALESCE(?7, png_path), last_error=?8,
            updated_at=CURRENT_TIMESTAMP WHERE id=?9",
        params![status, final_reason, flip_id, date, recipient, amount, png_path, error, item.id],
    ).map_err(db_err)?;
    transaction.execute(
        "INSERT INTO processing_logs (gmail_account_id, transaction_id, process_type, status, error_message)
         VALUES (?1, ?2, 'extraction', ?3, ?4)",
        params![item.account_id, item.id, status, final_reason],
    ).map_err(db_err)?;
    transaction.commit().map_err(db_err)?;
    emit_progress(app, item.id, status, final_reason.as_deref());
    Ok(status.to_string())
}

fn finalize_with_duplicate_check(
    database: &Database,
    item: &PendingItem,
    parsed: &super::email_parser::EmailTransactionFields,
    transfer_note: &Option<String>,
    raw_text: &str,
    image_path: &str,
    app: Option<&AppHandle>,
) -> Result<Option<String>, String> {
    let mut connection = database.connection.lock().map_err(lock_err)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_err)?;
    let duplicate: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM transactions WHERE flip_transaction_id=?1 AND id<>?2 AND flip_transaction_id IS NOT NULL)",
        params![parsed.transaction_id, item.id], |row| row.get(0),
    ).map_err(db_err)?;
    if duplicate {
        mark_existing_duplicate_rows(&transaction, parsed.transaction_id.as_deref().unwrap_or_default(), item.id).map_err(db_err)?;
    }
    let status = if duplicate { "Duplicate Review" } else { "Completed" };
    let reason = if duplicate { Some("Flip Transaction ID terdeteksi pada Message lain." ) } else { None };
    transaction.execute(
        "UPDATE transactions SET flip_transaction_id=?1, transaction_date=?2, recipient_name=?3,
            amount=?4, transfer_note=?5, ocr_raw_result=?6, png_path=?7, status=?8,
            review_reason=?9, last_error=NULL, updated_at=CURRENT_TIMESTAMP WHERE id=?10",
        params![parsed.transaction_id, parsed.transaction_date, parsed.recipient_name, parsed.amount, transfer_note, raw_text, image_path, status, reason, item.id],
    ).map_err(db_err)?;
    transaction.execute(
        "INSERT INTO processing_logs (gmail_account_id, transaction_id, process_type, status, error_message)
         VALUES (?1, ?2, 'extraction', ?3, ?4)",
        params![item.account_id, item.id, status, reason],
    ).map_err(db_err)?;
    transaction.commit().map_err(db_err)?;
    emit_progress(app, item.id, status, reason);
    Ok(Some(status.to_string()))
}


fn persist_ocr_raw(database: &Database, id: i64, raw: &str) -> Result<(), String> {
    let connection = database.connection.lock().map_err(lock_err)?;
    connection.execute(
        "UPDATE transactions SET ocr_raw_result=?1, updated_at=CURRENT_TIMESTAMP WHERE id=?2",
        params![raw, id],
    ).map_err(db_err)?;
    Ok(())
}


fn mark_existing_duplicate_rows(
    transaction: &Transaction<'_>,
    flip_transaction_id: &str,
    current_id: i64,
) -> Result<(), rusqlite::Error> {
    const DUPLICATE_REASON: &str = "Flip Transaction ID terdeteksi pada Message lain.";
    let affected = transaction.execute(
        "UPDATE transactions SET status='Duplicate Review',
            review_reason=CASE
                WHEN review_reason IS NULL OR trim(review_reason)='' THEN ?3
                WHEN instr(review_reason, ?3)>0 THEN review_reason
                ELSE review_reason || ' ' || ?3
            END,
            updated_at=CURRENT_TIMESTAMP
         WHERE flip_transaction_id=?1 AND id<>?2",
        params![flip_transaction_id, current_id, DUPLICATE_REASON],
    )?;
    if affected > 0 {
        transaction.execute(
            "INSERT INTO processing_logs (gmail_account_id, transaction_id, process_type, status, error_message)
             SELECT gmail_account_id, id, 'duplicate-detection', 'Duplicate Review', ?3
             FROM transactions WHERE flip_transaction_id=?1 AND id<>?2",
            params![flip_transaction_id, current_id, DUPLICATE_REASON],
        )?;
    }
    Ok(())
}


fn set_terminal_state(
    database: &Database,
    id: i64,
    status: &str,
    reason: Option<&str>,
    flip_id: Option<&str>,
    date: Option<&str>,
    recipient: Option<&str>,
    amount: Option<i64>,
    png_path: Option<&str>,
    error: Option<&str>,
) -> Result<(), String> {
    let connection = database.connection.lock().map_err(lock_err)?;
    connection.execute(
        "UPDATE transactions SET status=?1, review_reason=?2,
            flip_transaction_id=COALESCE(?3, flip_transaction_id),
            transaction_date=COALESCE(?4, transaction_date), recipient_name=COALESCE(?5, recipient_name),
            amount=COALESCE(?6, amount), png_path=COALESCE(?7, png_path), last_error=?8,
            updated_at=CURRENT_TIMESTAMP WHERE id=?9",
        params![status, reason, flip_id, date, recipient, amount, png_path, error, id],
    ).map_err(db_err)?;
    Ok(())
}


fn log_processing(database: &Database, id: i64, process_type: &str, status: &str, error: Option<&str>) -> Result<(), String> {
    let connection = database.connection.lock().map_err(lock_err)?;
    connection.execute(
        "INSERT INTO processing_logs (transaction_id, process_type, status, error_message)
         VALUES (?1, ?2, ?3, ?4)",
        params![id, process_type, status, error],
    ).map_err(db_err)?;
    Ok(())
}


fn count_status(summary: &mut ExtractionSummary, status: &str) {
    match status {
        "Completed" => summary.completed += 1,
        "Needs Review" => summary.needs_review += 1,
        "Duplicate Review" => summary.duplicate_review += 1,
        "Failed" => summary.failed += 1,
        _ => {}
    }
}


fn emit_progress(app: Option<&AppHandle>, transaction_id: i64, status: &str, message: Option<&str>) {
    if let Some(app) = app {
        let _ = app.emit(EXTRACTION_EVENT, ExtractionProgressEvent {
            transaction_id,
            status: status.to_string(),
            message: message.map(|value| value.to_string()),
        });
    }
}


fn decode_base64url(value: &str) -> Result<Vec<u8>, String> {
    URL_SAFE_NO_PAD.decode(value).or_else(|_| URL_SAFE.decode(value)).map_err(|error| error.to_string())
}


fn decoded_email_body(message_or_payload: &Value) -> String {
    // Gmail sync stores the full Message object. MIME data lives under `payload`.
    // Also accept a raw MIME payload for focused unit tests and other callers.
    let mime_payload = message_or_payload.get("payload").unwrap_or(message_or_payload);
    let mut plain = Vec::new();
    let mut html = Vec::new();
    collect_body_parts(mime_payload, &mut plain, &mut html);
    if !plain.is_empty() { plain.join("\n") } else { html.join("\n") }
}


fn collect_body_parts(value: &Value, plain: &mut Vec<String>, html: &mut Vec<String>) {
    let mime = value.get("mimeType").and_then(Value::as_str).unwrap_or_default();
    // Gmail includes `body.attachmentId: null` on ordinary body parts in some
    // payloads. Presence of the key alone must not classify the body as an attachment.
    let has_attachment_id = value.pointer("/body/attachmentId")
        .and_then(Value::as_str)
        .is_some_and(|attachment_id| !attachment_id.trim().is_empty());
    let is_attachment = value.get("filename").and_then(Value::as_str).is_some_and(|name| !name.trim().is_empty())
        || has_attachment_id;
    if is_attachment {
        return;
    }

    if let Some(data) = value.pointer("/body/data").and_then(Value::as_str) {
        if let Ok(bytes) = decode_base64url(data) {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            if mime.eq_ignore_ascii_case("text/plain") {
                if !text.trim().is_empty() { plain.push(text); }
            } else if mime.eq_ignore_ascii_case("text/html") {
                if !text.trim().is_empty() { html.push(html_to_text(&text)); }
            }
        }
    }
    if let Some(parts) = value.get("parts").and_then(Value::as_array) {
        for part in parts { collect_body_parts(part, plain, html); }
    }
}

/// Lightweight HTML-to-text conversion for invoice email bodies; avoids adding a
/// parser dependency to the desktop bundle. Scripts/styles are discarded.
fn html_to_text(html: &str) -> String {
    let mut output = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut tag_name = String::new();
    let mut skip_block: Option<String> = None;
    for ch in html.chars() {
        if ch == '<' {
            in_tag = true;
            tag_name.clear();
            continue;
        }
        if in_tag {
            if ch == '>' {
                in_tag = false;
                let trimmed = tag_name.trim().trim_end_matches('/').to_ascii_lowercase();
                if trimmed.starts_with("script") { skip_block = Some("script".into()); }
                if trimmed.starts_with("style") { skip_block = Some("style".into()); }
                if trimmed.starts_with("/script") || trimmed.starts_with("/style") { skip_block = None; }
                // Keep boundaries between HTML block elements. Flip receipt labels and
                // values are placed in adjacent divs and must not concatenate.
                let block_tag = trimmed.trim_start_matches('/');
                if ["br", "p", "div", "li", "tr", "td", "table", "h1", "h2", "h3"].contains(&block_tag)
                    && (trimmed.starts_with('/') || !trimmed.ends_with('/')) {
                    output.push('\n');
                }
            } else if tag_name.len() < 40 { tag_name.push(ch); }
            continue;
        }
        if skip_block.is_none() { output.push(ch); }
    }
    decode_html_entities(&output).replace('\u{00a0}', " ").trim().to_string()
}


fn decode_html_entities(text: &str) -> String {
    text.replace("&nbsp;", " ").replace("&#160;", " ").replace("&amp;", "&")
        .replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"")
        .replace("&#39;", "'").replace("&apos;", "'")
}


fn lock_err(error: impl std::fmt::Display) -> String {
    format!("Database lock failed: {error}")
}
fn db_err(error: rusqlite::Error) -> String { format!("Database error: {error}") }


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flip_receipt_html_preserves_label_value_boundaries() {
        let html = r#"<div class="section__content__detail"><div>ID Transaksi</div><div>#ST261121135251582SV50K</div></div><div><div>Waktu Terkirim</div><div>21 Nov 2026 13:53 WIB</div></div><div><div>Nama Tujuan</div><div>MUHAMAD ANWAR GUNAWAN</div></div><div><div>Nominal</div><div>Rp18.975.000</div></div>"#;
        let text = html_to_text(html);
        let parsed = parse_flip_email(None, &text);
        assert_eq!(parsed.transaction_id.as_deref(), Some("ST261121135251582SV50K"));
        assert_eq!(parsed.transaction_date.as_deref(), Some("21/11/2026"));
        assert_eq!(parsed.recipient_name.as_deref(), Some("MUHAMAD ANWAR GUNAWAN"));
        assert_eq!(parsed.amount, Some(18_975_000));
        assert!(parsed.review_reasons.is_empty(), "unexpected review reasons: {:?}", parsed.review_reasons);
    }


    #[test]
    fn reads_html_body_when_attachment_id_is_null() {
        let payload = serde_json::json!({
            "mimeType": "text/html",
            "body": {
                "attachmentId": null,
                "data": "PGRpdj5JRCBUcmFuc2Frc2k8L2Rpdj48ZGl2PiNTVDI2MTEyMTEzNTI1MTU4MlNWNTBLPC9kaXY-PGRpdj5XYWt0dSBUZXJraXJpbTwvZGl2PjxkaXY-MjEgTm92IDIwMjYgMTM6NTMgV0lCPC9kaXY-PGRpdj5OYW1hIFR1anVhbjwvZGl2PjxkaXY-TVVIQU1BRCBBTldBUiBHVU5BV0FOPC9kaXY-PGRpdj5Ob21pbmFsPC9kaXY-PGRpdj5ScDE4Ljk3NS4wMDA8L2Rpdj4"
            }
        });
        let body = decoded_email_body(&payload);
        let parsed = parse_flip_email(None, &body);
        assert_eq!(parsed.transaction_id.as_deref(), Some("ST261121135251582SV50K"));
        assert_eq!(parsed.transaction_date.as_deref(), Some("21/11/2026"));
        assert_eq!(parsed.recipient_name.as_deref(), Some("MUHAMAD ANWAR GUNAWAN"));
        assert_eq!(parsed.amount, Some(18_975_000));
        assert!(parsed.review_reasons.is_empty(), "unexpected review reasons: {:?}", parsed.review_reasons);
    }

    #[test]
    fn extracts_body_from_full_gmail_message_envelope() {
        // Gmail sync stores a full Message object; its MIME tree is nested in payload.
        let payload = serde_json::json!({
            "id": "sample-message",
            "threadId": "sample-thread",
            "payload": {
                "mimeType": "text/html",
                "body": {
                    "attachmentId": null,
                    "data": "PGRpdj5JRCBUcmFuc2Frc2k8L2Rpdj48ZGl2PiNTVDI2MTEyMTEzNTI1MTU4MlNWNTBLPC9kaXY-PGRpdj5XYWt0dSBUZXJraXJpbTwvZGl2PjxkaXY-MjEgTm92IDIwMjYgMTM6NTMgV0lCPC9kaXY-PGRpdj5OYW1hIFR1anVhbjwvZGl2PjxkaXY-TVVIQU1BRCBBTldBUiBHVU5BV0FOPC9kaXY-PGRpdj5Ob21pbmFsPC9kaXY-PGRpdj5ScDE4Ljk3NS4wMDA8L2Rpdj4"
                }
            }
        });
        let body = decoded_email_body(&payload);
        let parsed = parse_flip_email(None, &body);
        assert_eq!(parsed.transaction_id.as_deref(), Some("ST261121135251582SV50K"));
        assert_eq!(parsed.transaction_date.as_deref(), Some("21/11/2026"));
        assert_eq!(parsed.recipient_name.as_deref(), Some("MUHAMAD ANWAR GUNAWAN"));
        assert_eq!(parsed.amount, Some(18_975_000));
        assert!(parsed.review_reasons.is_empty(), "unexpected review reasons: {:?}", parsed.review_reasons);
    }


    #[test]
    fn html_conversion_removes_tags_and_script_content() {
        assert_eq!(html_to_text("<p>Hello &amp; World</p><script>alert(1)</script>"), "Hello & World");
    }

    #[test]
    fn decodes_nested_plain_text_and_prefers_plain_text() {
        let payload = serde_json::json!({"mimeType":"multipart/mixed","parts":[
            {"mimeType":"text/html","body":{"data":"PHA-SGVsbG8gQm9keTwvcD4="}},
            {"mimeType":"multipart/alternative","parts":[{"mimeType":"text/plain","body":{"data":"dGVzdCBwbGFpbg=="}}]}
        ]});
        assert_eq!(decoded_email_body(&payload), "test plain");
    }

    #[test]
    fn ignores_text_attachment_parts_when_parsing_email_body() {
        let payload = serde_json::json!({"mimeType":"multipart/mixed","parts":[
            {"mimeType":"text/plain","body":{"data":"Ym9keSB0ZXh0"}},
            {"mimeType":"text/plain","filename":"attached.txt","body":{"data":"YXR0YWNobWVudCB0ZXh0"}}
        ]});
        assert_eq!(decoded_email_body(&payload), "body text");
    }

    #[test]
    fn uses_html_only_when_plain_text_missing() {
        let payload = serde_json::json!({"mimeType":"text/html","body":{"data":"PHA-SGVsbG88L3A-"}});
        assert!(decoded_email_body(&payload).contains("Hello"));
    }

    fn create_test_database() -> (Database, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("flip-extraction-test-{}.sqlite", uuid::Uuid::new_v4()));
        let database = Database::new(&path).unwrap();
        {
            let connection = database.connection.lock().unwrap();
            connection.execute("INSERT INTO gmail_accounts(email_address) VALUES ('pipeline-test@gmail.com')", []).unwrap();
            connection.execute(
                "INSERT INTO gmail_messages(gmail_account_id,gmail_message_id,gmail_thread_id,payload_json) VALUES (1,'message-1','thread-1','{}')",
                [],
            ).unwrap();
            connection.execute(
                "INSERT INTO transactions(gmail_account_id,gmail_message_id,status) VALUES (1,'message-1','Detected')",
                [],
            ).unwrap();
        }
        (database, path)
    }


    fn remove_test_database(database: Database, path: &std::path::Path) {
        drop(database);
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }


    #[test]
    fn manual_claim_clears_stale_fields_and_resets_auto_retry_budget() {
        let (database, path) = create_test_database();
        {
            let connection = database.connection.lock().unwrap();
            connection.execute(
                "UPDATE transactions SET status='Needs Review',flip_transaction_id='ST12345678',
                 transaction_date='01/01/2026',recipient_name='Old Name',amount=5000,
                 transfer_note='Old Note',ocr_raw_result='Old OCR',png_path='old.png',attempt_count=3 WHERE id=1",
                [],
            ).unwrap();
        }
        assert!(claim_transaction(&database, 1, false).unwrap().is_some());
        {
            let connection = database.connection.lock().unwrap();
            let state: (String, i64, Option<String>, Option<String>) = connection.query_row(
                "SELECT status,attempt_count,flip_transaction_id,transfer_note FROM transactions WHERE id=1",
                [],
                |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
            ).unwrap();
            assert_eq!(state, ("Parsed".into(), 0, None, None));
        }
        remove_test_database(database, &path);
    }


    #[test]
    fn automatic_claim_increments_and_obeys_retry_cap() {
        let (database, path) = create_test_database();
        {
            let connection = database.connection.lock().unwrap();
            connection.execute("UPDATE transactions SET status='Failed',attempt_count=2 WHERE id=1", []).unwrap();
        }
        assert!(claim_transaction(&database, 1, true).unwrap().is_some());
        {
            let connection = database.connection.lock().unwrap();
            connection.execute("UPDATE transactions SET status='Failed' WHERE id=1", []).unwrap();
            let count: i64 = connection.query_row("SELECT attempt_count FROM transactions WHERE id=1", [], |row| row.get(0)).unwrap();
            assert_eq!(count, 3);
        }
        assert!(claim_transaction(&database, 1, true).unwrap().is_none());
        remove_test_database(database, &path);
    }


    #[test]
    fn startup_recovery_requeues_interrupted_states() {
        let (database, path) = create_test_database();
        {
            let connection = database.connection.lock().unwrap();
            connection.execute("UPDATE transactions SET status='OCR Processing' WHERE id=1", []).unwrap();
        }
        assert_eq!(recover_interrupted_extractions(&database).unwrap(), 1);
        {
            let connection = database.connection.lock().unwrap();
            let status: String = connection.query_row("SELECT status FROM transactions WHERE id=1", [], |row| row.get(0)).unwrap();
            assert_eq!(status, "Detected");
        }
        remove_test_database(database, &path);
    }
}
