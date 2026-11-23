//! IPC 命令层（§11）
//!
//! 所有 `#[tauri::command]` 集中于此。命令只做参数校验、状态机迁移与模块委派；
//! 重活交给 capture / audio / encode / image 模块。事件统一经 `emit_*` 辅助函数。

use crate::audio;
use crate::audio::AudioFrame;
use crate::capture::frame::FrameRing;
use crate::capture::{self};
use crate::config;
use crate::config::Settings;
use crate::encode::sync::QpcClock;
use crate::encode::{self};
use crate::error::{to_event_payload, AppError};
use crate::image;
use crate::models::*;
use crate::overlay;
use crate::state::{AppState, ScreenshotEntry, SessionHandle};
use anyhow::{anyhow, ensure, Result};
use crossbeam_channel;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

// ---- 事件辅助 -------------------------------------------------------------

pub(crate) fn emit_state(app: &AppHandle, session: SessionId, state: SessionState) {
    let _ = app.emit(
        "capture:state",
        serde_json::json!({ "session": session, "state": state }),
    );
}

pub(crate) fn emit_stats(app: &AppHandle, stats: CaptureStats) {
    let _ = app.emit("capture:stats", stats);
}

pub(crate) fn emit_error(app: &AppHandle, err: &AppError) {
    let _ = app.emit("capture:error", to_event_payload(err));
}

fn map_err<T>(result: Result<T>) -> Result<T, AppError> {
    result.map_err(AppError::from)
}

// ---- 显示 / 窗口枚举 ------------------------------------------------------

/// 列出所有显示器（虚拟桌面坐标，物理像素）。
#[tauri::command]
pub fn list_displays() -> Result<Vec<DisplayInfo>, AppError> {
    list_displays_impl().map_err(AppError::from)
}

#[cfg(windows)]
fn list_displays_impl() -> Result<Vec<DisplayInfo>> {
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HMONITOR, HDC, MONITORINFOEXW,
    };
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

    let mut infos: Vec<DisplayInfo> = Vec::new();
    unsafe extern "system" fn callback(
        hmonitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> windows::Win32::Foundation::BOOL {
        let infos = &mut *(data.0 as *mut Vec<DisplayInfo>);
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if !GetMonitorInfoW(hmonitor, &mut info.monitorInfo).as_bool() {
            return windows::Win32::Foundation::BOOL(1);
        }
        let r = info.monitorInfo.rcMonitor;
        let name = String::from_utf16_lossy(&info.szDevice)
            .trim_end_matches('\0')
            .to_string();
        let mut dpi_x = 96u32;
        let mut dpi_y = 96u32;
        // NOTE: GetDpiForMonitor 在 shcore.dll；若锁定版本模块路径不同请调整。
        let _ = GetDpiForMonitor(
            hmonitor,
            MDT_EFFECTIVE_DPI,
            &mut dpi_x,
            &mut dpi_y,
        );
        infos.push(DisplayInfo {
            handle: hmonitor.0 as isize,
            name,
            is_primary: (info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY) != 0,
            rect: Rect::new(r.left, r.top, (r.right - r.left) as u32, (r.bottom - r.top) as u32),
            scale_factor: (dpi_x as f64 / 96.0).max(0.25),
        });
        windows::Win32::Foundation::BOOL(1)
    }
    let ok = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(callback),
            LPARAM(&mut infos as *mut Vec<DisplayInfo> as isize),
        )
    };
    ensure!(ok.as_bool(), "EnumDisplayMonitors 失败");
    Ok(infos)
}

#[cfg(not(windows))]
fn list_displays_impl() -> Result<Vec<DisplayInfo>> {
    Ok(vec![DisplayInfo {
        handle: 1,
        name: "Primary".to_string(),
        is_primary: true,
        rect: Rect::new(0, 0, 1920, 1080),
        scale_factor: 1.0,
    }])
}

