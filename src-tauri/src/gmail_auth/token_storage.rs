use keyring::Entry;

const SERVICE_NAME: &str = "com.flipv2.desktop";

pub fn store_refresh_token(reference: &str, refresh_token: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, reference)
        .map_err(|e| format!("Failed to access Windows Credential Manager: {e}"))?;
    entry
        .set_password(refresh_token)
        .map_err(|e| format!("Failed to store Gmail refresh token securely: {e}"))
}

pub fn get_refresh_token(reference: &str) -> Result<String, String> {
    let entry = Entry::new(SERVICE_NAME, reference)
        .map_err(|e| format!("Failed to access Windows Credential Manager: {e}"))?;
    entry.get_password().map_err(|e| match e {
        keyring::Error::NoEntry => {
            "Gmail refresh token is missing. Reconnect this account.".to_string()
        }
        other => format!("Failed to read Gmail refresh token: {other}"),
    })
}

pub fn delete_refresh_token(reference: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, reference)
        .map_err(|e| format!("Failed to access Windows Credential Manager: {e}"))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        // A missing credential is already equivalent to disconnected storage.
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Failed to remove Gmail credential: {e}")),
    }
}
