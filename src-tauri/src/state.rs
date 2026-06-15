use crate::models::{SelectMode, TakeoverMode, TunnelStatus, Upstream};
use crate::proxy::ProxyHandle;
use crate::store::Store;
use crate::upstream::Pool;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

fn home() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_default()
}

pub fn data_dir() -> PathBuf {
    home().join(".ccl-lens")
}

fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

pub fn db_path() -> PathBuf {
    data_dir().join("history.db")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub port: u16,
    pub mode: SelectMode,
    pub pinned_id: Option<String>,
    pub upstreams: Vec<Upstream>,
    pub health_interval_secs: u64,
    #[serde(default)]
    pub takeover_mode: TakeoverMode,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            port: 31415,
            mode: SelectMode::Sticky,
            pinned_id: None,
            upstreams: vec![Upstream {
                id: "direct".to_string(),
                label: "直连".to_string(),
                kind: crate::models::UpstreamKind::Direct,
                url: String::new(),
                enabled: true,
            }],
            health_interval_secs: 20,
            takeover_mode: TakeoverMode::Config,
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        let path = config_path();
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<AppConfig>(&text) {
                return cfg;
            }
        }
        AppConfig::default()
    }

    pub fn save(&self) -> Result<()> {
        let dir = data_dir();
        std::fs::create_dir_all(&dir).ok();
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(config_path(), text)?;
        Ok(())
    }
}

pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub pool: Arc<Pool>,
    pub store: Arc<Store>,
    pub proxy: Mutex<Option<ProxyHandle>>,
    pub tunnel: Arc<Mutex<TunnelStatus>>,
    pub traffic: Arc<TrafficMeter>,
}

#[derive(Default)]
pub struct TrafficMeter {
    request_bytes: AtomicU64,
    response_bytes: AtomicU64,
}

impl TrafficMeter {
    pub fn add_request(&self, bytes: u64) {
        self.request_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn add_response(&self, bytes: u64) {
        self.response_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> (u64, u64) {
        (
            self.request_bytes.load(Ordering::Relaxed),
            self.response_bytes.load(Ordering::Relaxed),
        )
    }
}

impl AppState {
    pub fn new() -> Result<Self> {
        let config = AppConfig::load();
        let pool = Arc::new(Pool::new(
            config.upstreams.clone(),
            config.mode.clone(),
            config.pinned_id.clone(),
        ));
        let store = Arc::new(Store::open(&db_path())?);
        let tunnel = Arc::new(Mutex::new(TunnelStatus::stopped(config.port)));
        let traffic = Arc::new(TrafficMeter::default());
        Ok(AppState {
            config: Mutex::new(config),
            pool,
            store,
            proxy: Mutex::new(None),
            tunnel,
            traffic,
        })
    }

    pub fn is_running(&self) -> bool {
        self.proxy.lock().unwrap().is_some()
    }

    /// Push the current config's upstream/mode/pin into the live pool and persist.
    pub fn sync_pool_and_save(&self) {
        let cfg = self.config.lock().unwrap().clone();
        self.pool.set_all(
            cfg.upstreams.clone(),
            cfg.mode.clone(),
            cfg.pinned_id.clone(),
        );
        let _ = cfg.save();
    }
}
