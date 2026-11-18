//! 选区几何 helpers（§9.1）
//!
//! 虚拟桌面坐标、DPI 换算、跨屏裁剪、窗口吸附（`WindowFromPoint` +
//! `GetWindowRect`）。MVP 的 Canvas 选区在 UI 侧完成，本模块提供 Rust 侧几何。

use crate::models::Rect;

/// 单个显示器的几何信息。
#[derive(Clone, Debug)]
pub struct MonitorGeometry {
    pub handle: isize,
    pub rect: Rect,
    pub is_primary: bool,
    pub scale_factor: f64,
    pub name: String,
}

/// 虚拟桌面整体包围盒（物理像素）。
pub fn virtual_desktop_bounds(monitors: &[Rect]) -> Rect {
    monitors
        .iter()
        .copied()
        .reduce(|a, b| {
            let x = a.x.min(b.x);
            let y = a.y.min(b.y);
            let right = (a.x + a.width as i32).max(b.x + b.width as i32);
            let bottom = (a.y + a.height as i32).max(b.y + b.height as i32);
            Rect::new(x, y, (right - x) as u32, (bottom - y) as u32)
        })
        .unwrap_or_default()
}

/// DPI 换算：logical -> physical。
pub fn logical_to_physical(value: i32, scale: f64) -> i32 {
    (value as f64 * scale).round() as i32
}

/// DPI 换算：physical -> logical。
pub fn physical_to_logical(value: i32, scale: f64) -> i32 {
    (value as f64 / scale).round() as i32
}

/// 把一个跨屏矩形按显示器切成多个物理像素片段（§16.1 跨屏裁剪）。
pub fn crop_across_monitors(rect: Rect, monitors: &[Rect]) -> Vec<Rect> {
    let mut parts = Vec::new();
    for monitor in monitors {
        if let Some(piece) = rect.intersect(monitor) {
            parts.push(piece);
        }
    }
    if parts.is_empty() {
        // 完全落在显示区域外：原样返回，交由上层决定。
        parts.push(rect);
    }
    parts
}

/// 悬停窗口吸附：返回该点所在顶层窗口的屏幕矩形（物理像素）。
pub fn snap_window_at_point(x: i32, y: i32) -> Option<Rect> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{POINT, RECT};
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, WindowFromPoint};
        let hwnd = unsafe { WindowFromPoint(POINT { x, y }) };
        if hwnd.0.is_null() {
            return None;
        }
        let mut r = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut r) }.is_err() {
            return None;
        }
        Some(Rect::new(
            r.left,
            r.top,
            (r.right - r.left).max(0) as u32,
            (r.bottom - r.top).max(0) as u32,
        ))
    }
    #[cfg(not(windows))]
    {
        let _ = (x, y);
        None
    }
}

/// 某点所在显示器的 DPI 缩放系数（96 DPI = 1.0）。
pub fn scale_factor_at_point(x: i32, y: i32) -> f64 {
    #[cfg(windows)]
    {
        use windows::Win32::Graphics::Gdi::{
            MonitorFromPoint, MONITOR_DEFAULTTOPRIMARY,
        };
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::HiDpi::GetDpiForSystem;
        let _monitor = unsafe {
            MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTOPRIMARY)
        };
        // NOTE: 精确每屏 DPI 可用 GetDpiForMonitor；这里用系统 DPI 近似。
        let dpi = unsafe { GetDpiForSystem() };
        (dpi as f64 / 96.0).max(0.25)
    }
    #[cfg(not(windows))]
    {
        let _ = (x, y);
        1.0
    }
}

/// 归一化选区：处理反向拖拽（右下 -> 左上）。
pub fn normalize_selection(x0: i32, y0: i32, x1: i32, y1: i32) -> Rect {
    let x = x0.min(x1);
    let y = y0.min(y1);
    let w = (x0 - x1).abs().max(1) as u32;
    let h = (y0 - y1).abs().max(1) as u32;
    Rect::new(x, y, w, h)
}
