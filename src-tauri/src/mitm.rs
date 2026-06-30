use crate::ca::CaAuthority;
use crate::models::{now_ms, RequestRecord, TrafficSnapshot, UsageSnapshot};
use crate::pricing;
use crate::state::TrafficMeter;
use crate::store::Store;
use crate::upstream::Pool;
use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::service::service_fn;
use hyper::{HeaderMap, Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use serde_json::Value;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
use tokio_stream::wrappers::ReceiverStream;

const MAX_FAILOVER: usize = 3;
/// Deadline for an upstream to return *response headers* (not the body — streams
/// run for minutes). Kept generous on purpose: a dead/blackholed upstream is
/// caught fast by connect_timeout + HTTP/2 keepalive, so this only fires for a
/// slow-but-alive upstream, where failing over would re-send a non-idempotent
/// POST /v1/messages and risk double generation/billing.
const HEADERS_TIMEOUT: Duration = Duration::from_secs(20);
const HEAD_CAP: usize = 32 * 1024;
const TAIL_CAP: usize = 16 * 1024;

/// Only these hosts are decrypted; everything else is tunneled opaque.
pub fn should_mitm(host: &str) -> bool {
    host == "api.anthropic.com"
}

/// Hop-by-hop / framing headers we must not forward in either direction.
fn is_hop_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "host" | "content-length"
            | "connection"
            | "proxy-connection"
            | "keep-alive"
            | "transfer-encoding"
            | "te"
            | "trailer"
            | "upgrade"
            // strip so the upstream replies in identity encoding we can parse
            | "accept-encoding"
            | "content-encoding"
    )
}

#[derive(Clone)]
pub struct MitmCtx {
    pub pool: Arc<Pool>,
    pub store: Arc<Store>,
    pub traffic: Arc<TrafficMeter>,
    pub app: AppHandle,
    pub ca: Arc<CaAuthority>,
    pub usage: Arc<Mutex<Option<UsageSnapshot>>>,
}

type ResBody = BoxBody<Bytes, std::io::Error>;

/// Terminate TLS for `host` with a CA-signed leaf, then serve the decrypted
/// HTTP (h1 or h2), forwarding each request upstream and recording it.
pub async fn serve(ctx: MitmCtx, client: TcpStream, host: String) {
    let server_config = match ctx.ca.server_config(&host) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ccl-lens mitm: server config for {}: {}", host, e);
            return;
        }
    };
    let acceptor = TlsAcceptor::from(server_config);
    let tls = match acceptor.accept(client).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ccl-lens mitm: tls accept {}: {}", host, e);
            return;
        }
    };

    let io = TokioIo::new(tls);
    let ctx = Arc::new(ctx);
    let host = Arc::new(host);
    let service = service_fn(move |req| {
        let ctx = ctx.clone();
        let host = host.clone();
        async move { Ok::<_, Infallible>(handle_request(ctx, host, req).await) }
    });

    let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
        .serve_connection(io, service)
        .await;
}

