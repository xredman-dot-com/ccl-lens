use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

const BASE_URL_KEY: &str = "ANTHROPIC_BASE_URL";
const CA_CERT_KEY: &str = "NODE_EXTRA_CA_CERTS";
const PROXY_KEYS: [&str; 6] = [
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "ALL_PROXY",
    "https_proxy",
    "http_proxy",
    "all_proxy",
];
const NO_PROXY_KEYS: [&str; 2] = ["NO_PROXY", "no_proxy"];
const LOOPBACK_NO_PROXY: [&str; 4] = ["localhost", "127.0.0.1", "::1", "0.0.0.0"];

#[derive(Debug, Serialize, Deserialize)]
struct EnvSnapshot {
    values: BTreeMap<String, Option<Value>>,
}

fn home() -> Result<PathBuf> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .context("HOME not set")
}

fn settings_path() -> Result<PathBuf> {
    Ok(home()?.join(".claude").join("settings.json"))
}

fn backup_path() -> Result<PathBuf> {
    Ok(home()?.join(".claude").join("settings.json.ccl-lens.bak"))
}

fn snapshot_path() -> Result<PathBuf> {
    Ok(home()?
        .join(".claude")
        .join("settings.json.ccl-lens.proxy-state.json"))
}

fn read_root() -> Result<Value> {
    let path = settings_path()?;
    if !path.exists() {
        return Ok(json!({}));
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let v: Value = serde_json::from_str(&text).context("parse settings.json")?;
    Ok(v)
}

fn write_root_atomic(root: &Value) -> Result<()> {
    let path = settings_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let tmp = path.with_extension("json.ccl-tmp");
    let text = serde_json::to_string_pretty(root)?;
    std::fs::write(&tmp, text).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).context("atomic rename settings.json")?;
    Ok(())
}

fn write_snapshot(snapshot: &EnvSnapshot) -> Result<()> {
    let path = snapshot_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(snapshot)?;
    std::fs::write(&path, text).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn read_snapshot() -> Result<Option<EnvSnapshot>> {
    let path = snapshot_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text)
        .map(Some)
        .context("parse ccl-lens proxy snapshot")
}

fn remove_snapshot() -> Result<()> {
    let path = snapshot_path()?;
    if path.exists() {
        std::fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
    }
    Ok(())
}

fn merge_no_proxy(current: Option<&str>) -> String {
    let mut parts: Vec<String> = current
        .unwrap_or("")
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    for item in LOOPBACK_NO_PROXY {
        if !parts.iter().any(|p| p.eq_ignore_ascii_case(item)) {
            parts.push(item.to_string());
        }
    }
    parts.join(",")
}

fn local_proxy_value(port: u16) -> String {
    format!("http://127.0.0.1:{}", port)
}

fn is_local_ccl_base_url(value: &Value) -> bool {
    value
        .as_str()
        .map(|s| {
            let s = s.trim();
            s.starts_with("http://127.0.0.1:")
                || s.starts_with("http://localhost:")
                || s.starts_with("http://[::1]:")
        })
        .unwrap_or(false)
}

/// Point Claude Code at our local HTTP proxy. Keeps a snapshot of the env
/// values we mutate so stop can restore the previous settings exactly.
pub fn enable_intercept(port: u16, ca_cert: &std::path::Path) -> Result<()> {
    if read_snapshot()?.is_some() {
        disable_intercept()?;
    }

    let path = settings_path()?;
    let backup = backup_path()?;
    if path.exists() && !backup.exists() {
        std::fs::copy(&path, &backup).ok();
    }

    let mut root = read_root()?;
    if !root.is_object() {
        root = json!({});
    }
    let obj = root.as_object_mut().unwrap();
    let env = obj.entry("env").or_insert_with(|| json!({}));
    if !env.is_object() {
        *env = json!({});
    }
    let env = env.as_object_mut().unwrap();

    if read_snapshot()?.is_none() {
        let mut values = BTreeMap::new();
        for key in PROXY_KEYS
            .into_iter()
            .chain(NO_PROXY_KEYS)
            .chain([BASE_URL_KEY, CA_CERT_KEY])
        {
            let existing = env.get(key).cloned();
            let value =
                if key == BASE_URL_KEY && existing.as_ref().is_some_and(is_local_ccl_base_url) {
                    None
                } else {
                    existing
                };
            values.insert(key.to_string(), value);
        }
        write_snapshot(&EnvSnapshot { values })?;
    }

    let proxy = json!(local_proxy_value(port));
    for key in PROXY_KEYS {
        env.insert(key.to_string(), proxy.clone());
    }

    // Trust the MITM CA so Node (Claude Code) accepts our leaf certs for the
    // decrypted hosts. Scoped to Claude Code; no system keychain change.
    env.insert(
        CA_CERT_KEY.to_string(),
        json!(ca_cert.to_string_lossy().to_string()),
    );

    if env.get(BASE_URL_KEY).is_some_and(is_local_ccl_base_url) {
        env.remove(BASE_URL_KEY);
    }

    let merged_no_proxy = merge_no_proxy(
        env.get("NO_PROXY")
            .or_else(|| env.get("no_proxy"))
            .and_then(|v| v.as_str()),
    );
    env.insert("NO_PROXY".to_string(), json!(merged_no_proxy.clone()));
    env.insert("no_proxy".to_string(), json!(merged_no_proxy));
    write_root_atomic(&root)?;
    Ok(())
}

/// Self-heal on startup: if a previous run was killed/crashed without cleanup,
/// a proxy snapshot is left behind — restore the original settings.
pub fn recover_stale() -> Result<()> {
    if read_snapshot()?.is_some() {
        disable_intercept()?;
    }
    Ok(())
}

/// Restore env values that were present before enable.
pub fn disable_intercept() -> Result<()> {
    let path = settings_path()?;
    if !path.exists() {
        remove_snapshot()?;
        return Ok(());
    }
    let mut root = read_root()?;
    if let Some(env) = root.get_mut("env").and_then(|e| e.as_object_mut()) {
        if let Some(snapshot) = read_snapshot()? {
            for (key, value) in snapshot.values {
                match value {
                    Some(v) => {
                        env.insert(key, v);
                    }
                    None => {
                        env.remove(&key);
                    }
                }
            }
            remove_snapshot()?;
        } else {
            if env.get(BASE_URL_KEY).is_some_and(is_local_ccl_base_url) {
                env.remove(BASE_URL_KEY);
            }
        }
    }
    write_root_atomic(&root)?;
    Ok(())
}

pub fn current_proxy() -> Option<String> {
    let root = read_root().ok()?;
    let env = root.get("env")?;
    PROXY_KEYS
        .iter()
        .find_map(|key| env.get(*key)?.as_str().map(|s| s.to_string()))
}
