use crate::{
    databases::{Database, repositories::gmail_account_repository::GmailAccountRepository},
    gmail_sync::token_manager::refresh_access_token,
};
use base64::{engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD}, Engine};
use serde_json::{json, Value};
use std::{fs, path::PathBuf, time::Duration};
use tauri::{AppHandle, Manager, State};


fn mask_email(value: &str) -> String {
    value.split_whitespace().map(|part| {
        if let Some((name, address)) = part.split_once('<') {
            let addr = address.trim_end_matches('>');
            if addr.contains('@') { format!("{}<{}>", name, "[REDACTED_EMAIL]") } else { part.to_string() }
        } else if part.contains('@') {
            "[REDACTED_EMAIL]".to_string()
        } else { part.to_string() }
    }).collect::<Vec<_>>().join(" ")
}


fn decode_body(data: &str) -> String {
    URL_SAFE.decode(data).or_else(|_| URL_SAFE_NO_PAD.decode(data)).map(|bytes| String::from_utf8_lossy(&bytes).to_string()).unwrap_or_else(|_| "[BODY_DECODE_FAILED]".into())
}


fn sanitize_part(part: &Value) -> Value {
    let mut output = json!({
        "partId": part.get("partId"),
        "mimeType": part.get("mimeType"),
        "filename": part.get("filename"),
        "headers": part.get("headers").and_then(Value::as_array).map(|headers| headers.iter().map(|h| {
            let name = h.get("name").and_then(Value::as_str).unwrap_or("");
            let value = h.get("value").and_then(Value::as_str).unwrap_or("");
            if ["from", "to", "cc", "bcc", "reply-to"].iter().any(|s| name.eq_ignore_ascii_case(s)) {
                json!({"name": name, "value": mask_email(value)})
            } else { json!({"name": name, "value": value}) }
        }).collect::<Vec<_>>()).unwrap_or_default(),
        "body": part.get("body").map(|body| {
            let size = body.get("size").and_then(Value::as_u64);
            let attachment_id = body.get("attachmentId");
            let data_text = if part.get("mimeType").and_then(Value::as_str).unwrap_or("").starts_with("text/") {
                body.get("data").and_then(Value::as_str).map(decode_body)
            } else { None };
            json!({"size": size, "attachmentId": attachment_id, "decodedText": data_text})
        }),
        "parts": part.get("parts").and_then(Value::as_array).map(|parts| parts.iter().map(sanitize_part).collect::<Vec<_>>()).unwrap_or_default()
    });
    // Gmail attachment identifiers are not credentials, but are retained only for this diagnostic.
    output
}


#[tauri::command]
pub async fn export_gmail_message_diagnostic(
    app: AppHandle,
    database: State<'_, Database>,
    account_id: i64,
    message_id: String,
) -> Result<String, String> {
    if message_id.trim().is_empty() { return Err("Message ID wajib diisi.".into()); }
    let token_reference = {
        let connection = database.connection.lock().map_err(|e| format!("Database lock gagal: {e}"))?;
        GmailAccountRepository::find_token_reference(&connection, account_id)
            .map_err(|e| format!("Gagal membaca akun: {e}"))?
            .ok_or_else(|| "Akun Gmail tidak ditemukan atau token reference kosong.".to_string())?
    };
    let token = refresh_access_token(&app, &token_reference).await?;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().map_err(|e| e.to_string())?;
    let response = client.get(format!("https://gmail.googleapis.com/gmail/v1/users/me/messages/{message_id}"))
        .bearer_auth(token.value).query(&[("format", "full")]).send().await.map_err(|e| format!("Request Gmail gagal: {e}"))?;
    let status = response.status();
    if !status.is_success() { return Err(format!("Gmail API merespons HTTP {status}. Pastikan Message ID benar dan akun memiliki akses.")); }
    let message: Value = response.json().await.map_err(|e| format!("JSON Gmail tidak valid: {e}"))?;
    let payload = message.get("payload").map(sanitize_part).unwrap_or(Value::Null);
    let diagnostic = json!({
        "diagnosticVersion": 1,
        "note": "Headers From/To/Cc/Bcc/Reply-To disamarkan. Body email disertakan; periksa dan sensor informasi sensitif sebelum membagikan.",
        "messageId": message.get("id"),
        "threadId": message.get("threadId"),
        "internalDate": message.get("internalDate"),
        "snippet": message.get("snippet"),
        "payload": payload
    });
    let dir: PathBuf = app.path().app_config_dir().map_err(|e| format!("Folder config tidak tersedia: {e}"))?.join("diagnostics");
    fs::create_dir_all(&dir).map_err(|e| format!("Gagal membuat folder diagnostic: {e}"))?;
    let safe_id: String = message_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(60).collect();
    let path = dir.join(format!("gmail-message-{safe_id}.json"));
    fs::write(&path, serde_json::to_vec_pretty(&diagnostic).map_err(|e| e.to_string())?).map_err(|e| format!("Gagal menulis file diagnostic: {e}"))?;
    Ok(path.display().to_string())
}
