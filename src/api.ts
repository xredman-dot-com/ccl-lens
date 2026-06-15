import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppStateView,
  RequestRecord,
  Stats,
  SelectMode,
  TakeoverMode,
  TestResult,
  TunnelStatus,
  TrafficSnapshot,
  UpstreamKind,
  Upstream,
  UpstreamView,
} from "./types";

export const api = {
  getState: () => invoke<AppStateView>("get_state"),
  startIntercept: () => invoke<AppStateView>("start_intercept"),
  stopIntercept: () => invoke<AppStateView>("stop_intercept"),
  getTunnel: () => invoke<TunnelStatus>("get_tunnel"),
  testUpstream: (id: string) => invoke<TestResult>("test_upstream", { id }),
  setTakeoverMode: (mode: TakeoverMode) =>
    invoke<AppStateView>("set_takeover_mode", { mode }),
  setMode: (mode: SelectMode) => invoke<AppStateView>("set_mode", { mode }),
  setPinned: (id: string | null) => invoke<AppStateView>("set_pinned", { id }),
  addUpstream: (label: string, kind: UpstreamKind, url: string) =>
    invoke<AppStateView>("add_upstream", { label, kind, url }),
  updateUpstream: (upstream: Upstream) =>
    invoke<AppStateView>("update_upstream", { upstream }),
  removeUpstream: (id: string) => invoke<AppStateView>("remove_upstream", { id }),
  setUpstreamEnabled: (id: string, enabled: boolean) =>
    invoke<AppStateView>("set_upstream_enabled", { id, enabled }),
  listRequests: (limit: number, offset: number) =>
    invoke<RequestRecord[]>("list_requests", { limit, offset }),
  getRequest: (id: string) => invoke<RequestRecord | null>("get_request", { id }),
  getStats: () => invoke<Stats>("get_stats"),
  clearHistory: () => invoke<void>("clear_history"),
  probeNow: () => invoke<AppStateView>("probe_now"),
};

export function onRequest(cb: (r: RequestRecord) => void): Promise<UnlistenFn> {
  return listen<RequestRecord>("request", (e) => cb(e.payload));
}

export function onHealth(cb: (u: UpstreamView[]) => void): Promise<UnlistenFn> {
  return listen<UpstreamView[]>("health", (e) => cb(e.payload));
}

export function onTunnel(cb: (t: TunnelStatus) => void): Promise<UnlistenFn> {
  return listen<TunnelStatus>("tunnel", (e) => cb(e.payload));
}

export function onTraffic(cb: (t: TrafficSnapshot) => void): Promise<UnlistenFn> {
  return listen<TrafficSnapshot>("traffic", (e) => cb(e.payload));
}

export function onState(cb: (s: AppStateView) => void): Promise<UnlistenFn> {
  return listen<AppStateView>("state", (e) => cb(e.payload));
}
