//! GDI 截图实现
//!
//! 使用 GetDC / BitBlt / CreateDIBSection 读取 BGRA 像素。全平台 Windows 可用，
//! 是 RDP / 虚拟机 / 驱动异常时的截图兜底（§6.2）。按 `windows` 0.58 实际签名实现。

use super::frame::{CursorOverlay, VideoFrame};
use crate::error::AppError;
use crate::models::{CaptureTarget, Rect};
use anyhow::{anyhow, ensure, Context, Result};
use std::ffi::c_void;
use std::sync::Arc;

/// 进程级 DPI 感知：在 setup 早期调用，确保 `GetDC(NULL)` 返回物理像素。
#[cfg(windows)]
pub fn enable_dpi_awareness() {
    unsafe {
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        use windows::Win32::UI::WindowsAndMessaging::SetProcessDPIAware;
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_err() {
            let _ = SetProcessDPIAware();
        }
    }
}

#[cfg(not(windows))]
pub fn enable_dpi_awareness() {}

/// 把窗口标记为「对所有采集 API 不可见」(Windows 10 2004+)。
/// 这是防止工具自身界面出现在截图/录屏里的可靠方式：hide() + sleep 依赖 DWM
/// 重绘时序，不可靠；WDA_EXCLUDEFROMCAPTURE 由系统保证窗口不进入采集结果。
/// 注意：该标记不影响用户在屏幕上看到窗口，只影响采集。
#[cfg(windows)]
pub fn exclude_window_from_capture(hwnd: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
    };
    unsafe {
        SetWindowDisplayAffinity(HWND(hwnd as *mut c_void), WDA_EXCLUDEFROMCAPTURE).is_ok()
    }
}

#[cfg(not(windows))]
pub fn exclude_window_from_capture(_hwnd: isize) -> bool {
    false
}

/// 读取窗口的采集可见性标记，用于验收测试。
#[cfg(windows)]
pub fn window_display_affinity(hwnd: isize) -> u32 {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowDisplayAffinity;
    let mut affinity = 0u32;
    unsafe {
        let _ = GetWindowDisplayAffinity(HWND(hwnd as *mut c_void), &mut affinity);
    }
    affinity
}

#[cfg(not(windows))]
pub fn window_display_affinity(_hwnd: isize) -> u32 {
    0
}

/// 抓取当前屏幕一次，按 target 裁剪并返回 BGRA 帧。
pub fn capture(target: &CaptureTarget, include_cursor: bool) -> Result<VideoFrame> {
    #[cfg(windows)]
    {
        capture_windows(target, include_cursor)
    }
    #[cfg(not(windows))]
    {
        let _ = (target, include_cursor);
        Err(anyhow!("GDI 截图仅支持 Windows"))
    }
}