/// 列出可见顶层窗口。
#[tauri::command]
pub fn list_windows() -> Result<Vec<WindowInfo>, AppError> {
    list_windows_impl().map_err(AppError::from)
}

/// 供验收测试：列出窗口的同时返回“本进程 PID”，用于断言列表不含自身窗口。
#[cfg(windows)]
pub fn list_windows_with_pid() -> Result<(Vec<WindowInfo>, u32), AppError> {
    let ws = list_windows_impl().map_err(AppError::from)?;
    let pid = unsafe { windows::Win32::System::Threading::GetCurrentProcessId() };
    Ok((ws, pid))
}

#[cfg(windows)]
fn list_windows_impl() -> Result<Vec<WindowInfo>> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowLongW, GetWindowRect, GetWindowTextW,
        GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE,
    };

    // 用进程 ID 过滤掉本程序自己的窗口（尤其覆盖层）：
    // 覆盖层置顶且铺满虚拟桌面，若被列入会导致“窗口截图”永远选中它自身。
    let self_pid = unsafe { GetCurrentProcessId() };

    struct Ctx {
        infos: Vec<WindowInfo>,
        self_pid: u32,
        leaked_self: u32, // 验收用：若过滤失效会 > 0
    }
    let mut ctx = Ctx { infos: Vec::new(), self_pid, leaked_self: 0 };

    unsafe extern "system" fn callback(hwnd: HWND, data: LPARAM) -> BOOL {
        let ctx = &mut *(data.0 as *mut Ctx);
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        // 排除鼠标穿透的透明覆盖层（WS_EX_TRANSPARENT，如屏幕光标/标注工具窗口）：
        // 点击会穿透到下层窗口，用户本就无法选中它们；它们又常置顶铺满屏幕，
        // 会挡住真实目标的 hover 命中。
        // WS_EX_TRANSPARENT = 0x20
        let exstyle = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
        if exstyle & 0x20 != 0 {
            return BOOL(1);
        }
        // 排除本进程窗口。
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == ctx.self_pid {
            ctx.leaked_self += 1;
            return BOOL(1);
        }
        let mut title = [0u16; 512];
        let title_len = GetWindowTextW(hwnd, &mut title);
        if title_len == 0 {
            return BOOL(1);
        }
        let mut class = [0u16; 256];
        let class_len = GetClassNameW(hwnd, &mut class);
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return BOOL(1);
        }
        ctx.infos.push(WindowInfo {
            hwnd: hwnd.0 as isize,
            title: String::from_utf16_lossy(&title[..title_len as usize]),
            class_name: String::from_utf16_lossy(&class[..class_len as usize]),
            rect: Rect::new(
                rect.left,
                rect.top,
                (rect.right - rect.left).max(0) as u32,
                (rect.bottom - rect.top).max(0) as u32,
            ),
            is_visible: true,
        });
        BOOL(1)
    }
    unsafe { EnumWindows(Some(callback), LPARAM(&mut ctx as *mut Ctx as isize)) }
        .map_err(|e| anyhow!("EnumWindows 失败: {e:?}"))?;
    let _ = ctx.leaked_self; // 保留字段，便于将来扩展统计
    Ok(ctx.infos)
}

/// 验收用：返回指定窗口的 PID。
#[cfg(windows)]
pub fn window_pid(hwnd: isize) -> u32 {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(HWND(hwnd as *mut core::ffi::c_void), Some(&mut pid)) };
    pid
}

#[cfg(not(windows))]
fn list_windows_impl() -> Result<Vec<WindowInfo>> {
    Ok(Vec::new())
}

// ---- 选区覆盖层 -----------------------------------------------------------

