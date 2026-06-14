use crate::models::{now_ms, Health, HealthState, SelectMode, Upstream, UpstreamKind};
use reqwest::Client;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

const PROBE_URL: &str = "https://api.anthropic.com/";
const PROBE_TIMEOUT: Duration = Duration::from_secs(6);

struct Inner {
    upstreams: Vec<Upstream>,
    health: HashMap<String, Health>,
    clients: HashMap<String, Client>,
    mode: SelectMode,
    pinned_id: Option<String>,
}

pub struct Pool {
    inner: RwLock<Inner>,
}

#[derive(Clone)]
pub struct Selection {
    pub id: String,
    pub label: String,
    pub client: Client,
}

fn build_client(up: &Upstream) -> Option<Client> {
    let mut b = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .pool_idle_timeout(Duration::from_secs(90))
        // no overall timeout: streaming responses can run for minutes
        .user_agent("ccl-lens/0.1");
    match up.kind {
        UpstreamKind::Direct => {}
        UpstreamKind::Socks5 | UpstreamKind::Http => {
            if up.url.trim().is_empty() {
                return None;
            }
            match reqwest::Proxy::all(&up.url) {
                Ok(p) => b = b.proxy(p),
                Err(_) => return None,
            }
        }
    }
    b.build().ok()
}

impl Pool {
    pub fn new(upstreams: Vec<Upstream>, mode: SelectMode, pinned_id: Option<String>) -> Self {
        let mut health = HashMap::new();
        let mut clients = HashMap::new();
        for up in &upstreams {
            health.insert(up.id.clone(), Health::default());
            if up.enabled {
                if let Some(c) = build_client(up) {
                    clients.insert(up.id.clone(), c);
                }
            }
        }
        Pool {
            inner: RwLock::new(Inner {
                upstreams,
                health,
                clients,
                mode,
                pinned_id,
            }),
        }
    }

    pub fn set_all(&self, upstreams: Vec<Upstream>, mode: SelectMode, pinned_id: Option<String>) {
        let mut inner = self.inner.write().unwrap();
        let mut health = HashMap::new();
        let mut clients = HashMap::new();
        for up in &upstreams {
            let h = inner.health.get(&up.id).cloned().unwrap_or_default();
            health.insert(up.id.clone(), h);
            if up.enabled {
                if let Some(c) = build_client(up) {
                    clients.insert(up.id.clone(), c);
                }
            }
        }
        inner.upstreams = upstreams;
        inner.health = health;
        inner.clients = clients;
        inner.mode = mode;
        inner.pinned_id = pinned_id;
    }

    /// Pick an upstream for a new request according to the active mode.
    pub fn select(&self) -> Option<Selection> {
        let inner = self.inner.read().unwrap();
        let enabled: Vec<&Upstream> = inner
            .upstreams
            .iter()
            .filter(|u| u.enabled && inner.clients.contains_key(&u.id))
            .collect();
        if enabled.is_empty() {
            return None;
        }

        let is_usable = |id: &str| -> bool {
            matches!(
                inner.health.get(id).map(|h| &h.state),
                Some(HealthState::Up) | Some(HealthState::Unknown) | None
            )
        };
        let latency = |id: &str| -> u64 {
            inner
                .health
                .get(id)
                .and_then(|h| h.latency_ms)
                .unwrap_or(u64::MAX)
        };
        let best_healthy = || -> Option<&Upstream> {
            enabled
                .iter()
                .filter(|u| is_usable(&u.id))
                .min_by_key(|u| latency(&u.id))
                .copied()
        };
        let pinned = inner
            .pinned_id
            .as_ref()
            .and_then(|pid| enabled.iter().find(|u| &u.id == pid).copied());

        let chosen: Option<&Upstream> = match inner.mode {
            SelectMode::Fixed => pinned.or_else(|| enabled.first().copied()),
            SelectMode::Sticky => {
                if let Some(p) = pinned {
                    if is_usable(&p.id) {
                        Some(p)
                    } else {
                        best_healthy().or(Some(p))
                    }
                } else {
                    best_healthy().or_else(|| enabled.first().copied())
                }
            }
            SelectMode::Auto => best_healthy().or_else(|| enabled.first().copied()),
        };

        chosen.and_then(|u| {
            inner.clients.get(&u.id).map(|c| Selection {
                id: u.id.clone(),
                label: u.label.clone(),
                client: c.clone(),
            })
        })
    }

    pub fn snapshot(&self) -> Vec<(Upstream, Health)> {
        let inner = self.inner.read().unwrap();
        inner
            .upstreams
            .iter()
            .map(|u| {
                (
                    u.clone(),
                    inner.health.get(&u.id).cloned().unwrap_or_default(),
                )
            })
            .collect()
    }

    /// Probe every enabled upstream once and update health in place.
    pub async fn probe_all(&self) {
        let targets: Vec<(String, Client)> = {
            let inner = self.inner.read().unwrap();
            inner
                .upstreams
                .iter()
                .filter(|u| u.enabled)
                .filter_map(|u| inner.clients.get(&u.id).map(|c| (u.id.clone(), c.clone())))
                .collect()
        };

        for (id, client) in targets {
            let start = Instant::now();
            let result = client
                .get(PROBE_URL)
                .timeout(PROBE_TIMEOUT)
                .send()
                .await;
            let mut inner = self.inner.write().unwrap();
            let h = inner.health.entry(id.clone()).or_default();
            h.last_checked = Some(now_ms());
            match result {
                Ok(_) => {
                    let sample = start.elapsed().as_millis() as u64;
                    h.state = HealthState::Up;
                    h.success += 1;
                    h.last_error = None;
                    h.latency_ms = Some(match h.latency_ms {
                        Some(prev) => ((prev as f64) * 0.7 + (sample as f64) * 0.3) as u64,
                        None => sample,
                    });
                }
                Err(e) => {
                    h.state = HealthState::Down;
                    h.failure += 1;
                    h.last_error = Some(short_err(&e));
                }
            }
        }
    }
}

fn short_err(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "timeout".to_string()
    } else if e.is_connect() {
        "connect failed".to_string()
    } else {
        let s = e.to_string();
        if s.len() > 120 {
            s[..120].to_string()
        } else {
            s
        }
    }
}
