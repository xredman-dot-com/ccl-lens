use crate::models::SelectMode;
use crate::state::AppState;
use std::time::Duration;
use tauri::menu::{Menu, MenuBuilder};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Manager, Wry};

pub struct AppTray {
    pub tray: TrayIcon<Wry>,
}

pub fn setup(app: &mut tauri::App) -> tauri::Result<()> {
    let menu = build_menu(app.handle())?;
    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .title("ccl")
        .tooltip("ccl-lens")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "mode_fixed" => set_mode(app, SelectMode::Fixed),
            "mode_sticky" => set_mode(app, SelectMode::Sticky),
            "mode_auto" => set_mode(app, SelectMode::Auto),
            "quit" => app.exit(0),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone()).icon_as_template(true);
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

fn spawn_updater(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last_up = 0u64;
        let mut last_down = 0u64;
        loop {
            {
                let state = app.state::<AppState>();
                let (up, down) = state.traffic.snapshot();
                update_tray(
                    &app,
                    up.saturating_sub(last_up),
                    down.saturating_sub(last_down),
                );
                last_up = up;
                last_down = down;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

fn refresh(app: &AppHandle) {
    update_tray(app, 0, 0);
}

fn update_tray(app: &AppHandle, up_rate: u64, down_rate: u64) {
    let Ok(menu) = build_menu(app) else {
        return;
    };
    let tray = app.state::<AppTray>();
    let title = format!("↑{}/s ↓{}/s", fmt_bytes(up_rate), fmt_bytes(down_rate));
    let _ = tray.tray.set_title(Some(title));
    let _ = tray.tray.set_tooltip(Some(tray_tooltip(app)));
    let _ = tray.tray.set_menu(Some(menu));
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().unwrap().clone();
    let running = state.is_running();
    let stats = state.store.stats().ok();
    let history_up = stats.as_ref().map(|s| s.total_request_bytes).unwrap_or(0);
    let history_down = stats.as_ref().map(|s| s.total_response_bytes).unwrap_or(0);
    let total = history_up + history_down;

    MenuBuilder::new(app)
        .text(
            "summary",
            format!(
                "{} · {}",
                if running { "运行中" } else { "已停止" },
                mode_label(&cfg.mode)
            ),
        )
        .text(
            "traffic",
            format!(
                "累计 ↑{} ↓{} · {}",
                fmt_bytes(history_up),
                fmt_bytes(history_down),
                fmt_bytes(total)
            ),
        )
        .separator()
        .text("mode_fixed", checked("固定", cfg.mode == SelectMode::Fixed))
        .text(
            "mode_sticky",
            checked("优先+兜底", cfg.mode == SelectMode::Sticky),
        )
        .text(
            "mode_auto",
            checked("自动择优", cfg.mode == SelectMode::Auto),
        )
        .separator()
        .text("open", "打开 ccl-lens")
        .text("quit", "退出")
        .build()
}

fn tray_tooltip(app: &AppHandle) -> String {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().unwrap().clone();
    let stats = state.store.stats().ok();
    let up = stats.as_ref().map(|s| s.total_request_bytes).unwrap_or(0);
    let down = stats.as_ref().map(|s| s.total_response_bytes).unwrap_or(0);
    format!(
        "ccl-lens\n模式: {}\n累计上传: {}\n累计下载: {}",
        mode_label(&cfg.mode),
        fmt_bytes(up),
        fmt_bytes(down)
    )
}

fn checked(label: &str, on: bool) -> String {
    if on {
        format!("✓ {}", label)
    } else {
        format!("  {}", label)
    }
}

fn mode_label(mode: &SelectMode) -> &'static str {
    match mode {
        SelectMode::Fixed => "固定",
        SelectMode::Sticky => "优先+兜底",
        SelectMode::Auto => "自动择优",
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
