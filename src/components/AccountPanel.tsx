import type { AccountInfo, ServiceIncident, ServiceStatus, UsageSnapshot } from "../types";
import { fmtTime } from "../format";
import { parseWindows, usageLevel } from "../usage";
import { roleLabel } from "../account";
import { fmtAgo, impactLabel, severity, statusLabel } from "../status";

function dotClass(status: string): string {
  if (status === "operational" || status === "none") return "svc-dot op";
  if (status === "major_outage" || status === "critical") return "svc-dot down";
  return "svc-dot warn";
}

function IncidentCard({ inc, maint }: { inc: ServiceIncident; maint?: boolean }) {
  const ago = fmtAgo(inc.updated_at);
  return (
    <div className={"svc-inc " + severity(inc.impact)}>
      <div className="svc-inc-head">
        <span className="inc-badge">{maint ? "维护" : impactLabel(inc.impact)}</span>
        <span className="inc-name">{inc.name}</span>
        <span className="muted small">{statusLabel(inc.status)}</span>
      </div>
      {inc.affected.length > 0 && (
        <div className="muted small">影响：{inc.affected.join("、")}</div>
      )}
      {inc.latest_update && <div className="inc-body small">{inc.latest_update}</div>}
      {ago && <div className="muted small">{ago}</div>}
    </div>
  );
}

interface Props {
  account: AccountInfo | null;
  usage: UsageSnapshot | null;
  status: ServiceStatus | null;
  statusBusy: boolean;
  statusErr: string | null;
  onRefreshStatus: () => void;
}

export function AccountPanel({
  account,
  usage,
  status,
  statusBusy,
  statusErr,
  onRefreshStatus,
}: Props) {
  const windows = parseWindows(usage);

  return (
    <div className="account-panel">
      <section className="panel">
        <div className="panel-head">
          <h2>账号信息</h2>
          <span className="muted small">读取本地 ~/.claude.json</span>
        </div>
        {account ? (
          <div className="conn-rows">
            <Row k="组织" v={account.organization_name ?? "—"} />
            <Row k="角色" v={roleLabel(account.organization_role)} />
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
                  <div
                    className={"usage-fill " + usageLevel(w.pct)}
                    style={{ width: `${w.pct}%` }}
                  />
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
          <h2>服务状态与公告</h2>
          <button className="btn-mini" onClick={onRefreshStatus} disabled={statusBusy}>
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

            {(status.incidents.length > 0 || status.maintenances.length > 0) && (
              <div className="svc-notices">
                {status.incidents.map((i, idx) => (
                  <IncidentCard key={`i${idx}`} inc={i} />
                ))}
                {status.maintenances.map((m, idx) => (
                  <IncidentCard key={`m${idx}`} inc={m} maint />
                ))}
              </div>
            )}

            <div className="svc-list">
              {status.components.map((c) => (
                <div className="svc-item" key={c.name}>
                  <span className={dotClass(c.status)} />
                  <span className="svc-name">{c.name}</span>
                  <span className="muted small">{c.status.replace(/_/g, " ")}</span>
                </div>
              ))}
            </div>
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
