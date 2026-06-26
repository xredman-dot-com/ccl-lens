use crate::models::{SelectMode, TunnelStatus};
use crate::state::AppState;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Manager, Wry};

pub struct AppTray {
    pub tray: TrayIcon<Wry>,
}

/// Live up/down rate (bytes/s) shared into the menu builder so the dropdown can
/// show the current throughput without recomputing it.
#[derive(Default)]
pub struct TrayMetrics {
    up_rate: AtomicU64,
    down_rate: AtomicU64,
    /// Signature of the last menu we pushed via `set_menu`. We only rebuild the
    /// menu when this changes — calling `set_menu` on macOS dismisses an open
    /// dropdown, so per-second rate updates must NOT trigger a rebuild.
    last_menu_sig: std::sync::Mutex<String>,
}

pub fn setup(app: &mut tauri::App) -> tauri::Result<()> {
    app.manage(TrayMetrics::default());
    let (menu, _sig) = build_menu(app.handle())?;
    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip("ccl-lens")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "mode_fixed" => set_mode(app, SelectMode::Fixed),
            "mode_sticky" => set_mode(app, SelectMode::Sticky),
            "mode_auto" => set_mode(app, SelectMode::Auto),
            "quit" => quit(app),
            _ => {}
        });

    // Show the real app icon (colored), not a monochrome template silhouette.
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone()).icon_as_template(false);
    }

    let tray = builder.build(app.handle())?;
    app.manage(AppTray { tray });
    spawn_updater(app.handle().clone());
    Ok(())
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn set_mode(app: &AppHandle, mode: SelectMode) {
    let state = app.state::<AppState>();
    state.config.lock().unwrap().mode = mode;
    state.sync_pool_and_save();
    let _ = app.emit("state", crate::commands::build_view(&state));
    refresh(app);
}

fn quit(app: &AppHandle) {
    // Restore ~/.claude/settings.json and stop listening before terminating.
    app.state::<AppState>().shutdown();
    app.exit(0);
    // Belt-and-suspenders: guarantee the process dies even if a plugin or window
    // holds the runtime open (shutdown already did the cleanup above).
    std::process::exit(0);
}