/// 把本程序的所有窗口标记为「采集时不可见」，
/// 防止截图/录屏把工具自身界面拍进去（用户仍能看到窗口）。
#[tauri::command]
pub fn exclude_self_from_capture(app: AppHandle) -> Result<usize, AppError> {
    let mut count = 0usize;
    for label in ["main", "overlay", "editor", "recorder"] {
        if let Some(w) = app.get_webview_window(label) {
            if let Ok(hwnd) = w.hwnd() {
                if capture::gdi::exclude_window_from_capture(hwnd.0 as isize) {
                    count += 1;
                }
            }
        }
    }
    Ok(count)
}

/// 返回虚拟桌面矩形（物理像素，可含负坐标），供选区层把窗口坐标映射到覆盖层。
#[tauri::command]
pub fn get_virtual_desktop() -> Result<Rect, AppError> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
            SM_YVIRTUALSCREEN,
        };
        Ok(Rect::new(
            unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) },
            unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) },
            unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) }.max(1) as u32,
            unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) }.max(1) as u32,
        ))
    }
    #[cfg(not(windows))]
    {
        Ok(Rect::new(0, 0, 1920, 1080))
    }
}

/// 生成覆盖层导航地址。`mode` 必须进入 URL，不能只留在 Rust 参数里：
/// Overlay.vue 根据 `?mode=window` 决定是否启用窗口识别与点击选择。
pub(crate) fn overlay_navigation_url(mut url: tauri::Url, mode: &str) -> Result<tauri::Url, AppError> {
    if !matches!(mode, "screenshot" | "record" | "window") {
        return Err(AppError::internal(format!("未知覆盖层模式: {mode}")));
    }
    url.set_fragment(None);
    {
        let mut query = url.query_pairs_mut();
        query.clear();
        query.append_pair("view", "overlay");
        query.append_pair("mode", mode);
    }
    Ok(url)
}

/// 打开区域选择覆盖层，并等待 UI 通过 `overlay_submit` 提交结果。
#[tauri::command]
pub async fn open_region_overlay(
    app: AppHandle,
    state: State<'_, AppState>,
    mode: String,
) -> Result<SelectionPayload, AppError> {
    if state.pending_selection.lock().is_some() {
        return Err(AppError::internal("已有选区操作正在进行，请先完成或取消"));
    }
    let overlay = app
        .get_webview_window("overlay")
        .ok_or_else(|| AppError::internal("选区窗口尚未初始化"))?;
    let url = overlay_navigation_url(
        overlay
            .url()
            .map_err(|e| AppError::internal(format!("读取选区地址失败: {e}")))?,
        &mode,
    )?;

    // 每次打开都重新导航：既传递 mode，也清除上一次选区/hover/提交状态。
    // 在隐藏其它窗口之前完成，失败时不会留下不可恢复的挂起状态。
    overlay
        .navigate(url)
        .map_err(|e| AppError::internal(format!("加载选区窗口失败: {e}")))?;

    // 先隐藏自身的其它窗口，避免它们遮住桌面、也避免被误当作采集目标。
    let mut restore: Vec<String> = Vec::new();
    for label in ["main", "editor", "recorder"] {
        if let Some(w) = app.get_webview_window(label) {
            if w.is_visible().unwrap_or(false) {
                restore.push(label.to_string());
                let _ = w.hide();
            }
        }
    }

    let rx = {
        let (tx, rx) = tokio::sync::oneshot::channel::<SelectionPayload>();
        *state.pending_selection.lock() = Some(tx);
        rx
    };

    // 覆盖整个虚拟桌面（相对桌面左上角），否则只能框到窗口默认大小的一小块。
    #[cfg(windows)]
    {
        use tauri::PhysicalPosition;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
            SM_YVIRTUALSCREEN,
        };
        let vx = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
        let vy = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
        let vw = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
        let vh = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
        let _ = overlay.set_position(PhysicalPosition::new(vx, vy));
        let _ = overlay.set_size(tauri::PhysicalSize::new(vw.max(1) as u32, vh.max(1) as u32));
    }
    if let Err(e) = overlay.show().and_then(|_| overlay.set_focus()) {
        *state.pending_selection.lock() = None;
        if restore.iter().any(|l| l == "main") {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
            }
        }
        return Err(AppError::internal(format!("显示选区窗口失败: {e}")));
    }
    // 仅在覆盖层可见时注册 Enter/Esc 全局兜底；关闭后立即注销，避免吞掉
    // 标注编辑器输入框的回车（否则文字标注无法通过 Enter 提交）。
    if let Err(err) = crate::hotkey::install_overlay_nav(&app, state.inner().clone()) {
        tracing::warn!("注册覆盖层 Enter/Esc 兜底失败: {err:#}");
    }
    tracing::debug!("覆盖层已打开，mode={mode}");

    let payload = match rx.await {
        Ok(payload) => payload,
        Err(_) => SelectionPayload {
            rect: Rect::default(),
            hwnd: None,
            cancelled: true,
        },
    };
    crate::hotkey::remove_overlay_nav(&app);
    *state.pending_selection.lock() = None;
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.hide();
    }
    // 恢复此前可见的窗口（仅主窗口，编辑器/录制台保持隐藏避免干扰）。
    if restore.iter().any(|l| l == "main") {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.show();
        }
    }
    Ok(payload)
}

