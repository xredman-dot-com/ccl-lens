export function fmtNum(n: number | null | undefined): string {
  if (n === null || n === undefined) return "—";
  return n.toLocaleString("en-US");
}

export function fmtCost(n: number | null | undefined): string {
  if (n === null || n === undefined) return "—";
  if (n === 0) return "$0";
  if (n < 0.01) return "$" + n.toFixed(4);
  return "$" + n.toFixed(2);
}

/** Human-readable number in Chinese units: 144400000 -> "1.44亿", 626300 -> "62.6万". */
export function fmtCompact(n: number | null | undefined): string {
  if (n === null || n === undefined) return "—";
  const abs = Math.abs(n);
  const strip = (s: string) => s.replace(/\.?0+$/, "");
  if (abs >= 1e8) return strip((n / 1e8).toFixed(2)) + "亿";
  if (abs >= 1e4) return strip((n / 1e4).toFixed(1)) + "万";
  return Math.round(n).toLocaleString("en-US");
}

/** Secondary at-a-glance CNY figure; `rate` is the live USD->CNY rate. */
export function fmtCny(usd: number | null | undefined, rate: number): string {
  if (usd === null || usd === undefined) return "—";
  const v = usd * rate;
  return "≈ ¥" + v.toLocaleString("zh-CN", { maximumFractionDigits: 0 });
}

export function fmtMs(n: number | null | undefined): string {
  if (n === null || n === undefined) return "—";
  if (n < 1000) return n + "ms";
  return (n / 1000).toFixed(1) + "s";
}

export function fmtBytes(n: number | null | undefined): string {
  if (n === null || n === undefined) return "—";
  if (n >= 1024 * 1024 * 1024) return (n / 1024 / 1024 / 1024).toFixed(1) + " GB";
  if (n >= 1024 * 1024) return (n / 1024 / 1024).toFixed(1) + " MB";
  if (n >= 1024) return Math.round(n / 1024).toLocaleString("en-US") + " KB";
  return n.toLocaleString("en-US") + " B";
}

export function fmtTime(ms: number): string {
  const d = new Date(ms);
  return d.toLocaleTimeString("zh-CN", { hour12: false });
}

export function shortModel(m: string | null): string {
  if (!m) return "—";
  return m.replace(/^claude-/, "").replace(/-\d{8}$/, "");
}
