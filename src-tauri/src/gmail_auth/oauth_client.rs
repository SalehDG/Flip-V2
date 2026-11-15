
use oauth2::{
    basic::BasicClient,
    AuthType,
    AuthUrl,
    ClientId,
    ClientSecret,
    CsrfToken,
    PkceCodeChallenge,
    PkceCodeVerifier,
    RedirectUrl,
    Scope,
    TokenUrl,
};

use serde::Deserialize;
use std::{
    fs,
    path::Path,
};

// ============================================================
// 1. GOOGLE OAUTH CONFIGURATION
// ============================================================

#[derive(Debug, Deserialize)]
pub struct GoogleOAuthConfig {
    pub installed: InstalledConfig,
}

#[derive(Debug, Deserialize)]
pub struct InstalledConfig {
    pub client_id: String,
    pub client_secret: String,
    pub auth_uri: String,
    pub token_uri: String,
    pub redirect_uris: Vec<String>,
}

impl GoogleOAuthConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read OAuth config: {e}"))?;

        let config: Self = serde_json::from_str(&content)
            .map_err(|e| format!("Invalid OAuth config JSON: {e}"))?;

        if config.installed.client_id.is_empty() {
            return Err("OAuth client ID is empty".to_string());
        }

        Ok(config)
    }
}

// ============================================================
// 2. PREPARED AUTHORIZATION DATA
// ============================================================

pub(crate) struct PreparedAuthorization {
    pub authorization_url: String,
    pub state: CsrfToken,
    pub pkce_verifier: PkceCodeVerifier,
}

// ============================================================
// 3. BUILD AUTHORIZATION URL + PKCE
// ============================================================

pub(crate) fn build_authorization_url(
    config: &GoogleOAuthConfig,
    redirect_uri: &str,
) -> Result<PreparedAuthorization, String> {
    let client_id = ClientId::new(
        config.installed.client_id.clone()
    );

    let client_secret = ClientSecret::new(
        config.installed.client_secret.clone()
    );

    let auth_url = AuthUrl::new(
        config.installed.auth_uri.clone()
    )
    .map_err(|e| format!("Invalid authorization URL: {e}"))?;

    let token_url = TokenUrl::new(
        config.installed.token_uri.clone()
    )
    .map_err(|e| format!("Invalid token URL: {e}"))?;

    let redirect_url = RedirectUrl::new(
        redirect_uri.to_string()
    )
    .map_err(|e| format!("Invalid redirect URI: {e}"))?;

    let client = BasicClient::new(client_id)
        .set_client_secret(client_secret)
        .set_auth_uri(auth_url)
        .set_token_uri(token_url)
        .set_redirect_uri(redirect_url)
        .set_auth_type(AuthType::RequestBody);

    // Generate PKCE challenge and verifier.
    let (pkce_challenge, pkce_verifier) =
        PkceCodeChallenge::new_random_sha256();

    // Build Google authorization URL.
    let (authorization_url, state) = client
    .authorize_url(CsrfToken::new_random)
    .add_scope(Scope::new(
        "https://www.googleapis.com/auth/gmail.readonly".to_string()
    ))
        .add_extra_param("access_type", "offline")
        .add_extra_param("prompt", "consent")
        .set_pkce_challenge(pkce_challenge)
        .url();

    Ok(PreparedAuthorization {
        authorization_url: authorization_url.to_string(),
        state,
        pkce_verifier,
    })
}

// ============================================================
// OAUTH CONFIGURATION TEST COMMAND
// ============================================================

#[tauri::command]
pub fn load_oauth_config() -> Result<String, String> {
    let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../secrets/oauth-client.json");

    let config = GoogleOAuthConfig::load(&config_path)?;

    if config.installed.auth_uri.is_empty()
        || config.installed.token_uri.is_empty()
        || config.installed.redirect_uris.is_empty()
    {
        return Err("OAuth configuration is incomplete".to_string());
    }

    Ok("OAuth configuration loaded successfully".to_string())
}

// ============================================================
// 4. UNIT TEST
// ============================================================

#[cfg(test)]
mod oauth_tests {
    use super::*;
    use url::Url;

    fn test_config() -> GoogleOAuthConfig {
        GoogleOAuthConfig {
            installed: InstalledConfig {
                client_id: "test-client-id".to_string(),
                client_secret: "test-client-secret".to_string(),
                auth_uri: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
                token_uri: "https://oauth2.googleapis.com/token".to_string(),
                redirect_uris: vec![
                    "http://localhost".to_string()
                ],
            },
        }
    }

    #[test]
    fn creates_authorization_url_with_required_parameters() {
        let config = test_config();

        let result = build_authorization_url(
            &config,
            "http://127.0.0.1:49152/",
        );

        assert!(result.is_ok());

        let prepared = result.unwrap();
        let url = Url::parse(&prepared.authorization_url).unwrap();

        let params: std::collections::HashMap<_, _> =
            url.query_pairs().into_owned().collect();

        assert_eq!(
            params.get("client_id").unwrap(),
            "test-client-id"
        );

        assert_eq!(
            params.get("redirect_uri").unwrap(),
            "http://127.0.0.1:49152/"
        );

        assert_eq!(
            params.get("scope").unwrap(),
            "https://www.googleapis.com/auth/gmail.readonly"
        );

        assert_eq!(
            params.get("access_type").unwrap(),
            "offline"
        );

        assert_eq!(
            params.get("code_challenge_method").unwrap(),
            "S256"
        );

        assert!(params.contains_key("code_challenge"));
        assert!(params.contains_key("state"));
    }
}