#[cfg(windows)]
fn capture_windows(target: &CaptureTarget, include_cursor: bool) -> Result<VideoFrame> {
    use windows::Win32::Foundation::{HANDLE, HWND};
    use windows::Win32::Graphics::Gdi::{
        GetDC, GetMonitorInfoW, HMONITOR, HDC,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorInfo, GetSystemMetrics, GetWindowRect, CURSORINFO, CURSOR_SHOWING,
        SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };

    let vx = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let vy = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let vw = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) }.max(1) as u32;
    let vh = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) }.max(1) as u32;

    let null_hwnd = HWND::default();

    // (HDC, 释放 DC 时使用的 HWND, 源坐标, 尺寸, 内容矩形)
    let (hdc, dc_hwnd, src_x, src_y, w, h, content_rect): (
        HDC,
        HWND,
        i32,
        i32,
        u32,
        u32,
        Rect,
    ) = match target {
        CaptureTarget::Region { rect } => (
            unsafe { GetDC(null_hwnd) },
            null_hwnd,
            rect.x,
            rect.y,
            rect.width.max(1),
            rect.height.max(1),
            *rect,
        ),
        CaptureTarget::Full => (
            unsafe { GetDC(null_hwnd) },
            null_hwnd,
            vx,
            vy,
            vw,
            vh,
            Rect::new(vx, vy, vw, vh),
        ),
        CaptureTarget::Display { monitor_handle } => {
            let hm = HMONITOR(*monitor_handle as *mut c_void);
            let mut info = windows::Win32::Graphics::Gdi::MONITORINFO {
                cbSize: std::mem::size_of::<windows::Win32::Graphics::Gdi::MONITORINFO>() as u32,
                ..Default::default()
            };
            ensure!(
                unsafe { GetMonitorInfoW(hm, &mut info) }.as_bool(),
                "GetMonitorInfoW 失败"
            );
            let r = info.rcMonitor;
            let rect = Rect::new(
                r.left,
                r.top,
                (r.right - r.left) as u32,
                (r.bottom - r.top) as u32,
            );
            (
                unsafe { GetDC(null_hwnd) },
                null_hwnd,
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                rect,
            )
        }
        CaptureTarget::Window { hwnd, .. } => {
            // hwnd=0 是 UI 传来的“当前前台窗口”哨兵值。
            let resolved = if *hwnd == 0 {
                unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }
            } else {
                HWND(*hwnd as *mut c_void)
            };
            let hw = if resolved.0.is_null() {
                HWND(*hwnd as *mut c_void)
            } else {
                resolved
            };
            let mut r = windows::Win32::Foundation::RECT::default();
            unsafe { GetWindowRect(hw, &mut r) }
                .map_err(|e| anyhow!("GetWindowRect 失败: {e:?}"))?;
            let w = (r.right - r.left).max(1) as u32;
            let h = (r.bottom - r.top).max(1) as u32;
            let rect = Rect::new(r.left, r.top, w, h);

            // GPU / DirectComposition 渲染的窗口（微信、QQ、Chromium/Electron 等）
            // 用 GetWindowDC + BitBlt 只能抓到全黑：DWM 合成后的画面只存在于
            // 屏幕 DC 中。改走屏幕 DC + 窗口屏幕矩形，与区域截图同一路径，
            // 才能抓到用户实际看到的内容。
            // 抓之前尽量恢复并前置目标窗口，降低被其它窗口遮挡而抓错的概率。
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::{
                    IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
                };
                if IsIconic(hw).as_bool() {
                    let _ = ShowWindow(hw, SW_RESTORE);
                }
                let _ = SetForegroundWindow(hw);
            }
            (
                unsafe { GetDC(null_hwnd) },
                null_hwnd,
                r.left,
                r.top,
                w,
                h,
                rect,
            )
        }
    };

    let bgra = capture_dc(hdc, src_x, src_y, w, h).with_context(|| {
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::ReleaseDC(dc_hwnd, hdc);
        }
        "capture_dc 失败"
    })?;
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::ReleaseDC(dc_hwnd, hdc);
    }

    let mut frame = VideoFrame::new(w, h, bgra);
    frame.content_rect = content_rect;

    if include_cursor {
        let mut ci = CURSORINFO {
            cbSize: std::mem::size_of::<CURSORINFO>() as u32,
            flags: CURSOR_SHOWING,
            hCursor: Default::default(),
            ptScreenPos: Default::default(),
        };
        let _ = unsafe { GetCursorInfo(&mut ci) };
        if ci.flags == CURSOR_SHOWING {
            let px = ci.ptScreenPos.x - content_rect.x;
            let py = ci.ptScreenPos.y - content_rect.y;
            if px >= 0 && py >= 0 {
                frame.cursor = Some(CursorOverlay {
                    x: px,
                    y: py,
                    shape_handle: ci.hCursor.0 as isize,
                    click_ring: None,
                });
                draw_cursor_arrow(Arc::make_mut(&mut frame.bgra), w, h, px, py);
            }
        }
    }

    // 显式丢弃未使用导入的类型检查占位。
    let _ = std::mem::size_of::<HANDLE>();
    Ok(frame)
}

