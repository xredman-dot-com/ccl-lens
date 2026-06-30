import type { AccountInfo, AppStateView, TunnelStatus, UsageSnapshot } from "../types";
import { parseWindows, usageLevel } from "../usage";
import { planLabel, roleLabel } from "../account";
import { fmtTime } from "../format";

interface Props {
  state: AppStateView | null;
  tunnel: TunnelStatus | null;
  account: AccountInfo | null;
  usage: UsageSnapshot | null;
  theme: "system" | "light" | "dark";
  onThemeChange: (theme: "system" | "light" | "dark") => void;
}

export function Header({ state, tunnel, account, usage, theme, onThemeChange }: Props) {
  const running = tunnel?.running ?? state?.running ?? false;
  const port = state?.port ?? 31415;
  const proxy = state?.claude_proxy ?? null;
  const interceptOn = !!proxy && proxy.includes(String(port));
  const windows = parseWindows(usage);
  const acctName = account?.display_name || account?.email || null;
  const capturedAt = usage ? fmtTime(usage.captured_at) : null;

  const extraUsage =
    account?.has_extra_usage_enabled == null
      ? "—"
      : account.has_extra_usage_enabled
        ? "已开启"
        : "未开启";

  return (
    <header className="header">
      <div className="brand">
        <span className="brand-dot" data-on={running} />
        <div>
          <h1>ccl-lens</h1>
          <p className="muted">Claude Code 拦截代理 · 端口 {port}</p>
        </div>
      </div>

      {acctName && (
        <div className="hacct">
          <span className="hacct-name">{acctName}</span>
          {account?.email && account.email !== acctName && (
            <span className="hacct-email">{account.email}</span>
          )}
          {account?.organization_type && (
            <span className="hacct-plan">{planLabel(account.organization_type)}</span>
          )}
          <div className="hpop">
            <PopRow k="组织" v={account?.organization_name ?? "—"} />
            <PopRow k="角色" v={roleLabel(account?.organization_role ?? null)} />
            <PopRow k="限流档" v={account?.rate_limit_tier ?? "—"} />
            <PopRow k="额外用量" v={extraUsage} />
          </div>
        </div>
      )}

      {windows.length > 0 && (
        <div className="hq">
          {windows.slice(0, 3).map((w) => (
            <span className="hq-item" key={w.key}>
              <span className="hq-label">{w.shortLabel}</span>
              <span className="hq-bar">
                <i className={usageLevel(w.pct)} style={{ width: `${w.pct}%` }} />
              </span>
              <span className="hq-pct">{w.pct.toFixed(0)}%</span>
            </span>
          ))}
          {capturedAt && <span className="hq-time">{capturedAt}</span>}
          <div className="hpop hq-pop">
            <div className="hpop-head">实时配额{capturedAt ? ` · 捕获于 ${capturedAt}` : ""}</div>
            {windows.map((w) => (
              <div className="hqp-item" key={w.key}>
                <div className="hqp-row">
                  <span>{w.label}</span>
                  <span className="hqp-pct">{w.pct.toFixed(0)}%</span>
                </div>
                <div className="hqp-bar">
                  <i className={usageLevel(w.pct)} style={{ width: `${w.pct}%` }} />
                </div>
                {w.reset && <span className="muted small">{w.reset}</span>}
              </div>
            ))}
          </div>
        </div>
      )}

      <div className="intercept-status">
        <span className="muted small">settings.json</span>
        <code>{interceptOn ? `Proxy → :${port}` : "未接管"}</code>
      </div>

      <label className="theme-switch">
        <select value={theme} onChange={(e) => onThemeChange(e.target.value as Props["theme"])}>
          <option value="system">跟随系统</option>
          <option value="light">浅色</option>
          <option value="dark">深色</option>
        </select>
      </label>
    </header>
  );
}

function PopRow({ k, v }: { k: string; v: string }) {
  return (
    <div className="hpop-row">
      <span className="muted">{k}</span>
      <span>{v}</span>
    </div>
  );
}
