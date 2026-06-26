import { useState, useRef, useEffect } from "react";
import type {
  AppStateView,
  Health,
  SelectMode,
  TestResult,
  TunnelStatus,
  UpstreamKind,
} from "../types";
import { api } from "../api";
import { fmtMs } from "../format";
import { parseProxyInput, maskUrl } from "../parse";

const MODES: { value: SelectMode; label: string; hint: string }[] = [
  { value: "fixed", label: "固定", hint: "始终使用固定通道，不自动切换" },
  { value: "sticky", label: "优先+兜底", hint: "优先使用固定通道，异常时自动切换，恢复后切回" },
  { value: "auto", label: "自动择优", hint: "按延迟自动选择最快的健康通道" },
];

interface Props {
  state: AppStateView | null;
  tunnel: TunnelStatus | null;
  onChange: (s: AppStateView) => void;
}

/// Dot appearance per health + enabled state. For a live (up) channel the blink
/// frequency encodes latency: lower latency → faster pulse → "snappier" line.
function dotProps(h: Health, enabled: boolean): {
  className: string;
  style?: React.CSSProperties;
} {
  if (!enabled) return { className: "dot dot-off" };
  if (h.state === "up") {
    const lat = h.latency_ms ?? 1000;
    const dur = Math.max(0.6, Math.min(3, 0.5 + lat / 800));
    return { className: "dot dot-live", style: { animationDuration: `${dur.toFixed(2)}s` } };
  }
  if (h.state === "down") {
    return h.consecutive_failures >= 2
      ? { className: "dot dot-down" }
      : { className: "dot dot-retrying" };
  }
  return { className: "dot dot-probing" };
}

