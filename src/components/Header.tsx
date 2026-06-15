import type { AppStateView, TunnelStatus } from "../types";

interface Props {
  state: AppStateView | null;
  tunnel: TunnelStatus | null;
  theme: "system" | "light" | "dark";
  onThemeChange: (theme: "system" | "light" | "dark") => void;
}

export function Header({ state, tunnel, theme, onThemeChange }: Props) {
  const running = tunnel?.running ?? state?.running ?? false;
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

      <div className="intercept-status">
        <span className="muted small">settings.json</span>
        <code>{interceptOn ? `ANTHROPIC_BASE_URL → :${port}` : "未接管"}</code>
      </div>

      <label className="theme-switch">
        <span className="muted small">主题</span>
        <select value={theme} onChange={(e) => onThemeChange(e.target.value as Props["theme"])}>
          <option value="system">跟随系统</option>
          <option value="light">浅色</option>
          <option value="dark">深色</option>
        </select>
      </label>
    </header>
  );
}
