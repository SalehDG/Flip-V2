use crate::gmail_auth::{oauth_client::GoogleOAuthConfig, token_storage};
use serde::Deserialize;
use std::time::Duration;
use tauri::AppHandle;

#[derive(Debug, Clone)]
pub struct AccessToken {
    pub value: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    token_type: Option<String>,
}

pub async fn refresh_access_token(
    app: &AppHandle,
    token_reference: &str,
) -> Result<AccessToken, String> {
    let refresh_token = token_storage::get_refresh_token(token_reference)?;
    let config = GoogleOAuthConfig::load_for_app(app)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("Cannot initialize Google token client: {error}"))?;
    let response = client
        .post(&config.installed.token_uri)
        .form(&[
            ("client_id", config.installed.client_id.as_str()),
            ("client_secret", config.installed.client_secret.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| format!("Cannot reach Google token endpoint: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        if body.contains("invalid_grant") {
            return Err("GMAIL_TOKEN_REVOKED: Google rejected the refresh token. Reconnect this Gmail account.".into());
        }
        return Err(format!("Google token refresh failed ({status}): {body}"));
    }
    let token: TokenResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid Google token response: {e}"))?;
    if token.access_token.is_empty() || token.token_type.as_deref().unwrap_or("Bearer") != "Bearer"
    {
        return Err("Google returned an invalid access token response".into());
    }
    Ok(AccessToken {
        value: token.access_token,
    })
}
