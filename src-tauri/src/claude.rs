use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::PathBuf;

const BASE_URL_KEY: &str = "ANTHROPIC_BASE_URL";
const LOOPBACK_NO_PROXY: [&str; 4] = ["localhost", "127.0.0.1", "::1", "0.0.0.0"];

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

fn read_root() -> Result<Value> {
    let path = settings_path()?;
    if !path.exists() {
        return Ok(json!({}));
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read {}", path.display()))?;
    let v: Value = serde_json::from_str(&text).context("parse settings.json")?;
    Ok(v)
}

fn write_root_atomic(root: &Value) -> Result<()> {
    let path = settings_path()?;
    let tmp = path.with_extension("json.ccl-tmp");
    let text = serde_json::to_string_pretty(root)?;
    std::fs::write(&tmp, text).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).context("atomic rename settings.json")?;
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

/// Point Claude Code at our local proxy. Non-destructive: only adds
/// ANTHROPIC_BASE_URL and ensures NO_PROXY exempts loopback. Backs up the
/// original once before the first mutation.
pub fn enable_intercept(port: u16) -> Result<()> {
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
    let env = obj
        .entry("env")
        .or_insert_with(|| json!({}));
    if !env.is_object() {
        *env = json!({});
    }
    let env = env.as_object_mut().unwrap();
    env.insert(
        BASE_URL_KEY.to_string(),
        json!(format!("http://127.0.0.1:{}", port)),
    );
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

/// Remove our base url override. Leaves all other env keys untouched.
pub fn disable_intercept() -> Result<()> {
    let path = settings_path()?;
    if !path.exists() {
        return Ok(());
    }
    let mut root = read_root()?;
    if let Some(env) = root.get_mut("env").and_then(|e| e.as_object_mut()) {
        env.remove(BASE_URL_KEY);
    }
    write_root_atomic(&root)?;
    Ok(())
}

pub fn current_base_url() -> Option<String> {
    let root = read_root().ok()?;
    root.get("env")?
        .get(BASE_URL_KEY)?
        .as_str()
        .map(|s| s.to_string())
}
