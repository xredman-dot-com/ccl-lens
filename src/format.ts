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

/** Human-readable compact number: 2001 -> "2K", 131558 -> "131.6K", 1.47e8 -> "147.4M". */
export function fmtCompact(n: number | null | undefined): string {
  if (n === null || n === undefined) return "—";
  const abs = Math.abs(n);
  const trim = (v: number) => v.toFixed(1).replace(/\.0$/, "");
  if (abs >= 1e9) return trim(n / 1e9) + "B";
  if (abs >= 1e6) return trim(n / 1e6) + "M";
  if (abs >= 1e3) return trim(n / 1e3) + "K";
  return String(n);
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
