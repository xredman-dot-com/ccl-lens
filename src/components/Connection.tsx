import type { AppStateView, TakeoverMode, TunnelStatus } from "../types";
import { fmtMs } from "../format";

const MODES: { value: TakeoverMode; label: string; hint: string }[] = [
  { value: "config", label: "改主配置", hint: "写入 ~/.claude/settings.json，CC 自动走代理（可逆）" },
  { value: "env", label: "环境变量", hint: "不改配置，自己导出 ANTHROPIC_BASE_URL" },
  { value: "test", label: "仅测隧道", hint: "不改配置，只绑定端口并验证上游隧道是否通" },
];

interface Props {
  state: AppStateView | null;
  tunnel: TunnelStatus | null;
  busy: boolean;
  error: string | null;
  onToggle: () => void;
  onSetMode: (m: TakeoverMode) => void;
}

export function Connection({ state, tunnel, busy, error, onToggle, onSetMode }: Props) {
  if (!state) return null;
  const running = tunnel?.running ?? state.running;
  const port = state.port;
  const mode = state.takeover_mode;

  return (
    <section className="panel conn">
      <div className="panel-head">
        <h2>连接管理</h2>
      </div>

      <div className="conn-status">
        <span className="dot dot-up" data-off={!running} />
        <span className={running ? "" : "muted"}>{running ? "运行中" : "已停止"}</span>
      </div>

      <div className="seg">
        {MODES.map((m) => (
          <button
            key={m.value}
            title={m.hint}
            className={"seg-btn" + (mode === m.value ? " on" : "")}
            disabled={running}
            onClick={() => onSetMode(m.value)}
          >
            {m.label}
          </button>
        ))}
      </div>

      <div className="conn-rows">
        <Row k="端口" v={String(port)} />
        <Row k="状态" v={running ? tunnel?.proxy_state ?? "ProxyReady" : "Stopped"} />
        {running && (
          <>
            <Row
              k="隧道"
              v={
                tunnel?.error
                  ? `异常：${tunnel.error}`
                  : tunnel?.tunnel_ok
                    ? `正常 (${fmtMs(tunnel.tunnel_latency_ms)})`
                    : "探测中…"
              }
              bad={!!tunnel?.error}
            />
            <Row
              k="上游"
              v={
                tunnel?.upstream_endpoint
                  ? `${(tunnel.upstream_kind ?? "").toUpperCase()} ${tunnel.upstream_endpoint}`
                  : tunnel?.upstream_label ?? "—"
              }
            />
            <Row
              k="出口 IP"
              v={
                tunnel?.exit_ip
                  ? `${tunnel.exit_ip}${tunnel.exit_geo ? ` (${tunnel.exit_geo})` : ""}`
                  : "—"
              }
            />
          </>
        )}
      </div>

      {mode === "env" && (
        <div className="env-hint">
          <span className="muted small">在你的 shell 里导出：</span>
          <code>export ANTHROPIC_BASE_URL=http://127.0.0.1:{port}</code>
        </div>
      )}

      <button
        className={running ? "btn btn-stop wide" : "btn btn-start wide"}
        onClick={onToggle}
        disabled={busy}
      >
        {busy ? "处理中…" : running ? "停止代理" : "启动代理"}
      </button>

      {error && <div className="banner-error">{error}</div>}
    </section>
  );
}

function Row({ k, v, bad }: { k: string; v: string; bad?: boolean }) {
  return (
    <div className="conn-row">
      <span className="muted">{k}</span>
      <span className={bad ? "bad" : ""}>{v}</span>
    </div>
  );
}
