
use serde::Deserialize;
use std::{fs, path::Path};

#[derive(Debug, Deserialize)]
pub struct GoogleOAuthConfig {
    pub installed: InstalledConfig,
}

#[derive(Debug, Deserialize)]
pub struct InstalledConfig {
    pub client_id: String,
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
