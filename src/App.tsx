import { useEffect, useRef, useState } from "react";
import type {
  AccountInfo,
  AppStateView,
  RequestRecord,
  ServiceStatus,
  Stats,
  TakeoverMode,
  TrafficSnapshot,
  TunnelStatus,
  UsageSnapshot,
} from "./types";
import { api, onHealth, onRequest, onState, onTraffic, onTunnel, onUsage } from "./api";
import { Header } from "./components/Header";
import { Connection } from "./components/Connection";
import { Upstreams } from "./components/Upstreams";
import { Settings } from "./components/Settings";
import { Timeline } from "./components/Timeline";
import { StatsPanel } from "./components/Stats";
import { AccountPanel } from "./components/AccountPanel";
import { StatusBar } from "./components/StatusBar";
import { RequestDetail } from "./components/RequestDetail";

const MAX_ROWS = 500;
type ThemeMode = "system" | "light" | "dark";

function todayMidnight(): number {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

export default function App() {
  const [state, setState] = useState<AppStateView | null>(null);
  const [tunnel, setTunnel] = useState<TunnelStatus | null>(null);
  const [requests, setRequests] = useState<RequestRecord[]>([]);
  const [stats, setStats] = useState<Stats | null>(null);
  const [tab, setTab] = useState<"timeline" | "stats" | "account">("timeline");
  const [usage, setUsage] = useState<UsageSnapshot | null>(null);
  const [account, setAccount] = useState<AccountInfo | null>(null);
  const [svcStatus, setSvcStatus] = useState<ServiceStatus | null>(null);
  const [svcBusy, setSvcBusy] = useState(false);
  const [svcErr, setSvcErr] = useState<string | null>(null);
  const [theme, setTheme] = useState<ThemeMode>(() => {
    const saved = localStorage.getItem("ccl-theme");
    return saved === "light" || saved === "dark" || saved === "system" ? saved : "system";
  });
  const [traffic, setTraffic] = useState<TrafficSnapshot>({
    session_request_bytes: 0,
    session_response_bytes: 0,
  });
  const [trafficRate, setTrafficRate] = useState({ up: 0, down: 0 });
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Bumped when the user manually switches channel, so the connection panel can
  // show the hand-off immediately instead of waiting for the tunnel to catch up.
  const [switchNonce, setSwitchNonce] = useState(0);

  // Stats date filter — default to today
  const [statsSince, setStatsSince] = useState<number | null>(() => todayMidnight());
  const statsSinceRef = useRef<number | null>(statsSince);
  statsSinceRef.current = statsSince;

  // Resizable side / detail panels
  const [sidebarWidth, setSidebarWidth] = useState(() => {
    const s = localStorage.getItem("ccl-sidebar-width");
    return s ? Math.max(240, Math.min(520, Number(s))) : 300;
  });
  const [detailWidth, setDetailWidth] = useState(() => {
    const s = localStorage.getItem("ccl-detail-width");
    return s ? Math.max(280, Math.min(700, Number(s))) : 400;
  });
  const sidebarWidthRef = useRef(sidebarWidth);
  sidebarWidthRef.current = sidebarWidth;
  const detailWidthRef = useRef(detailWidth);
  detailWidthRef.current = detailWidth;
  const resizeRef = useRef<{ target: "sidebar" | "detail"; startX: number; startW: number } | null>(
    null
  );

  const onResizeDown = (target: "sidebar" | "detail") => (e: React.MouseEvent) => {
    resizeRef.current = {
      target,
      startX: e.clientX,
      startW: target === "sidebar" ? sidebarWidthRef.current : detailWidthRef.current,
    };
    e.preventDefault();
  };

  const statsTimer = useRef<number | null>(null);
  const trafficSample = useRef<{ t: number; up: number; down: number } | null>(null);

  const refreshStats = () => {
    if (statsTimer.current) return;
    statsTimer.current = window.setTimeout(() => {
      statsTimer.current = null;
      api.getStats(statsSinceRef.current).then(setStats);
    }, 1200);
  };

  // Re-fetch stats when the date filter changes
  useEffect(() => {
    api.getStats(statsSince).then(setStats);
  }, [statsSince]);

  const loadStatus = () => {
    setSvcBusy(true);
    setSvcErr(null);
    api
      .getServiceStatus()
      .then(setSvcStatus)
      .catch((e) => setSvcErr(String(e)))
      .finally(() => setSvcBusy(false));
  };

  // Poll service status globally (independent of the active tab) so the bottom
  // bar always reflects current incidents.
  useEffect(() => {
    loadStatus();
    const timer = window.setInterval(loadStatus, 60000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      const r = resizeRef.current;
      if (!r) return;
      if (r.target === "sidebar") {
        const next = Math.max(240, Math.min(520, r.startW + (e.clientX - r.startX)));
        sidebarWidthRef.current = next;
        setSidebarWidth(next);
      } else {
        const next = Math.max(280, Math.min(700, r.startW + (r.startX - e.clientX)));
        detailWidthRef.current = next;
        setDetailWidth(next);
      }
    };
    const onUp = () => {
      const r = resizeRef.current;
      if (!r) return;
      resizeRef.current = null;
      localStorage.setItem("ccl-sidebar-width", String(sidebarWidthRef.current));
      localStorage.setItem("ccl-detail-width", String(detailWidthRef.current));
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, []);

  useEffect(() => {
    const applyTheme = () => {
      const resolved =
        theme === "system"
          ? window.matchMedia("(prefers-color-scheme: light)").matches
            ? "light"
            : "dark"
          : theme;
      document.documentElement.dataset.theme = resolved;
      localStorage.setItem("ccl-theme", theme);
    };
    applyTheme();
    const media = window.matchMedia("(prefers-color-scheme: light)");
    media.addEventListener("change", applyTheme);
    return () => media.removeEventListener("change", applyTheme);
  }, [theme]);

  useEffect(() => {
    api.getState().then(setState);
    api.getTunnel().then(setTunnel);
    api.getUsage().then(setUsage);
    api.getAccount().then(setAccount);
    api.listRequests(MAX_ROWS, 0).then(setRequests);
    // Stats initial fetch handled by the statsSince effect above

    const unReq = onRequest((r) => {
      setRequests((prev) => [r, ...prev].slice(0, MAX_ROWS));
      refreshStats();
    });
    const unHealth = onHealth((ups) => {
      setState((prev) => (prev ? { ...prev, upstreams: ups } : prev));
    });
    const unTunnel = onTunnel(setTunnel);
    const unState = onState(setState);
    const unUsage = onUsage(setUsage);
    const unTraffic = onTraffic((next) => {
      const now = performance.now();
      const prev = trafficSample.current;
      if (prev) {
        const seconds = Math.max((now - prev.t) / 1000, 0.2);
        setTrafficRate({
          up: Math.max(0, (next.session_request_bytes - prev.up) / seconds),
          down: Math.max(0, (next.session_response_bytes - prev.down) / seconds),
        });
      }
      trafficSample.current = {
        t: now,
        up: next.session_request_bytes,
        down: next.session_response_bytes,
      };
      setTraffic(next);
    });
    return () => {
      unReq.then((f) => f());
      unHealth.then((f) => f());
      unTunnel.then((f) => f());
      unState.then((f) => f());
      unUsage.then((f) => f());
      unTraffic.then((f) => f());
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
    api.getStats(statsSinceRef.current).then(setStats);
  };

  const showDetail = tab === "timeline" && selectedId !== null;
  const gridCols = showDetail
    ? `${sidebarWidth}px 4px 1fr 4px ${detailWidth}px`
    : `${sidebarWidth}px 4px 1fr`;

  return (
    <div className="app">
      <Header
        state={state}
        tunnel={tunnel}
        account={account}
        usage={usage}
        theme={theme}
        onThemeChange={setTheme}
      />

      <div className="layout" style={{ gridTemplateColumns: gridCols }}>
        <div className="sidebar">
          <Connection
            state={state}
            tunnel={tunnel}
            busy={busy}
            error={error}
            switchNonce={switchNonce}
            onToggle={toggle}
            onSetMode={setTakeover}
          />
          <Upstreams
            state={state}
            tunnel={tunnel}
            onChange={setState}
            onSwitch={() => setSwitchNonce((n) => n + 1)}
          />
          <Settings />
        </div>

        <div className="resize-handle" onMouseDown={onResizeDown("sidebar")} />

        <main className="content">
          <div className="tabs">
            <button
              className={tab === "timeline" ? "tab on" : "tab"}
              onClick={() => setTab("timeline")}
            >
              时间线
            </button>
            <button
              className={tab === "stats" ? "tab on" : "tab"}
              onClick={() => setTab("stats")}
            >
              统计
            </button>
            <button
              className={tab === "account" ? "tab on" : "tab"}
              onClick={() => setTab("account")}
            >
              账号
            </button>
            <span className="grow" />
            <span className="muted small">{requests.length} 条</span>
          </div>

          {tab === "timeline" ? (
            <Timeline
              requests={requests}
              selectedId={selectedId}
              onSelect={(r) => setSelectedId(r.id)}
            />
          ) : tab === "stats" ? (
            <StatsPanel
              stats={stats}
              traffic={traffic}
              trafficRate={trafficRate}
              sinceTs={statsSince}
              onSinceChange={setStatsSince}
              onClear={clearHistory}
            />
          ) : (
            <AccountPanel
              status={svcStatus}
              statusBusy={svcBusy}
              statusErr={svcErr}
              onRefreshStatus={loadStatus}
            />
          )}
        </main>

        {showDetail && (
          <div className="resize-handle" onMouseDown={onResizeDown("detail")} />
        )}
        <RequestDetail
          id={showDetail ? selectedId : null}
          width={detailWidth}
          onClose={() => setSelectedId(null)}
        />
      </div>

      <StatusBar status={svcStatus} busy={svcBusy} onRefresh={loadStatus} />
    </div>
  );
}
