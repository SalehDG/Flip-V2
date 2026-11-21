use crate::{
    databases::{Database, repositories::gmail_account_repository::GmailAccountRepository},
    gmail_sync::{SyncCoordinator, token_manager::refresh_access_token},
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
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State};

const GMAIL_API: &str = "https://gmail.googleapis.com/gmail/v1/users/me";
const MAX_API_RETRIES: u32 = 4;

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
struct MessageRef {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryListResponse {
    history: Option<Vec<HistoryRecord>>,
    next_page_token: Option<String>,
    history_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryRecord {
    messages_added: Option<Vec<HistoryMessageAdded>>,
}

#[derive(Debug, Deserialize)]
struct HistoryMessageAdded {
    message: MessageRef,
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

struct AccountSyncInfo {
    token_reference: String,
    start_date: Option<String>,
    account_history_id: Option<String>,
    sync_status: String,
    sync_start_date: Option<String>,
    page_token: Option<String>,
    initial_history_id: Option<String>,
    latest_history_id: Option<String>,
}

async fn api_get(
    client: &Client,
    token: &str,
    url: &str,
    query: &[(&str, String)],
) -> Result<reqwest::Response, String> {
    let mut attempt = 0;

    loop {
        let response = match client.get(url).bearer_auth(token).query(query).send().await {
            Ok(response) => response,
            Err(_error) if attempt < MAX_API_RETRIES => {
                let delay = 1u64 << attempt;
                tokio::time::sleep(Duration::from_secs(delay)).await;
                attempt += 1;
                continue;
            }
            Err(error) => {
                return Err(format!(
                    "Gmail network request failed after retries: {error}"
                ));
            }
        };

        let status = response.status();

        if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            let body = response.text().await.unwrap_or_default();
            if attempt >= MAX_API_RETRIES {
                return Err(format!(
                    "Gmail API temporary failure after retries ({status}): {body}"
                ));
            }
            tokio::time::sleep(Duration::from_secs(1u64 << attempt)).await;
            attempt += 1;
            continue;
        }

        if status == StatusCode::FORBIDDEN {
            let body = response.text().await.unwrap_or_default();
            let quota_error = body.contains("rateLimitExceeded")
                || body.contains("userRateLimitExceeded")
                || body.contains("quotaExceeded");

            if quota_error && attempt < MAX_API_RETRIES {
                tokio::time::sleep(Duration::from_secs(1u64 << attempt)).await;
                attempt += 1;
                continue;
            }

            return Err(format!("Gmail API returned {status}: {body}"));
        }

        if status == StatusCode::UNAUTHORIZED {
            return Err(
                "GMAIL_ACCESS_TOKEN_EXPIRED: Gmail rejected the access token; refresh is required."
                    .into(),
            );
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Gmail API returned {status}: {body}"));
        }

        return Ok(response);
    }
}

async fn api_get_with_refresh(
    app: &AppHandle,
    client: &Client,
    token_reference: &str,
    token: &mut String,
    url: &str,
    query: &[(&str, String)],
) -> Result<reqwest::Response, String> {
    match api_get(client, token, url, query).await {
        Ok(response) => Ok(response),
        Err(error) if error.starts_with("GMAIL_ACCESS_TOKEN_EXPIRED:") => {
            let refreshed = refresh_access_token(app, token_reference).await?;
            *token = refreshed.value;
            api_get(client, token, url, query).await
        }
        Err(error) => Err(error),
    }
}

fn decode_base64url(value: &str) -> Result<Vec<u8>, String> {
    URL_SAFE_NO_PAD
        .decode(value)
        .or_else(|_| URL_SAFE.decode(value))
        .map_err(|error| format!("Invalid Gmail base64url payload: {error}"))
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
        .find(|item| item.name.eq_ignore_ascii_case(name))
        .map(|item| item.value.as_str())
}

fn is_flip_sender(sender: Option<&str>) -> bool {
    let sender = sender.unwrap_or_default().trim().to_ascii_lowercase();
    sender == "no-reply@flip.id" || sender.contains("<no-reply@flip.id>")
}

fn collect_parts<'a>(part: &'a MimePart, output: &mut Vec<&'a MimePart>) {
    if let Some(parts) = &part.parts {
        for child in parts {
            collect_parts(child, output);
        }
    }

