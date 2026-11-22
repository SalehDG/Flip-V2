mod commands;
mod databases;
mod gmail_auth;
mod gmail_sync;
mod services;

use tauri::{Emitter, Manager};

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;
            let db_path = app_data_dir.join("flip-v2.sqlite3");
            let database = databases::Database::new(&db_path)?;

            // Recover any rows interrupted during a prior process shutdown before
            // scheduling the automatic startup extraction worker.
            services::extraction::pipeline::recover_interrupted_extractions(&database)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;

            app.manage(database.clone());
            app.manage(gmail_auth::oauth_session::OAuthSessionStore::default());
            app.manage(gmail_sync::SyncCoordinator::default());

            // Startup extraction runs in a blocking worker so OCR does not block
            // Tauri's async runtime or hold the SQLite mutex while Tesseract runs.
            let startup_database = database.clone();
            let startup_app = app.handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(error) = services::extraction::pipeline::run_pending_extraction(
                    &startup_database,
                    &services::ocr::TesseractOptions::default(),
                    Some(&startup_app),
                ) {
                    let _ = startup_app.emit("flip://extraction-error", error);
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::gmail_account_commands::get_gmail_accounts,
            commands::gmail_account_commands::update_sync_start_date,
            commands::gmail_history_commands::get_gmail_history,
            commands::gmail_history_commands::get_gmail_message_body,
            commands::gmail_diagnostic_commands::export_gmail_message_diagnostic,
            commands::extraction_commands::run_transaction_extraction,
            commands::extraction_commands::retry_transaction_extraction_command,
            commands::extraction_commands::correct_transaction_transfer_note,
            commands::extraction_commands::get_extraction_transactions,
            commands::extraction_commands::get_extraction_status_summary,
            gmail_auth::oauth_session::start_gmail_oauth,
            gmail_auth::oauth_session::complete_gmail_oauth,
            gmail_auth::oauth_session::cancel_gmail_oauth,
            gmail_auth::oauth_session::disconnect_gmail_account,
            gmail_sync::service::start_initial_gmail_sync,
            gmail_sync::service::start_incremental_gmail_sync,
            gmail_sync::service::get_gmail_sync_state,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
