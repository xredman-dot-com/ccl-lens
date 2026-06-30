use crate::ca::CaAuthority;
use crate::mitm::{self, MitmCtx};
use crate::models::{RequestRecord, UpstreamKind, UsageSnapshot};
use crate::state::TrafficMeter;
use crate::store::Store;
use crate::upstream::Pool;
use base64::Engine;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_socks::tcp::Socks5Stream;

pub struct ProxyHandle {
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl ProxyHandle {
    pub fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

#[derive(Clone)]
struct ProxyCtx {
    pool: Arc<Pool>,
    store: Arc<Store>,
    traffic: Arc<TrafficMeter>,
    app: AppHandle,
    ca: Arc<CaAuthority>,
    usage: Arc<Mutex<Option<UsageSnapshot>>>,
}

impl ProxyCtx {
    fn mitm(&self) -> MitmCtx {
        MitmCtx {
            pool: self.pool.clone(),
            store: self.store.clone(),
            traffic: self.traffic.clone(),
            app: self.app.clone(),
            ca: self.ca.clone(),
            usage: self.usage.clone(),
        }
    }
}

pub async fn start(
    pool: Arc<Pool>,
    store: Arc<Store>,
    traffic: Arc<TrafficMeter>,
    app: AppHandle,
    ca: Arc<CaAuthority>,
    usage: Arc<Mutex<Option<UsageSnapshot>>>,
    port: u16,
) -> anyhow::Result<ProxyHandle> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    let ctx = ProxyCtx {
        pool,
        store,
        traffic,
        app,
        ca,
        usage,
    };
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let mut shutdown = rx;
        loop {
            tokio::select! {
                _ = &mut shutdown => break,
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, _)) => {
                            let ctx = ctx.clone();
                            tokio::spawn(async move {
                                handle_proxy_connection(ctx, stream).await;
                            });
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    });
    Ok(ProxyHandle { shutdown: Some(tx) })
}

/// Max upstreams to try within one request before giving up (bounds latency).
const MAX_FAILOVER: usize = 3;
/// Per-upstream connect/handshake deadline. Without this a stalled socks5/http
/// handshake hangs Claude Code forever instead of failing over to the next
/// upstream. Matches reqwest's connect_timeout used by the health probes.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);

fn finalize_error(
    ctx: &ProxyCtx,
    mut record: RequestRecord,
    start: Instant,
    status: u16,
    msg: String,
) {
    record.status = Some(status);
    record.error = Some(msg);
    record.duration_ms = Some(start.elapsed().as_millis() as u64);
    let _ = ctx.store.insert(&record);
    let _ = ctx.app.emit("request", &record);
}

async fn handle_proxy_connection(ctx: ProxyCtx, stream: TcpStream) {
    let req_start = Instant::now();
    let mut reader = BufReader::new(stream);
    let mut header = Vec::with_capacity(1024);
    loop {
        let mut byte = [0u8; 1];
        match reader.read_exact(&mut byte).await {
            Ok(_) => {
                if header.len() > 64 * 1024 {
                    let mut stream = reader.into_inner();
                    let _ =
                        write_proxy_error(&mut stream, 431, "Request Header Fields Too Large")
                            .await;
                    return;
                }
                header.push(byte[0]);
                if header.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => return,
        }
    }

    let header_text = match String::from_utf8(header) {
        Ok(s) => s,
        Err(_) => {
            let mut stream = reader.into_inner();
            let _ = write_proxy_error(&mut stream, 400, "Bad Request").await;
            return;
        }
    };
    let first_line = header_text.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");

    if method.eq_ignore_ascii_case("CONNECT") {
        handle_connect_stream(ctx, reader, target.to_string(), req_start).await;
    } else {
        let mut stream = reader.into_inner();
        let _ = write_proxy_error(&mut stream, 405, "CONNECT Required").await;
    }
}

async fn write_proxy_error(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
) -> std::io::Result<()> {
    let body = format!("ccl-lens: {}\n", reason);
    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        reason,
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).await
}