export function Upstreams({ state, tunnel, onChange }: Props) {
  const [label, setLabel] = useState("");
  const [kind, setKind] = useState<UpstreamKind>("socks5");
  const [url, setUrl] = useState("");
  const [paste, setPaste] = useState("");
  const [formError, setFormError] = useState("");
  const [testingId, setTestingId] = useState<string | null>(null);
  const [testFor, setTestFor] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<TestResult | null>(null);
  // Channels currently being re-probed by "立即探测" (per-card testing state).
  const [probingIds, setProbingIds] = useState<Set<string>>(new Set());
  const probeStartRef = useRef(0);

  // Pointer-based drag-to-reorder (HTML5 DnD is unreliable in WKWebView/Tauri).
  const [dragId, setDragId] = useState<string | null>(null);
  const [liveOrder, setLiveOrder] = useState<string[] | null>(null);
  const draggingRef = useRef<string | null>(null);
  const liveOrderRef = useRef<string[] | null>(null);
  const origOrderRef = useRef<string[]>([]);
  const listRef = useRef<HTMLUListElement>(null);

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      const id = draggingRef.current;
      if (!id || !listRef.current) return;
      const items = Array.from(
        listRef.current.querySelectorAll<HTMLElement>("[data-up-id]")
      );
      let targetId: string | null = null;
      for (const el of items) {
        if (el.dataset.upId === id) continue;
        const r = el.getBoundingClientRect();
        if (e.clientY < r.top + r.height / 2) {
          targetId = el.dataset.upId ?? null;
          break;
        }
      }
      const cur = liveOrderRef.current ?? origOrderRef.current;
      const without = cur.filter((x) => x !== id);
      if (targetId == null) {
        without.push(id);
      } else {
        const idx = without.indexOf(targetId);
        without.splice(idx, 0, id);
      }
      liveOrderRef.current = without;
      setLiveOrder(without);
    };
    const onUp = () => {
      if (!draggingRef.current) return;
      draggingRef.current = null;
      setDragId(null);
      const live = liveOrderRef.current;
      liveOrderRef.current = null;
      setLiveOrder(null);
      if (live && live.join() !== origOrderRef.current.join()) {
        api.reorderUpstreams(live).then(onChange);
      }
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, [onChange]);

  // Clear each card's "testing" state the moment its fresh probe result arrives
  // (health.last_checked stamped after this probe round started).
  useEffect(() => {
    if (!state) return;
    setProbingIds((prev) => {
      if (!prev.size) return prev;
      const next = new Set(prev);
      for (const { upstream, health } of state.upstreams) {
        if (
          next.has(upstream.id) &&
          health.last_checked != null &&
          health.last_checked >= probeStartRef.current
        ) {
          next.delete(upstream.id);
        }
      }
      return next.size === prev.size ? prev : next;
    });
  }, [state]);

  if (!state) return null;

  const startDrag = (e: React.MouseEvent, id: string) => {
    e.preventDefault();
    e.stopPropagation();
    const ids = state.upstreams.map(({ upstream }) => upstream.id);
    origOrderRef.current = ids;
    liveOrderRef.current = ids;
    draggingRef.current = id;
    setLiveOrder(ids);
    setDragId(id);
  };

  // Render in the live (in-flight) order while dragging, else the real order.
  const viewMap = new Map(state.upstreams.map((v) => [v.upstream.id, v]));
  const ordered = liveOrder
    ? (liveOrder.map((id) => viewMap.get(id)).filter(Boolean) as typeof state.upstreams)
    : state.upstreams;

  const activeMode = MODES.find((m) => m.value === state.mode);
  const needsPin = state.mode === "fixed" && !state.pinned_id;

  const activeTunnelId = (() => {
    if (state.mode === "fixed" || !tunnel?.upstream_label) return null;
    return (
      state.upstreams.find(({ upstream }) => upstream.label === tunnel.upstream_label)
        ?.upstream.id ?? null
    );
  })();

  const onPaste = (raw: string) => {
    setPaste(raw);
    setFormError("");
    const p = parseProxyInput(raw);
    if (p) {
      setKind(p.kind);
      setUrl(p.url);
      if (!label.trim()) setLabel(p.label);
    }
  };

  const add = async () => {
    const parsedPaste = parseProxyInput(paste);
    const parsedUrl = parseProxyInput(url);
    const upstream = parsedPaste ?? parsedUrl;
    const nextKind = upstream?.kind ?? kind;
    const nextUrl = nextKind === "direct" ? "" : upstream?.url ?? url.trim();
    const name = label.trim() || upstream?.label || "";

    if (!name) {
      setFormError("请填写名称，或粘贴 host:port:user:pass。");
      return;
    }
    if (nextKind !== "direct" && !nextUrl) {
      setFormError("请填写代理地址，支持 host:port:user:pass 或 socks5h://...");
      return;
    }
    if (nextKind !== "direct" && !parseProxyInput(nextUrl)) {
      setFormError("代理地址格式不正确。");
      return;
    }

    setFormError("");
    const s = await api.addUpstream(name, nextKind, nextUrl);
    onChange(s);
    setLabel("");
    setUrl("");
    setPaste("");
  };

  const probe = async () => {
    if (probingIds.size) return;
    probeStartRef.current = Date.now();
    setProbingIds(
      new Set(state.upstreams.filter((v) => v.upstream.enabled).map((v) => v.upstream.id))
    );
    try {
      onChange(await api.probeNow());
    } finally {
      // Safety net: clear any card still marked testing if its result never lands.
      setTimeout(() => setProbingIds(new Set()), 20000);
    }
  };

  const runTest = async (id: string) => {
    setTestingId(id);
    setTestFor(id);
    setTestResult(null);
    try {
      setTestResult(await api.testUpstream(id));
    } catch (e) {
      setTestResult({
        ok: false,
        upstream_label: "",
        latency_ms: null,
        exit_ip: null,
        exit_geo: null,
        exit_org: null,
        status_reachable: false,
        status_latency_ms: null,
        status_indicator: null,
        status_desc: null,
        error: String(e),
      });
    } finally {
      setTestingId(null);
    }
  };

  return (
    <section className="panel">
      <div className="panel-head">
        <h2>上游代理</h2>
        <button
          className={"btn btn-ghost btn-sm" + (probingIds.size ? " busy" : "")}
          disabled={probingIds.size > 0}
          onClick={probe}
        >
          {probingIds.size > 0 && <span className="spinner" />}
          {probingIds.size > 0 ? "探测中" : "立即探测"}
        </button>
      </div>

      <div className="seg">
        {MODES.map((m) => (
          <button
            key={m.value}
            className={"seg-btn seg-mode-" + m.value + (state.mode === m.value ? " on" : "")}
            onClick={() => api.setMode(m.value).then(onChange)}
          >
            {m.label}
          </button>
        ))}
      </div>
      <p className="mode-desc muted small">
        {activeMode?.hint}
        {needsPin && <span className="warn-text"> 请点击一个通道卡片来选定</span>}
      </p>

      <ul className="upstream-list" ref={listRef}>
        {ordered.map(({ upstream: u, health: h }) => {
          const pinned = state.pinned_id === u.id;
          const isActive = activeTunnelId === u.id;
          const isCardProbing = probingIds.has(u.id);
          const isCircuitBroken = h.state === "down" && h.consecutive_failures >= 2;
          const isRetrying = h.state === "down" && h.consecutive_failures < 2;
          const dot = isCardProbing
            ? { className: "dot dot-testing", style: undefined }
            : dotProps(h, u.enabled);

          return (
            <li
              key={u.id}
              data-up-id={u.id}
              className={[
                "upstream",
                !u.enabled ? "disabled" : "",
                state.mode === "fixed" ? "upstream-selectable" : "",
                pinned && state.mode === "fixed" ? "upstream-selected" : "",
                isActive ? "upstream-active" : "",
                isCardProbing ? "upstream-probing" : "",
                testFor === u.id ? "upstream-pop" : "",
                dragId === u.id ? "upstream-dragging" : "",
              ]
                .filter(Boolean)
                .join(" ")}
              onClick={() => {
                if (dragId) return;
                if (state.mode === "fixed") {
                  api.setPinned(pinned ? null : u.id).then(onChange);
                }
              }}
            >
              <div className="up-row1">
                <span
                  className="drag-handle"
                  title="拖拽排序"
                  onMouseDown={(e) => startDrag(e, u.id)}
                  onClick={(e) => e.stopPropagation()}
                >
                  ⠿
                </span>
                <span className={dot.className} style={dot.style} title={h.last_error ?? h.state} />
                <strong className="up-label">{u.label}</strong>
                <span className="tag">{u.kind}</span>
                {pinned && state.mode === "fixed" && (
                  <span className="tag tag-selected">已选</span>
                )}
                {pinned && state.mode !== "fixed" && (
                  <span className="tag tag-pin">固定</span>
                )}
                {isActive && <span className="tag tag-active">当前</span>}
                {isCardProbing && <span className="tag tag-testing">测试中</span>}
                {!isCardProbing && u.enabled && isRetrying && (
                  <span className="tag tag-retry">重试</span>
                )}
                {!isCardProbing && u.enabled && isCircuitBroken && (
                  <span className="tag tag-broken">熔断</span>
                )}
                <span className="grow" />
                <span className="up-latency">
                  {isCardProbing ? <span className="spinner" /> : fmtMs(h.latency_ms)}
                </span>
              </div>
              {u.url && <div className="up-url muted">{maskUrl(u.url)}</div>}
              <div className="up-actions" onClick={(e) => e.stopPropagation()}>
                <span className="muted small" style={{ fontVariantNumeric: "tabular-nums" }}>
                  {h.success}/{h.success + h.failure}
                </span>
                <span className="grow" />
                {state.mode !== "fixed" && (
                  <button
                    className={"mini" + (pinned ? " on" : "")}
                    onClick={() => api.setPinned(pinned ? null : u.id).then(onChange)}
                  >
                    {pinned ? "取消固定" : "固定"}
                  </button>
                )}
                <button
                  className={"mini" + (testingId === u.id ? " busy" : "")}
                  disabled={testingId === u.id}
                  onClick={() => runTest(u.id)}
                >
                  {testingId === u.id && <span className="spinner" />}
                  {testingId === u.id ? "测试中" : "测试"}
                </button>
                <label
                  className="toggle"
                  title={u.enabled ? "点击停用" : "点击启用"}
                  onClick={(e) => e.stopPropagation()}
                >
                  <input
                    type="checkbox"
                    checked={u.enabled}
                    onChange={() => api.setUpstreamEnabled(u.id, !u.enabled).then(onChange)}
                  />
                  <span className="toggle-track">
                    <span className="toggle-thumb" />
                  </span>
                </label>
                {u.kind !== "direct" && (
                  <button
                    className="mini danger"
                    onClick={() => api.removeUpstream(u.id).then(onChange)}
                  >
                    删除
                  </button>
                )}
              </div>
              {testFor === u.id && (testingId === u.id || testResult) && (
                <div className="test-popover" onClick={(e) => e.stopPropagation()}>
                  <button
                    className="test-close"
                    title="关闭"
                    onClick={() => {
                      setTestFor(null);
                      setTestResult(null);
                    }}
                  >
                    ×
                  </button>
                  {testingId === u.id && !testResult ? (
                    <div className="test-loading">
                      <span className="spinner" />
                      正在测试出口 IP 与 Anthropic 可达性…
                    </div>
                  ) : (
                    testResult && <TestView r={testResult} />
                  )}
                </div>
              )}
            </li>
          );
        })}
      </ul>

      <input
        className="paste-input"
        placeholder="粘贴 host:port:user:pass 快速添加"
        value={paste}
        onChange={(e) => onPaste(e.target.value)}
      />
      <div className="add-form">
        <input placeholder="名称" value={label} onChange={(e) => setLabel(e.target.value)} />
        <select value={kind} onChange={(e) => setKind(e.target.value as UpstreamKind)}>
          <option value="socks5">socks5</option>
          <option value="http">http</option>
          <option value="direct">direct</option>
        </select>
        <input
          placeholder={kind === "direct" ? "(无需地址)" : "socks5h://host:1080"}
          value={url}
          disabled={kind === "direct"}
          onChange={(e) => setUrl(e.target.value)}
        />
        <button className="btn btn-sm" onClick={add}>
          +
        </button>
        {formError && <div className="form-error">{formError}</div>}
      </div>
    </section>
  );
}

