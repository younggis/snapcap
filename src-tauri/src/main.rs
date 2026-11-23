//! SnapCap — Windows 轻量截图 / 录屏工具（Tauri 2 入口）
//!
//! 负责：插件注册、全局热键、托盘菜单、共享状态注入、崩溃恢复扫描与命令分发。
//! 具体业务逻辑分别在 capture / audio / encode / image / overlay 模块中。

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod audio;
mod capture;
mod config;
mod encode;
mod error;
mod hotkey;
mod image;
mod ipc;
mod models;
mod overlay;
mod state;
mod verify;

use state::AppState;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};

fn main() {
    // 截图链路诊断：snapcap.exe --capture-test [输出.png]
    // 直接执行与 UI「全屏截图」相同的 GDI 抓帧 + PNG 编码，并打印耗时。
    if let Some(pos) = std::env::args().position(|a| a == "--capture-test") {
        let out = std::env::args()
            .nth(pos + 1)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("snapcap-capture-test.png"));
        let t0 = std::time::Instant::now();
        let result = capture::capture_frame(&models::CaptureTarget::Full, false)
            .map_err(|e| format!("capture_frame: {e:#}"))
            .and_then(|frame| {
                let dims = (frame.width, frame.height);
                image::encode::encode_image(&frame.bgra, frame.width, frame.height, models::ImageFormat::Png, 92)
                    .map(|(bytes, _)| (dims, bytes))
                    .map_err(|e| format!("encode_image: {e:#}"))
            });
        match result {
            Ok(((w, h), bytes)) => {
                if let Err(e) = std::fs::write(&out, &bytes) {
                    eprintln!("CAPTURE TEST FAIL write: {e}");
                    std::process::exit(1);
                }
                println!(
                    "CAPTURE TEST OK {w}x{h} {} bytes in {}ms -> {}",
                    bytes.len(),
                    t0.elapsed().as_millis(),
                    out.display()
                );
            }
            Err(e) => {
                eprintln!("CAPTURE TEST FAIL: {e} in {}ms", t0.elapsed().as_millis());
                std::process::exit(1);
            }
        }
        return;
    }

    // 录制链路诊断：snapcap.exe --record-test <out.mp4> [秒数] [fps]
    if let Some(pos) = std::env::args().position(|a| a == "--record-test") {
        let output = std::env::args()
            .nth(pos + 1)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("snapcap-record-test.mp4"));
        let seconds = std::env::args().nth(pos + 2).and_then(|s| s.parse().ok()).unwrap_or(3u64);
        let fps = std::env::args().nth(pos + 3).and_then(|s| s.parse().ok()).unwrap_or(30u32);
        match encode::record_test(&output, seconds, fps) {
            Ok(r) => println!("RECORD TEST OK path={} duration_ms={} size={}", r.path, r.duration_ms, r.size),
            Err(e) => { eprintln!("RECORD TEST FAIL: {e:#}"); std::process::exit(1); }
        }
        return;
    }

    // 验收自测模式：snapcap.exe --selftest [输出路径]
    // 生成合成帧 → 真实 H.264/MP4，便于用 ffprobe 校验编码链路。
    if let Some(pos) = std::env::args().position(|a| a == "--selftest") {
        let output = std::env::args()
            .nth(pos + 1)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                let mut p = std::env::temp_dir();
                p.push("snapcap-selftest.mp4");
                p
            });
        match encode::selftest_encode(&output, 90, 30, 320, 240) {
            Ok(result) => {
                let line = format!(
                    "SELFTEST OK path={} duration_ms={} size={}",
                    result.path, result.duration_ms, result.size
                );
                println!("{line}");
                let mut report = output.clone();
                report.set_extension("txt");
                let _ = std::fs::write(report, line);
            }
            Err(err) => {
                let line = format!("SELFTEST FAIL: {err:#}");
                eprintln!("{line}");
                let mut report = output.clone();
                report.set_extension("txt");
                let _ = std::fs::write(report, line);
                std::process::exit(1);
            }
        }
        return;
    }

    // 诊断：列出可选窗口（验证已过滤自身窗口）：snapcap.exe --list-windows
    if std::env::args().any(|a| a == "--list-windows") {
        match ipc::list_windows() {
            Ok(ws) => {
                println!("WINDOWS count={}", ws.len());
                for w in ws.iter().take(20) {
                    println!("  hwnd={} '{}' rect={},{} {}x{}", w.hwnd, w.title, w.rect.x, w.rect.y, w.rect.width, w.rect.height);
                }
            }
            Err(e) => { eprintln!("LIST WINDOWS FAIL: {e:?}"); std::process::exit(1); }
        }
        return;
    }

    // 诊断：显示输出目录解析结果：snapcap.exe --output-dir
    if std::env::args().any(|a| a == "--output-dir") {
        let settings = config::load_settings().unwrap_or_default();
        match image::ops::expand_output_dir(&settings.paths.output_dir) {
            Ok(p) => println!("OUTPUT_DIR {}", p.display()),
            Err(e) => { eprintln!("OUTPUT DIR FAIL: {e}"); std::process::exit(1); }
        }
        return;
    }

    // 全功能验收：snapcap.exe --verify-all
    if std::env::args().any(|a| a == "--verify-all") {
        let checks = verify::run_all();
        if !verify::print_report(&checks) {
            std::process::exit(1);
        }
        return;
    }

    tauri::Builder::default()
        // 自启动（spec §12 general.autostart 由设置页控制，插件本身在此注册）。
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // 全局快捷键。
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_log::Builder::default().build())
        // 更新器：默认只检查、用户确认后安装（§14）。
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // 1) DPI 感知必须在任何截图前完成。
            capture::gdi::enable_dpi_awareness();

            // 2) 加载配置并注入共享状态。
            let settings = config::load_settings().unwrap_or_else(|err| {
                tracing::warn!("加载设置失败，使用默认值: {err:#}");
                config::Settings::default()
            });
            let app_state = AppState::new(settings.clone());
            app.manage(app_state.clone());
            app_state.set_app_handle(app.handle().clone());

            // 3) 全局热键：先装处理器（一次），再注册组合键。
            if let Err(err) = hotkey::install_handler(&app.handle(), &settings) {
                tracing::warn!("安装/注册热键失败: {err:#}");
            }

            // Esc / Enter 兜底不再常驻注册：全局快捷键会在系统层吞掉按键，
            // 导致标注编辑器输入框的回车无法提交文字。改为仅在覆盖层打开期间
            // 注册（见 ipc::open_region_overlay），关闭后立刻注销。

            // 让所有自身窗口对截图/录屏采集不可见（用户仍可见）。
            // 在窗口创建后立即设置，避免每次抓屏时 hide()/show() 的时序竞态。
            {
                let count = ipc::exclude_self_from_capture(app.handle().clone()).unwrap_or(0);
                tracing::info!("已标记 {count} 个窗口为采集不可见");
            }

            // 4) 托盘菜单（Tauri 2 必须在 Rust 中构建，配置只定义托盘图标）。
            if let Some(tray) = app.tray_by_id("tray") {
                let menu = MenuBuilder::new(app)
                    .text("show_settings", "显示设置")
                    .text("region", "区域截图")
                    .text("full", "全屏截图")
                    .text("window", "窗口截图")
                    .separator()
                    .text("record", "开始录制")
                    .text("pause", "暂停 / 继续")
                    .text("stop", "停止录制")
                    .separator()
                    .text("quit", "退出")
                    .build()?;
                tray.set_menu(Some(menu))?;

                // NOTE: MenuEvent::id() 在不同 Tauri 2 小版本返回 MenuId / &MenuId；
                // 这里用 AsRef<str> 统一读取 id 字符串。
                // 单击托盘图标重新打开设置窗口（关闭按钮只是隐藏，不退出）。
                tray.on_tray_icon_event(|tray, event| {
                    let is_left_click = matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    );
                    let is_double_click = matches!(
                        event,
                        TrayIconEvent::DoubleClick {
                            button: MouseButton::Left,
                            ..
                        }
                    );
                    if is_left_click || is_double_click {
                        if let Some(window) = tray.app_handle().get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                });

                tray.on_menu_event(|app, event| {
                    let id = event.id().as_ref();
                    match id {
                        "show_settings" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "region" | "full" | "window" | "record" | "pause" | "annotate" => {
                            let _ = app.emit(
                                "hotkey:triggered",
                                serde_json::json!({ "action": id }),
                            );
                        }
                        "stop" => {
                            let _ = app.emit("hotkey:triggered", serde_json::json!({ "action": "stop" }));
                        }
                        "quit" => app.exit(0),
                        _ => {}
                    }
                });
            }

            // 5) 启动时扫描未完成录制（§8.6 / AC-4），把候选交给 UI 提示。
            if let Ok(dir) = image::ops::expand_output_dir(&settings.paths.output_dir) {
                match encode::scan_recovery(&dir) {
                    Ok(candidates) if !candidates.is_empty() => {
                        tracing::warn!("发现 {} 个未完成录制文件", candidates.len());
                        let _ = app.emit("recovery:candidates", &candidates);
                    }
                    Ok(_) => {}
                    Err(err) => tracing::warn!("扫描恢复文件失败: {err:#}"),
                }
            }

            // 6) 启动时显示主窗口，让用户明确看到程序已响应；
            //    关闭窗口时会隐藏到托盘（见 on_window_event），托盘图标可再次唤起。
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }

            Ok(())
        })
        // 关闭主/编辑/录制窗口时隐藏到托盘，而不是退出进程。
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                match window.label() {
                    "main" | "editor" | "recorder" => {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                    _ => {}
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            ipc::list_displays,
            ipc::list_windows,
            ipc::get_virtual_desktop,
            ipc::exclude_self_from_capture,
            ipc::open_region_overlay,
            ipc::overlay_submit,
            ipc::screenshot_capture,
            ipc::screenshot_output,
            ipc::annotation_apply,
            ipc::record_start,
            ipc::record_pause,
            ipc::record_resume,
            ipc::record_stop,
            ipc::get_settings,
            ipc::set_settings,
            ipc::open_output_dir,
            ipc::reveal_file,
            ipc::list_recoverable,
            ipc::recover_file,
            ipc::delete_temp,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
