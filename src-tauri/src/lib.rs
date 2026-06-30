mod account;
mod ca;
mod claude;
mod commands;
mod mitm;
mod models;
mod pricing;
mod proxy;
mod state;
mod store;
mod tray;
mod upstream;

use state::AppState;
use std::time::Duration;
use tauri::{Emitter, Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Needed by the MITM TLS server (rustls 0.23 requires an installed provider).
    let _ = rustls::crypto::ring::default_provider().install_default();

    let app_state = AppState::new().expect("failed to initialise ccl-lens state");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(app_state)
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            // Undo any settings left patched by a previous unclean exit.
            let _ = claude::recover_stale();
            tray::setup(app)?;
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
            commands::probe_now,
            commands::reorder_upstreams,
            commands::get_account,
            commands::get_usage,
            commands::get_service_status,
            commands::get_trends
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Restore ~/.claude/settings.json and stop listening on any exit
            // path (tray Quit, Cmd+Q, app termination), not just the tray.
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                app_handle.state::<AppState>().shutdown();
            }
        });
}
