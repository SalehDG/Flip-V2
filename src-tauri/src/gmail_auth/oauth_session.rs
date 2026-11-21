use crate::{
    databases::{
        Database,
        repositories::gmail_account_repository::{GmailAccount, GmailAccountRepository},
    },
    gmail_sync::SyncCoordinator,
};
use rusqlite::OptionalExtension;
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, State};
use uuid::Uuid;

use super::{
    callback_server::CallbackServer,
    oauth_client::{GoogleOAuthConfig, build_authorization_url},
    token_storage,
};

const KEYRING_REFERENCE_PREFIX: &str = "gmail-refresh-token:";

struct PendingOAuth {
    callback_server: CallbackServer,
    expected_state: String,
    pkce_verifier: String,
    redirect_uri: String,
    cancel_signal: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct OAuthSessionStore {
    pending: Mutex<Option<PendingOAuth>>,
    cancel_signal: Mutex<Option<Arc<AtomicBool>>>,
}

#[derive(Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    #[allow(dead_code)]
    expires_in: Option<u64>,
    #[allow(dead_code)]
    token_type: Option<String>,
}

#[derive(Deserialize)]
struct GmailProfile {
    #[serde(rename = "emailAddress")]
    email_address: String,
}

#[tauri::command]
pub fn start_gmail_oauth(
    app: AppHandle,
    store: State<'_, OAuthSessionStore>,
) -> Result<String, String> {
    let mut pending_slot = store
        .pending
        .lock()
        .map_err(|e| format!("OAuth session lock failed: {e}"))?;
    if pending_slot.is_some() {
        return Err("A Gmail authorization is already in progress".into());
    }

    let config = GoogleOAuthConfig::load_for_app(&app)?;
    let callback_server = CallbackServer::bind()?;
    let redirect_uri = callback_server.redirect_uri().to_string();
    let prepared = build_authorization_url(&config, &redirect_uri)?;

    let cancel_signal = Arc::new(AtomicBool::new(false));
    *store
        .cancel_signal
        .lock()
        .map_err(|e| format!("OAuth cancellation lock failed: {e}"))? = Some(cancel_signal.clone());

    *pending_slot = Some(PendingOAuth {
        callback_server,
        expected_state: prepared.state.secret().to_string(),
        pkce_verifier: prepared.pkce_verifier.secret().to_string(),
        redirect_uri,
        cancel_signal,
    });
    Ok(prepared.authorization_url)
}

#[tauri::command]
pub fn cancel_gmail_oauth(store: State<'_, OAuthSessionStore>) -> Result<(), String> {
    let mut pending = store
        .pending
        .lock()
        .map_err(|e| format!("OAuth session lock failed: {e}"))?;
    *pending = None;
    if let Some(signal) = store
        .cancel_signal
        .lock()
        .map_err(|e| format!("OAuth cancellation lock failed: {e}"))?
        .as_ref()
    {
        signal.store(true, Ordering::Release);
    }
    Ok(())
}