async fn handle_connect_stream(
    ctx: ProxyCtx,
    reader: BufReader<TcpStream>,
    target: String,
    req_start: Instant,
) {
    let mut client = reader.into_inner();
    let host = match split_host_port(&target) {
        Ok((host, _)) => host,
        Err(_) => {
            let _ = write_proxy_error(&mut client, 400, "Invalid CONNECT Target").await;
            return;
        }
    };

    // Decrypt inspected hosts: ack the tunnel, then MITM the TLS so we can read
    // model/tokens/cost. Other hosts fall through to an opaque byte tunnel.
    if mitm::should_mitm(&host) {
        if client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await
            .is_err()
        {
            return;
        }
        mitm::serve(ctx.mitm(), client, host).await;
        return;
    }

    let mut record = RequestRecord::new("CONNECT".to_string(), target.clone());
    let candidates = ctx.pool.select_ordered();
    if candidates.is_empty() {
        let msg = "无可用上游".to_string();
        finalize_error(&ctx, record, req_start, 503, msg);
        let _ = write_proxy_error(&mut client, 503, "No Upstream").await;
        return;
    }

    let mut chosen: Option<(crate::upstream::Selection, TcpStream)> = None;
    let mut last_err = String::new();
    let mut tried = 0usize;
    for sel in candidates.into_iter().take(MAX_FAILOVER) {
        tried += 1;
        let attempt_start = Instant::now();
        let attempt = tokio::time::timeout(CONNECT_TIMEOUT, connect_via_upstream(&sel, &target)).await;
        match attempt {
            Ok(Ok(stream)) => {
                ctx.pool
                    .record_success(&sel.id, attempt_start.elapsed().as_millis() as u64);
                chosen = Some((sel, stream));
                break;
            }
            Ok(Err(e)) => {
                last_err = e;
                ctx.pool.record_failure(&sel.id, last_err.clone());
            }
            Err(_) => {
                last_err = format!("连接上游超时 ({}s)", CONNECT_TIMEOUT.as_secs());
                ctx.pool.record_failure(&sel.id, last_err.clone());
            }
        }
    }

    let (sel, mut upstream) = match chosen {
        Some(c) => c,
        None => {
            let msg = format!("CONNECT 全部上游不可用 ({} 次尝试): {}", tried, last_err);
            finalize_error(&ctx, record, req_start, 502, msg);
            let _ = write_proxy_error(&mut client, 502, "Bad Gateway").await;
            return;
        }
    };

    record.upstream_id = Some(sel.id.clone());
    record.upstream_label = Some(sel.label.clone());
    record.status = Some(200);
    record.ttfb_ms = Some(req_start.elapsed().as_millis() as u64);

    if let Err(e) = client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await
    {
        record.error = Some(format!("CONNECT response: {}", e));
        record.duration_ms = Some(req_start.elapsed().as_millis() as u64);
        let _ = ctx.store.insert(&record);
        let _ = ctx.app.emit("request", &record);
        return;
    }

    match tokio::io::copy_bidirectional(&mut client, &mut upstream).await {
        Ok((request_bytes, response_bytes)) => {
            ctx.traffic.add_request(request_bytes);
            ctx.traffic.add_response(response_bytes);
            record.request_bytes = request_bytes;
            record.response_bytes = response_bytes;
        }
        Err(e) => {
            record.error = Some(format!("CONNECT tunnel: {}", e));
        }
    }

    record.duration_ms = Some(req_start.elapsed().as_millis() as u64);
    let _ = ctx.store.insert(&record);
    let _ = ctx.app.emit("request", &record);
    let (session_request_bytes, session_response_bytes) = ctx.traffic.snapshot();
    let _ = ctx.app.emit(
        "traffic",
        crate::models::TrafficSnapshot {
            session_request_bytes,
            session_response_bytes,
        },
    );
}

async fn connect_via_upstream(
    sel: &crate::upstream::Selection,
    target: &str,
) -> Result<TcpStream, String> {
    match sel.kind {
        UpstreamKind::Direct => TcpStream::connect(target)
            .await
            .map_err(|e| format!("direct connect {}: {}", target, e)),
        UpstreamKind::Http => connect_via_http_proxy(&sel.url, target).await,
        UpstreamKind::Socks5 => connect_via_socks5(&sel.url, target).await,
    }
}