    if part
        .filename
        .as_deref()
        .is_some_and(|name| !name.is_empty())
        || part
            .body
            .as_ref()
            .and_then(|body| body.attachment_id.as_ref())
            .is_some()
    {
        output.push(part);
    }
}

fn safe_filename(name: &str) -> String {
    let basename = name
        .rsplit(|character| character == '/' || character == '\\')
        .next()
        .unwrap_or(name);
    let stem = basename
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(basename);

    let clean: String = stem
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .take(100)
        .collect();

    if clean.is_empty() {
        "attachment".into()
    } else {
        clean
    }
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| format!("Cannot create attachment directory: {error}"))
}

async fn fetch_attachment(
    app: &AppHandle,
    client: &Client,
    token_reference: &str,
    token: &mut String,
    message_id: &str,
    part: &MimePart,
) -> Result<Option<(String, String, String, Vec<u8>)>, String> {
    let body = match &part.body {
        Some(body) => body,
        None => return Ok(None),
    };

    let data = if let Some(data) = &body.data {
        data.clone()
    } else if let Some(attachment_id) = &body.attachment_id {
        let url = format!("{GMAIL_API}/messages/{message_id}/attachments/{attachment_id}");
        let response = api_get_with_refresh(app, client, token_reference, token, &url, &[]).await?;
        let value: Value = response
            .json()
            .await
            .map_err(|error| format!("Invalid attachment response: {error}"))?;

        value
            .get("data")
            .and_then(Value::as_str)
            .ok_or("Attachment response has no data")?
            .to_string()
    } else {
        return Ok(None);
    };

    let bytes = decode_base64url(&data)?;

    // The accepted format is determined only from file signatures (magic bytes).
    // Filename extension and declared MIME type are deliberately not trusted.
    let format = match detect_image(&bytes) {
        Some(format) => format,
        None => return Ok(None),
    };

    let digest = Sha256::digest(&bytes);
    let sha256 = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    let filename = part
        .filename
        .clone()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| format!("attachment.{format}"));

    Ok(Some((filename, format.to_string(), sha256, bytes)))
}

fn message_exists(database: &Database, account_id: i64, message_id: &str) -> Result<bool, String> {
    let connection = database
        .connection
        .lock()
        .map_err(|error| format!("Database lock failed: {error}"))?;

    connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM gmail_messages
                WHERE gmail_account_id = ?1 AND gmail_message_id = ?2
            )",
            params![account_id, message_id],
            |row| row.get(0),
        )
        .map_err(|error| format!("Failed to check duplicate Gmail message: {error}"))
}

