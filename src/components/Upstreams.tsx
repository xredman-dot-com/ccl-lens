import { useState } from "react";
import type { AppStateView, SelectMode, UpstreamKind } from "../types";
import { api } from "../api";
import { fmtMs } from "../format";
import { parseProxyInput } from "../parse";

const MODES: { value: SelectMode; label: string; hint: string }[] = [
  { value: "fixed", label: "固定", hint: "永远用 pin 的节点，不切换" },
  { value: "sticky", label: "粘性优先", hint: "优先 pin；挂了自动切，恢复切回" },
  { value: "auto", label: "自动最快", hint: "始终选延迟最低的健康节点" },
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

  if (!state) return null;

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

  return (
    <section className="panel">
      <div className="panel-head">
        <h2>上游池 / SOCKS</h2>
        <button className="btn btn-ghost btn-sm" onClick={() => api.probeNow().then(onChange)}>
          立即探测
        </button>
      </div>

      <div className="mode-row">
        {MODES.map((m) => (
          <button
            key={m.value}
            title={m.hint}
            className={"chip" + (state.mode === m.value ? " chip-on" : "")}
            onClick={() => api.setMode(m.value).then(onChange)}
          >
            {m.label}
          </button>
        ))}
      </div>

      <ul className="upstream-list">
        {state.upstreams.map(({ upstream: u, health: h }) => {
          const pinned = state.pinned_id === u.id;
          return (
            <li key={u.id} className={"upstream" + (u.enabled ? "" : " disabled")}>
              <span className={"dot dot-" + h.state} title={h.last_error ?? h.state} />
              <div className="upstream-main">
                <div className="upstream-title">
                  <strong>{u.label}</strong>
                  <span className="tag">{u.kind}</span>
                  {pinned && <span className="tag tag-pin">PIN</span>}
                </div>
                <div className="muted upstream-url">{u.url || "(直连)"}</div>
              </div>
              <div className="upstream-meta">
                <span>{fmtMs(h.latency_ms)}</span>
                <span className="muted small">
                  {h.success}/{h.success + h.failure}
                </span>
              </div>
              <div className="upstream-actions">
                <button
                  className={"icon-btn" + (pinned ? " on" : "")}
                  title="设为优先"
                  onClick={() => api.setPinned(pinned ? null : u.id).then(onChange)}
                >
                  ◎
                </button>
                <button
                  className="icon-btn"
                  title={u.enabled ? "停用" : "启用"}
                  onClick={() => api.setUpstreamEnabled(u.id, !u.enabled).then(onChange)}
                >
                  {u.enabled ? "‖" : "▶"}
                </button>
                {u.kind !== "direct" && (
                  <button
                    className="icon-btn danger"
                    title="删除"
                    onClick={() => api.removeUpstream(u.id).then(onChange)}
                  >
                    ✕
                  </button>
                )}
              </div>
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
        <input
          placeholder="名称"
          value={label}
          onChange={(e) => setLabel(e.target.value)}
        />
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