#[tauri::command]
pub async fn complete_gmail_oauth(
    app: AppHandle,
    store: State<'_, OAuthSessionStore>,
    database: State<'_, Database>,
) -> Result<GmailAccount, String> {
    let pending = {
        let mut slot = store
            .pending
            .lock()
            .map_err(|e| format!("OAuth session lock failed: {e}"))?;
        slot.take()
            .ok_or_else(|| "No active Gmail authorization session".to_string())?
    };

    let PendingOAuth {
        callback_server,
        expected_state,
        pkce_verifier,
        redirect_uri,
        cancel_signal,
    } = pending;
    let callback = tauri::async_runtime::spawn_blocking(move || {
        callback_server.wait_for_callback(&expected_state, cancel_signal)
    })
    .await
    .map_err(|e| format!("Callback worker failed: {e}"))??;

    let config = GoogleOAuthConfig::load_for_app(&app)?;
    let http = reqwest::Client::new();
    let token_response = http
        .post(&config.installed.token_uri)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", callback.code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("client_id", config.installed.client_id.as_str()),
            ("client_secret", config.installed.client_secret.as_str()),
            ("code_verifier", pkce_verifier.as_str()),
        ])
        .send()
        .await
        .map_err(|e| format!("Failed to contact Google token endpoint: {e}"))?;

    let token_response = token_response
        .error_for_status()
        .map_err(|e| format!("Google rejected the authorization code: {e}"))?;
    let tokens: GoogleTokenResponse = token_response
        .json()
        .await
        .map_err(|e| format!("Invalid Google token response: {e}"))?;

    let refresh_token = tokens.refresh_token
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Google did not return a refresh token. Revoke this app's access in your Google Account and try connecting again.".to_string())?;

    // Gmail readonly scope already permits reading the account's own Gmail profile,
    // so no additional profile scope is requested.
    let profile_response = http
        .get("https://gmail.googleapis.com/gmail/v1/users/me/profile")
        .bearer_auth(&tokens.access_token)
        .send()
        .await
        .map_err(|e| format!("Failed to retrieve Gmail profile: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Gmail profile request failed: {e}"))?;
    let profile: GmailProfile = profile_response
        .json()
        .await
        .map_err(|e| format!("Invalid Gmail profile response: {e}"))?;

    if profile.email_address.trim().is_empty() {
        return Err("Gmail returned an empty account address".into());
    }

    // Keep the previous credential until the new reference has been committed.
    let previous_reference = {
        let connection = database
            .connection
            .lock()
            .map_err(|e| format!("Database lock failed: {e}"))?;
        let existing_id = connection
            .query_row(
                "SELECT id FROM gmail_accounts WHERE email_address = ?1 COLLATE NOCASE",
                [&profile.email_address],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|e| format!("Failed to inspect existing Gmail account: {e}"))?;
        match existing_id {
            Some(id) => GmailAccountRepository::find_token_reference(&connection, id)
                .map_err(|e| format!("Failed to read previous token reference: {e}"))?,
            None => None,
        }
    };

    let token_reference = format!("{KEYRING_REFERENCE_PREFIX}{}", Uuid::new_v4());
    token_storage::store_refresh_token(&token_reference, &refresh_token)?;

    let account_result = {
        let connection = database
            .connection
            .lock()
            .map_err(|e| format!("Database lock failed: {e}"))?;
        GmailAccountRepository::upsert_connected_account(
            &connection,
            &profile.email_address,
            &token_reference,
        )
        .and_then(|id| {
            GmailAccountRepository::find_by_id(&connection, id)?
                .ok_or(rusqlite::Error::QueryReturnedNoRows)
        })
    };

    match account_result {
        Ok(account) => {
            if let Some(old_reference) = previous_reference {
                if old_reference != token_reference {
                    let _ = token_storage::delete_refresh_token(&old_reference);
                }
            }
            Ok(account)
        }
        Err(error) => {
            let _ = token_storage::delete_refresh_token(&token_reference);
            Err(format!("Failed to save connected Gmail account: {error}"))
        }
    }
}

#[tauri::command]
pub fn disconnect_gmail_account(
    account_id: i64,
    database: State<'_, Database>,
    coordinator: State<'_, SyncCoordinator>,
) -> Result<bool, String> {
    let _permit = coordinator.try_acquire(account_id)?;
    let connection = database
        .connection
        .lock()
        .map_err(|e| format!("Database lock failed: {e}"))?;
    let token_reference = GmailAccountRepository::find_token_reference(&connection, account_id)
        .map_err(|e| format!("Failed to read Gmail token reference: {e}"))?;

    if let Some(reference) = token_reference {
        token_storage::delete_refresh_token(&reference)?;
    }
    GmailAccountRepository::clear_token_reference(&connection, account_id)
        .map_err(|e| format!("Failed to disconnect Gmail account: {e}"))
}
