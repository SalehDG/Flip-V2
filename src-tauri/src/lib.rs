mod commands;
mod databases;
mod gmail_auth;
mod gmail_sync;
mod services;
mod storage_paths;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WindowEvent,
};

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_data_dir = storage_paths::app_data_dir(app.handle()).map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
            std::fs::create_dir_all(&app_data_dir)?;
            let db_path = app_data_dir.join("flip-v2.sqlite3");
            let database = databases::Database::new(&db_path)?;

            // Recover any rows interrupted during a prior process shutdown before
            // scheduling the automatic startup extraction worker.
            services::extraction::pipeline::recover_interrupted_extractions(&database)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;

            app.manage(database.clone());
            app.manage(gmail_auth::oauth_session::OAuthSessionStore::default());
            let sync_coordinator = gmail_sync::SyncCoordinator::default();
            app.manage(sync_coordinator.clone());

            // Gmail monitoring starts with the app and remains active independently
            // of the dashboard window. It performs an initial cycle immediately,
            // then checks connected accounts every 60 seconds.
            gmail_sync::monitoring::start(app.handle().clone(), database.clone(), sync_coordinator);

            // Retention cleanup runs once at startup and then every 24 hours.
            let retention_database = database.clone();
            let retention_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    let worker_database = retention_database.clone();
                    let result = tauri::async_runtime::spawn_blocking(move || {
                        services::retention::run_cleanup(&worker_database)
                    }).await;
                    match result {
                        Ok(Ok(summary)) => { let _ = retention_app.emit("flip://retention-cleanup-complete", summary); }
                        Ok(Err(error)) => { let _ = retention_app.emit("flip://retention-cleanup-error", error); }
                        Err(error) => { let _ = retention_app.emit("flip://retention-cleanup-error", error.to_string()); }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(24 * 60 * 60)).await;
                }
            });

            // System tray keeps the Gmail monitoring engine alive when the
            // dashboard window is closed. Tray menu offers explicit exit.
            let open_dashboard = MenuItem::with_id(app, "open-dashboard", "Buka Dashboard", true, None::<&str>)?;
            let exit_app = MenuItem::with_id(app, "exit-app", "Exit Flip V2", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&open_dashboard, &exit_app])?;
            TrayIconBuilder::new()
                .tooltip("Flip V2 - Monitoring Gmail")
                .icon(app.default_window_icon().expect("default window icon missing").clone())
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open-dashboard" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "exit-app" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // Closing the dashboard hides it instead of terminating the app.
            // Monitoring continues until Exit is selected from the tray menu.
            if let Some(window) = app.get_webview_window("main") {
                let close_window = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = close_window.hide();
                    }
                });
            }

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
            commands::extraction_commands::save_transaction_correction,
            commands::extraction_commands::get_processing_logs,
            commands::extraction_commands::get_extraction_transactions,
            commands::extraction_commands::get_transaction_invoice_data_url,
            commands::extraction_commands::get_extraction_status_summary,
            commands::extraction_commands::mark_transactions_copied,
            commands::extraction_commands::merge_duplicate_transactions,
            commands::retention_commands::get_retention_settings,
            commands::retention_commands::set_retention_settings,
            commands::retention_commands::preview_retention_cleanup,
            commands::retention_commands::run_retention_cleanup,
            commands::settings_commands::get_monitoring_settings,
            commands::settings_commands::set_monitoring_settings,
            commands::settings_commands::get_storage_info,
            commands::settings_commands::open_storage_directory,
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