fn spawn_updater(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last_up = 0u64;
        let mut last_down = 0u64;
        loop {
            {
                let state = app.state::<AppState>();
                let (up, down) = state.traffic.snapshot();
                update_tray(&app, up.saturating_sub(last_up), down.saturating_sub(last_down));
                last_up = up;
                last_down = down;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

fn refresh(app: &AppHandle) {
    let (up, down) = {
        let m = app.state::<TrayMetrics>();
        (m.up_rate.load(Ordering::Relaxed), m.down_rate.load(Ordering::Relaxed))
    };
    update_tray(app, up, down);
}

fn update_tray(app: &AppHandle, up_rate: u64, down_rate: u64) {
    let metrics = app.state::<TrayMetrics>();
    metrics.up_rate.store(up_rate, Ordering::Relaxed);
    metrics.down_rate.store(down_rate, Ordering::Relaxed);

    let state = app.state::<AppState>();
    let running = state.is_running();
    let tunnel = state.tunnel.lock().unwrap().clone();

    let tray = app.state::<AppTray>();
    // Title + tooltip are cheap and do NOT dismiss an open menu — update freely.
    let _ = tray.tray.set_title(tray_title(running, &tunnel, up_rate, down_rate));
    let _ = tray.tray.set_tooltip(Some(tray_tooltip(running, &tunnel)));
    // The menu, however, is only re-pushed when its structural content changes,
    // so hovering an open dropdown doesn't make it vanish on the next tick.
    if let Ok((menu, sig)) = build_menu(app) {
        let mut last = metrics.last_menu_sig.lock().unwrap();
        if *last != sig {
            *last = sig;
            drop(last);
            let _ = tray.tray.set_menu(Some(menu));
        }
    }
}

/// Menu-bar title next to the icon: live rate when healthy, a warning when not.
fn tray_title(
    running: bool,
    tunnel: &TunnelStatus,
    up_rate: u64,
    down_rate: u64,
) -> Option<String> {
    if !running {
        return None;
    }
    if tunnel.error.is_some() {
        return Some("⚠ 异常".to_string());
    }
    if !tunnel.tunnel_ok {
        return Some("◌ 探测中".to_string());
    }
    Some(format!("↑{}/s ↓{}/s", fmt_bytes(up_rate), fmt_bytes(down_rate)))
}

fn build_menu(app: &AppHandle) -> tauri::Result<(Menu<Wry>, String)> {
    let state = app.state::<AppState>();
    let metrics = app.state::<TrayMetrics>();
    let cfg = state.config.lock().unwrap().clone();
    let running = state.is_running();
    let tunnel = state.tunnel.lock().unwrap().clone();
    let up_rate = metrics.up_rate.load(Ordering::Relaxed);
    let down_rate = metrics.down_rate.load(Ordering::Relaxed);

    let stats = state.store.stats(None).ok();
    let history_up = stats.as_ref().map(|s| s.total_request_bytes).unwrap_or(0);
    let history_down = stats.as_ref().map(|s| s.total_response_bytes).unwrap_or(0);

    // Status line (disabled, info only).
    let status_text = if !running {
        "○ 已停止".to_string()
    } else if let Some(err) = &tunnel.error {
        format!("⚠ 异常 · {}", truncate(err, 36))
    } else if tunnel.tunnel_ok {
        match tunnel.tunnel_latency_ms {
            Some(ms) => format!("● 正常 · 隧道 {}ms", ms),
            None => "● 正常".to_string(),
        }
    } else {
        "◌ 探测中".to_string()
    };

    // Signature of everything that changes the menu's *structure/labels* — but
    // NOT the live rate/traffic (those tick every second and would force a
    // rebuild that dismisses an open dropdown). Rate stays live in the title.
    let sig = format!(
        "{}|{}|{}|{}|{}|{:?}|{:?}|{:?}|{:?}|{:?}",
        running,
        mode_label(&cfg.mode),
        cfg.port,
        status_text,
        tunnel.tunnel_ok,
        tunnel.upstream_endpoint,
        tunnel.upstream_kind,
        tunnel.upstream_label,
        tunnel.exit_ip,
        tunnel.exit_geo,
    );

    let info = |id: &str, text: String| -> tauri::Result<_> {
        MenuItemBuilder::with_id(id, text).enabled(false).build(app)
    };

    let mut mb = MenuBuilder::new(app)
        .item(&info("status", status_text)?)
        .item(&info(
            "summary",
            format!("模式 {} · 端口 {}", mode_label(&cfg.mode), cfg.port),
        )?);

    if running {
        if let Some(ep) = tunnel.upstream_endpoint.clone().filter(|e| e != "direct") {
            let kind = tunnel.upstream_kind.clone().unwrap_or_default().to_uppercase();
            mb = mb.item(&info("upstream", format!("上游 {} {}", kind, ep))?);
        } else if let Some(label) = tunnel.upstream_label.clone() {
            mb = mb.item(&info("upstream", format!("上游 {}", label))?);
        }
        if let Some(ip) = tunnel.exit_ip.clone() {
            let geo = tunnel
                .exit_geo
                .clone()
                .map(|g| format!(" ({})", g))
                .unwrap_or_default();
            mb = mb.item(&info("exit", format!("出口 {}{}", ip, geo))?);
        }
        mb = mb.item(&info(
            "rate",
            format!("速率 ↑{}/s ↓{}/s", fmt_bytes(up_rate), fmt_bytes(down_rate)),
        )?);
    }

    mb = mb
        .item(&info(
            "traffic",
            format!("累计 ↑{} ↓{}", fmt_bytes(history_up), fmt_bytes(history_down)),
        )?)
        .separator()
        .text("mode_fixed", checked("固定", cfg.mode == SelectMode::Fixed))
        .text(
            "mode_sticky",
            checked("优先+兜底", cfg.mode == SelectMode::Sticky),
        )
        .text("mode_auto", checked("自动择优", cfg.mode == SelectMode::Auto))
        .separator()
        .text("open", "打开 ccl-lens")
        .text("quit", "退出 ccl-lens");

    Ok((mb.build()?, sig))
}

fn tray_tooltip(running: bool, tunnel: &TunnelStatus) -> String {
    let status = if !running {
        "已停止"
    } else if tunnel.error.is_some() {
        "异常"
    } else if tunnel.tunnel_ok {
        "正常"
    } else {
        "探测中"
    };
    format!("ccl-lens · {}", status)
}

fn checked(label: &str, on: bool) -> String {
    if on {
        format!("✓ {}", label)
    } else {
        format!("　{}", label)
    }
}

fn mode_label(mode: &SelectMode) -> &'static str {
    match mode {
        SelectMode::Fixed => "固定",
        SelectMode::Sticky => "优先+兜底",
        SelectMode::Auto => "自动择优",
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

fn fmt_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let x = n as f64;
    if x >= GB {
        format!("{:.1}GB", x / GB)
    } else if x >= MB {
        format!("{:.1}MB", x / MB)
    } else if x >= KB {
        format!("{:.0}KB", x / KB)
    } else {
        format!("{}B", n)
    }
}
