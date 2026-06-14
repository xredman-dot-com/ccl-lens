import { useEffect, useRef, useState } from "react";
import type { AppStateView, RequestRecord, Stats, TakeoverMode, TunnelStatus } from "./types";
import { api, onHealth, onRequest, onTunnel } from "./api";
import { Header } from "./components/Header";
import { Connection } from "./components/Connection";
import { Upstreams } from "./components/Upstreams";
import { Timeline } from "./components/Timeline";
import { StatsPanel } from "./components/Stats";
import { RequestDetail } from "./components/RequestDetail";

const MAX_ROWS = 500;

export default function App() {
  const [state, setState] = useState<AppStateView | null>(null);
  const [tunnel, setTunnel] = useState<TunnelStatus | null>(null);
  const [requests, setRequests] = useState<RequestRecord[]>([]);
  const [stats, setStats] = useState<Stats | null>(null);
  const [tab, setTab] = useState<"timeline" | "stats">("timeline");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const statsTimer = useRef<number | null>(null);

  const refreshStats = () => {
    if (statsTimer.current) return;
    statsTimer.current = window.setTimeout(() => {
      statsTimer.current = null;
      api.getStats().then(setStats);
    }, 1200);
  };

  useEffect(() => {
    api.getState().then(setState);
    api.getTunnel().then(setTunnel);
    api.listRequests(MAX_ROWS, 0).then(setRequests);
    api.getStats().then(setStats);

    const unReq = onRequest((r) => {
      setRequests((prev) => [r, ...prev].slice(0, MAX_ROWS));
      refreshStats();
    });
    const unHealth = onHealth((ups) => {
      setState((prev) => (prev ? { ...prev, upstreams: ups } : prev));
    });
    const unTunnel = onTunnel(setTunnel);
    return () => {
      unReq.then((f) => f());
      unHealth.then((f) => f());
      unTunnel.then((f) => f());
    };
  }, []);

  const setTakeover = async (m: TakeoverMode) => {
    setState(await api.setTakeoverMode(m));
  };

  const toggle = async () => {
    setBusy(true);
    setError(null);
    try {
      const s = state?.running ? await api.stopIntercept() : await api.startIntercept();
      setState(s);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const clearHistory = async () => {
    await api.clearHistory();
    setRequests([]);
    api.getStats().then(setStats);
  };

  return (
    <div className="app">
      <Header state={state} tunnel={tunnel} />

      <div className="layout">
        <div className="sidebar">
          <Connection
            state={state}
            tunnel={tunnel}
            busy={busy}
            error={error}
            onToggle={toggle}
            onSetMode={setTakeover}
          />
          <Upstreams state={state} onChange={setState} />
        </div>

        <main className="content">
          <div className="tabs">
            <button
              className={tab === "timeline" ? "tab on" : "tab"}
              onClick={() => setTab("timeline")}
            >
              实时时间线
            </button>
            <button
              className={tab === "stats" ? "tab on" : "tab"}
              onClick={() => setTab("stats")}
            >
              Token / 成本
            </button>
            <span className="grow" />
            <span className="muted small">{requests.length} 条记录</span>
          </div>

          {tab === "timeline" ? (
            <Timeline
              requests={requests}
              selectedId={selectedId}
              onSelect={(r) => setSelectedId(r.id)}
            />
          ) : (
            <StatsPanel stats={stats} onClear={clearHistory} />
          )}
        </main>

        <RequestDetail id={selectedId} onClose={() => setSelectedId(null)} />
      </div>
    </div>
  );
}
