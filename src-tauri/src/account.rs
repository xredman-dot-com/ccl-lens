use crate::state::home_dir;
use serde::Serialize;
use serde_json::Value;

/// Claude account profile, read straight from Claude Code's own
/// `~/.claude.json` (the `oauthAccount` block it persists after login).
/// Purely local — no network, no token use.
#[derive(Debug, Clone, Serialize)]
pub struct AccountInfo {
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub organization_name: Option<String>,
    pub organization_type: Option<String>,
    pub organization_role: Option<String>,
    pub billing_type: Option<String>,
    pub account_created_at: Option<String>,
    pub subscription_created_at: Option<String>,
    pub rate_limit_tier: Option<String>,
    pub has_extra_usage_enabled: Option<bool>,
}

pub fn read_account() -> Option<AccountInfo> {
    let path = home_dir()?.join(".claude.json");
    let text = std::fs::read_to_string(&path).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let oa = v.get("oauthAccount")?;
    let s = |k: &str| oa.get(k).and_then(|x| x.as_str()).map(String::from);
    Some(AccountInfo {
        email: s("emailAddress"),
        display_name: s("displayName"),
        organization_name: s("organizationName"),
        organization_type: s("organizationType"),
        organization_role: s("organizationRole"),
        billing_type: s("billingType"),
        account_created_at: s("accountCreatedAt"),
        subscription_created_at: s("subscriptionCreatedAt"),
        rate_limit_tier: s("organizationRateLimitTier"),
        has_extra_usage_enabled: oa.get("hasExtraUsageEnabled").and_then(|x| x.as_bool()),
    })
}
