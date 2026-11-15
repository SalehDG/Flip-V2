
mod databases;
mod commands;
mod gmail_auth;

use tauri::Manager;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Ambil direktori penyimpanan data aplikasi.
            let app_data_dir = app.path().app_data_dir()?;

            // Pastikan direktori tersedia.
            std::fs::create_dir_all(&app_data_dir)?;

            // Tentukan lokasi database.
            let db_path = app_data_dir.join("flip-v2.sqlite3");

            // Buat koneksi database.
            let database = databases::Database::new(&db_path)?;

            // Simpan koneksi sebagai state aplikasi.
            app.manage(database);
            app.manage(gmail_auth::oauth_session::OAuthSessionStore::default());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::gmail_account_commands::get_gmail_accounts,
            commands::gmail_account_commands::update_sync_start_date,
            gmail_auth::oauth_session::start_gmail_oauth,
            gmail_auth::oauth_session::complete_gmail_oauth,
            gmail_auth::oauth_session::cancel_gmail_oauth,
            gmail_auth::oauth_session::disconnect_gmail_account,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