/// UI 提交选区结果：兑现挂起的通道，隐藏覆盖层并广播 `overlay:selection`。
/// 若当前没有等待中的选区，则为空操作。
#[tauri::command]
pub async fn overlay_submit(
    state: State<'_, AppState>,
    payload: SelectionPayload,
) -> Result<(), AppError> {
    if !state.submit_selection(payload.clone()) {
        return Ok(());
    }
    state.publish_selection(&payload);
    Ok(())
}

// ---- 截图 -----------------------------------------------------------------

/// 抓帧 -> 裁剪 -> 编码 -> data URL，结果缓存在 AppState 供后续输出 / 标注。
/// 注意：GDI 抓屏 + PNG 编码是重活，必须放到 blocking 线程池，
/// 否则会阻塞 Tauri 的 async executor，导致 UI 无响应。
#[tauri::command]
pub async fn screenshot_capture(
    target: CaptureTarget,
    options: ScreenshotOptions,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ScreenshotResult, AppError> {
    let app_state = state.inner().clone();
    // 主要手段：WDA_EXCLUDEFROMCAPTURE（在 setup 中已设置，这里重试以防窗口重建）。
    // 对用户仍可见，但对所有采集 API 不可见，无 hide()/show() 时序竞态。
    let _ = exclude_self_from_capture(app.clone());
    tauri::async_runtime::spawn_blocking(move || {
        screenshot_capture_impl(&target, &options, &app_state)
    })
    .await
    .map_err(|e| AppError::internal(format!("截图任务失败: {e}")))?
}

fn screenshot_capture_impl(
    target: &CaptureTarget,
    options: &ScreenshotOptions,
    state: &AppState,
) -> Result<ScreenshotResult, AppError> {
    let frame = capture::capture_frame(target, options.include_cursor)
        .map_err(|err| AppError::unsupported(format!("截图失败: {err:#}")))?;
    let (bytes, data_url) = map_err(image::encode::encode_image(
        &frame.bgra,
        frame.width,
        frame.height,
        options.format,
        options.jpeg_quality,
    ))?;

    let id = Uuid::new_v4().to_string();
    state.screenshots.write().insert(
        id.clone(),
        ScreenshotEntry {
            bgra: frame.bgra.clone(),
            width: frame.width,
            height: frame.height,
            encoded: std::sync::Arc::new(bytes),
            format: options.format,
        },
    );

    let result = ScreenshotResult {
        id,
        width: frame.width,
        height: frame.height,
        data_url,
    };
    // 广播给所有窗口（含隐藏的 editor 窗口），使其加载同一张截图，
    // 避免 editor 打开后再抓一次屏造成“截到自身”的递归。
    if let Some(app) = state.app_handle.read().as_ref() {
        let _ = app.emit("screenshot:ready", &result);
    }
    Ok(result)
}

/// 把缓存截图输出到文件或剪贴板。
#[tauri::command]
pub async fn screenshot_output(
    id: String,
    destination: String,
    path: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    let app_state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        screenshot_output_impl(&id, &destination, path.as_deref(), &app_state)
    })
    .await
    .map_err(|e| AppError::internal(format!("输出任务失败: {e}")))?
}

