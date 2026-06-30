import { useEffect, useState } from "react";
import type { DayStat, Stats, TrafficSnapshot, Trends } from "../types";
import { api } from "../api";
import { fmtBytes, fmtCompact, fmtCost, fmtNum, shortModel } from "../format";

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

function deltaInfo(cur: number, prev: number): { text: string; cls: string } | null {
  if (prev === 0) return cur > 0 ? { text: "新增", cls: "up" } : null;
  const p = ((cur - prev) / prev) * 100;
  if (Math.abs(p) < 1) return { text: "持平", cls: "flat" };
  return { text: `${p >= 0 ? "+" : ""}${p.toFixed(0)}%`, cls: p >= 0 ? "up" : "down" };
}

export function StatsPanel({
  stats,
  traffic,
  trafficRate,
  sinceTs,
  onSinceChange,
  onClear,
}: Props) {
  const [trends, setTrends] = useState<Trends | null>(null);

  // Refetch trends whenever stats refresh (a new request landed) or on mount.
  useEffect(() => {
    api.getTrends().then(setTrends).catch(() => {});
  }, [stats]);

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
        <Card label="请求总数" value={fmtCompact(stats.total_requests)} title={fmtNum(stats.total_requests)} />
        <Card label="输入 Token" value={fmtCompact(stats.total_input)} title={fmtNum(stats.total_input)} />
        <Card label="输出 Token" value={fmtCompact(stats.total_output)} title={fmtNum(stats.total_output)} />
        <Card label="缓存 Token" value={fmtCompact(cacheTotal)} title={fmtNum(cacheTotal)} />
        <Card label="总成本" value={fmtCost(stats.total_cost)} accent />
        <Card label="异常" value={fmtNum(stats.errors)} warn={stats.errors > 0} />
      </div>

      {trends && (
        <>
          <div className="panel-head">
            <h2>趋势</h2>
          </div>
          <div className="trend-cards">
            <TrendCard title="今天" d={trends.today} compare={trends.yesterday} />
            <TrendCard title="昨天" d={trends.yesterday} />
            <TrendCard title="最近 7 天" d={trends.last7} />
          </div>
        </>
      )}

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
            <span className="num" title={fmtNum(m.requests)}>{fmtCompact(m.requests)}</span>
            <span className="num" title={fmtNum(m.input_tokens)}>{fmtCompact(m.input_tokens)}</span>
            <span className="num" title={fmtNum(m.output_tokens)}>{fmtCompact(m.output_tokens)}</span>
            <span className="num">{fmtCost(m.cost_usd)}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function TrendCard({ title, d, compare }: { title: string; d: DayStat; compare?: DayStat }) {
  const tokens = d.input + d.output;
  const delta = compare ? deltaInfo(d.requests, compare.requests) : null;
  return (
    <div className="trend-card">
      <div className="trend-head">
        <span className="trend-title">{title}</span>
        {delta && <span className={"trend-delta " + delta.cls}>{delta.text}</span>}
      </div>
      <div className="trend-rows">
        <TR k="请求" v={fmtCompact(d.requests)} title={fmtNum(d.requests)} />
        <TR k="Token" v={fmtCompact(tokens)} title={fmtNum(tokens)} />
        <TR k="成本" v={fmtCost(d.cost)} />
        {d.errors > 0 && <TR k="异常" v={fmtNum(d.errors)} bad />}
      </div>
    </div>
  );
}

function TR({ k, v, title, bad }: { k: string; v: string; title?: string; bad?: boolean }) {
  return (
    <div className="trend-row">
      <span className="muted small">{k}</span>
      <span className={"trend-val" + (bad ? " bad" : "")} title={title}>
        {v}
      </span>
    </div>
  );
}

function Card({
  label,
  value,
  accent,
  warn,
  title,
}: {
  label: string;
  value: string;
  accent?: boolean;
  warn?: boolean;
  title?: string;
}) {
  return (
    <div className={"card" + (accent ? " accent" : "") + (warn ? " warn" : "")} title={title}>
      <div className="card-value">{value}</div>
      <div className="card-label">{label}</div>
    </div>
  );
}
