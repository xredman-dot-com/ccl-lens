use serde_json::Value;

const TEXT_CAP: usize = 200_000;

/// Incrementally parses an Anthropic streaming (SSE) or single JSON response,
/// extracting token usage, model, stop reason and (capped) assistant text.
#[derive(Default)]
pub struct SseAccumulator {
    line_buf: String,
    pub model: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_creation: Option<u64>,
    pub stop_reason: Option<String>,
    pub error: Option<String>,
    pub text: String,
}

fn get_u64(v: &Value, key: &str) -> Option<u64> {
    v.get(key).and_then(|x| x.as_u64())
}

impl SseAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed_sse(&mut self, chunk: &[u8]) {
        self.line_buf.push_str(&String::from_utf8_lossy(chunk));
        while let Some(pos) = self.line_buf.find('\n') {
            let line: String = self.line_buf.drain(..=pos).collect();
            self.process_line(line.trim());
        }
    }

    fn process_line(&mut self, line: &str) {
        let data = match line.strip_prefix("data:") {
            Some(d) => d.trim(),
            None => return,
        };
        if data.is_empty() || data == "[DONE]" {
            return;
        }
        if let Ok(v) = serde_json::from_str::<Value>(data) {
            self.handle_event(&v);
        }
    }

    fn handle_event(&mut self, v: &Value) {
        match v.get("type").and_then(|t| t.as_str()) {
            Some("message_start") => {
                if let Some(msg) = v.get("message") {
                    if let Some(m) = msg.get("model").and_then(|m| m.as_str()) {
                        self.model = Some(m.to_string());
                    }
                    if let Some(u) = msg.get("usage") {
                        self.input_tokens = get_u64(u, "input_tokens").or(self.input_tokens);
                        self.cache_read =
                            get_u64(u, "cache_read_input_tokens").or(self.cache_read);
                        self.cache_creation =
                            get_u64(u, "cache_creation_input_tokens").or(self.cache_creation);
                        // output may appear here too
                        if let Some(o) = get_u64(u, "output_tokens") {
                            self.output_tokens = Some(o);
                        }
                    }
                }
            }
            Some("message_delta") => {
                if let Some(u) = v.get("usage") {
                    if let Some(o) = get_u64(u, "output_tokens") {
                        self.output_tokens = Some(o);
                    }
                }
                if let Some(sr) = v
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(|s| s.as_str())
                {
                    self.stop_reason = Some(sr.to_string());
                }
            }
            Some("content_block_delta") => {
                if let Some(t) = v
                    .get("delta")
                    .and_then(|d| d.get("text"))
                    .and_then(|s| s.as_str())
                {
                    if self.text.len() < TEXT_CAP {
                        self.text.push_str(t);
                    }
                }
            }
            Some("error") => {
                if let Some(m) = v
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|s| s.as_str())
                {
                    self.error = Some(m.to_string());
                }
            }
            _ => {}
        }
    }

    /// Parse a complete non-streaming JSON body.
    pub fn finish_json(&mut self, bytes: &[u8]) {
        let v: Value = match serde_json::from_slice(bytes) {
            Ok(v) => v,
            Err(_) => return,
        };
        if let Some(m) = v.get("error").and_then(|e| e.get("message")).and_then(|s| s.as_str()) {
            self.error = Some(m.to_string());
            return;
        }
        if let Some(m) = v.get("model").and_then(|m| m.as_str()) {
            self.model = Some(m.to_string());
        }
        if let Some(u) = v.get("usage") {
            self.input_tokens = get_u64(u, "input_tokens").or(self.input_tokens);
            self.output_tokens = get_u64(u, "output_tokens").or(self.output_tokens);
            self.cache_read = get_u64(u, "cache_read_input_tokens").or(self.cache_read);
            self.cache_creation =
                get_u64(u, "cache_creation_input_tokens").or(self.cache_creation);
        }
        if let Some(sr) = v.get("stop_reason").and_then(|s| s.as_str()) {
            self.stop_reason = Some(sr.to_string());
        }
        if let Some(arr) = v.get("content").and_then(|c| c.as_array()) {
            for block in arr {
                if let Some(t) = block.get("text").and_then(|s| s.as_str()) {
                    if self.text.len() < TEXT_CAP {
                        self.text.push_str(t);
                    }
                }
            }
        }
    }
}