/// 返回保存后的完整路径（剪贴板返回空串）。
fn screenshot_output_impl(
    id: &str,
    destination: &str,
    path: Option<&str>,
    state: &AppState,
) -> Result<String, AppError> {
    let entry = state
        .screenshots
        .read()
        .get(id)
        .cloned()
        .ok_or_else(|| AppError::internal("未找到截图缓存，请重新截图"))?;

    match destination {
        "clipboard" => {
            map_err(image::clipboard::set_image(&entry.bgra, entry.width, entry.height))?;
            return Ok(String::new());
        }
        "file" => {
            let settings = state.config.read().clone();
            let (dir, file_name) = if let Some(raw) = path {
                resolve_output_path(&raw, &settings)?
            } else {
                let dir = map_err(image::ops::expand_output_dir(&settings.paths.output_dir))?;
                let ext = match entry.format {
                    ImageFormat::Png => "png",
                    ImageFormat::Jpeg => "jpg",
                };
                (dir, image::ops::format_filename(&settings.paths.naming, ext))
            };
            fs::create_dir_all(&dir).map_err(AppError::from)?;
            let final_path = dir.join(&file_name);
            let tmp = final_path.with_extension(format!(
                "{}.tmp",
                final_path.extension().and_then(|e| e.to_str()).unwrap_or("png")
            ));
            fs::write(&tmp, entry.encoded.as_slice()).map_err(AppError::from)?;
            match fs::rename(&tmp, &final_path) {
                Ok(()) => {}
                Err(_) => {
                    let _ = fs::remove_file(&final_path);
                    fs::rename(&tmp, &final_path).map_err(AppError::from)?;
                }
            }
            tracing::info!("截图已保存: {}", final_path.display());
            return Ok(final_path.to_string_lossy().into_owned());
        }
        other => return Err(AppError::internal(format!("未知输出目标: {other}"))),
    }
}

fn resolve_output_path(raw: &str, settings: &config::Settings) -> Result<(PathBuf, String)> {
    let pb = PathBuf::from(raw);
    if let (Some(parent), Some(name)) = (pb.parent(), pb.file_name()) {
        if !parent.as_os_str().is_empty() {
            return Ok((parent.to_path_buf(), name.to_string_lossy().into_owned()));
        }
    }
    // 仅有文件名：放到配置的输出目录。
    let dir = image::ops::expand_output_dir(&settings.paths.output_dir)?;
    Ok((dir, pb.to_string_lossy().into_owned()))
}

/// 对缓存截图应用标注操作，生成新 id（保证落盘/剪贴板与预览一致）。
#[tauri::command]
pub async fn annotation_apply(
    id: String,
    ops: Vec<AnnotationOp>,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    let app_state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || annotation_apply_impl(&id, &ops, &app_state))
        .await
        .map_err(|e| AppError::internal(format!("标注任务失败: {e}")))?
}

fn annotation_apply_impl(
    id: &str,
    ops: &[AnnotationOp],
    state: &AppState,
) -> Result<String, AppError> {
    let entry = state
        .screenshots
        .read()
        .get(id)
        .cloned()
        .ok_or_else(|| AppError::internal("未找到要标注的截图"))?;

    let mut bgra = (*entry.bgra).clone();
    map_err(overlay::annotate::apply_ops(
        &mut bgra,
        entry.width,
        entry.height,
        &ops,
    ))?;
    let (bytes, _) = map_err(image::encode::encode_image(
        &bgra,
        entry.width,
        entry.height,
        entry.format,
        92,
    ))?;

    let new_id = Uuid::new_v4().to_string();
    state.screenshots.write().insert(
        new_id.clone(),
        ScreenshotEntry {
            bgra: std::sync::Arc::new(bgra),
            width: entry.width,
            height: entry.height,
            encoded: std::sync::Arc::new(bytes),
            format: entry.format,
        },
    );
    Ok(new_id)
}

