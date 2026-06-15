use crate::models::RequestRecord;
use crate::sse::SseAccumulator;
use crate::state::TrafficMeter;
use crate::store::Store;
use crate::upstream::Pool;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Method, Uri};
use axum::response::Response;
use axum::Router;
use futures::StreamExt;
use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;
use tauri::{AppHandle, Emitter};

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
}

pub async fn start(
    pool: Arc<Pool>,
    store: Arc<Store>,
    traffic: Arc<TrafficMeter>,
    app: AppHandle,
    port: u16,
) -> anyhow::Result<ProxyHandle> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    let ctx = ProxyCtx {
        pool,
        store,
        traffic,
        app,
    };
    let router = Router::new().fallback(handler).with_state(ctx);
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await;
    });
    Ok(ProxyHandle { shutdown: Some(tx) })
}

/// Max upstreams to try within one request before giving up (bounds latency).
const MAX_FAILOVER: usize = 3;

fn short_send_err(e: &reqwest::Error) -> String {
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

fn is_hop_request(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "host"
            | "content-length"
            | "accept-encoding"
            | "connection"
            | "proxy-connection"
            | "proxy-authorization"
            | "transfer-encoding"
            | "te"
            | "upgrade"
            | "keep-alive"
    )
}

fn is_hop_response(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "transfer-encoding" | "content-length" | "connection" | "keep-alive"
    )
}

fn error_response(code: u16, msg: &str) -> Response {
    let body = serde_json::json!({
        "type": "error",
        "error": { "type": "api_error", "message": msg }
    })
    .to_string();
    Response::builder()
        .status(code)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap()
}

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

fn emit_traffic(ctx: &ProxyCtx) {
    let (request_bytes, response_bytes) = ctx.traffic.snapshot();
    let _ = ctx.app.emit(
        "traffic",
        crate::models::TrafficSnapshot {
            session_request_bytes: request_bytes,
            session_response_bytes: response_bytes,
        },
    );
}