fn save_message_and_attachments(
    database: &Database,
    account_id: i64,
    message: &Message,
    payload_json: &str,
    sender: Option<&str>,
    subject: Option<&str>,
    attachment_rows: &[(String, String, String, String, String, String, String)],
) -> Result<bool, String> {
    let connection = database
        .connection
        .lock()
        .map_err(|error| format!("Database lock failed: {error}"))?;

    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| format!("Failed to start message database transaction: {error}"))?;

    let exists: bool = transaction
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM gmail_messages
                WHERE gmail_account_id = ?1 AND gmail_message_id = ?2
            )",
            params![account_id, message.id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;

    if exists {
        return Ok(false);
    }

    transaction
        .execute(
            "INSERT INTO gmail_messages (
                gmail_account_id, gmail_message_id, gmail_thread_id, history_id,
                internal_date, sender, subject, snippet, payload_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                account_id,
                message.id,
                message.thread_id,
                message.history_id,
                message.internal_date,
                sender,
                subject,
                message.snippet,
                payload_json
            ],
        )
        .map_err(|error| format!("Failed to save Gmail message: {error}"))?;

    transaction
        .execute(
            "INSERT OR IGNORE INTO transactions (
                gmail_account_id, gmail_message_id, status
             ) VALUES (?1, ?2, 'Detected')",
            params![account_id, message.id],
        )
        .map_err(|error| format!("Failed to create transaction record: {error}"))?;

    for (part_key, attachment_id, filename, mime, format, sha256, path) in attachment_rows {
        let size_u64 = fs::metadata(path)
            .map(|metadata| metadata.len())
            .map_err(|error| format!("Cannot inspect saved attachment {path}: {error}"))?;

        let size = i64::try_from(size_u64)
            .map_err(|_| format!("Attachment is too large for SQLite INTEGER: {path}"))?;

        transaction
            .execute(
                "INSERT OR IGNORE INTO gmail_attachments (
                    gmail_account_id, gmail_message_id, part_key, attachment_id,
                    filename, declared_mime_type, detected_format, file_size,
                    sha256, local_path
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    account_id,
                    message.id,
                    part_key,
                    attachment_id,
                    filename,
                    mime,
                    format,
                    size,
                    sha256,
                    path
                ],
            )
            .map_err(|error| format!("Failed to save attachment metadata: {error}"))?;
    }

    transaction
        .commit()
        .map_err(|error| format!("Failed to commit Gmail message: {error}"))?;

    Ok(true)
}

async fn retrieve_and_store_message(
    app: &AppHandle,
    database: &Database,
    client: &Client,
    token_reference: &str,
    token: &mut String,
    account_id: i64,
    message_id: &str,
) -> Result<(bool, u64), String> {
    // Avoid spending Gmail API quota downloading messages already committed locally.
    if message_exists(database, account_id, message_id)? {
        return Ok((false, 0));
    }

    let url = format!("{GMAIL_API}/messages/{message_id}");
    let response = api_get_with_refresh(
        app,
        client,
        token_reference,
        token,
        &url,
        &[("format", "full".into())],
    )
    .await?;

    let raw: Value = response
        .json()
        .await
        .map_err(|error| format!("Invalid Gmail message JSON: {error}"))?;

    let message: Message = serde_json::from_value(raw.clone())
        .map_err(|error| format!("Invalid Gmail message structure: {error}"))?;

    if message.id != message_id {
        return Err(format!(
            "Gmail returned message {} while {} was requested",
            message.id, message_id
        ));
    }

    let root = match &message.payload {
        Some(payload) => payload,
        None => return Err(format!("Gmail message {} has no MIME payload", message.id)),
    };

    let headers = root.headers.as_deref().unwrap_or(&[]);
    let sender = header(headers, "From");

    // History API reports all newly added Gmail messages, not only Flip emails.
    // Apply the same sender constraint used by initial synchronization.
    if !is_flip_sender(sender) {
        return Ok((false, 0));
    }

    let subject = header(headers, "Subject");
    let mut parts = Vec::new();
    collect_parts(root, &mut parts);

    let account_dir: PathBuf = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Cannot resolve application data directory: {error}"))?
        .join("attachments")
        .join(account_id.to_string())
        .join(&message.id);

    let mut attachment_rows = Vec::new();
    let mut attachment_count = 0u64;

    for (index, part) in parts.iter().enumerate() {
        if let Some((filename, format, sha256, bytes)) =
            fetch_attachment(app, client, token_reference, token, &message.id, part).await?
        {
            ensure_dir(&account_dir)?;

            let stored_name = format!("{index}-{}.{}", safe_filename(&filename), format);
            let final_path = account_dir.join(stored_name);

            let existing_matches = if final_path.is_file() {
                fs::read(&final_path)
                    .map(|existing| {
                        let digest = Sha256::digest(existing);
                        digest
                            .iter()
                            .map(|byte| format!("{byte:02x}"))
                            .collect::<String>()
                            == sha256
                    })
                    .unwrap_or(false)
            } else {
                false
            };

            if !existing_matches {
                let temp_path = final_path.with_extension(format!("{format}.tmp"));
                fs::write(&temp_path, &bytes)
                    .map_err(|error| format!("Cannot write attachment: {error}"))?;
                if final_path.exists() {
                    fs::remove_file(&final_path).map_err(|error| {
                        format!("Cannot replace incomplete attachment: {error}")
                    })?;
                }
                fs::rename(&temp_path, &final_path)
                    .map_err(|error| format!("Cannot finalize attachment: {error}"))?;
            }

            let part_key = part
                .body
                .as_ref()
                .and_then(|body| body.attachment_id.clone())
                .unwrap_or_else(|| format!("part-{index}"));

            attachment_rows.push((
                part_key,
                part.body
                    .as_ref()
                    .and_then(|body| body.attachment_id.clone())
                    .unwrap_or_default(),
                filename,
                part.mime_type.clone().unwrap_or_default(),
                format,
                sha256,
                final_path.to_string_lossy().to_string(),
            ));
            attachment_count += 1;
        }
    }

    let raw_text = raw.to_string();
    let inserted = save_message_and_attachments(
        database,
        account_id,
        &message,
        &raw_text,
        sender,
        subject,
        &attachment_rows,
    )?;

    Ok((inserted, if inserted { attachment_count } else { 0 }))
}