// ---- 录制 -----------------------------------------------------------------

/// 启动录制会话：创建帧队列 / 时间基准 / 线程运行体，进入 Recording 状态。
#[tauri::command]
pub async fn record_start(
    target: CaptureTarget,
    options: RecordingOptions,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SessionId, AppError> {
    let app_state = state.inner().clone();
    let app_handle = app.clone();
    // 录制同样让自身窗口对采集不可见（WDA 在 setup 已设置，这里重试）。
    let _ = exclude_self_from_capture(app.clone());
    tauri::async_runtime::spawn_blocking(move || {
        record_start_impl(&target, &options, &app_handle, &app_state)
    })
    .await
    .map_err(|e| AppError::internal(format!("录制启动任务失败: {e}")))?
}

fn record_start_impl(
    target: &CaptureTarget,
    options: &RecordingOptions,
    app: &AppHandle,
    state: &AppState,
) -> Result<SessionId, AppError> {
    if state.session.read().is_some() {
        return Err(AppError::internal("已有录制会话正在进行"));
    }

    // 先用一次抓帧确定目标尺寸（GDI，快速且无需授权）。
    let probe = capture::capture_frame(&target, false)
        .map_err(|err| AppError::unsupported(format!("无法确定录制区域: {err:#}")))?;
    let width = probe.width;
    let height = probe.height;

    let settings = state.config.read().clone();
    let dir = map_err(image::ops::expand_output_dir(&settings.paths.output_dir))?;
    fs::create_dir_all(&dir).map_err(AppError::from)?;
    let file_name = image::ops::format_filename(&settings.paths.naming, "mp4");
    let final_path = dir.join(file_name);

    let fps = options.fps.max(1);
    let frames = std::sync::Arc::new(FrameRing::new(60));
    let stats = std::sync::Arc::new(parking_lot::RwLock::new(CaptureStats::default()));
    let result = std::sync::Arc::new(parking_lot::RwLock::new(None));
    let paused = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let frequency = qpc_frequency();
    let started_qpc = capture::frame::now_qpc();
    let clock = std::sync::Arc::new(QpcClock::new(started_qpc, frequency));

    // 采集线程。
    let capture_handle = map_err(capture::spawn_capture(
        &target,
        fps,
        paused.clone(),
        frames.producer(),
    ))?;

    // 音频线程（失败则降级为无声）。
    let (audio_tx, audio_rx) = crossbeam_channel::unbounded::<AudioFrame>();
    let audio_handle = if options.system_audio || options.mic_audio {
        match audio::spawn_audio(
            &options,
            audio_tx,
            state.system_mute.clone(),
            state.mic_mute.clone(),
        ) {
            Ok(handle) => Some(handle),
            Err(err) => {
                tracing::warn!("音频不可用，改为无声录制: {err:#}");
                emit_error(
                    &app,
                    &AppError::audio_device_missing(format!("音频不可用: {err}")),
                );
                None
            }
        }
    } else {
        None
    };

    // 编码线程。
    let session_id = SessionId::new();
    let request = encode::EncodeRequest {
        final_path: final_path.clone(),
        width,
        height,
        fps,
        has_audio: audio_handle.is_some(),
        params: encode::mf_h264::EncodeParams {
            width,
            height,
            fps,
            cqp: settings.video.quality.cqp,
            keyframe_sec: settings.video.keyframe_sec,
        },
        pref: settings.video.hardware,
        clock: clock.clone(),
        stats: stats.clone(),
        result: result.clone(),
        session_id,
        app: app.clone(),
    };
    let encoder_handle = map_err(encode::spawn_encode(request, frames.clone(), audio_rx))?;

    // 统计上报 tokio 任务（500ms），其 AbortHandle 存入会话。
    let stats_app = app.clone();
    let stats_clock = stats.clone();
    let stats_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let snapshot = *stats_clock.read();
            emit_stats(&stats_app, snapshot);
        }
    });

    let handle = SessionHandle {
        id: session_id,
        state: SessionState::Recording,
        abort: stats_task.abort_handle(),
        stats,
        paused: paused.clone(),
        output_path: Some(final_path),
        started_qpc,
        frames,
        clock,
        result,
        capture: Some(capture_handle),
        audio: audio_handle,
        encoder: Some(encoder_handle),
    };
    *state.session.write() = Some(handle);

    emit_state(&app, session_id, SessionState::Starting);
    emit_state(&app, session_id, SessionState::Recording);
    emit_stats(&app, CaptureStats::default());
    Ok(session_id)
}

