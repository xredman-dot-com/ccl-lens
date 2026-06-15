use crate::models::{now_ms, Health, HealthState, SelectMode, Upstream, UpstreamKind};
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

const PROBE_URL: &str = "https://api.anthropic.com/";
const PROBE_TIMEOUT: Duration = Duration::from_secs(6);
/// Consecutive real-request failures before an upstream is circuit-broken (Down).
const CIRCUIT_THRESHOLD: u32 = 2;

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

/// Build a standalone client for a single upstream (used by the test command).
pub fn client_for(up: &Upstream) -> Option<Client> {
    build_client(up)
}

fn build_client(up: &Upstream) -> Option<Client> {
    let mut b = Client::builder()
        .no_proxy()
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
            let proxy_url = if up.kind == UpstreamKind::Socks5
                && up.url.to_ascii_lowercase().starts_with("socks5://")
            {
                format!("socks5h://{}", &up.url["socks5://".len()..])
            } else {
                up.url.clone()
            };
            match reqwest::Proxy::all(&proxy_url) {
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

    /// Ordered failover candidates for a new request, per the active mode.
    /// Fixed -> only the pinned channel (strict, no failover).
    /// Sticky -> usable pinned first, then healthy-by-latency, then the rest.
    /// Auto -> healthy-by-latency, then the rest.
    pub fn select_ordered(&self) -> Vec<Selection> {
        let inner = self.inner.read().unwrap();
        let enabled: Vec<&Upstream> = inner
            .upstreams
            .iter()
            .filter(|u| u.enabled && inner.clients.contains_key(&u.id))
            .collect();
        if enabled.is_empty() {
            return vec![];
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
        // All enabled, usable first then by latency.
        let by_pref = || -> Vec<&Upstream> {
            let mut v = enabled.clone();
            v.sort_by_key(|u| (!is_usable(&u.id), latency(&u.id)));
            v
        };
        let pinned = inner
            .pinned_id
            .as_ref()
            .and_then(|pid| enabled.iter().find(|u| &u.id == pid).copied());

        let order: Vec<&Upstream> = match inner.mode {
            SelectMode::Fixed => match pinned {
                Some(p) => vec![p],
                None => vec![enabled[0]],
            },
            SelectMode::Sticky => {
                let mut v: Vec<&Upstream> = Vec::new();
                if let Some(p) = pinned {
                    if is_usable(&p.id) {
                        v.push(p);
                    }
                }
                for u in by_pref() {
                    if !v.iter().any(|x| x.id == u.id) {
                        v.push(u);
                    }
                }
                v
            }
            SelectMode::Auto => by_pref(),
        };

        order
            .into_iter()
            .filter_map(|u| {
                inner.clients.get(&u.id).map(|c| Selection {
                    id: u.id.clone(),
                    label: u.label.clone(),
                    kind: u.kind.clone(),
                    url: u.url.clone(),
                    client: c.clone(),
                })
            })
            .collect()
    }

    /// Top candidate (used by the tunnel panel).
    pub fn select(&self) -> Option<Selection> {
        self.select_ordered().into_iter().next()
    }

    /// Feed a successful real request back into health (passive check).
    pub fn record_success(&self, id: &str, latency_ms: u64) {
        let mut inner = self.inner.write().unwrap();
        if let Some(h) = inner.health.get_mut(id) {
            h.state = HealthState::Up;
            h.success += 1;
            h.consecutive_failures = 0;
            h.last_error = None;
            h.last_checked = Some(now_ms());
            h.latency_ms = Some(match h.latency_ms {
                Some(prev) => ((prev as f64) * 0.7 + (latency_ms as f64) * 0.3) as u64,
                None => latency_ms,
            });
        }
    }

    /// Feed a failed real request back into health; circuit-break on threshold.
    pub fn record_failure(&self, id: &str, err: String) {
        let mut inner = self.inner.write().unwrap();
        if let Some(h) = inner.health.get_mut(id) {
            h.failure += 1;
            h.consecutive_failures += 1;
            h.last_error = Some(err);
            h.last_checked = Some(now_ms());
            if h.consecutive_failures >= CIRCUIT_THRESHOLD {
                h.state = HealthState::Down;
            }
        }
    }

    pub fn any_enabled_down(&self) -> bool {
        let inner = self.inner.read().unwrap();
        inner.upstreams.iter().any(|u| {
            u.enabled
                && matches!(
                    inner.health.get(&u.id).map(|h| &h.state),
                    Some(HealthState::Down)
                )
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
                    h.consecutive_failures = 0;
                    h.last_error = None;
                    h.latency_ms = Some(match h.latency_ms {
                        Some(prev) => ((prev as f64) * 0.7 + (sample as f64) * 0.3) as u64,
                        None => sample,
                    });
                }
                Err(e) => {
                    h.state = HealthState::Down;
                    h.failure += 1;
                    h.consecutive_failures += 1;
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

    fn ups() -> Vec<crate::models::Upstream> {
        use crate::models::{Upstream, UpstreamKind};
        vec![
            Upstream { id: "a".into(), label: "a".into(), kind: UpstreamKind::Direct, url: "".into(), enabled: true },
            Upstream { id: "b".into(), label: "b".into(), kind: UpstreamKind::Socks5, url: "socks5://127.0.0.1:1080".into(), enabled: true },
        ]
    }

    #[test]
    fn circuit_breaks_and_orders_unhealthy_last() {
        use crate::models::SelectMode;
        let pool = super::Pool::new(ups(), SelectMode::Auto, None);
        pool.record_failure("a", "x".into());
        pool.record_failure("a", "x".into()); // >= CIRCUIT_THRESHOLD -> Down
        let ordered = pool.select_ordered();
        assert_eq!(ordered.len(), 2);
        assert_eq!(ordered.first().unwrap().id, "b"); // healthy first
        assert_eq!(ordered.last().unwrap().id, "a"); // circuit-broken last
    }

    #[test]
    fn fixed_mode_has_no_failover() {
        use crate::models::SelectMode;
        let pool = super::Pool::new(ups(), SelectMode::Fixed, Some("b".into()));
        let ordered = pool.select_ordered();
        assert_eq!(ordered.len(), 1);
        assert_eq!(ordered[0].id, "b");
    }

    #[test]
    fn success_clears_circuit() {
        use crate::models::SelectMode;
        let pool = super::Pool::new(ups(), SelectMode::Auto, None);
        pool.record_failure("a", "x".into());
        pool.record_failure("a", "x".into());
        pool.record_success("a", 100);
        assert!(!pool.any_enabled_down());
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
