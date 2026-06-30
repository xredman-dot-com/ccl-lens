import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { ServiceStatus } from "../types";
import {
  combineNotices,
  fmtAgo,
  impactLabel,
  noticeUrl,
  severity,
  statusLabel,
} from "../status";

interface Props {
  status: ServiceStatus | null;
  busy: boolean;
  onRefresh: () => void;
}

const ROTATE_MS = 7000;

export function StatusBar({ status, busy, onRefresh }: Props) {
  const notices = combineNotices(status);
  const [idx, setIdx] = useState(0);

  // Keep index in range as the notice set changes.
  useEffect(() => {
    if (idx >= notices.length) setIdx(0);
  }, [notices.length, idx]);

  // Auto-rotate when there is more than one notice.
  useEffect(() => {
    if (notices.length <= 1) return;
    const t = window.setInterval(
      () => setIdx((i) => (i + 1) % notices.length),
      ROTATE_MS
    );
    return () => window.clearInterval(t);
  }, [notices.length]);

  const cur = notices[idx] ?? null;
  const sev = cur ? severity(cur.impact) : "";
  const url = cur ? noticeUrl(cur) : null;
  const ago = cur ? fmtAgo(cur.updated_at) : null;

  const open = () => {
    if (url) openUrl(url).catch(() => {});
  };

  return (
    <footer className={"statusbar" + (sev ? " " + sev : "")}>
      <span className={"sb-dot " + (cur ? sev || "info" : "op")} />

      {cur ? (
        <button
          className="sb-msg"
          onClick={open}
          disabled={!url}
          title={url ? `${cur.name}（点击查看详情）` : cur.name}
        >
          <span className="sb-badge">{cur.maint ? "维护" : impactLabel(cur.impact)}</span>
          <span className="sb-name">{cur.name}</span>
          {cur.affected.length > 0 && (
            <span className="sb-affected">影响：{cur.affected.join("、")}</span>
          )}
          <span className="sb-state">{statusLabel(cur.status)}</span>
          {ago && <span className="sb-ago">{ago}</span>}
        </button>
      ) : (
        <span className="sb-ok">
          {status?.description ?? (busy ? "正在获取服务状态…" : "服务状态未知")}
        </span>
      )}

      <span className="sb-spacer" />

      {notices.length > 1 && (
        <span className="sb-nav">
          <button
            className="sb-arrow"
            onClick={() => setIdx((i) => (i - 1 + notices.length) % notices.length)}
            title="上一条"
          >
            ‹
          </button>
          <span className="sb-count">
            {idx + 1}/{notices.length}
          </span>
          <button
            className="sb-arrow"
            onClick={() => setIdx((i) => (i + 1) % notices.length)}
            title="下一条"
          >
            ›
          </button>
        </span>
      )}

      <button className="sb-refresh" onClick={onRefresh} disabled={busy} title="刷新服务状态">
        {busy ? "刷新中" : "刷新"}
      </button>
    </footer>
  );
}
