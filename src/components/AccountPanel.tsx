import { useEffect, useState } from "react";
import type { AccountInfo, ServiceStatus, UsageSnapshot } from "../types";
import { api, onUsage } from "../api";
import { fmtTime } from "../format";

const PLAN_LABELS: Record<string, string> = {
  claude_max: "Claude Max",
  claude_pro: "Claude Pro",
  claude_team: "Claude Team",
  claude_enterprise: "Claude Enterprise",
};

const ROLE_LABELS: Record<string, string> = {
  admin: "管理员",
  owner: "所有者",
  member: "成员",
};

const WINDOW_LABELS: Record<string, string> = {
  five_hour: "5 小时",
  seven_day: "7 天",
  seven_day_opus: "7 天 · Opus",
  seven_day_sonnet: "7 天 · Sonnet",
  seven_day_oauth_apps: "7 天 · 第三方应用",
};

const WINDOW_ORDER = [
  "five_hour",
  "seven_day",
  "seven_day_opus",
  "seven_day_sonnet",
  "seven_day_oauth_apps",
];

function planLabel(t: string | null): string {
  if (!t) return "—";
  return PLAN_LABELS[t] ?? t;
}

function fmtReset(iso: unknown): string | null {
  if (typeof iso !== "string") return null;
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return null;
  const diff = t - Date.now();
  if (diff <= 0) return "已重置";
  const h = Math.floor(diff / 3600000);
  const m = Math.floor((diff % 3600000) / 60000);
  if (h >= 24) return `${Math.floor(h / 24)} 天 ${h % 24} 小时后重置`;
  if (h > 0) return `${h} 小时 ${m} 分后重置`;
  return `${m} 分后重置`;
}

interface UsageWindow {
  key: string;
  label: string;
  pct: number;
  reset: string | null;
}

function parseWindows(raw: Record<string, unknown> | undefined): UsageWindow[] {
  if (!raw) return [];
  const out: UsageWindow[] = [];
  for (const [key, val] of Object.entries(raw)) {
    if (!val || typeof val !== "object") continue;
    const obj = val as Record<string, unknown>;
    const util = obj.utilization;
    if (typeof util !== "number") continue;
    const pct = util > 1 ? util : util * 100;
    out.push({
      key,
      label: WINDOW_LABELS[key] ?? key,
      pct: Math.max(0, Math.min(100, pct)),
      reset: fmtReset(obj.resets_at),
    });
  }
  out.sort((a, b) => {
    const ia = WINDOW_ORDER.indexOf(a.key);
    const ib = WINDOW_ORDER.indexOf(b.key);
    return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib);
  });
  return out;
}

function barClass(pct: number): string {
  if (pct >= 90) return "usage-fill high";
  if (pct >= 70) return "usage-fill mid";
  return "usage-fill";
}

function dotClass(status: string): string {
  if (status === "operational" || status === "none") return "svc-dot op";
  if (status === "major_outage" || status === "critical") return "svc-dot down";
  return "svc-dot warn";
}

export function AccountPanel() {
  const [account, setAccount] = useState<AccountInfo | null>(null);
  const [usage, setUsage] = useState<UsageSnapshot | null>(null);
  const [status, setStatus] = useState<ServiceStatus | null>(null);
  const [statusErr, setStatusErr] = useState<string | null>(null);
  const [statusBusy, setStatusBusy] = useState(false);

  const loadStatus = () => {
    setStatusBusy(true);
    setStatusErr(null);
    api
      .getServiceStatus()
      .then((s) => setStatus(s))
      .catch((e) => setStatusErr(String(e)))
      .finally(() => setStatusBusy(false));
  };

  useEffect(() => {
    api.getAccount().then(setAccount);
    api.getUsage().then(setUsage);
    loadStatus();
    const un = onUsage(setUsage);
    const timer = window.setInterval(loadStatus, 60000);
    return () => {
      un.then((f) => f());
      window.clearInterval(timer);
    };
  }, []);

  const windows = parseWindows(usage?.raw);

  return (
    <div className="account-panel">
      <section className="panel">
        <div className="panel-head">
          <h2>账号信息</h2>
          <span className="muted small">读取本地 ~/.claude.json</span>
        </div>
        {account ? (
          <div className="conn-rows">
            <Row k="邮箱" v={account.email ?? "—"} />
            <Row k="名称" v={account.display_name ?? "—"} />
            <Row k="套餐" v={planLabel(account.organization_type)} strong />
            <Row k="组织" v={account.organization_name ?? "—"} />
            <Row
              k="角色"
              v={account.organization_role ? ROLE_LABELS[account.organization_role] ?? account.organization_role : "—"}
            />
            <Row k="限流档" v={account.rate_limit_tier ?? "—"} />
            <Row
              k="额外用量"
              v={account.has_extra_usage_enabled == null ? "—" : account.has_extra_usage_enabled ? "已开启" : "未开启"}
            />
          </div>
        ) : (
          <p className="muted small">未找到账号信息（Claude Code 是否已登录？）</p>
        )}
      </section>

      <section className="panel">
        <div className="panel-head">
          <h2>实时配额</h2>
          <span className="muted small">
            {usage ? `捕获于 ${fmtTime(usage.captured_at)}` : "来自 /api/oauth/usage"}
          </span>
        </div>
        {windows.length > 0 ? (
          <div className="usage-list">
            {windows.map((w) => (
              <div className="usage-item" key={w.key}>
                <div className="usage-row">
                  <span className="usage-label">{w.label}</span>
                  <span className="usage-pct">{w.pct.toFixed(0)}%</span>
                </div>
                <div className="usage-bar">
                  <div className={barClass(w.pct)} style={{ width: `${w.pct}%` }} />
                </div>
                {w.reset && <span className="muted small">{w.reset}</span>}
              </div>
            ))}
          </div>
        ) : usage ? (
          <pre className="usage-raw">{JSON.stringify(usage.raw, null, 2)}</pre>
        ) : (
          <p className="muted small">
            尚未捕获。在 Claude Code 中运行 <code>/usage</code> 后，这里会显示实时配额。
          </p>
        )}
      </section>

      <section className="panel">
        <div className="panel-head">
          <h2>服务状态</h2>
          <button className="btn-mini" onClick={loadStatus} disabled={statusBusy}>
            {statusBusy ? "刷新中" : "刷新"}
          </button>
        </div>
        {statusErr ? (
          <p className="muted small bad">{statusErr}</p>
        ) : status ? (
          <>
            <div className="conn-status">
              <span className={dotClass(status.indicator ?? "none")} />
              <span>{status.description ?? "—"}</span>
            </div>
            <div className="svc-list">
              {status.components.map((c) => (
                <div className="svc-item" key={c.name}>
                  <span className={dotClass(c.status)} />
                  <span className="svc-name">{c.name}</span>
                  <span className="muted small">{c.status.replace(/_/g, " ")}</span>
                </div>
              ))}
            </div>
            {status.incidents.length > 0 && (
              <div className="svc-incidents">
                <span className="muted small">进行中事件</span>
                {status.incidents.map((i, idx) => (
                  <div className="bad small" key={idx}>
                    {i}
                  </div>
                ))}
              </div>
            )}
          </>
        ) : (
          <p className="muted small">加载中…</p>
        )}
      </section>
    </div>
  );
}

function Row({ k, v, strong }: { k: string; v: string; strong?: boolean }) {
  return (
    <div className="conn-row">
      <span className="muted">{k}</span>
      <span className={"conn-val" + (strong ? " strong" : "")}>{v}</span>
    </div>
  );
}