async fn handler(
    State(ctx): State<ProxyCtx>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let req_start = Instant::now();
    let path_q = uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/")
        .to_string();

    let mut record = RequestRecord::new(method.as_str().to_string(), path_q.clone());
    record.request_bytes = body.len() as u64;
    ctx.traffic.add_request(record.request_bytes);
    emit_traffic(&ctx);

    // Parse request body for model / stream flag and keep a pretty copy.
    if !body.is_empty() {
        if let Ok(v) = serde_json::from_slice::<Value>(&body) {
            record.model = v.get("model").and_then(|m| m.as_str()).map(String::from);
            record.stream = v.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
            record.request_body = serde_json::to_string_pretty(&v).ok();
        }
    }

    // Ordered failover candidates (Fixed = 1, Sticky/Auto = many).
    let candidates = ctx.pool.select_ordered();
    if candidates.is_empty() {
        finalize_error(&ctx, record, req_start, 503, "无可用上游".to_string());
        return error_response(503, "ccl-lens: 无可用上游");
    }

    let url = format!("https://api.anthropic.com{}", path_q);
    let mut chosen: Option<(crate::upstream::Selection, reqwest::Response)> = None;
    let mut last_err = String::new();
    let mut tried = 0usize;
    // Only transport-level failures fail over; any HTTP response means the
    // tunnel worked, so we keep it (even a 5xx from Anthropic).
    for sel in candidates.into_iter().take(MAX_FAILOVER) {
        tried += 1;
        let attempt_start = Instant::now();
        let mut rb = sel.client.request(method.clone(), &url);
        for (name, value) in headers.iter() {
            if !is_hop_request(name.as_str()) {
                rb = rb.header(name.clone(), value.clone());
            }
        }
        rb = rb.body(reqwest::Body::from(body.clone()));
        match rb.send().await {
            Ok(resp) => {
                ctx.pool
                    .record_success(&sel.id, attempt_start.elapsed().as_millis() as u64);
                chosen = Some((sel, resp));
                break;
            }
            Err(e) => {
                last_err = short_send_err(&e);
                ctx.pool.record_failure(&sel.id, last_err.clone());
            }
        }
    }

    let (sel, resp) = match chosen {
        Some(c) => c,
        None => {
            // Every candidate failed: kick an immediate re-probe and report.
            let pool = ctx.pool.clone();
            let app = ctx.app.clone();
            tauri::async_runtime::spawn(async move {
                pool.probe_all().await;
                let _ = app.emit("health", crate::commands::health_view(&pool));
            });
            let msg = format!("全部上游不可用 ({} 次尝试): {}", tried, last_err);
            finalize_error(&ctx, record, req_start, 502, msg.clone());
            return error_response(502, &format!("ccl-lens: {}", msg));
        }
    };
    record.upstream_id = Some(sel.id.clone());
    record.upstream_label = Some(sel.label.clone());
    let stream_upstream_id = sel.id.clone();

    let status = resp.status();
    record.status = Some(status.as_u16());
    record.ttfb_ms = Some(req_start.elapsed().as_millis() as u64);
    let resp_headers = resp.headers().clone();
    let ct = resp_headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let is_sse = ct.contains("text/event-stream");
    record.stream = is_sse;

    let store = ctx.store.clone();
    let app = ctx.app.clone();
    let pool_for_stream = ctx.pool.clone();
    let traffic = ctx.traffic.clone();
    let traffic_app = ctx.app.clone();

    let stream = async_stream::stream! {
        let mut acc = SseAccumulator::new();
        let mut json_buf: Vec<u8> = Vec::new();
        let mut response_bytes = 0u64;
        let mut upstream = resp.bytes_stream();
        while let Some(item) = upstream.next().await {
            match item {
                Ok(bytes) => {
                    response_bytes += bytes.len() as u64;
                    traffic.add_response(bytes.len() as u64);
                    let (session_request_bytes, session_response_bytes) = traffic.snapshot();
                    let _ = traffic_app.emit(
                        "traffic",
                        crate::models::TrafficSnapshot {
                            session_request_bytes,
                            session_response_bytes,
                        },
                    );
                    if is_sse {
                        acc.feed_sse(&bytes);
                    } else if json_buf.len() < 2_000_000 {
                        json_buf.extend_from_slice(&bytes);
                    }
                    yield Ok::<Bytes, std::io::Error>(bytes);
                }
                Err(e) => {
                    // Mid-stream drop: degrade this upstream so the next request
                    // (CC's retry) avoids it.
                    pool_for_stream.record_failure(&stream_upstream_id, format!("stream: {}", e));
                    acc.error = Some(format!("stream error: {}", e));
                    yield Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
                    break;
                }
            }
        }
        if !is_sse {
            acc.finish_json(&json_buf);
        }

        let mut rec = record;
        rec.duration_ms = Some(req_start.elapsed().as_millis() as u64);
        rec.response_bytes = response_bytes;
        if rec.model.is_none() {
            rec.model = acc.model.clone();
        }
        rec.input_tokens = acc.input_tokens;
        rec.output_tokens = acc.output_tokens;
        rec.cache_read_tokens = acc.cache_read;
        rec.cache_creation_tokens = acc.cache_creation;
        rec.stop_reason = acc.stop_reason.clone();
        if rec.error.is_none() && acc.error.is_some() {
            rec.error = acc.error.clone();
        }
        if !acc.text.is_empty() {
            rec.response_text = Some(acc.text.clone());
        }
        let model_for_cost = rec.model.clone().unwrap_or_default();
        rec.cost_usd = Some(crate::pricing::cost_usd(
            &model_for_cost,
            rec.input_tokens.unwrap_or(0),
            rec.output_tokens.unwrap_or(0),
            rec.cache_read_tokens.unwrap_or(0),
            rec.cache_creation_tokens.unwrap_or(0),
        ));
        let _ = store.insert(&rec);
        let _ = app.emit("request", &rec);
    };

    let mut builder = Response::builder().status(status);
    for (name, value) in resp_headers.iter() {
        if !is_hop_response(name.as_str()) {
            builder = builder.header(name.clone(), value.clone());
        }
    }
    builder
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| error_response(500, "ccl-lens: failed to build response"))
}
