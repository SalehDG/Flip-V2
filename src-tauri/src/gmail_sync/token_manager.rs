use crate::gmail_auth::{oauth_client::GoogleOAuthConfig, token_storage};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct AccessToken {
    pub value: String,
    pub expires_in: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: Option<u64>,
    token_type: Option<String>,
}

pub async fn refresh_access_token(token_reference: &str) -> Result<AccessToken, String> {
    let refresh_token = token_storage::get_refresh_token(token_reference)?;
    let config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../secrets/oauth-client.json");
    let config = GoogleOAuthConfig::load(&config_path)?;
    let response = reqwest::Client::new()
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
        expires_in: token.expires_in.unwrap_or(3600),
    })
}
