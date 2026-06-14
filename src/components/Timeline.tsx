import type { RequestRecord } from "../types";
import { fmtCost, fmtMs, fmtNum, fmtTime, shortModel } from "../format";

interface Props {
  requests: RequestRecord[];
  selectedId: string | null;
  onSelect: (r: RequestRecord) => void;
}

function statusClass(r: RequestRecord): string {
  if (r.error) return "bad";
  if (r.status && r.status >= 400) return "bad";
  if (r.status && r.status >= 200 && r.status < 300) return "ok";
  return "pending";
}

export function Timeline({ requests, selectedId, onSelect }: Props) {
  if (requests.length === 0) {
    return (
      <div className="empty">
        还没有请求。启动拦截后，在任意项目运行 <code>claude</code> 即可看到流量。
      </div>
    );
  }
  return (
    <div className="table">
      <div className="trow thead">
        <span>时间</span>
        <span>模型</span>
        <span>状态</span>
        <span>TTFB</span>
        <span>时长</span>
        <span className="num">In</span>
        <span className="num">Out</span>
        <span className="num">Cache</span>
        <span className="num">成本</span>
        <span>上游</span>
      </div>
      {requests.map((r) => (
        <div
          key={r.id}
          className={"trow" + (selectedId === r.id ? " sel" : "")}
          onClick={() => onSelect(r)}
        >
          <span>{fmtTime(r.ts)}</span>
          <span className="model">{shortModel(r.model)}</span>
          <span className={"status " + statusClass(r)}>
            {r.error ? "ERR" : r.status ?? "…"}
          </span>
          <span>{fmtMs(r.ttfb_ms)}</span>
          <span>{fmtMs(r.duration_ms)}</span>
          <span className="num">{fmtNum(r.input_tokens)}</span>
          <span className="num">{fmtNum(r.output_tokens)}</span>
          <span className="num muted">{fmtNum(r.cache_read_tokens)}</span>
          <span className="num">{fmtCost(r.cost_usd)}</span>
          <span className="muted small">{r.upstream_label ?? "—"}</span>
        </div>
      ))}
    </div>
  );
}
