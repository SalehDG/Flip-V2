use crate::{
    databases::{Database, repositories::gmail_account_repository::GmailAccountRepository},
    gmail_sync::token_manager::refresh_access_token,
};
use base64::{
    Engine,
    engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD},
};
use reqwest::{Client, StatusCode};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State};

const GMAIL_API: &str = "https://gmail.googleapis.com/gmail/v1/users/me";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    history_id: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListResponse {
    messages: Option<Vec<MessageRef>>,
    next_page_token: Option<String>,
    result_size_estimate: Option<u64>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageRef {
    id: String,
    thread_id: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Message {
    id: String,
    thread_id: String,
    history_id: Option<String>,
    internal_date: Option<String>,
    snippet: Option<String>,
    payload: Option<MimePart>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MimePart {
    mime_type: Option<String>,
    filename: Option<String>,
    headers: Option<Vec<Header>>,
    body: Option<MimeBody>,
    parts: Option<Vec<MimePart>>,
}
#[derive(Debug, Deserialize)]
struct Header {
    name: String,
    value: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MimeBody {
    attachment_id: Option<String>,
    data: Option<String>,
    size: Option<u64>,
}
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SyncState {
    pub account_id: i64,
    pub status: String,
    pub start_date: Option<String>,
    pub processed_messages: u64,
    pub page_token: Option<String>,
    pub last_error: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSummary {
    pub account_id: i64,
    pub processed_messages: u64,
    pub skipped_messages: u64,
    pub attachment_count: u64,
    pub completed: bool,
}

async fn api_get(
    client: &Client,
    token: &str,
    url: &str,
    query: &[(&str, String)],
) -> Result<reqwest::Response, String> {
    let mut attempt = 0;
    loop {
        let response = client
            .get(url)
            .bearer_auth(token)
            .query(query)
            .send()
            .await
            .map_err(|e| format!("Gmail network request failed: {e}"))?;
        if response.status() == StatusCode::TOO_MANY_REQUESTS || response.status().is_server_error()
        {
            if attempt >= 4 {
                return Err(format!(
                    "Gmail API temporary failure after retries: {}",
                    response.status()
                ));
            }
            let delay = 1u64 << attempt;
            tokio::time::sleep(Duration::from_secs(delay)).await;
            attempt += 1;
            continue;
        }
        if response.status() == StatusCode::FORBIDDEN {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let quota_error = body.contains("rateLimitExceeded")
                || body.contains("userRateLimitExceeded")
                || body.contains("quotaExceeded");
            if quota_error && attempt < 4 {
                tokio::time::sleep(Duration::from_secs(1u64 << attempt)).await;
                attempt += 1;
                continue;
            }
            return Err(format!("Gmail API returned {status}: {body}"));
        }
        if response.status() == StatusCode::UNAUTHORIZED {
            return Err("GMAIL_TOKEN_REVOKED: Gmail rejected the access token. Reconnect this account if refreshing does not resolve it.".into());
        }
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Gmail API returned {status}: {body}"));
        }
        return Ok(response);
    }
}

fn decode_base64url(value: &str) -> Result<Vec<u8>, String> {
    URL_SAFE_NO_PAD
        .decode(value)
        .or_else(|_| URL_SAFE.decode(value))
        .map_err(|e| format!("Invalid Gmail base64url payload: {e}"))
}
fn detect_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(b"\xFF\xD8\xFF") {
        Some("jpeg")
    } else {
        None
    }
}
fn header<'a>(headers: &'a [Header], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}
fn collect_parts<'a>(part: &'a MimePart, out: &mut Vec<&'a MimePart>) {
    if let Some(parts) = &part.parts {
        for child in parts {
            collect_parts(child, out);
        }
    }
    if part
        .filename
        .as_deref()
        .map(|s| !s.is_empty())
        .unwrap_or(false)
        || part
            .body
            .as_ref()
            .and_then(|b| b.attachment_id.as_ref())
            .is_some()
    {
        out.push(part);
    }
}
fn safe_filename(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if clean.is_empty() {
        "attachment".into()
    } else {
        clean
    }
}
fn ensure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| format!("Cannot create attachment directory: {e}"))
}

