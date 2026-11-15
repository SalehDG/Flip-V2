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
use std::{fs, path::Path};

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

        if config.installed.client_id.trim().is_empty()
            || config.installed.auth_uri.trim().is_empty()
            || config.installed.token_uri.trim().is_empty()
        {
            return Err("OAuth configuration is incomplete".to_string());
        }
        Ok(config)
    }
}

pub(crate) struct PreparedAuthorization {
    pub authorization_url: String,
    pub state: CsrfToken,
    pub pkce_verifier: PkceCodeVerifier,
}

pub(crate) fn build_authorization_url(
    config: &GoogleOAuthConfig,
    redirect_uri: &str,
) -> Result<PreparedAuthorization, String> {
    let client = BasicClient::new(ClientId::new(config.installed.client_id.clone()))
        .set_client_secret(ClientSecret::new(config.installed.client_secret.clone()))
        .set_auth_uri(
            AuthUrl::new(config.installed.auth_uri.clone())
                .map_err(|e| format!("Invalid authorization URL: {e}"))?,
        )
        .set_token_uri(
            TokenUrl::new(config.installed.token_uri.clone())
                .map_err(|e| format!("Invalid token URL: {e}"))?,
        )
        .set_redirect_uri(
            RedirectUrl::new(redirect_uri.to_string())
                .map_err(|e| format!("Invalid redirect URI: {e}"))?,
        )
        .set_auth_type(AuthType::RequestBody);

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let (authorization_url, state) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new(
            "https://www.googleapis.com/auth/gmail.readonly".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use url::Url;

    fn test_config() -> GoogleOAuthConfig {
        GoogleOAuthConfig {
            installed: InstalledConfig {
                client_id: "test-client-id".into(),
                client_secret: "test-client-secret".into(),
                auth_uri: "https://accounts.google.com/o/oauth2/v2/auth".into(),
                token_uri: "https://oauth2.googleapis.com/token".into(),
                redirect_uris: vec!["http://localhost".into()],
            },
        }
    }

    #[test]
    fn creates_authorization_url_with_pkce_and_readonly_scope() {
        let prepared = build_authorization_url(
            &test_config(),
            "http://127.0.0.1:49152/",
        ).unwrap();
        let url = Url::parse(&prepared.authorization_url).unwrap();
        let params: HashMap<_, _> = url.query_pairs().into_owned().collect();

        assert_eq!(params.get("client_id").unwrap(), "test-client-id");
        assert_eq!(params.get("redirect_uri").unwrap(), "http://127.0.0.1:49152/");
        assert_eq!(params.get("scope").unwrap(), "https://www.googleapis.com/auth/gmail.readonly");
        assert_eq!(params.get("access_type").unwrap(), "offline");
        assert_eq!(params.get("code_challenge_method").unwrap(), "S256");
        assert!(params.contains_key("code_challenge"));
        assert!(params.contains_key("state"));
    }
}
