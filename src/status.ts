import type { ServiceComponent, ServiceIncident, ServiceStatus } from "./types";

export interface Notice extends ServiceIncident {
  maint: boolean;
}

// Only the services Claude Code actually depends on.
const CC_KEYS = ["claude code", "api.anthropic.com", "claude api"];

export function isCCRelevant(name: string): boolean {
  const n = name.toLowerCase();
  return CC_KEYS.some((k) => n.includes(k));
}

/** "Claude API (api.anthropic.com)" -> "Claude API" */
export function shortComponent(name: string): string {
  return name.replace(/\s*\(.*\)\s*$/, "").trim();
}

export function ccComponents(status: ServiceStatus | null): ServiceComponent[] {
  return status?.components.filter((c) => isCCRelevant(c.name)) ?? [];
}

/** Notices that are global (no affected list) or touch a Claude Code service. */
export function ccNotices(status: ServiceStatus | null): Notice[] {
  return combineNotices(status).filter(
    (n) => n.affected.length === 0 || n.affected.some((a) => isCCRelevant(a))
  );
}

export const IMPACT_LABELS: Record<string, string> = {
  none: "公告",
  minor: "轻微",
  major: "较大",
  critical: "严重",
  maintenance: "维护",
};

export const INC_STATUS_LABELS: Record<string, string> = {
  investigating: "排查中",
  identified: "已定位",
  monitoring: "观察中",
  resolved: "已解决",
  postmortem: "复盘",
  scheduled: "计划中",
  in_progress: "进行中",
  verifying: "验证中",
  completed: "已完成",
};

/** "" (info) | "warn" (minor) | "down" (major/critical) */
export function severity(impact: string): "" | "warn" | "down" {
  if (impact === "critical" || impact === "major") return "down";
  if (impact === "minor") return "warn";
  return "";
}

export function impactLabel(impact: string): string {
  return IMPACT_LABELS[impact] ?? impact;
}

export function statusLabel(s: string): string {
  return INC_STATUS_LABELS[s] ?? s;
}

export function combineNotices(status: ServiceStatus | null): Notice[] {
  if (!status) return [];
  return [
    ...status.incidents.map((i) => ({ ...i, maint: false })),
    ...status.maintenances.map((m) => ({ ...m, maint: true })),
  ];
}

const URL_RE = /(https?:\/\/[^\s)]+)/;

/** Prefer the incident shortlink; fall back to a URL embedded in the body. */
export function noticeUrl(n: Notice): string | null {
  if (n.url) return n.url;
  if (n.latest_update) {
    const m = n.latest_update.match(URL_RE);
    if (m) return m[1];
  }
  return null;
}

export function fmtAgo(iso: string | null): string | null {
  if (!iso) return null;
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return null;
  const diff = Date.now() - t;
  if (diff < 0) return new Date(t).toLocaleString("zh-CN", { hour12: false });
  const m = Math.floor(diff / 60000);
  if (m < 1) return "刚刚更新";
  if (m < 60) return `${m} 分钟前更新`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h} 小时前更新`;
  return `${Math.floor(h / 24)} 天前更新`;
}