/// 暂停录制：只停止推帧，保留会话与时间基准，并补偿暂停区间。
#[tauri::command]
pub fn record_pause(session: SessionId, app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    let qpc = capture::frame::now_qpc();
    let mut guard = state.session.write();
    let handle = guard
        .as_mut()
        .filter(|h| h.id == session)
        .ok_or_else(|| AppError::target_gone("会话不存在"))?;
    handle.paused.store(true, std::sync::atomic::Ordering::SeqCst);
    handle.clock.pause(qpc);
    handle.frames.clear();
    handle.state = SessionState::Paused;
    emit_state(&app, session, SessionState::Paused);
    Ok(())
}

/// 恢复录制：时间原点前移暂停时长，清空队列后继续。
#[tauri::command]
pub fn record_resume(session: SessionId, app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    let qpc = capture::frame::now_qpc();
    let mut guard = state.session.write();
    let handle = guard
        .as_mut()
        .filter(|h| h.id == session)
        .ok_or_else(|| AppError::target_gone("会话不存在"))?;
    handle.paused.store(false, std::sync::atomic::Ordering::SeqCst);
    handle.clock.resume(qpc);
    handle.frames.clear();
    handle.state = SessionState::Recording;
    emit_state(&app, session, SessionState::Recording);
    Ok(())
}