struct ProxyParts {
    host: String,
    port: u16,
    username: String,
    password: Option<String>,
}

fn parse_proxy_url(url: &str, default_port: u16) -> Result<ProxyParts, String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("invalid proxy url: {}", e))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "proxy url missing host".to_string())?
        .to_string();
    let port = parsed
        .port()
        .or_else(|| parsed.port_or_known_default())
        .unwrap_or(default_port);
    Ok(ProxyParts {
        host,
        port,
        username: parsed.username().to_string(),
        password: parsed.password().map(String::from),
    })
}

async fn connect_via_http_proxy(proxy_url: &str, target: &str) -> Result<TcpStream, String> {
    let proxy = parse_proxy_url(proxy_url, 8080)?;
    let mut stream = TcpStream::connect((proxy.host.as_str(), proxy.port))
        .await
        .map_err(|e| format!("http proxy connect: {}", e))?;
    let mut req = format!("CONNECT {} HTTP/1.1\r\nHost: {}\r\n", target, target);
    if !proxy.username.is_empty() {
        let raw = format!("{}:{}", proxy.username, proxy.password.unwrap_or_default());
        let token = base64::engine::general_purpose::STANDARD.encode(raw);
        req.push_str(&format!("Proxy-Authorization: Basic {}\r\n", token));
    }
    req.push_str("\r\n");
    stream
        .write_all(req.as_bytes())
        .await
        .map_err(|e| format!("http proxy write: {}", e))?;

    let header = read_proxy_header(&mut stream).await?;
    if !header.starts_with("HTTP/1.1 2") && !header.starts_with("HTTP/1.0 2") {
        let line = header.lines().next().unwrap_or("bad proxy response");
        return Err(format!("http proxy CONNECT failed: {}", line));
    }
    Ok(stream)
}

async fn read_proxy_header(stream: &mut TcpStream) -> Result<String, String> {
    let mut buf = Vec::with_capacity(1024);
    let mut tmp = [0u8; 256];
    loop {
        let n = stream
            .read(&mut tmp)
            .await
            .map_err(|e| format!("proxy read: {}", e))?;
        if n == 0 {
            return Err("proxy closed while reading response".to_string());
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            return String::from_utf8(buf).map_err(|e| format!("proxy response utf8: {}", e));
        }
        if buf.len() > 16 * 1024 {
            return Err("proxy response header too large".to_string());
        }
    }
}

/// Open a tunnel to `target` through a socks5 proxy. Uses tokio-socks (the same
/// proven client family reqwest uses for the health probes), with remote DNS so
/// the proxy resolves api.anthropic.com — never the local, GFW-poisoned resolver.
async fn connect_via_socks5(proxy_url: &str, target: &str) -> Result<TcpStream, String> {
    let proxy = parse_proxy_url(proxy_url, 1080)?;
    let (target_host, target_port) = split_host_port(target)?;
    let proxy_addr = format!("{}:{}", proxy.host, proxy.port);
    let dest = (target_host.as_str(), target_port);

    let stream = if proxy.username.is_empty() {
        Socks5Stream::connect(proxy_addr.as_str(), dest).await
    } else {
        Socks5Stream::connect_with_password(
            proxy_addr.as_str(),
            dest,
            &proxy.username,
            proxy.password.as_deref().unwrap_or(""),
        )
        .await
    };
    stream
        .map(|s| s.into_inner())
        .map_err(|e| format!("socks5: {}", e))
}

fn split_host_port(target: &str) -> Result<(String, u16), String> {
    let (host, port) = target
        .rsplit_once(':')
        .ok_or_else(|| "CONNECT target missing port".to_string())?;
    let port = port
        .parse::<u16>()
        .map_err(|e| format!("CONNECT target port: {}", e))?;
    Ok((host.trim_matches(['[', ']']).to_string(), port))
}
