use crate::models::{
    kind_str, Health, RequestRecord, SelectMode, Stats, TakeoverMode, TestResult, Trends,
    TunnelStatus, Upstream, UpstreamKind, UsageSnapshot,
};
use crate::state::AppState;
use crate::upstream::{client_for, endpoint_of, probe_exit_ip, Pool};
use serde::Serialize;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, State};

#[derive(Serialize, Clone)]
pub struct UpstreamView {
    pub upstream: Upstream,
    pub health: Health,
}

#[derive(Serialize, Clone)]
pub struct AppStateView {
    pub port: u16,
    pub running: bool,
    pub mode: SelectMode,
    pub pinned_id: Option<String>,
    pub claude_proxy: Option<String>,
    pub takeover_mode: TakeoverMode,
    pub upstreams: Vec<UpstreamView>,
}

pub fn health_view(pool: &Pool) -> Vec<UpstreamView> {
    pool.snapshot()
        .into_iter()
        .map(|(upstream, health)| UpstreamView { upstream, health })
        .collect()
}

pub fn build_view(state: &AppState) -> AppStateView {
    let cfg = state.config.lock().unwrap().clone();
    AppStateView {
        port: cfg.port,
        running: state.is_running(),
        mode: cfg.mode,
        pinned_id: cfg.pinned_id,
        claude_proxy: crate::claude::current_proxy(),
        takeover_mode: cfg.takeover_mode,
        upstreams: health_view(&state.pool),
    }
}

/// Probe the active tunnel (exit IP + geo + latency) and broadcast it.
/// No-op when the proxy isn't running.
pub async fn update_tunnel(app: AppHandle, pool: Arc<Pool>, tunnel: Arc<Mutex<TunnelStatus>>) {
    let (running, port, mode) = {
        let t = tunnel.lock().unwrap();
        (t.running, t.port, t.takeover_mode.clone())
    };
    if !running {
        return;
    }
    let mut ts = TunnelStatus::ready(port, mode);
    match pool.select() {
        Some(sel) => {
            ts.upstream_label = Some(sel.label.clone());
            ts.upstream_kind = Some(kind_str(&sel.kind));
            ts.upstream_endpoint = Some(endpoint_of(&sel.url));
            let (ip, geo, lat, err) = probe_exit_ip(&sel.client).await;
            ts.tunnel_ok = ip.is_some();
            ts.exit_ip = ip;
            ts.exit_geo = geo;
            ts.tunnel_latency_ms = lat;
            ts.error = err;
        }
        None => {
            ts.error = Some("无可用上游".to_string());
        }
    }
    *tunnel.lock().unwrap() = ts.clone();
    let _ = app.emit("tunnel", ts);
}

fn emit_tunnel_now(app: &AppHandle, tunnel: &Arc<Mutex<TunnelStatus>>) {
    let snap = tunnel.lock().unwrap().clone();
    let _ = app.emit("tunnel", snap);
}

