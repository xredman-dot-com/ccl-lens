export type UpstreamKind = "direct" | "socks5" | "http";
export type HealthState = "unknown" | "up" | "down";
export type SelectMode = "fixed" | "sticky" | "auto";
export type TakeoverMode = "config" | "env" | "test";

export interface TunnelStatus {
  running: boolean;
  port: number;
  proxy_state: string;
  takeover_mode: TakeoverMode;
  tunnel_ok: boolean;
  tunnel_latency_ms: number | null;
  upstream_label: string | null;
  upstream_kind: string | null;
  upstream_endpoint: string | null;
  exit_ip: string | null;
  exit_geo: string | null;
  error: string | null;
}

export interface Upstream {
  id: string;
  label: string;
  kind: UpstreamKind;
  url: string;
  enabled: boolean;
}

export interface Health {
  state: HealthState;
  latency_ms: number | null;
  last_checked: number | null;
  success: number;
  failure: number;
  consecutive_failures: number;
  last_error: string | null;
}

export interface UpstreamView {
  upstream: Upstream;
  health: Health;
}

export interface AppStateView {
  port: number;
  running: boolean;
  mode: SelectMode;
  pinned_id: string | null;
  claude_proxy: string | null;
  takeover_mode: TakeoverMode;
  upstreams: UpstreamView[];
}

export interface RequestRecord {
  id: string;
  ts: number;
  method: string;
  path: string;
  model: string | null;
  status: number | null;
  upstream_id: string | null;
  upstream_label: string | null;
  ttfb_ms: number | null;
  duration_ms: number | null;
  input_tokens: number | null;
  output_tokens: number | null;
  cache_read_tokens: number | null;
  cache_creation_tokens: number | null;
  cost_usd: number | null;
  stop_reason: string | null;
  error: string | null;
  stream: boolean;
  request_bytes: number;
  response_bytes: number;
  request_body?: string | null;
  response_text?: string | null;
}

export interface TestResult {
  ok: boolean;
  upstream_label: string;
  latency_ms: number | null;
  exit_ip: string | null;
  exit_geo: string | null;
  exit_org: string | null;
  status_reachable: boolean;
  status_latency_ms: number | null;
  status_indicator: string | null;
  status_desc: string | null;
  error: string | null;
}

export interface ModelStat {
  model: string;
  requests: number;
  input_tokens: number;
  output_tokens: number;
  cost_usd: number;
}

export interface Stats {
  total_requests: number;
  total_request_bytes: number;
  total_response_bytes: number;
  total_input: number;
  total_output: number;
  total_cache_read: number;
  total_cache_creation: number;
  total_cost: number;
  errors: number;
  by_model: ModelStat[];
}

export interface TrafficSnapshot {
  session_request_bytes: number;
  session_response_bytes: number;
}

export interface AccountInfo {
  email: string | null;
  display_name: string | null;
  organization_name: string | null;
  organization_type: string | null;
  organization_role: string | null;
  billing_type: string | null;
  account_created_at: string | null;
  subscription_created_at: string | null;
  rate_limit_tier: string | null;
  has_extra_usage_enabled: boolean | null;
}

export interface UsageSnapshot {
  captured_at: number;
  // verbatim /api/oauth/usage payload; shape varies, rendered generically
  raw: Record<string, unknown>;
}

export interface ServiceComponent {
  name: string;
  status: string;
}

export interface ServiceIncident {
  name: string;
  impact: string;
  status: string;
  affected: string[];
  updated_at: string | null;
  latest_update: string | null;
  url: string | null;
}

export interface ServiceStatus {
  indicator: string | null;
  description: string | null;
  components: ServiceComponent[];
  incidents: ServiceIncident[];
  maintenances: ServiceIncident[];
}