fn update_state(
    database: &Database,
    account_id: i64,
    status: &str,
    start_date: &str,
    page_token: Option<&str>,
    initial_history_id: &str,
    latest_history_id: Option<&str>,
    error: Option<&str>,
) -> Result<(), String> {
    let connection = database
        .connection
        .lock()
        .map_err(|error| format!("Database lock failed: {error}"))?;

    connection
        .execute(
            "INSERT INTO gmail_sync_state (
                gmail_account_id, sync_status, sync_start_date, page_token,
                initial_history_id, latest_history_id, last_error, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CURRENT_TIMESTAMP)
             ON CONFLICT(gmail_account_id) DO UPDATE SET
                sync_status = excluded.sync_status,
                sync_start_date = excluded.sync_start_date,
                page_token = excluded.page_token,
                initial_history_id = excluded.initial_history_id,
                latest_history_id = excluded.latest_history_id,
                last_error = excluded.last_error,
                updated_at = CURRENT_TIMESTAMP",
            params![
                account_id,
                status,
                start_date,
                page_token,
                initial_history_id,
                latest_history_id,
                error
            ],
        )
        .map_err(|error| format!("Failed to update Gmail sync checkpoint: {error}"))?;

    Ok(())
}

fn mark_sync_failed(
    database: &Database,
    account_id: i64,
    error: &str,
    authentication_required: bool,
) -> Result<(), String> {
    let connection = database
        .connection
        .lock()
        .map_err(|lock_error| format!("Database lock failed: {lock_error}"))?;

    let status = if authentication_required {
        "authentication_required"
    } else {
        "failed"
    };

    connection
        .execute(
            "INSERT INTO gmail_sync_state (
                gmail_account_id, sync_status, last_error, updated_at
             ) VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
             ON CONFLICT(gmail_account_id) DO UPDATE SET
                sync_status = excluded.sync_status,
                last_error = excluded.last_error,
                updated_at = CURRENT_TIMESTAMP",
            params![account_id, status, error],
        )
        .map_err(|db_error| format!("Failed to persist sync error: {db_error}"))?;

    if authentication_required {
        connection
            .execute(
                "UPDATE gmail_accounts
                 SET account_status = 'authentication_required',
                     updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?1",
                [account_id],
            )
            .map_err(|db_error| {
                format!("Failed to update Gmail authentication status: {db_error}")
            })?;
    }

    Ok(())
}

