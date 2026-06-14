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
            let tunnel = state.tunnel.clone();
            let interval = state.config.lock().unwrap().health_interval_secs.max(5);
            tauri::async_runtime::spawn(async move {
                loop {
                    pool.probe_all().await;
                    let _ = handle.emit("health", commands::health_view(&pool));
                    commands::update_tunnel(handle.clone(), pool.clone(), tunnel.clone()).await;
                    // Adaptive: probe faster while any upstream is down so
                    // recovery (and failover targets) are detected quickly.
                    let secs = if pool.any_enabled_down() {
                        interval.min(5)
                    } else {
                        interval
                    };
                    tokio::time::sleep(Duration::from_secs(secs)).await;
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::start_intercept,
            commands::stop_intercept,
            commands::get_tunnel,
            commands::test_upstream,
            commands::set_takeover_mode,
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
