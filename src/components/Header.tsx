import type { AppStateView } from "../types";

interface Props {
  state: AppStateView | null;
  busy: boolean;
  error: string | null;
  onToggle: () => void;
}

export function Header({ state, busy, error, onToggle }: Props) {
  const running = state?.running ?? false;
  const port = state?.port ?? 31415;
  const baseUrl = state?.claude_base_url ?? null;
  const interceptOn = !!baseUrl && baseUrl.includes(String(port));

  return (
    <header className="header">
      <div className="brand">
        <span className="brand-dot" data-on={running} />
        <div>
          <h1>ccl-lens</h1>
          <p className="muted">Claude Code 拦截代理 · 端口 {port}</p>
        </div>
      </div>

      <div className="header-right">
        <div className="intercept-status">
          <span className="muted">settings.json</span>
          <code>{interceptOn ? `ANTHROPIC_BASE_URL → :${port}` : "未接管"}</code>
        </div>
        <button
          className={running ? "btn btn-stop" : "btn btn-start"}
          onClick={onToggle}
          disabled={busy}
        >
          {busy ? "处理中…" : running ? "停止拦截" : "启动拦截"}
        </button>
      </div>

      {error && <div className="banner-error">{error}</div>}
    </header>
  );
}
