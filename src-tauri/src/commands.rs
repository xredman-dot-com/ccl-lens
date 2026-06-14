use crate::models::{Health, RequestRecord, SelectMode, Stats, Upstream, UpstreamKind};
use crate::state::AppState;
use crate::upstream::Pool;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

#[derive(Serialize, Clone)]
pub struct UpstreamView {
    pub upstream: Upstream,
    pub health: Health,
}

#[derive(Serialize)]
pub struct AppStateView {
    pub port: u16,
    pub running: bool,
    pub mode: SelectMode,
    pub pinned_id: Option<String>,
    pub claude_base_url: Option<String>,
    pub upstreams: Vec<UpstreamView>,
}

pub fn health_view(pool: &Pool) -> Vec<UpstreamView> {
    pool.snapshot()
        .into_iter()
        .map(|(upstream, health)| UpstreamView { upstream, health })
        .collect()
}

fn build_view(state: &AppState) -> AppStateView {
    let cfg = state.config.lock().unwrap().clone();
    AppStateView {
        port: cfg.port,
        running: state.is_running(),
        mode: cfg.mode,
        pinned_id: cfg.pinned_id,
        claude_base_url: crate::claude::current_base_url(),
        upstreams: health_view(&state.pool),
    }
}

fn spawn_probe(app: AppHandle, pool: Arc<Pool>) {
    tokio::spawn(async move {
        pool.probe_all().await;
        let _ = app.emit("health", health_view(&pool));
    });
}

#[tauri::command]
pub fn get_state(state: State<'_, AppState>) -> AppStateView {
    build_view(&state)
}

#[tauri::command]
pub async fn start_intercept(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppStateView, String> {
    let port = state.config.lock().unwrap().port;
    if !state.is_running() {
        let handle = crate::proxy::start(
            state.pool.clone(),
            state.store.clone(),
            app.clone(),
            port,
        )
        .await
        .map_err(|e| format!("启动监听失败 (端口 {}): {}", port, e))?;
        *state.proxy.lock().unwrap() = Some(handle);
    }
    crate::claude::enable_intercept(port).map_err(|e| e.to_string())?;
    spawn_probe(app, state.pool.clone());
    Ok(build_view(&state))
}

#[tauri::command]
pub fn stop_intercept(state: State<'_, AppState>) -> Result<AppStateView, String> {
    if let Some(h) = state.proxy.lock().unwrap().take() {
        h.stop();
    }
    crate::claude::disable_intercept().map_err(|e| e.to_string())?;
    Ok(build_view(&state))
}

#[tauri::command]
pub fn set_mode(state: State<'_, AppState>, mode: SelectMode) -> AppStateView {
    state.config.lock().unwrap().mode = mode;
    state.sync_pool_and_save();
    build_view(&state)
}

#[tauri::command]
pub fn set_pinned(state: State<'_, AppState>, id: Option<String>) -> AppStateView {
    state.config.lock().unwrap().pinned_id = id;
    state.sync_pool_and_save();
    build_view(&state)
}

#[tauri::command]
pub fn add_upstream(
    app: AppHandle,
    state: State<'_, AppState>,
    label: String,
    kind: UpstreamKind,
    url: String,
) -> AppStateView {
    let up = Upstream {
        id: crate::models::next_id(),
        label,
        kind,
        url,
        enabled: true,
    };
    state.config.lock().unwrap().upstreams.push(up);
    state.sync_pool_and_save();
    spawn_probe(app, state.pool.clone());
    build_view(&state)
}

#[tauri::command]
pub fn update_upstream(state: State<'_, AppState>, upstream: Upstream) -> AppStateView {
    {
        let mut cfg = state.config.lock().unwrap();
        if let Some(slot) = cfg.upstreams.iter_mut().find(|u| u.id == upstream.id) {
            *slot = upstream;
        }
    }
    state.sync_pool_and_save();
    build_view(&state)
}

#[tauri::command]
pub fn remove_upstream(state: State<'_, AppState>, id: String) -> AppStateView {
    {
        let mut cfg = state.config.lock().unwrap();
        cfg.upstreams.retain(|u| u.id != id);
        if cfg.pinned_id.as_deref() == Some(id.as_str()) {
            cfg.pinned_id = None;
        }
    }
    state.sync_pool_and_save();
    build_view(&state)
}

#[tauri::command]
pub fn set_upstream_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> AppStateView {
    {
        let mut cfg = state.config.lock().unwrap();
        if let Some(slot) = cfg.upstreams.iter_mut().find(|u| u.id == id) {
            slot.enabled = enabled;
        }
    }
    state.sync_pool_and_save();
    spawn_probe(app, state.pool.clone());
    build_view(&state)
}

#[tauri::command]
pub fn list_requests(
    state: State<'_, AppState>,
    limit: i64,
    offset: i64,
) -> Result<Vec<RequestRecord>, String> {
    state.store.list(limit, offset).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_request(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<RequestRecord>, String> {
    state.store.get(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_stats(state: State<'_, AppState>) -> Result<Stats, String> {
    state.store.stats().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    state.store.clear().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn probe_now(app: AppHandle, state: State<'_, AppState>) -> AppStateView {
    spawn_probe(app, state.pool.clone());
    build_view(&state)
}