function TestView({ r }: { r: TestResult }) {
  return (
    <div className={"test-out" + (r.ok ? " ok" : " bad")}>
      <div className="test-header">
        <span className="test-badge">{r.ok ? "连通" : "失败"}</span>
      </div>
      <div className="test-line">
        <span>出口 IP</span>
        <span>{r.exit_ip ? `${r.exit_ip}${r.exit_geo ? ` (${r.exit_geo})` : ""}` : "—"}</span>
      </div>
      {r.exit_org && (
        <div className="test-line">
          <span>归属</span>
          <span>{r.exit_org}</span>
        </div>
      )}
      <div className="test-line">
        <span>隧道延迟</span>
        <span>{fmtMs(r.latency_ms)}</span>
      </div>
      <div className="test-line">
        <span>Anthropic 状态页</span>
        <span className={r.status_reachable ? "" : "bad"}>
          {r.status_reachable
            ? `可达${r.status_latency_ms != null ? ` (${fmtMs(r.status_latency_ms)})` : ""}`
            : "不可达"}
        </span>
      </div>
      {r.status_desc && (
        <div className="test-line">
          <span>系统状态</span>
          <span>{r.status_desc}</span>
        </div>
      )}
      {r.error && <div className="test-err">{r.error}</div>}
    </div>
  );
}