fn load_account_sync_info(database: &Database, account_id: i64) -> Result<AccountSyncInfo, String> {
    let connection = database
        .connection
        .lock()
        .map_err(|error| format!("Database lock failed: {error}"))?;

    let account = GmailAccountRepository::find_by_id(&connection, account_id)
        .map_err(|error| format!("Failed to load Gmail account: {error}"))?
        .ok_or("Gmail account not found")?;

    let token_reference = GmailAccountRepository::find_token_reference(&connection, account_id)
        .map_err(|error| format!("Failed to read Gmail token reference: {error}"))?
        .ok_or("Gmail account is disconnected")?;

    let state = connection
        .query_row(
            "SELECT sync_status, sync_start_date, page_token,
                    initial_history_id, latest_history_id
             FROM gmail_sync_state
             WHERE gmail_account_id = ?1",
            [account_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("Failed to load Gmail sync state: {error}"))?
        .unwrap_or_else(|| ("idle".into(), None, None, None, None));

    Ok(AccountSyncInfo {
        token_reference,
        start_date: account.sync_start_date,
        account_history_id: account.history_id,
        sync_status: state.0,
        sync_start_date: state.1,
        page_token: state.2,
        initial_history_id: state.3,
        latest_history_id: state.4,
    })
}

fn update_account_history(
    database: &Database,
    account_id: i64,
    history_id: &str,
) -> Result<(), String> {
    let connection = database
        .connection
        .lock()
        .map_err(|error| format!("Database lock failed: {error}"))?;

    connection
        .execute(
            "UPDATE gmail_accounts
             SET history_id = ?1,
                 last_sync_at = CURRENT_TIMESTAMP,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?2",
            params![history_id, account_id],
        )
        .map_err(|error| format!("Failed to update Gmail History ID: {error}"))?;

    Ok(())
}

async fn get_profile(
    app: &AppHandle,
    client: &Client,
    token_reference: &str,
    token: &mut String,
) -> Result<Profile, String> {
    let response = api_get_with_refresh(
        app,
        client,
        token_reference,
        token,
        &format!("{GMAIL_API}/profile"),
        &[],
    )
    .await?;

    response
        .json()
        .await
        .map_err(|error| format!("Invalid Gmail profile response: {error}"))
}

fn build_http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(45))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| format!("Cannot initialize Gmail HTTP client: {error}"))
}

fn emit_progress(
    app: &AppHandle,
    account_id: i64,
    status: &str,
    processed: u64,
    attachments: u64,
    estimated_total: Option<u64>,
) {
    let _ = app.emit(
        "gmail-sync-progress",
        serde_json::json!({
            "accountId": account_id,
            "status": status,
            "processedMessages": processed,
            "attachmentCount": attachments,
            "estimatedTotal": estimated_total
        }),
    );
}