async fn handle_request(ctx: Arc<MitmCtx>, host: Arc<String>, req: Request<Incoming>) -> Response<ResBody> {
    let start = Instant::now();
    let method = req.method().clone();
    let path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());

    let (parts, body) = req.into_parts();
    let req_bytes = body
        .collect()
        .await
        .map(|c| c.to_bytes())
        .unwrap_or_else(|_| Bytes::new());
    let req_len = req_bytes.len() as u64;
    let (model_req, is_stream) = parse_request(&req_bytes);

    let url = format!("https://{}{}", host, path);
    let mut fwd_headers = HeaderMap::new();
    for (name, value) in parts.headers.iter() {
        if !is_hop_header(name.as_str()) {
            fwd_headers.append(name.clone(), value.clone());
        }
    }

    // Try upstreams in order until one returns response headers.
    let candidates = ctx.pool.select_ordered();
    let mut chosen = None;
    let mut last_err = "无可用上游".to_string();
    for sel in candidates.into_iter().take(MAX_FAILOVER) {
        let send = sel
            .client
            .request(method.clone(), &url)
            .headers(fwd_headers.clone())
            .body(req_bytes.clone())
            .send();
        match tokio::time::timeout(HEADERS_TIMEOUT, send).await {
            Ok(Ok(resp)) => {
                ctx.pool
                    .record_success(&sel.id, start.elapsed().as_millis() as u64);
                chosen = Some((sel, resp));
                break;
            }
            Ok(Err(e)) => {
                last_err = e.to_string();
                ctx.pool.record_failure(&sel.id, last_err.clone());
            }
            Err(_) => {
                last_err = format!("上游响应超时 ({}s)", HEADERS_TIMEOUT.as_secs());
                ctx.pool.record_failure(&sel.id, last_err.clone());
            }
        }
    }

    let (sel, resp) = match chosen {
        Some(c) => c,
        None => {
            let mut rec = RequestRecord::new(method.as_str().to_string(), path);
            rec.model = model_req;
            rec.stream = is_stream;
            rec.request_bytes = req_len;
            rec.request_body = Some(truncate_text(&req_bytes));
            rec.error = Some(last_err);
            rec.status = Some(502);
            rec.duration_ms = Some(start.elapsed().as_millis() as u64);
            let _ = ctx.store.insert(&rec);
            let _ = ctx.app.emit("request", &rec);
            return error_response(502, "ccl-lens: 全部上游不可用\n");
        }
    };

    let ttfb = start.elapsed().as_millis() as u64;
    let status = resp.status();
    let resp_headers = resp.headers().clone();

    // Capture the quota snapshot when this is Claude Code's own `/usage` call —
    // the small JSON body fits entirely in `head`, so we parse it after the stream.
    let is_usage = status == StatusCode::OK && path.starts_with("/api/oauth/usage");

    // Stream the body to the client while capturing it for parsing; finalize
    // the record when the stream ends (output tokens land in the last event).
    let (tx, rx) = mpsc::channel::<Result<Frame<Bytes>, std::io::Error>>(16);
    let task_ctx = ctx.clone();
    tokio::spawn(async move {
        let mut stream = resp.bytes_stream();
        let mut head: Vec<u8> = Vec::new();
        let mut tail: Vec<u8> = Vec::new();
        let mut resp_bytes = 0u64;
        let mut err: Option<String> = None;
        while let Some(item) = stream.next().await {
            match item {
                Ok(chunk) => {
                    resp_bytes += chunk.len() as u64;
                    capture(&mut head, &mut tail, &chunk);
                    if tx.send(Ok(Frame::data(chunk))).await.is_err() {
                        break; // client went away
                    }
                }
                Err(e) => {
                    err = Some(e.to_string());
                    break;
                }
            }
        }
        drop(tx);

        let parsed = parse_response(&head, &tail, resp_bytes);
        let model = parsed.model.or(model_req);
        let mut rec = RequestRecord::new(method.as_str().to_string(), path);
        rec.model = model.clone();
        rec.status = Some(status.as_u16());
        rec.upstream_id = Some(sel.id.clone());
        rec.upstream_label = Some(sel.label.clone());
        rec.ttfb_ms = Some(ttfb);
        rec.duration_ms = Some(start.elapsed().as_millis() as u64);
        rec.input_tokens = nonzero(parsed.input);
        rec.output_tokens = nonzero(parsed.output);
        rec.cache_read_tokens = nonzero(parsed.cache_read);
        rec.cache_creation_tokens = nonzero(parsed.cache_creation);
        rec.stop_reason = parsed.stop_reason;
        rec.stream = is_stream;
        rec.request_bytes = req_len;
        rec.response_bytes = resp_bytes;
        rec.request_body = Some(truncate_text(&req_bytes));
        rec.response_text = Some(truncate_text(&head));
        rec.error = err;
        if let Some(m) = model.as_deref() {
            if parsed.input + parsed.output + parsed.cache_read + parsed.cache_creation > 0 {
                rec.cost_usd = Some(pricing::cost_usd(
                    m,
                    parsed.input,
                    parsed.output,
                    parsed.cache_read,
                    parsed.cache_creation,
                ));
            }
        }

        task_ctx.traffic.add_request(req_len);
        task_ctx.traffic.add_response(resp_bytes);
        let _ = task_ctx.store.insert(&rec);
        let _ = task_ctx.app.emit("request", &rec);

        if is_usage {
            if let Ok(v) = serde_json::from_slice::<Value>(&head) {
                let snap = UsageSnapshot { captured_at: now_ms(), raw: v };
                if let Ok(mut g) = task_ctx.usage.lock() {
                    *g = Some(snap.clone());
                }
                let _ = task_ctx.app.emit("usage", &snap);
            }
        }

        let (up, down) = task_ctx.traffic.snapshot();
        let _ = task_ctx.app.emit(
            "traffic",
            TrafficSnapshot {
                session_request_bytes: up,
                session_response_bytes: down,
            },
        );
    });

    let body: ResBody = BoxBody::new(StreamBody::new(ReceiverStream::new(rx)));
    let mut builder = Response::builder().status(status);
    for (name, value) in resp_headers.iter() {
        if !is_hop_header(name.as_str()) {
            builder = builder.header(name, value);
        }
    }
    builder.body(body).unwrap_or_else(|_| error_response(502, "ccl-lens: bad response\n"))
}

