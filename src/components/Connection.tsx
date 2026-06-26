import { useEffect, useRef, useState } from "react";
import type { AppStateView, TakeoverMode, TunnelStatus } from "../types";
import { fmtMs } from "../format";

const MODES: { value: TakeoverMode; label: string; hint: string }[] = [
  { value: "config", label: "改主配置", hint: "写入 ~/.claude/settings.json 的 Proxy，CC 自动走代理（可逆）" },
  { value: "env", label: "环境变量", hint: "不改配置，手动导出 HTTPS_PROXY/HTTP_PROXY" },
  { value: "test", label: "仅测隧道", hint: "不改配置，仅绑定端口并验证上游隧道" },
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
  const running = tunnel?.running ?? state?.running ?? false;
  const upKey = tunnel?.upstream_endpoint ?? tunnel?.upstream_label ?? null;
  const tunnelOk = tunnel?.tunnel_ok ?? false;
  const prevKey = useRef<string | null>(null);
  const timers = useRef<number[]>([]);
  // Visualises the channel hand-off: 终止旧隧道 → 建立新隧道 → 正常.
  const [phase, setPhase] = useState<null | "tear" | "build">(null);

  // Kick off the switch sequence whenever the active upstream identity changes.
  useEffect(() => {
    const prev = prevKey.current;
    prevKey.current = upKey;
    if (!running) {
      setPhase(null);
      return;
    }
    if (prev && upKey && prev !== upKey) {
      timers.current.forEach(clearTimeout);
      timers.current = [
        window.setTimeout(() => setPhase("build"), 650),
        window.setTimeout(() => setPhase(null), 4000),
      ];
      setPhase("tear");
    }
  }, [upKey, running]);

  // Resolve to 正常 the moment the freshly-built tunnel reports healthy.
  useEffect(() => {
    if (phase === "build" && tunnelOk) {
      const t = window.setTimeout(() => setPhase(null), 900);
      return () => clearTimeout(t);
    }
  }, [phase, tunnelOk]);

  useEffect(() => () => timers.current.forEach(clearTimeout), []);

  if (!state) return null;
  const port = state.port;
  const mode = state.takeover_mode;

  const upstreamVal = tunnel?.upstream_endpoint
    ? `${(tunnel.upstream_kind ?? "").toUpperCase()} ${tunnel.upstream_endpoint}`
    : tunnel?.upstream_label ?? "—";

  const tunnelVal = tunnel?.error
    ? `异常：${tunnel.error}`
    : tunnel?.tunnel_ok
      ? `正常 (${fmtMs(tunnel.tunnel_latency_ms)})`
      : "探测中";

  const exitIpVal = tunnel?.exit_ip
    ? `${tunnel.exit_ip}${tunnel.exit_geo ? ` (${tunnel.exit_geo})` : ""}`
    : running
      ? "检测中..."
      : "—";

  return (
    <section className="panel conn">
      <div className="panel-head">
        <h2>连接管理</h2>
      </div>

      <div className="conn-status">
        <span
          className={"dot dot-up" + (phase ? " dot-switching" : "")}
          data-off={!running}
        />
        <span className={running ? "" : "muted"}>
          {phase ? "切换通道中" : running ? "运行中" : "已停止"}
        </span>
      </div>

      {phase && (
        <div className="conn-switch">
          <span className="switch-steps">
            <span className={"switch-step" + (phase === "tear" ? " on" : " done")}>
              终止旧隧道
            </span>
            <span className="switch-arrow">→</span>
            <span className={"switch-step" + (phase === "build" ? " on" : "")}>
              建立新隧道
            </span>
            <span className="switch-arrow">→</span>
            <span className="switch-step">正常</span>
          </span>
        </div>
      )}

      <div className="conn-sep" />

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
        {running && (
          <>
            <Row
              k="隧道"
              v={tunnelVal}
              bad={!!tunnel?.error}
              muted={!tunnel?.tunnel_ok && !tunnel?.error}
            />
            <Row k="上游" v={upstreamVal} animKey={upstreamVal} />
            <Row k="出口 IP" v={exitIpVal} muted={running && !tunnel?.exit_ip} />
          </>
        )}
      </div>

      {mode === "env" && (
        <div className="env-hint">
          <span className="muted small">Shell 环境变量</span>
          <code>export HTTPS_PROXY=http://127.0.0.1:{port}</code>
          <code>export HTTP_PROXY=http://127.0.0.1:{port}</code>
          <code>export NODE_EXTRA_CA_CERTS=~/.ccl-lens/ca.crt</code>
          <span className="muted small">第三行用于解密 api.anthropic.com 以采集模型/Token/成本</span>
        </div>
      )}

      <button
        className={running ? "btn btn-stop wide" : "btn btn-start wide"}
        onClick={onToggle}
        disabled={busy}
      >
        {busy ? "处理中" : running ? "停止代理" : "启动代理"}
      </button>

      {error && <div className="banner-error">{error}</div>}
    </section>
  );
}

function Row({
  k,
  v,
  bad,
  muted,
  animKey,
}: {
  k: string;
  v: string;
  bad?: boolean;
  muted?: boolean;
  animKey?: string;
}) {
  return (
    <div className="conn-row">
      <span className="muted">{k}</span>
      <span
        key={animKey ?? v}
        className={"conn-val" + (bad ? " bad" : muted ? " muted" : "")}
      >
        {v}
      </span>
    </div>
  );
}
