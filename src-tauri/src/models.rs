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
    pub total_input: u64,
    pub total_output: u64,
    pub total_cache_read: u64,
    pub total_cache_creation: u64,
    pub total_cost: f64,
    pub errors: u64,
    pub by_model: Vec<ModelStat>,
}