fn error_response(status: u16, msg: &str) -> Response<ResBody> {
    let body = Full::new(Bytes::from(msg.to_string()))
        .map_err(|never| match never {})
        .boxed();
    Response::builder()
        .status(StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY))
        .header("content-type", "text/plain")
        .body(body)
        .unwrap()
}

fn capture(head: &mut Vec<u8>, tail: &mut Vec<u8>, chunk: &[u8]) {
    if head.len() < HEAD_CAP {
        let take = (HEAD_CAP - head.len()).min(chunk.len());
        head.extend_from_slice(&chunk[..take]);
    }
    tail.extend_from_slice(chunk);
    if tail.len() > TAIL_CAP {
        let drop = tail.len() - TAIL_CAP;
        tail.drain(..drop);
    }
}

fn truncate_text(bytes: &[u8]) -> String {
    let end = bytes.len().min(HEAD_CAP);
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

fn nonzero(v: u64) -> Option<u64> {
    (v > 0).then_some(v)
}

fn parse_request(body: &[u8]) -> (Option<String>, bool) {
    match serde_json::from_slice::<Value>(body) {
        Ok(v) => (
            v.get("model").and_then(|m| m.as_str()).map(String::from),
            v.get("stream").and_then(|s| s.as_bool()).unwrap_or(false),
        ),
        Err(_) => (None, false),
    }
}

#[derive(Default)]
struct Parsed {
    model: Option<String>,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_creation: u64,
    stop_reason: Option<String>,
}

fn parse_response(head: &[u8], tail: &[u8], resp_bytes: u64) -> Parsed {
    let mut p = Parsed::default();

    // Non-streamed JSON: the whole body fit in head -> parse directly.
    if resp_bytes as usize <= head.len() {
        if let Ok(v) = serde_json::from_slice::<Value>(head) {
            p.model = v.get("model").and_then(|m| m.as_str()).map(String::from);
            p.stop_reason = v
                .get("stop_reason")
                .and_then(|s| s.as_str())
                .map(String::from);
            if let Some(u) = v.get("usage") {
                apply_usage(&mut p, u);
            }
            if p.input + p.output + p.cache_read + p.cache_creation > 0 {
                return p;
            }
        }
    }

    // Streamed SSE: input/model land near the start (head), output + stop_reason
    // accumulate to the last message_delta (kept in tail).
    apply_sse(&mut p, head);
    apply_sse(&mut p, tail);
    p
}

fn apply_usage(p: &mut Parsed, u: &Value) {
    if let Some(n) = u.get("input_tokens").and_then(|x| x.as_u64()) {
        p.input = n;
    }
    if let Some(n) = u.get("output_tokens").and_then(|x| x.as_u64()) {
        p.output = n;
    }
    if let Some(n) = u.get("cache_read_input_tokens").and_then(|x| x.as_u64()) {
        p.cache_read = n;
    }
    if let Some(n) = u
        .get("cache_creation_input_tokens")
        .and_then(|x| x.as_u64())
    {
        p.cache_creation = n;
    }
}

fn apply_sse(p: &mut Parsed, buf: &[u8]) {
    let text = String::from_utf8_lossy(buf);
    for line in text.lines() {
        let line = line.trim_start();
        let json = match line.strip_prefix("data:") {
            Some(rest) => rest.trim(),
            None => continue,
        };
        if json.is_empty() || json == "[DONE]" {
            continue;
        }
        let v: Value = match serde_json::from_str(json) {
            Ok(v) => v,
            Err(_) => continue, // partial line at a truncated boundary
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("message_start") => {
                if let Some(m) = v.get("message") {
                    if let Some(model) = m.get("model").and_then(|x| x.as_str()) {
                        p.model.get_or_insert_with(|| model.to_string());
                    }
                    if let Some(u) = m.get("usage") {
                        apply_usage(p, u);
                    }
                }
            }
            Some("message_delta") => {
                if let Some(u) = v.get("usage") {
                    // output_tokens here is cumulative; later events overwrite.
                    if let Some(n) = u.get("output_tokens").and_then(|x| x.as_u64()) {
                        p.output = n;
                    }
                }
                if let Some(sr) = v
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(|x| x.as_str())
                {
                    p.stop_reason = Some(sr.to_string());
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_request_model_and_stream() {
        let body = br#"{"model":"claude-opus-4-20250514","stream":true,"messages":[]}"#;
        let (model, stream) = parse_request(body);
        assert_eq!(model.as_deref(), Some("claude-opus-4-20250514"));
        assert!(stream);
    }

    #[test]
    fn parses_streamed_sse_usage() {
        let head = b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-opus-4-20250514\",\"usage\":{\"input_tokens\":100,\"cache_read_input_tokens\":20,\"cache_creation_input_tokens\":5,\"output_tokens\":1}}}\n\n";
        let tail = b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":250}}\n\n";
        // resp_bytes large so the whole-JSON branch is skipped (true SSE).
        let p = parse_response(head, tail, 1_000_000);
        assert_eq!(p.input, 100);
        assert_eq!(p.output, 250);
        assert_eq!(p.cache_read, 20);
        assert_eq!(p.cache_creation, 5);
        assert_eq!(p.model.as_deref(), Some("claude-opus-4-20250514"));
        assert_eq!(p.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn parses_nonstreamed_json_usage() {
        let body = br#"{"model":"claude-3-5-haiku-20241022","stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":20}}"#;
        let p = parse_response(body, body, body.len() as u64);
        assert_eq!(p.input, 10);
        assert_eq!(p.output, 20);
        assert_eq!(p.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(p.model.as_deref(), Some("claude-3-5-haiku-20241022"));
    }

    #[test]
    fn tail_with_partial_leading_line_still_parses_final_delta() {
        // Simulate a tail whose first line is truncated mid-JSON.
        let tail = b"pe\":\"content_block_delta\",\"index\":0}\n\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"},\"usage\":{\"output_tokens\":4096}}\n\n";
        let p = parse_response(b"", tail, 1_000_000);
        assert_eq!(p.output, 4096);
        assert_eq!(p.stop_reason.as_deref(), Some("max_tokens"));
    }
}
