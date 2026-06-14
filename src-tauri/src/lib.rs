mod claude;
mod commands;
mod models;
mod pricing;
mod proxy;
mod sse;
mod state;
mod store;
mod upstream;

use state::AppState;
use std::time::Duration;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = AppState::new().expect("failed to initialise ccl-lens state");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        .setup(|app| {
            let handle = app.handle().clone();
            let state = app.state::<AppState>();
            let pool = state.pool.clone();
            let interval = state.config.lock().unwrap().health_interval_secs.max(5);
            tauri::async_runtime::spawn(async move {
                loop {
                    pool.probe_all().await;
                    let _ = handle.emit("health", commands::health_view(&pool));
                    tokio::time::sleep(Duration::from_secs(interval)).await;
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::start_intercept,
            commands::stop_intercept,
            commands::set_mode,
            commands::set_pinned,
            commands::add_upstream,
            commands::update_upstream,
            commands::remove_upstream,
            commands::set_upstream_enabled,
            commands::list_requests,
            commands::get_request,
            commands::get_stats,
            commands::clear_history,
            commands::probe_now
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