async fn fetch_attachment(
    client: &Client,
    token: &str,
    message_id: &str,
    part: &MimePart,
) -> Result<Option<(String, String, String, Vec<u8>)>, String> {
    let body = match &part.body {
        Some(b) => b,
        None => return Ok(None),
    };
    let data = if let Some(data) = &body.data {
        data.clone()
    } else if let Some(attachment_id) = &body.attachment_id {
        let url = format!("{GMAIL_API}/messages/{message_id}/attachments/{attachment_id}");
        let response = api_get(client, token, &url, &[]).await?;
        let value: Value = response
            .json()
            .await
            .map_err(|e| format!("Invalid attachment response: {e}"))?;
        value
            .get("data")
            .and_then(Value::as_str)
            .ok_or("Attachment response has no data")?
            .to_string()
    } else {
        return Ok(None);
    };
    let bytes = decode_base64url(&data)?;
    let format = match detect_image(&bytes) {
        Some(v) => v,
        None => return Ok(None),
    };
    let digest = Sha256::digest(&bytes);
    let sha = digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let filename = part
        .filename
        .clone()
        .unwrap_or_else(|| format!("attachment.{format}"));
    Ok(Some((filename, format.to_string(), sha, bytes)))
}

fn save_message_and_attachments(
    db: &Database,
    account_id: i64,
    message: &Message,
    payload_json: &str,
    sender: Option<&str>,
    subject: Option<&str>,
    attachment_rows: &[(String, String, String, String, String, String, String)],
) -> Result<bool, String> {
    let connection = db
        .connection
        .lock()
        .map_err(|e| format!("Database lock failed: {e}"))?;
    let tx = connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM gmail_messages WHERE gmail_account_id=?1 AND gmail_message_id=?2)", params![account_id,message.id], |r| r.get(0)).map_err(|e| e.to_string())?;
    if exists {
        return Ok(false);
    }
    tx.execute("INSERT INTO gmail_messages (gmail_account_id,gmail_message_id,gmail_thread_id,history_id,internal_date,sender,subject,snippet,payload_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![account_id,message.id,message.thread_id,message.history_id,message.internal_date,sender,subject,message.snippet,payload_json]).map_err(|e| e.to_string())?;
    tx.execute("INSERT OR IGNORE INTO transactions (gmail_account_id,gmail_message_id,status) VALUES (?1,?2,'Detected')", params![account_id,message.id]).map_err(|e| e.to_string())?;
    for (part_key, attachment_id, filename, mime, format, sha, path) in attachment_rows
        .iter()
        .map(|(a, b, c, d, e, f, g)| (a, b, c, d, e, f, g))
    {
        // SQLite INTEGER is a signed 64-bit value. Convert explicitly rather than
        // passing u64 to rusqlite (which does not implement ToSql for u64).
        let size_u64 = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let size = i64::try_from(size_u64)
            .map_err(|_| format!("Attachment is too large for SQLite INTEGER: {path}"))?;
        tx.execute(
            "INSERT OR IGNORE INTO gmail_attachments (gmail_account_id,gmail_message_id,part_key,attachment_id,filename,declared_mime_type,detected_format,file_size,sha256,local_path) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![account_id,message.id,part_key,attachment_id,filename,mime,format,size,sha,path],
        ).map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(true)
}

fn update_state(
    db: &Database,
    account_id: i64,
    status: &str,
    start_date: &str,
    page_token: Option<&str>,
    history_id: &str,
    error: Option<&str>,
) -> Result<(), String> {
    let connection = db.connection.lock().map_err(|e| e.to_string())?;
    connection.execute("INSERT INTO gmail_sync_state (gmail_account_id,sync_status,sync_start_date,page_token,initial_history_id,last_error,updated_at) VALUES (?1,?2,?3,?4,?5,?6,CURRENT_TIMESTAMP) ON CONFLICT(gmail_account_id) DO UPDATE SET sync_status=excluded.sync_status,sync_start_date=excluded.sync_start_date,page_token=excluded.page_token,initial_history_id=excluded.initial_history_id,last_error=excluded.last_error,updated_at=CURRENT_TIMESTAMP", params![account_id,status,start_date,page_token,history_id,error]).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_gmail_sync_state(
    database: State<'_, Database>,
    account_id: i64,
) -> Result<SyncState, String> {
    let connection = database.connection.lock().map_err(|e| e.to_string())?;
    let state = connection.query_row("SELECT sync_status,sync_start_date,page_token,last_error FROM gmail_sync_state WHERE gmail_account_id=?1", [account_id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,Option<String>>(3)?))).optional().map_err(|e| e.to_string())?;
    let (status, start_date, page_token, last_error) =
        state.unwrap_or(("idle".into(), None, None, None));
    // COUNT(*) is returned by SQLite as a signed INTEGER (i64).
    let processed_messages_i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM gmail_messages WHERE gmail_account_id=?1",
            [account_id],
            |r| r.get::<_, i64>(0),
        )
        .map_err(|e| e.to_string())?;
    let processed_messages = u64::try_from(processed_messages_i64)
        .map_err(|_| "SQLite returned a negative message count".to_string())?;
    Ok(SyncState {
        account_id,
        status,
        start_date,
        processed_messages,
        page_token,
        last_error,
    })
}

