import { useState } from "react";
import type { AppStateView, SelectMode, TestResult, UpstreamKind } from "../types";
import { api } from "../api";
import { fmtMs } from "../format";
import { parseProxyInput, maskUrl } from "../parse";

const MODES: { value: SelectMode; label: string; hint: string }[] = [
  { value: "fixed", label: "固定", hint: "始终用你固定(PIN)的通道，不自动切换。" },
  { value: "sticky", label: "优先+兜底", hint: "优先用 PIN 的通道；它异常时自动切到健康通道，恢复后切回。" },
  { value: "auto", label: "自动择优", hint: "按延迟自动选最快的健康通道。" },
];

interface Props {
  state: AppStateView | null;
  onChange: (s: AppStateView) => void;
}

export function Upstreams({ state, onChange }: Props) {
  const [label, setLabel] = useState("");
  const [kind, setKind] = useState<UpstreamKind>("socks5");
  const [url, setUrl] = useState("");
  const [paste, setPaste] = useState("");
  const [testingId, setTestingId] = useState<string | null>(null);
  const [testFor, setTestFor] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<TestResult | null>(null);

  if (!state) return null;

  const activeMode = MODES.find((m) => m.value === state.mode);
  const needsPin =
    (state.mode === "fixed" || state.mode === "sticky") && !state.pinned_id;

  const onPaste = (raw: string) => {
    setPaste(raw);
    const p = parseProxyInput(raw);
    if (p) {
      setKind(p.kind);
      setUrl(p.url);
      if (!label.trim()) setLabel(p.label);
    }
  };

  const add = async () => {
    const name = label.trim() || (paste.trim() ? parseProxyInput(paste)?.label ?? "" : "");
    if (!name) return;
    const s = await api.addUpstream(name, kind, url.trim());
    onChange(s);
    setLabel("");
    setUrl("");
    setPaste("");
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
        <h2>上游池 / SOCKS</h2>
        <button className="btn btn-ghost btn-sm" onClick={() => api.probeNow().then(onChange)}>
          立即探测
        </button>
      </div>

      <div className="seg">
        {MODES.map((m) => (
          <button
            key={m.value}
            className={"seg-btn" + (state.mode === m.value ? " on" : "")}
            onClick={() => api.setMode(m.value).then(onChange)}
          >
            {m.label}
          </button>
        ))}
      </div>
      <p className="mode-desc muted small">
        {activeMode?.hint}
        {needsPin && <span className="warn-text"> 请点某个通道的「固定」来选定。</span>}
      </p>

      <ul className="upstream-list">
        {state.upstreams.map(({ upstream: u, health: h }) => {
          const pinned = state.pinned_id === u.id;
          return (
            <li key={u.id} className={"upstream" + (u.enabled ? "" : " disabled")}>
              <div className="up-row1">
                <span className={"dot dot-" + h.state} title={h.last_error ?? h.state} />
                <strong className="up-label">{u.label}</strong>
                <span className="tag">{u.kind}</span>
                {pinned && <span className="tag tag-pin">固定</span>}
                <span className="grow" />
                <span className="up-latency">{fmtMs(h.latency_ms)}</span>
              </div>
              {u.url && <div className="up-url muted">{maskUrl(u.url)}</div>}
              <div className="up-actions">
                <span className="muted small">
                  {h.success}/{h.success + h.failure}
                </span>
                <span className="grow" />
                <button className={"mini" + (pinned ? " on" : "")} onClick={() => api.setPinned(pinned ? null : u.id).then(onChange)}>
                  {pinned ? "取消固定" : "固定"}
                </button>
                <button className="mini" disabled={testingId === u.id} onClick={() => runTest(u.id)}>
                  {testingId === u.id ? "测试中…" : "测试"}
                </button>
                <button className="mini" onClick={() => api.setUpstreamEnabled(u.id, !u.enabled).then(onChange)}>
                  {u.enabled ? "停用" : "启用"}
                </button>
                {u.kind !== "direct" && (
                  <button className="mini danger" onClick={() => api.removeUpstream(u.id).then(onChange)}>
                    删除
                  </button>
                )}
              </div>
              {testFor === u.id && testResult && <TestView r={testResult} />}
            </li>
          );
        })}
      </ul>

      <input
        className="paste-input"
        placeholder="快捷粘贴  host:port:user:pass"
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
          placeholder={kind === "direct" ? "(无需地址)" : "socks5://host:1080"}
          value={url}
          disabled={kind === "direct"}
          onChange={(e) => setUrl(e.target.value)}
        />
        <button className="btn btn-sm" onClick={add}>
          添加
        </button>
      </div>
    </section>
  );
}

function TestView({ r }: { r: TestResult }) {
  return (
    <div className={"test-out" + (r.ok ? "" : " bad")}>
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