fn spawn_probe(app: AppHandle, pool: Arc<Pool>) {
    // async_runtime::spawn works from sync commands too (tokio::spawn would
    // panic without an active runtime and abort across the webview callback).
    tauri::async_runtime::spawn(async move {
        let (a, p) = (app.clone(), pool.clone());
        // Emit health after every individual probe so each channel's card
        // updates the instant its result lands (progressive "testing" state).
        pool.probe_all_cb(|| {
            let _ = a.emit("health", health_view(&p));
        })
        .await;
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
    let (port, mode) = {
        let c = state.config.lock().unwrap();
        (c.port, c.takeover_mode.clone())
    };
    if !state.is_running() {
        let handle = crate::proxy::start(
            state.pool.clone(),
            state.store.clone(),
            state.traffic.clone(),
            app.clone(),
            state.ca.clone(),
            state.usage.clone(),
            port,
        )
        .await
        .map_err(|e| format!("启动监听失败 (端口 {}): {}", port, e))?;
        *state.proxy.lock().unwrap() = Some(handle);
    }
    // Only the Config mode touches ~/.claude/settings.json.
    if mode == TakeoverMode::Config {
        crate::claude::enable_intercept(port, state.ca.ca_cert_path())
            .map_err(|e| e.to_string())?;
    }

    *state.tunnel.lock().unwrap() = TunnelStatus::ready(port, mode);
    emit_tunnel_now(&app, &state.tunnel);

    // Probe the tunnel (exit IP + geo) and upstream health asynchronously.
    let (a, p, t) = (app.clone(), state.pool.clone(), state.tunnel.clone());
    tauri::async_runtime::spawn(async move { update_tunnel(a, p, t).await });
    spawn_probe(app, state.pool.clone());
    Ok(build_view(&state))
}

#[tauri::command]
pub fn stop_intercept(app: AppHandle, state: State<'_, AppState>) -> Result<AppStateView, String> {
    let mode = state.tunnel.lock().unwrap().takeover_mode.clone();
    if let Some(h) = state.proxy.lock().unwrap().take() {
        h.stop();
    }
    if mode == TakeoverMode::Config {
        crate::claude::disable_intercept().map_err(|e| e.to_string())?;
    }
    let port = state.config.lock().unwrap().port;
    *state.tunnel.lock().unwrap() = TunnelStatus::stopped(port);
    emit_tunnel_now(&app, &state.tunnel);
    Ok(build_view(&state))
}

#[tauri::command]
pub fn get_tunnel(state: State<'_, AppState>) -> TunnelStatus {
    state.tunnel.lock().unwrap().clone()
}

/// Actively test one upstream: resolve exit IP/geo and hit Anthropic's
/// /v1/models, returning status + a body snippet so you can see the response.
#[tauri::command]
pub async fn test_upstream(state: State<'_, AppState>, id: String) -> Result<TestResult, String> {
    let up = {
        let c = state.config.lock().unwrap();
        c.upstreams.iter().find(|u| u.id == id).cloned()
    };
    let up = up.ok_or_else(|| "找不到该上游".to_string())?;
    let client = client_for(&up).ok_or_else(|| "代理地址无效".to_string())?;

    let mut res = TestResult {
        ok: false,
        upstream_label: up.label.clone(),
        latency_ms: None,
        exit_ip: None,
        exit_geo: None,
        exit_org: None,
        status_reachable: false,
        status_latency_ms: None,
        status_indicator: None,
        status_desc: None,
        error: None,
    };

    let push_err = |res: &mut TestResult, m: String| {
        res.error = Some(match res.error.take() {
            Some(p) => format!("{}; {}", p, m),
            None => m,
        });
    };

    // 1) 出口 IP 详情（ipinfo.io）
    let t0 = Instant::now();
    match client
        .get("https://ipinfo.io/json")
        .timeout(Duration::from_secs(10))
        .send()
        .await
    {
        Ok(r) => {
            res.latency_ms = Some(t0.elapsed().as_millis() as u64);
            if let Ok(txt) = r.text().await {
                if let Ok(v) = serde_json::from_str::<Value>(&txt) {
                    res.exit_ip = v.get("ip").and_then(|x| x.as_str()).map(String::from);
                    let city = v.get("city").and_then(|x| x.as_str()).unwrap_or("");
                    let region = v.get("region").and_then(|x| x.as_str()).unwrap_or("");
                    let country = v.get("country").and_then(|x| x.as_str()).unwrap_or("");
                    let parts: Vec<&str> = [city, region, country]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect();
                    if !parts.is_empty() {
                        res.exit_geo = Some(parts.join(", "));
                    }
                    res.exit_org = v.get("org").and_then(|x| x.as_str()).map(String::from);
                }
            }
        }
        Err(e) => push_err(&mut res, format!("出口 IP 查询失败: {}", e)),
    }

    // 2) Claude 状态页（验证可达性，非 API、无需鉴权；status.anthropic.com 会跳到这里）
    let t1 = Instant::now();
    match client
        .get("https://status.claude.com/api/v2/status.json")
        .timeout(Duration::from_secs(10))
        .send()
        .await
    {
        Ok(r) => {
            res.status_reachable = r.status().is_success();
            res.status_latency_ms = Some(t1.elapsed().as_millis() as u64);
            if let Ok(txt) = r.text().await {
                if let Ok(v) = serde_json::from_str::<Value>(&txt) {
                    res.status_indicator = v
                        .get("status")
                        .and_then(|s| s.get("indicator"))
                        .and_then(|x| x.as_str())
                        .map(String::from);
                    res.status_desc = v
                        .get("status")
                        .and_then(|s| s.get("description"))
                        .and_then(|x| x.as_str())
                        .map(String::from);
                }
            }
        }
        Err(e) => push_err(&mut res, format!("状态页请求失败: {}", e)),
    }

    res.ok = res.exit_ip.is_some() || res.status_reachable;
    Ok(res)
}

#[tauri::command]
pub fn set_takeover_mode(state: State<'_, AppState>, mode: TakeoverMode) -> AppStateView {
    state.config.lock().unwrap().takeover_mode = mode;
    state.sync_pool_and_save();
    build_view(&state)
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
pub fn get_stats(state: State<'_, AppState>, since_ts: Option<i64>) -> Result<Stats, String> {
    state.store.stats(since_ts).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    state.store.clear().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_trends(state: State<'_, AppState>) -> Result<Trends, String> {
    state.store.trends().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn probe_now(app: AppHandle, state: State<'_, AppState>) -> AppStateView {
    spawn_probe(app, state.pool.clone());
    build_view(&state)
}

/// Claude account profile, read locally from `~/.claude.json` (no network).
#[tauri::command]
pub fn get_account() -> Option<crate::account::AccountInfo> {
    crate::account::read_account()
}

/// Latest real-time quota captured from proxied `/api/oauth/usage` responses.
/// `None` until Claude Code calls it (e.g. the user runs `/usage`).
#[tauri::command]
pub fn get_usage(state: State<'_, AppState>) -> Option<UsageSnapshot> {
    state.usage.lock().unwrap().clone()
}

#[derive(Serialize, Clone)]
pub struct ServiceComponent {
    pub name: String,
    pub status: String,
}

/// An active incident or scheduled maintenance from the status page — the
/// official notice ("Sonnet experiencing elevated errors", etc.).
#[derive(Serialize, Clone)]
pub struct ServiceIncident {
    pub name: String,
    pub impact: String,
    pub status: String,
    pub affected: Vec<String>,
    pub updated_at: Option<String>,
    pub latest_update: Option<String>,
    pub url: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct ServiceStatus {
    pub indicator: Option<String>,
    pub description: Option<String>,
    pub components: Vec<ServiceComponent>,
    pub incidents: Vec<ServiceIncident>,
    pub maintenances: Vec<ServiceIncident>,
}

fn parse_incident(i: &Value) -> Option<ServiceIncident> {
    let name = i.get("name")?.as_str()?.to_string();
    Some(ServiceIncident {
        name,
        impact: i
            .get("impact")
            .and_then(|x| x.as_str())
            .unwrap_or("none")
            .to_string(),
        status: i
            .get("status")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        affected: i
            .get("components")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| c.get("name")?.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        updated_at: i.get("updated_at").and_then(|x| x.as_str()).map(String::from),
        // incident_updates is newest-first; surface the latest official message.
        latest_update: i
            .get("incident_updates")
            .and_then(|u| u.as_array())
            .and_then(|arr| arr.first())
            .and_then(|u| u.get("body")?.as_str().map(String::from)),
        url: i.get("shortlink").and_then(|x| x.as_str()).map(String::from),
    })
}

/// Claude / Anthropic service status from the public Statuspage summary
/// (no auth). Routed through the active upstream so it works behind the GFW.
#[tauri::command]
pub async fn get_service_status(state: State<'_, AppState>) -> Result<ServiceStatus, String> {
    let client = state
        .pool
        .select()
        .map(|s| s.client)
        .unwrap_or_else(reqwest::Client::new);
    let resp = client
        .get("https://status.claude.com/api/v2/summary.json")
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body = resp.text().await.map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let indicator = v
        .pointer("/status/indicator")
        .and_then(|x| x.as_str())
        .map(String::from);
    let description = v
        .pointer("/status/description")
        .and_then(|x| x.as_str())
        .map(String::from);
    let components = v
        .get("components")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|c| !c.get("group").and_then(|g| g.as_bool()).unwrap_or(false))
                .filter_map(|c| {
                    Some(ServiceComponent {
                        name: c.get("name")?.as_str()?.to_string(),
                        status: c.get("status")?.as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let incidents = v
        .get("incidents")
        .and_then(|c| c.as_array())
        .map(|arr| arr.iter().filter_map(parse_incident).collect())
        .unwrap_or_default();
    let maintenances = v
        .get("scheduled_maintenances")
        .and_then(|c| c.as_array())
        .map(|arr| arr.iter().filter_map(parse_incident).collect())
        .unwrap_or_default();
    Ok(ServiceStatus {
        indicator,
        description,
        components,
        incidents,
        maintenances,
    })
}

#[tauri::command]
pub fn reorder_upstreams(state: State<'_, AppState>, ids: Vec<String>) -> AppStateView {
    {
        let mut cfg = state.config.lock().unwrap();
        let mut ordered: Vec<Upstream> = Vec::with_capacity(ids.len());
        for id in &ids {
            if let Some(pos) = cfg.upstreams.iter().position(|u| u.id == *id) {
                ordered.push(cfg.upstreams[pos].clone());
            }
        }
        for u in &cfg.upstreams {
            if !ids.contains(&u.id) {
                ordered.push(u.clone());
            }
        }
        cfg.upstreams = ordered;
    }
    state.sync_pool_and_save();
    build_view(&state)
}