/// 停止录制：按 §10 顺序停止采集 -> 排空 -> 停止音频 -> Finalize -> 返回结果。
#[tauri::command]
pub async fn record_stop(
    session: SessionId,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<RecordingResult, AppError> {
    let mut handle = {
        let mut guard = state.session.write();
        match guard.take() {
            Some(h) if h.id == session => h,
            Some(h) => {
                *guard = Some(h);
                return Err(AppError::target_gone("会话 ID 不匹配"));
            }
            None => return Err(AppError::target_gone("会话不存在")),
        }
    };

    handle.state = SessionState::Stopping;
    emit_state(&app, session, SessionState::Stopping);
    if handle.clock.is_paused() {
        handle.clock.resume(capture::frame::now_qpc());
    }
    handle.paused.store(false, std::sync::atomic::Ordering::SeqCst);

    // 1) 停采集并 join。
    if let Some(capture) = handle.capture.take() {
        drop(capture);
    }
    // 2) 停音频。
    if let Some(audio) = handle.audio.take() {
        drop(audio);
    }
    // 3) 停编码线程：Drop 会置 stop 并 join，编码线程完成 Finalize + 重命名。
    if let Some(encoder) = handle.encoder.take() {
        drop(encoder);
    }
    // 4) 取消统计任务。
    handle.abort.abort();

    // 等待编码结果（最长 5s）。
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let result = loop {
        if let Some(result) = handle.result.read().clone() {
            break result;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(AppError::timeout("编码结果等待超时"));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };

    emit_state(&app, session, SessionState::Done);
    emit_stats(&app, *handle.stats.read());
    Ok(result)
}

fn qpc_frequency() -> i64 {
    #[cfg(windows)]
    {
        // NOTE: QueryPerformanceFrequency 返回 Result<()>。
        let mut value = 0i64;
        let _ = unsafe {
            windows::Win32::System::Performance::QueryPerformanceFrequency(&mut value)
        };
        value
    }
    #[cfg(not(windows))]
    {
        10_000_000
    }
}

// ---- 设置 -----------------------------------------------------------------

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<Settings, AppError> {
    Ok(state.config.read().clone())
}

#[tauri::command]
pub fn set_settings(
    settings: Settings,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Settings, AppError> {
    let old = state.config.read().clone();
    map_err(config::save_settings(&settings))?;
    *state.config.write() = settings.clone();

    // 热键变更：注销旧组合键并注册新组合键（忽略解析错误，避免阻断设置保存）。
    if let Err(err) = crate::hotkey::refresh(&app, &old, &settings) {
        tracing::warn!("热键刷新失败: {err:#}");
    }
    let _ = app.emit("settings:changed", &settings);
    Ok(settings)
}

/// 在文件管理器中打开输出目录。
#[tauri::command]
pub fn open_output_dir(state: State<'_, AppState>) -> Result<(), AppError> {
    let settings = state.config.read().clone();
    let dir = map_err(image::ops::ensure_output_dir(&settings.paths.output_dir))?;
    map_err(image::ops::reveal_dir(&dir))
}

/// 在文件管理器中定位并选中指定文件。
#[tauri::command]
pub fn reveal_file(path: String) -> Result<(), AppError> {
    let p = std::path::PathBuf::from(&path);
    if !p.exists() {
        return Err(AppError::internal(format!("文件不存在: {path}")));
    }
    map_err(image::ops::reveal_file(&p))
}

// ---- 崩溃恢复（扩展命令） -------------------------------------------------

#[tauri::command]
pub fn list_recoverable(state: State<'_, AppState>) -> Result<Vec<RecoveryCandidate>, AppError> {
    let settings = state.config.read().clone();
    let dir = map_err(image::ops::expand_output_dir(&settings.paths.output_dir))?;
    map_err(encode::scan_recovery(&dir))
}

#[tauri::command]
pub fn recover_file(candidate: RecoveryCandidate) -> Result<String, AppError> {
    map_err(encode::mp4::recover(&candidate))
}

#[tauri::command]
pub fn delete_temp(candidate: RecoveryCandidate) -> Result<(), AppError> {
    map_err(encode::mp4::delete_temp(&candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_url_carries_window_mode_without_leaking_old_query() {
        let base = tauri::Url::parse("http://tauri.localhost/index.html?view=overlay&mode=screenshot")
            .unwrap();
        let url = overlay_navigation_url(base, "window").unwrap();
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(url.path(), "/index.html");
        assert_eq!(query.get("view").map(String::as_str), Some("overlay"));
        assert_eq!(query.get("mode").map(String::as_str), Some("window"));
        assert_eq!(url.query_pairs().count(), 2);
    }

    #[test]
    fn overlay_url_preserves_dev_server_origin() {
        let base = tauri::Url::parse("http://127.0.0.1:5173/?view=overlay").unwrap();
        let url = overlay_navigation_url(base, "record").unwrap();
        assert_eq!(url.origin().ascii_serialization(), "http://127.0.0.1:5173");
        assert_eq!(url.path(), "/");
        assert!(url.query().unwrap().contains("mode=record"));
    }

    #[test]
    fn overlay_url_rejects_unknown_mode() {
        let base = tauri::Url::parse("http://tauri.localhost/index.html").unwrap();
        assert!(overlay_navigation_url(base, "malware").is_err());
    }
}
