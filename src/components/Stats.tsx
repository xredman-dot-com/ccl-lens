import type { Stats, TrafficSnapshot } from "../types";
import { fmtBytes, fmtCost, fmtNum, shortModel } from "../format";

interface Props {
  stats: Stats | null;
  traffic: TrafficSnapshot;
  trafficRate: { up: number; down: number };
  sinceTs: number | null;
  onSinceChange: (ts: number | null) => void;
  onClear: () => void;
}

function todayMidnight(): number {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

export function StatsPanel({
  stats,
  traffic,
  trafficRate,
  sinceTs,
  onSinceChange,
  onClear,
}: Props) {
  if (!stats) return null;
  const cacheTotal = stats.total_cache_read + stats.total_cache_creation;
  const totalBytes = stats.total_request_bytes + stats.total_response_bytes;
  const sessionBytes = traffic.session_request_bytes + traffic.session_response_bytes;
  const isFiltered = sinceTs !== null;

  return (
    <div className="stats">
      <div className="stats-filter">
        <button
          className={"filter-btn" + (isFiltered ? " on" : "")}
          onClick={() => onSinceChange(isFiltered ? null : todayMidnight())}
        >
          今天
        </button>
        <button
          className={"filter-btn" + (!isFiltered ? " on" : "")}
          onClick={() => onSinceChange(null)}
        >
          全部
        </button>
      </div>

      <div className="cards">
        <Card label="实时上传" value={`${fmtBytes(trafficRate.up)}/s`} />
        <Card label="实时下载" value={`${fmtBytes(trafficRate.down)}/s`} accent />
        <Card label="本次会话流量" value={fmtBytes(sessionBytes)} />
        <Card label={isFiltered ? "今日流量" : "历史总流量"} value={fmtBytes(totalBytes)} />
        <Card label="请求总数" value={fmtNum(stats.total_requests)} />
        <Card label="输入 Token" value={fmtNum(stats.total_input)} />
        <Card label="输出 Token" value={fmtNum(stats.total_output)} />
        <Card label="缓存 Token" value={fmtNum(cacheTotal)} />
        <Card label="总成本" value={fmtCost(stats.total_cost)} accent />
        <Card label="异常" value={fmtNum(stats.errors)} warn={stats.errors > 0} />
      </div>

      <div className="panel-head">
        <h2>模型统计</h2>
        <button className="btn btn-ghost btn-sm danger" onClick={onClear}>
          清空历史
        </button>
      </div>
      <div className="table">
        <div className="trow thead model-table">
          <span>模型</span>
          <span className="num">请求</span>
          <span className="num">输入</span>
          <span className="num">输出</span>
          <span className="num">成本</span>
        </div>
        {stats.by_model.map((m) => (
          <div key={m.model} className="trow model-table">
            <span className="model">{shortModel(m.model)}</span>
            <span className="num">{fmtNum(m.requests)}</span>
            <span className="num">{fmtNum(m.input_tokens)}</span>
            <span className="num">{fmtNum(m.output_tokens)}</span>
            <span className="num">{fmtCost(m.cost_usd)}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function Card({
  label,
  value,
  accent,
  warn,
}: {
  label: string;
  value: string;
  accent?: boolean;
  warn?: boolean;
}) {
  return (
    <div className={"card" + (accent ? " accent" : "") + (warn ? " warn" : "")}>
      <div className="card-value">{value}</div>
      <div className="card-label">{label}</div>
    </div>
  );
}
