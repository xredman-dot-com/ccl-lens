import type { ServiceIncident, ServiceStatus } from "../types";
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
  status: ServiceStatus | null;
  statusBusy: boolean;
  statusErr: string | null;
  onRefreshStatus: () => void;
}

export function AccountPanel({ status, statusBusy, statusErr, onRefreshStatus }: Props) {
  return (
    <div className="account-panel">
      <section className="panel">
        <div className="panel-head">
          <h2>服务状态与公告</h2>
          <button className="btn-mini" onClick={onRefreshStatus} disabled={statusBusy}>
            {statusBusy ? "刷新中" : "刷新"}
          </button>
        </div>
        <p className="muted small">账号与实时配额已移至顶部栏（悬停查看详情）。</p>
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
