use crate::models::{now_ms, Health, HealthState, SelectMode, Upstream, UpstreamKind};
use reqwest::Client;
use serde_json::Value;
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
    pub kind: UpstreamKind,
    pub url: String,
    pub client: Client,
}

/// host:port from an upstream url, stripping scheme and auth (hide password).
pub fn endpoint_of(url: &str) -> String {
    if url.trim().is_empty() {
        return "direct".to_string();
    }
    let no_scheme = url.split("://").nth(1).unwrap_or(url);
    let after_auth = no_scheme.rsplit('@').next().unwrap_or(no_scheme);
    after_auth.split('/').next().unwrap_or(after_auth).to_string()
}

/// Query the exit IP + geo through a given client.
/// Returns (ip, "City, CC", latency_ms, error).
pub async fn probe_exit_ip(
    client: &Client,
) -> (Option<String>, Option<String>, Option<u64>, Option<String>) {
    let start = Instant::now();
    match client
        .get("https://ipinfo.io/json")
        .timeout(Duration::from_secs(12))
        .send()
        .await
    {
        Ok(r) => {
            let latency = start.elapsed().as_millis() as u64;
            match r.text().await {
                Ok(t) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&t) {
                        let ip = v.get("ip").and_then(|x| x.as_str()).map(String::from);
                        let city = v.get("city").and_then(|x| x.as_str()).unwrap_or("");
                        let country = v.get("country").and_then(|x| x.as_str()).unwrap_or("");
                        let geo = match (city.is_empty(), country.is_empty()) {
                            (true, true) => None,
                            (true, false) => Some(country.to_string()),
                            (false, true) => Some(city.to_string()),
                            (false, false) => Some(format!("{}, {}", city, country)),
                        };
                        (ip, geo, Some(latency), None)
                    } else {
                        (None, None, Some(latency), Some("解析出口信息失败".to_string()))
                    }
                }
                Err(e) => (None, None, Some(latency), Some(short_err(&e))),
            }
        }
        Err(e) => (None, None, None, Some(short_err(&e))),
    }
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
                kind: u.kind.clone(),
                url: u.url.clone(),
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

#[cfg(test)]
mod tests {
    use super::endpoint_of;

    #[test]
    fn endpoint_hides_auth() {
        assert_eq!(endpoint_of("socks5://u:secret@1.2.3.4:1080"), "1.2.3.4:1080");
        assert_eq!(endpoint_of("http://10.0.0.1:8888"), "10.0.0.1:8888");
        assert_eq!(endpoint_of("socks5://h:p@host:5782/path"), "host:5782");
        assert_eq!(endpoint_of(""), "direct");
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
