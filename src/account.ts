export const PLAN_LABELS: Record<string, string> = {
  claude_max: "Claude Max",
  claude_pro: "Claude Pro",
  claude_team: "Claude Team",
  claude_enterprise: "Claude Enterprise",
};

export const ROLE_LABELS: Record<string, string> = {
  admin: "管理员",
  owner: "所有者",
  member: "成员",
};

export function planLabel(t: string | null): string {
  if (!t) return "—";
  return PLAN_LABELS[t] ?? t;
}

export function roleLabel(r: string | null): string {
  if (!r) return "—";
  return ROLE_LABELS[r] ?? r;
}