/// 从 HDC 的 (x,y) 位置读取 w*h 区域为 32bpp BGRA（top-down）。
#[cfg(windows)]
fn capture_dc(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
) -> Result<Vec<u8>> {
    use windows::Win32::Foundation::{HANDLE, HWND};
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
        SelectObject,
        BI_RGB, DIB_RGB_COLORS, SRCCOPY, BITMAPINFO, BITMAPINFOHEADER, RGBQUAD,
    };

    let mem_dc = unsafe { CreateCompatibleDC(hdc) };
    ensure!(mem_dc.0 != std::ptr::null_mut(), "CreateCompatibleDC 失败");

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [RGBQUAD::default(); 1],
    };

    let mut bits: *mut c_void = std::ptr::null_mut();
    let hbm = unsafe {
        CreateDIBSection(
            hdc,
            &bmi,
            DIB_RGB_COLORS,
            &mut bits,
            HANDLE::default(),
            0,
        )
    }
    .map_err(|e| anyhow!("CreateDIBSection 失败: {e:?}"))?;
    ensure!(!bits.is_null(), "CreateDIBSection 未返回像素指针");

    let old = unsafe { SelectObject(mem_dc, hbm) };
    // BitBlt 偶发 ERROR_INVALID_HANDLE（DC 句柄短暂失效/会话切换），重试一次并
    // 重新获取屏幕 DC，提升生产环境健壮性。
    let mut blt = unsafe { BitBlt(mem_dc, 0, 0, w as i32, h as i32, hdc, x, y, SRCCOPY) };
    if blt.is_err() {
        std::thread::sleep(std::time::Duration::from_millis(30));
        let fresh = unsafe { GetDC(HWND::default()) };
        if fresh.0 != std::ptr::null_mut() {
            blt = unsafe { BitBlt(mem_dc, 0, 0, w as i32, h as i32, fresh, x, y, SRCCOPY) };
            unsafe { let _ = ReleaseDC(HWND::default(), fresh); }
        }
    }

    let size = w as usize * h as usize * 4;
    let mut bgra = unsafe { std::slice::from_raw_parts(bits as *const u8, size) }.to_vec();
    // 32bpp DIB + BitBlt(SRCCOPY) 只复制 RGB，不写入 alpha：CreateDIBSection
    // 未初始化的 alpha 字节通常为 0。屏幕 DC 恰好会带回 255，但 GetWindowDC
    // 抓单个窗口时 alpha 保持 0 —— 编码成 PNG 后大片区域透明，编辑器里
    // 看起来就是「截图空白」。屏幕采集结果本身不透明，统一写成 255。
    for px in bgra.chunks_exact_mut(4) {
        px[3] = 255;
    }

    unsafe {
        let _ = SelectObject(mem_dc, old);
        let _ = DeleteObject(hbm);
        let _ = DeleteDC(mem_dc);
    }
    blt.map_err(|e| anyhow!("BitBlt 失败: {e:?}"))?;
    Ok(bgra)
}

/// 在 BGRA 缓冲上画一个简易白色箭头光标（含黑色描边）。
#[cfg(windows)]
fn draw_cursor_arrow(bgra: &mut Vec<u8>, w: u32, h: u32, x: i32, y: i32) {
    const ARROW: [&str; 24] = [
        "#.................",
        "##................",
        "#.#...............",
        "#..#..............",
        "#...#.............",
        "#....#............",
        "#.....#...........",
        "#......#..........",
        "#.......#.........",
        "#........#........",
        "#.........#.......",
        "#..........#......",
        "#...........#.....",
        "#............#....",
        "#........#####....",
        "#....#...#........",
        "#...#.#..#........",
        "#..#..#..#........",
        "#.#....#.#........",
        "##......#.........",
        "#........#........",
        "..................",
        "..................",
        "..................",
    ];
    for (row, line) in ARROW.iter().enumerate() {
        for (col, ch) in line.bytes().enumerate() {
            if ch == b' ' {
                continue;
            }
            let px = x + col as i32;
            let py = y + row as i32;
            if px < 0 || py < 0 || px >= w as i32 || py >= h as i32 {
                continue;
            }
            let idx = (py as usize * w as usize + px as usize) * 4;
            let color = if ch == b'#' {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            };
            if idx + 3 < bgra.len() {
                bgra[idx..idx + 4].copy_from_slice(&color);
            }
        }
    }
}

#[cfg(not(windows))]
fn draw_cursor_arrow(_bgra: &mut Vec<u8>, _w: u32, _h: u32, _x: i32, _y: i32) {}

/// 把 `anyhow` 错误转换为 IPC 错误（供 ipc.rs 统一调用）。
pub fn to_app_error(err: anyhow::Error) -> AppError {
    AppError::unsupported(format!("GDI 截图失败: {err:#}"))
}
