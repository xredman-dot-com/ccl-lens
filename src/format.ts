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