async fn run_initial_sync(
    app: &AppHandle,
    database: &Database,
    account_id: i64,
    start_date: &str,
) -> Result<SyncSummary, String> {
    if start_date.len() != 10
        || start_date.chars().nth(4) != Some('-')
        || start_date.chars().nth(7) != Some('-')
    {
        return Err("Tanggal awal harus berformat YYYY-MM-DD".into());
    }

    let info = load_account_sync_info(database, account_id)?;

    let resume = info.sync_start_date.as_deref() == Some(start_date)
        && matches!(
            info.sync_status.as_str(),
            "initial_sync" | "failed" | "authentication_required"
        )
        && info.initial_history_id.is_some();

    let mut token = refresh_access_token(app, &info.token_reference)
        .await?
        .value;

    let client = build_http_client()?;

    let history_id = if resume {
        info.initial_history_id
            .clone()
            .expect("resume requires initial history id")
    } else {
        get_profile(app, &client, &info.token_reference, &mut token)
            .await?
            .history_id
    };

    let mut page = if resume {
        info.page_token.clone()
    } else {
        None
    };

    update_state(
        database,
        account_id,
        "initial_sync",
        start_date,
        page.as_deref(),
        &history_id,
        None,
        None,
    )?;

    {
        let connection = database
            .connection
            .lock()
            .map_err(|error| format!("Database lock failed: {error}"))?;
        connection
            .execute(
                "UPDATE gmail_accounts
                 SET sync_start_date = ?1, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?2",
                params![start_date, account_id],
            )
            .map_err(|error| format!("Failed to save sync start date: {error}"))?;
    }

    emit_progress(app, account_id, "initial_sync", 0, 0, None);

    let query = format!(
        "after:{} from:no-reply@flip.id",
        start_date.replace('-', "/")
    );

    let mut processed = 0u64;
    let mut skipped = 0u64;
    let mut attachment_count = 0u64;

    loop {
        let mut query_params = vec![("q", query.clone()), ("maxResults", "100".to_string())];

        if let Some(page_token) = &page {
            query_params.push(("pageToken", page_token.clone()));
        }

        let response = match api_get_with_refresh(
            app,
            &client,
            &info.token_reference,
            &mut token,
            &format!("{GMAIL_API}/messages"),
            &query_params,
        )
        .await
        {
            Ok(response) => response,
            Err(error) if page.is_some() && is_invalid_page_token_error(&error) => {
                // Gmail page tokens can become invalid when the mailbox changes.
                // Restart the date-bounded scan; committed messages are deduplicated.
                page = None;
                update_state(
                    database,
                    account_id,
                    "initial_sync",
                    start_date,
                    None,
                    &history_id,
                    None,
                    None,
                )?;
                continue;
            }
            Err(error) => return Err(error),
        };

        let list: ListResponse = response
            .json()
            .await
            .map_err(|error| format!("Invalid Gmail list response: {error}"))?;

        let estimated_total = list.result_size_estimate;

        for message_ref in list.messages.unwrap_or_default() {
            let (inserted, attachments) = retrieve_and_store_message(
                app,
                database,
                &client,
                &info.token_reference,
                &mut token,
                account_id,
                &message_ref.id,
            )
            .await?;

            if inserted {
                processed += 1;
                attachment_count += attachments;
            } else {
                skipped += 1;
            }

            emit_progress(
                app,
                account_id,
                "initial_sync",
                processed,
                attachment_count,
                estimated_total,
            );
        }

        page = list.next_page_token;

        // The checkpoint is advanced only after every message in this page is committed.
        // Replaying a page after a crash is safe because Gmail Message IDs are idempotent.
        update_state(
            database,
            account_id,
            "initial_sync",
            start_date,
            page.as_deref(),
            &history_id,
            None,
            None,
        )?;

        if page.is_none() {
            break;
        }
    }

    update_account_history(database, account_id, &history_id)?;
    update_state(
        database,
        account_id,
        "completed",
        start_date,
        None,
        &history_id,
        Some(&history_id),
        None,
    )?;

    emit_progress(
        app,
        account_id,
        "completed",
        processed,
        attachment_count,
        Some(processed),
    );

    Ok(SyncSummary {
        account_id,
        processed_messages: processed,
        skipped_messages: skipped,
        attachment_count,
        completed: true,
    })
}

fn is_expired_history_error(error: &str) -> bool {
    error.contains("Gmail API returned 404")
        || error.contains("startHistoryId")
        || error.contains("historyId")
}

fn is_invalid_page_token_error(error: &str) -> bool {
    let normalized = error.to_ascii_lowercase();
    normalized.contains("pagetoken") || normalized.contains("page token")
}