#[tauri::command]
pub async fn start_initial_gmail_sync(
    app: AppHandle,
    database: State<'_, Database>,
    account_id: i64,
    start_date: String,
) -> Result<SyncSummary, String> {
    if start_date.len() != 10
        || start_date.chars().nth(4) != Some('-')
        || start_date.chars().nth(7) != Some('-')
    {
        return Err("Tanggal awal harus berformat YYYY-MM-DD".into());
    }
    let (email, token_ref, saved_start, saved_page, saved_history, saved_status) = {
        let connection = database.connection.lock().map_err(|e| e.to_string())?;
        let account = GmailAccountRepository::find_by_id(&connection, account_id)
            .map_err(|e| e.to_string())?
            .ok_or("Gmail account not found")?;
        let token_ref = GmailAccountRepository::find_token_reference(&connection, account_id)
            .map_err(|e| e.to_string())?
            .ok_or("Gmail account is disconnected")?;
        let state = connection.query_row("SELECT sync_start_date,page_token,initial_history_id,sync_status FROM gmail_sync_state WHERE gmail_account_id=?1", [account_id], |r| Ok((r.get::<_,Option<String>>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,String>(3)?))).optional().map_err(|e| e.to_string())?;
        (
            account.email_address,
            token_ref,
            state.as_ref().and_then(|s| s.0.clone()),
            state.as_ref().and_then(|s| s.1.clone()),
            state.as_ref().and_then(|s| s.2.clone()),
            state.map(|s| s.3).unwrap_or_default(),
        )
    };
    let resume =
        saved_start.as_deref() == Some(start_date.as_str()) && saved_status == "initial_sync";
    let token = refresh_access_token(&token_ref).await.map_err(|e| {
        if e.contains("GMAIL_TOKEN_REVOKED") { let _ = database.connection.lock().map(|c| c.execute("UPDATE gmail_accounts SET account_status='authentication_required' WHERE id=?1", [account_id])); }
        e
    })?;
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Cannot initialize Gmail HTTP client: {e}"))?;
    let profile: Profile = api_get(&client, &token.value, &format!("{GMAIL_API}/profile"), &[])
        .await?
        .json()
        .await
        .map_err(|e| format!("Invalid Gmail profile: {e}"))?;
    let history_id = if resume {
        saved_history.unwrap_or(profile.history_id)
    } else {
        profile.history_id
    };
    let page_token = if resume { saved_page } else { None };
    update_state(
        &database,
        account_id,
        "initial_sync",
        &start_date,
        page_token.as_deref(),
        &history_id,
        None,
    )?;
    {
        let connection = database.connection.lock().map_err(|e| e.to_string())?;
        connection.execute("UPDATE gmail_accounts SET sync_start_date=?1,updated_at=CURRENT_TIMESTAMP WHERE id=?2",rusqlite::params![start_date,account_id]).map_err(|e| e.to_string())?;
    }
    let _ = app.emit(
        "gmail-sync-progress",
        serde_json::json!({"accountId":account_id,"status":"initial_sync","processedMessages":0}),
    );
    let query = format!(
        "after:{} from:no-reply@flip.id",
        start_date.replace('-', "/")
    );
    let mut page = page_token;
    let mut processed = 0u64;
    let mut skipped = 0u64;
    let mut attachment_count = 0u64;
    loop {
        let mut params = vec![("q", query.clone()), ("maxResults", "100".into())];
        if let Some(p) = &page {
            params.push(("pageToken", p.clone()));
        }
        let list: ListResponse = api_get(
            &client,
            &token.value,
            &format!("{GMAIL_API}/messages"),
            &params,
        )
        .await?
        .json()
        .await
        .map_err(|e| format!("Invalid Gmail list response: {e}"))?;
        for item in list.messages.unwrap_or_default() {
            let url = format!("{GMAIL_API}/messages/{}", item.id);
            let response =
                api_get(&client, &token.value, &url, &[("format", "full".into())]).await?;
            let raw: Value = response
                .json()
                .await
                .map_err(|e| format!("Invalid Gmail message JSON: {e}"))?;
            let message: Message = serde_json::from_value(raw.clone())
                .map_err(|e| format!("Invalid Gmail message structure: {e}"))?;
            let root = match &message.payload {
                Some(v) => v,
                None => {
                    skipped += 1;
                    continue;
                }
            };
            let headers = root.headers.as_deref().unwrap_or(&[]);
            let sender = header(headers, "From");
            let subject = header(headers, "Subject");
            let mut parts = Vec::new();
            collect_parts(root, &mut parts);
            let mut attachment_rows = Vec::new();
            for (index, part) in parts.iter().enumerate() {
                if let Some((filename, format, sha, bytes)) =
                    fetch_attachment(&client, &token.value, &message.id, part).await?
                {
                    let account_dir: PathBuf = app
                        .path()
                        .app_data_dir()
                        .map_err(|e| e.to_string())?
                        .join("attachments")
                        .join(account_id.to_string())
                        .join(&message.id);
                    ensure_dir(&account_dir)?;
                    let stored_name = format!("{}-{}.{}", index, safe_filename(&filename), format);
                    let final_path = account_dir.join(stored_name);
                    if !final_path.exists() {
                        let temp_path = final_path.with_extension(format!("{}.tmp", format));
                        fs::write(&temp_path, &bytes)
                            .map_err(|e| format!("Cannot write attachment: {e}"))?;
                        fs::rename(&temp_path, &final_path)
                            .map_err(|e| format!("Cannot finalize attachment: {e}"))?;
                    }
                    let part_key = part
                        .body
                        .as_ref()
                        .and_then(|b| b.attachment_id.clone())
                        .unwrap_or_else(|| format!("part-{index}"));
                    attachment_rows.push((
                        part_key,
                        part.body
                            .as_ref()
                            .and_then(|b| b.attachment_id.clone())
                            .unwrap_or_default(),
                        filename,
                        part.mime_type.clone().unwrap_or_default(),
                        format,
                        sha,
                        final_path.to_string_lossy().to_string(),
                    ));
                    attachment_count += 1;
                }
            }
            let raw_text = raw.to_string();
            if save_message_and_attachments(
                &database,
                account_id,
                &message,
                &raw_text,
                sender,
                subject,
                &attachment_rows,
            )? {
                processed += 1;
            } else {
                skipped += 1;
            }
            let _ = app.emit("gmail-sync-progress",serde_json::json!({"accountId":account_id,"status":"initial_sync","processedMessages":processed,"attachmentCount":attachment_count,"estimatedTotal":list.result_size_estimate}));
        }
        page = list.next_page_token;
        update_state(
            &database,
            account_id,
            "initial_sync",
            &start_date,
            page.as_deref(),
            &history_id,
            None,
        )?;
        if page.is_none() {
            break;
        }
    }
    update_state(
        &database,
        account_id,
        "completed",
        &start_date,
        None,
        &history_id,
        None,
    )?;
    {
        let connection = database.connection.lock().map_err(|e| e.to_string())?;
        connection.execute("UPDATE gmail_accounts SET history_id=?1,last_sync_at=CURRENT_TIMESTAMP,updated_at=CURRENT_TIMESTAMP WHERE id=?2",rusqlite::params![history_id,account_id]).map_err(|e|e.to_string())?;
        let _ = &email;
    }
    let _=app.emit("gmail-sync-progress",serde_json::json!({"accountId":account_id,"status":"completed","processedMessages":processed,"attachmentCount":attachment_count}));
    Ok(SyncSummary {
        account_id,
        processed_messages: processed,
        skipped_messages: skipped,
        attachment_count,
        completed: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_png_from_magic_bytes_not_filename_or_mime() {
        assert_eq!(detect_image(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(detect_image(b"\xFF\xD8\xFF\xE0rest"), Some("jpeg"));
        assert_eq!(detect_image(b"not-an-image"), None);
    }

    #[test]
    fn decodes_gmail_base64url_with_or_without_padding() {
        let encoded = URL_SAFE_NO_PAD.encode(b"flip-v2");
        assert_eq!(decode_base64url(&encoded).unwrap(), b"flip-v2");
        let padded = URL_SAFE.encode(b"flip-v2");
        assert_eq!(decode_base64url(&padded).unwrap(), b"flip-v2");
    }
}
