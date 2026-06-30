import type { UsageSnapshot } from "./types";

export interface UsageWindow {
  key: string;
  label: string;
  shortLabel: string;
  pct: number;
  reset: string | null;
}

const WINDOW_LABELS: Record<string, string> = {
  five_hour: "5 小时",
  seven_day: "7 天",
  seven_day_opus: "7 天 · Opus",
  seven_day_sonnet: "7 天 · Sonnet",
  seven_day_oauth_apps: "7 天 · 第三方应用",
};

const WINDOW_SHORT: Record<string, string> = {
  five_hour: "5h",
  seven_day: "7d",
  seven_day_opus: "7d Opus",
  seven_day_sonnet: "7d Sonnet",
  seven_day_oauth_apps: "7d 应用",
};

const WINDOW_ORDER = [
  "five_hour",
  "seven_day",
  "seven_day_opus",
  "seven_day_sonnet",
  "seven_day_oauth_apps",
];

export function fmtReset(iso: unknown): string | null {
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

/** "" (ok) | "mid" (>=70%) | "high" (>=90%) — drives the bar colour. */
export function usageLevel(pct: number): "" | "mid" | "high" {
  if (pct >= 90) return "high";
  if (pct >= 70) return "mid";
  return "";
}

export function parseWindows(usage: UsageSnapshot | null): UsageWindow[] {
  const raw = usage?.raw;
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
      shortLabel: WINDOW_SHORT[key] ?? key,
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