async fn run_incremental_sync(
    app: &AppHandle,
    database: &Database,
    account_id: i64,
) -> Result<SyncSummary, String> {
    let info = load_account_sync_info(database, account_id)?;

    let start_history_id = info
        .account_history_id
        .clone()
        .ok_or("Initial Gmail synchronization must complete before incremental sync.")?;

    let start_date = info
        .start_date
        .clone()
        .or(info.sync_start_date.clone())
        .ok_or("Tanggal awal sinkronisasi belum ditentukan.")?;

    let resume = info.initial_history_id.as_deref() == Some(start_history_id.as_str())
        && matches!(
            info.sync_status.as_str(),
            "incremental_sync" | "failed" | "authentication_required"
        );

    let mut token = refresh_access_token(app, &info.token_reference)
        .await?
        .value;
    let client = build_http_client()?;

    let mut page = if resume {
        info.page_token.clone()
    } else {
        None
    };
    let mut latest_history_id = if resume {
        info.latest_history_id.clone()
    } else {
        None
    };

    update_state(
        database,
        account_id,
        "incremental_sync",
        &start_date,
        page.as_deref(),
        &start_history_id,
        latest_history_id.as_deref(),
        None,
    )?;

    let mut processed = 0u64;
    let mut skipped = 0u64;
    let mut attachment_count = 0u64;
    let mut seen_message_ids = HashSet::new();

    loop {
        let mut query_params = vec![
            ("startHistoryId", start_history_id.clone()),
            ("historyTypes", "messageAdded".to_string()),
            ("maxResults", "100".to_string()),
        ];

        if let Some(page_token) = &page {
            query_params.push(("pageToken", page_token.clone()));
        }

        let response = match api_get_with_refresh(
            app,
            &client,
            &info.token_reference,
            &mut token,
            &format!("{GMAIL_API}/history"),
            &query_params,
        )
        .await
        {
            Ok(response) => response,
            Err(error) if page.is_some() && is_invalid_page_token_error(&error) => {
                // Restart this history window from its original History ID.
                // Message-level deduplication makes replay safe.
                page = None;
                latest_history_id = None;
                update_state(
                    database,
                    account_id,
                    "incremental_sync",
                    &start_date,
                    None,
                    &start_history_id,
                    None,
                    None,
                )?;
                continue;
            }
            Err(error) if is_expired_history_error(&error) => {
                // Gmail History IDs are short-lived. Re-scan from the configured
                // start date; message-level deduplication prevents duplicates.
                emit_progress(app, account_id, "history_expired_fallback", 0, 0, None);
                return run_initial_sync(app, database, account_id, &start_date).await;
            }
            Err(error) => return Err(error),
        };

        let history: HistoryListResponse = response
            .json()
            .await
            .map_err(|error| format!("Invalid Gmail history response: {error}"))?;

        if let Some(current_history_id) = history.history_id {
            latest_history_id = Some(current_history_id);
        }

        for record in history.history.unwrap_or_default() {
            for added in record.messages_added.unwrap_or_default() {
                if !seen_message_ids.insert(added.message.id.clone()) {
                    continue;
                }

                let (inserted, attachments) = retrieve_and_store_message(
                    app,
                    database,
                    &client,
                    &info.token_reference,
                    &mut token,
                    account_id,
                    &added.message.id,
                )
                .await?;

                if inserted {
                    processed += 1;
                    attachment_count += attachments;
                } else {
                    skipped += 1;
                }

                emit_progress(
                    app,
                    account_id,
                    "incremental_sync",
                    processed,
                    attachment_count,
                    None,
                );
            }
        }

        page = history.next_page_token;

        update_state(
            database,
            account_id,
            "incremental_sync",
            &start_date,
            page.as_deref(),
            &start_history_id,
            latest_history_id.as_deref(),
            None,
        )?;

        if page.is_none() {
            break;
        }
    }

    let latest_history_id = match latest_history_id {
        Some(history_id) => history_id,
        None => {
            get_profile(app, &client, &info.token_reference, &mut token)
                .await?
                .history_id
        }
    };

    update_account_history(database, account_id, &latest_history_id)?;
    update_state(
        database,
        account_id,
        "completed",
        &start_date,
        None,
        &latest_history_id,
        Some(&latest_history_id),
        None,
    )?;

    emit_progress(
        app,
        account_id,
        "completed",
        processed,
        attachment_count,
        Some(processed),
    );

    Ok(SyncSummary {
        account_id,
        processed_messages: processed,
        skipped_messages: skipped,
        attachment_count,
        completed: true,
    })
}

