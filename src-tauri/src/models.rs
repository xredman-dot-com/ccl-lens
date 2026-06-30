use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn next_id() -> String {
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{}", now_ms(), c)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum UpstreamKind {
    Direct,
    Socks5,
    Http,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Upstream {
    pub id: String,
    pub label: String,
    pub kind: UpstreamKind,
    /// e.g. socks5://user:pass@host:1080 or http://127.0.0.1:8888 ; empty for direct
    #[serde(default)]
    pub url: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HealthState {
    Unknown,
    Up,
    Down,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    pub state: HealthState,
    pub latency_ms: Option<u64>,
    pub last_checked: Option<i64>,
    pub success: u32,
    pub failure: u32,
    #[serde(default)]
    pub consecutive_failures: u32,
    pub last_error: Option<String>,
}

impl Default for Health {
    fn default() -> Self {
        Health {
            state: HealthState::Unknown,
            latency_ms: None,
            last_checked: None,
            success: 0,
            failure: 0,
            consecutive_failures: 0,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SelectMode {
    /// Always use the pinned upstream, no failover.
    Fixed,
    /// Prefer the pinned upstream; fail over when unhealthy, snap back on recovery.
    Sticky,
    /// Always route to the fastest healthy upstream.
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestRecord {
    pub id: String,
    pub ts: i64,
    pub method: String,
    pub path: String,
    pub model: Option<String>,
    pub status: Option<u16>,
    pub upstream_id: Option<String>,
    pub upstream_label: Option<String>,
    pub ttfb_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub stop_reason: Option<String>,
    pub error: Option<String>,
    pub stream: bool,
    #[serde(default)]
    pub request_bytes: u64,
    #[serde(default)]
    pub response_bytes: u64,
    #[serde(default)]
    pub request_body: Option<String>,
    #[serde(default)]
    pub response_text: Option<String>,
}

impl RequestRecord {
    pub fn new(method: String, path: String) -> Self {
        RequestRecord {
            id: next_id(),
            ts: now_ms(),
            method,
            path,
            model: None,
            status: None,
            upstream_id: None,
            upstream_label: None,
            ttfb_ms: None,
            duration_ms: None,
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            cost_usd: None,
            stop_reason: None,
            error: None,
            stream: false,
            request_bytes: 0,
            response_bytes: 0,
            request_body: None,
            response_text: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelStat {
    pub model: String,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub total_requests: u64,
    pub total_request_bytes: u64,
    pub total_response_bytes: u64,
    pub total_input: u64,
    pub total_output: u64,
    pub total_cache_read: u64,
    pub total_cache_creation: u64,
    pub total_cost: f64,
    pub errors: u64,
    pub by_model: Vec<ModelStat>,
}

/// One local-day rollup of usage, persisted permanently (independent of the
/// capped detail history) so trends survive beyond the 2000-row detail window.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DayStat {
    pub day: String,
    pub requests: u64,
    pub input: u64,
    pub output: u64,
    pub cache: u64,
    pub cost: f64,
    pub errors: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Trends {
    pub today: DayStat,
    pub yesterday: DayStat,
    pub last7: DayStat,
    pub days: Vec<DayStat>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrafficSnapshot {
    pub session_request_bytes: u64,
    pub session_response_bytes: u64,
}

/// Latest `/api/oauth/usage` response captured passively as it flows through the
/// MITM proxy (Claude Code's own `/usage` data — real-time quota, not computed).
/// `raw` is the verbatim JSON so the UI can render whatever windows it returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSnapshot {
    pub captured_at: i64,
    pub raw: serde_json::Value,
}

/// How ccl-lens routes Claude Code through the local HTTP proxy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TakeoverMode {
    /// Patch ~/.claude/settings.json proxy env vars (auto-routes CC).
    Config,
    /// Don't touch config; user exports proxy env vars themselves.
    Env,
    /// Don't touch config; only bind the port and verify the upstream tunnel.
    Test,
}

impl Default for TakeoverMode {
    fn default() -> Self {
        TakeoverMode::Config
    }
}

pub fn kind_str(k: &UpstreamKind) -> String {
    match k {
        UpstreamKind::Direct => "direct",
        UpstreamKind::Socks5 => "socks5",
        UpstreamKind::Http => "http",
    }
    .to_string()
}

#[derive(Debug, Clone, Serialize)]
pub struct TunnelStatus {
    pub running: bool,
    pub port: u16,
    pub proxy_state: String,
    pub takeover_mode: TakeoverMode,
    pub tunnel_ok: bool,
    pub tunnel_latency_ms: Option<u64>,
    pub upstream_label: Option<String>,
    pub upstream_kind: Option<String>,
    pub upstream_endpoint: Option<String>,
    pub exit_ip: Option<String>,
    pub exit_geo: Option<String>,
    pub error: Option<String>,
}

impl TunnelStatus {
    pub fn stopped(port: u16) -> Self {
        TunnelStatus {
            running: false,
            port,
            proxy_state: "Stopped".to_string(),
            takeover_mode: TakeoverMode::Config,
            tunnel_ok: false,
            tunnel_latency_ms: None,
            upstream_label: None,
            upstream_kind: None,
            upstream_endpoint: None,
            exit_ip: None,
            exit_geo: None,
            error: None,
        }
    }

    pub fn ready(port: u16, mode: TakeoverMode) -> Self {
        let mut s = Self::stopped(port);
        s.running = true;
        s.proxy_state = "ProxyReady".to_string();
        s.takeover_mode = mode;
        s
    }
}

/// Result of actively testing one upstream's tunnel (not the Claude API).
#[derive(Debug, Clone, Serialize)]
pub struct TestResult {
    pub ok: bool,
    pub upstream_label: String,
    /// exit-IP lookup latency (tunnel round trip)
    pub latency_ms: Option<u64>,
    pub exit_ip: Option<String>,
    pub exit_geo: Option<String>,
    pub exit_org: Option<String>,
    /// status.anthropic.com reachability
    pub status_reachable: bool,
    pub status_latency_ms: Option<u64>,
    pub status_indicator: Option<String>,
    pub status_desc: Option<String>,
    pub error: Option<String>,
}
