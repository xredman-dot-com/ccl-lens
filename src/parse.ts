import type { UpstreamKind } from "./types";

export interface ParsedProxy {
  kind: UpstreamKind;
  url: string;
  label: string;
}

/**
 * Parse common proxy shorthand into a usable upstream url.
 * Supports:
 *   host:port:user:pass        (e.g. 207.251.13.173:5782:7nfnpt54:gQXVaTPSz6av)
 *   host:port
 *   user:pass@host:port
 *   socks5://... / socks5h://... / http://... / https://...
 * Shorthand SOCKS inputs are normalized to socks5h:// so DNS is resolved
 * through the proxy, which is more reliable with Clash/TUN enabled.
 */
export function parseProxyInput(raw: string): ParsedProxy | null {
  const s = raw.trim();
  if (!s) return null;

  // already a full url
  if (/^(socks5h?|https?):\/\//i.test(s)) {
    const kind: UpstreamKind = /^https?:/i.test(s) ? "http" : "socks5";
    return { kind, url: s, label: endpointOf(s) };
  }

  let user = "";
  let pass = "";
  let host = "";
  let port = "";

  if (s.includes("@")) {
    const at = s.lastIndexOf("@");
    const cred = s.slice(0, at);
    const hp = s.slice(at + 1);
    const ci = cred.indexOf(":");
    user = ci >= 0 ? cred.slice(0, ci) : cred;
    pass = ci >= 0 ? cred.slice(ci + 1) : "";
    [host, port] = splitHostPort(hp);
  } else {
    const parts = s.split(":");
    if (parts.length === 2) {
      [host, port] = parts;
    } else if (parts.length === 4) {
      [host, port, user, pass] = parts;
    } else {
      return null;
    }
  }

  if (!host || !/^\d+$/.test(port)) return null;
  const auth = user ? `${encodeURIComponent(user)}:${encodeURIComponent(pass)}@` : "";
  return {
    kind: "socks5",
    url: `socks5h://${auth}${host}:${port}`,
    label: `${host}:${port}`,
  };
}

/** Hide the password in a proxy url for display: socks5://u:pass@h:p -> socks5://u:***@h:p */
export function maskUrl(url: string): string {
  if (!url.trim()) return "直连";
  return url.replace(/\/\/([^:@/]+):([^@/]+)@/, "//$1:***@");
}

function splitHostPort(hp: string): [string, string] {
  const i = hp.lastIndexOf(":");
  if (i < 0) return [hp, ""];
  return [hp.slice(0, i), hp.slice(i + 1)];
}

function endpointOf(url: string): string {
  const noScheme = url.split("://")[1] ?? url;
  const afterAuth = noScheme.includes("@") ? noScheme.split("@").pop()! : noScheme;
  return afterAuth.split("/")[0];
}