fn persist_command_error(database: &Database, account_id: i64, error: &str) {
    let authentication_required = error.contains("GMAIL_TOKEN_REVOKED")
        || error.contains("GMAIL_ACCESS_TOKEN_EXPIRED")
        || error.contains("refresh token is missing");

    let _ = mark_sync_failed(database, account_id, error, authentication_required);
}

#[tauri::command]
pub async fn get_gmail_sync_state(
    database: State<'_, Database>,
    account_id: i64,
) -> Result<SyncState, String> {
    let connection = database
        .connection
        .lock()
        .map_err(|error| format!("Database lock failed: {error}"))?;

    let state = connection
        .query_row(
            "SELECT sync_status, sync_start_date, page_token, last_error
             FROM gmail_sync_state
             WHERE gmail_account_id = ?1",
            [account_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("Failed to read Gmail sync state: {error}"))?;

    let (status, start_date, page_token, last_error) =
        state.unwrap_or(("idle".into(), None, None, None));

    let processed_messages_i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM gmail_messages WHERE gmail_account_id = ?1",
            [account_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| format!("Failed to count Gmail messages: {error}"))?;

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
    coordinator: State<'_, SyncCoordinator>,
    account_id: i64,
    start_date: String,
) -> Result<SyncSummary, String> {
    let _permit = coordinator.try_acquire(account_id)?;
    let result = run_initial_sync(&app, &database, account_id, &start_date).await;

    if let Err(error) = &result {
        persist_command_error(&database, account_id, error);
    }

    result
}

#[tauri::command]
pub async fn start_incremental_gmail_sync(
    app: AppHandle,
    database: State<'_, Database>,
    coordinator: State<'_, SyncCoordinator>,
    account_id: i64,
) -> Result<SyncSummary, String> {
    let _permit = coordinator.try_acquire(account_id)?;
    let result = run_incremental_sync(&app, &database, account_id).await;

    if let Err(error) = &result {
        persist_command_error(&database, account_id, error);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_png_and_jpeg_from_magic_bytes_not_filename_or_mime() {
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

    #[test]
    fn parses_history_messages_added_and_next_page_token() {
        let response: HistoryListResponse = serde_json::from_str(
            r#"{
                "historyId": "9002",
                "nextPageToken": "next-page",
                "history": [
                    {
                        "id": "9001",
                        "messagesAdded": [
                            {
                                "message": {
                                    "id": "message-123",
                                    "threadId": "thread-123"
                                }
                            }
                        ]
                    }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(response.history_id.as_deref(), Some("9002"));
        assert_eq!(response.next_page_token.as_deref(), Some("next-page"));
        let history = response.history.unwrap();
        let added = history[0].messages_added.as_ref().unwrap();
        assert_eq!(added[0].message.id, "message-123");
    }

    #[test]
    fn recognizes_expired_pagination_tokens() {
        assert!(is_invalid_page_token_error(
            "Gmail API returned 400: Invalid pageToken"
        ));
        assert!(!is_invalid_page_token_error(
            "Gmail API returned 500: temporary error"
        ));
    }

    #[test]
    fn accepts_only_the_configured_flip_sender() {
        assert!(is_flip_sender(Some("Flip <no-reply@flip.id>")));
        assert!(is_flip_sender(Some("no-reply@flip.id")));
        assert!(!is_flip_sender(Some("someone@example.com")));
        assert!(!is_flip_sender(None));
    }

    #[test]
    fn detects_expired_history_response() {
        assert!(is_expired_history_error(
            "Gmail API returned 404: startHistoryId is too old"
        ));
        assert!(!is_expired_history_error(
            "Gmail API returned 500: temporary server error"
        ));
    }

    #[test]
    fn sanitizes_attachment_names() {
        assert_eq!(safe_filename("../../receipt #1.png"), "receipt__1");
        assert_eq!(safe_filename(""), "attachment");
    }
}
