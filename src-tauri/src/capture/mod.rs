//! 采集模块
//!
//! 提供 `CaptureSource` trait（§6.1）、三种实现（WGC / DXGI / GDI）的选择策略
//! （§6.2）以及统一的一次性抓帧 / 连续采集入口。跨线程只传递拥有型的
//! `VideoFrame`，采集线程只入队不编码（§10）。

pub mod dxgi;
pub mod frame;
pub mod gdi;
pub mod wgc;

use crate::error::{AppError, E_CAPTURE_CONSENT_DENIED};
use crate::models::{CaptureTarget, Rect};
use anyhow::Result;
use crossbeam_channel::Sender;
use frame::VideoFrame;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// 采集器能力描述（§6.1）。
#[derive(Clone, Copy, Debug)]
pub struct CaptureCapabilities {
    pub supports_window: bool,
    pub supports_display: bool,
    pub supports_region: bool,
    pub requires_consent: bool,
    pub max_fps: u32,
}

/// 采集源 trait（§6.1）。
pub trait CaptureSource: Send {
    fn capabilities(&self) -> CaptureCapabilities;
    fn start(&mut self, target: &CaptureTarget, tx: Sender<VideoFrame>) -> Result<CaptureHandle>;
    fn set_paused(&mut self, paused: bool) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
}

/// 已启动采集会话的句柄。Drop 时自动置停止标志。
pub struct CaptureHandle {
    pub stop: Arc<AtomicBool>,
    pub paused: Arc<AtomicBool>,
    pub join: Option<std::thread::JoinHandle<()>>,
}

impl CaptureHandle {
    pub fn pause(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// 屏幕采集授权状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureConsent {
    Granted,
    Denied,
    Unknown,
}

/// 选择出的采集实现（§6.2）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Wgc,
    Dxgi,
    Gdi,
}

/// 当前 Windows 构建号；非 Windows 返回 0。
pub fn windows_build() -> u32 {
    #[cfg(windows)]
    {
        // NOTE: 也可用 GetVersionExW / RtlGetVersion；这里用简单的注册表无关实现。
        use windows::Win32::System::SystemInformation::GetVersion;
        let packed = unsafe { GetVersion() };
        let major = packed & 0xff;
        let minor = (packed >> 8) & 0xff;
        let build = (packed >> 16) & 0xffff;
        // Win10 = 10.0 => 主版本映射为 10000 系列构建号；此处仅粗略判断阈值。
        if major >= 10 {
            build.max(17763)
        } else {
            0
        }
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// 选择策略（§6.2）。
pub fn select_source(target: &CaptureTarget, build: u32, consent: CaptureConsent) -> SourceKind {
    let granted = consent == CaptureConsent::Granted;
    match target {
        CaptureTarget::Window { .. } => {
            if build >= 17763 && granted {
                SourceKind::Wgc
            } else {
                SourceKind::Gdi
            }
        }
        CaptureTarget::Display { .. } | CaptureTarget::Full => {
            if build >= 19041 && granted {
                SourceKind::Wgc
            } else if build >= 8 {
                SourceKind::Dxgi
            } else {
                SourceKind::Gdi
            }
        }
        CaptureTarget::Region { .. } => SourceKind::Gdi,
    }
}

/// 若授权被拒，返回对应的结构化错误（UI 可据此引导用户）。
pub fn consent_error() -> AppError {
    AppError::new(
        E_CAPTURE_CONSENT_DENIED,
        "屏幕采集授权被拒绝",
        "请在 Windows 授权对话框中选择“允许”，或改用区域截图。",
        true,
    )
}

/// 一次性抓帧（截图路径）。
///
/// NOTE: 生产环境对 Window/Display 优先 WGC，但一次性截图需要系统授权弹窗；
/// 为响应速度和可靠性（<100ms，§G4），截图统一走 GDI CPU 兜底路径，
/// WGC/DXGI 用于连续录制（见 `spawn_capture`）。
pub fn capture_frame(target: &CaptureTarget, include_cursor: bool) -> Result<VideoFrame> {
    let _kind = select_source(target, windows_build(), CaptureConsent::Granted);
    gdi::capture(target, include_cursor)
}

/// 连续采集入口（录制路径）：优先 WGC，失败回退 DXGI，最后 GDI 轮询。
pub fn spawn_capture(
    target: &CaptureTarget,
    fps: u32,
    paused: Arc<AtomicBool>,
    tx: Sender<VideoFrame>,
) -> Result<CaptureHandle> {
    let build = windows_build();
    let _ = build;

    #[cfg(windows)]
    {
        if let Ok(handle) = wgc::spawn_wgc(target, paused.clone(), tx.clone()) {
            return Ok(handle);
        }
        tracing::info!("WGC 不可用，尝试 DXGI 桌面复制");
        if let Ok(handle) = dxgi::spawn_dxgi(target, fps, paused.clone(), tx.clone()) {
            return Ok(handle);
        }
        tracing::info!("DXGI 不可用，回退 GDI 轮询采集");
    }

    // GDI 轮询兜底（全平台可编译；Windows 上为真实抓屏）。
    spawn_gdi_poll(target, fps, paused, tx)
}

/// GDI 轮询采集线程：按目标帧率节流，仅入队。
fn spawn_gdi_poll(
    target: &CaptureTarget,
    fps: u32,
    paused: Arc<AtomicBool>,
    tx: Sender<VideoFrame>,
) -> Result<CaptureHandle> {
    let target = target.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let paused_thread = paused.clone();
    let interval = Duration::from_micros(1_000_000u64 / fps.max(1) as u64);

    let join = std::thread::Builder::new()
        .name("snapcap-capture-gdi".to_string())
        .spawn(move || {
            while !stop_thread.load(Ordering::SeqCst) {
                let tick = std::time::Instant::now();
                if !paused_thread.load(Ordering::SeqCst) {
                    match capture_frame_gdi(&target) {
                        Ok(frame) => {
                            if tx.send(frame).is_err() {
                                break; // 接收端已关闭
                            }
                        }
                        Err(err) => {
                            tracing::warn!("GDI 采集失败: {err:#}");
                        }
                    }
                }
                let elapsed = tick.elapsed();
                if elapsed < interval {
                    std::thread::sleep(interval - elapsed);
                }
            }
        })?;

    Ok(CaptureHandle {
        stop,
        paused,
        join: Some(join),
    })
}

fn capture_frame_gdi(target: &CaptureTarget) -> Result<VideoFrame> {
    gdi::capture(target, false)
}

/// 把虚拟桌面矩形裁剪到最可能的显示器范围内（供 overlay/region 复用）。
pub fn clamp_to_desktop(rect: Rect, monitors: &[Rect]) -> Rect {
    let mut result = rect;
    let desktop = monitors.iter().copied().reduce(|a, b| {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        let r = (a.x + a.width as i32).max(b.x + b.width as i32);
        let bo = (a.y + a.height as i32).max(b.y + b.height as i32);
        Rect::new(x, y, (r - x) as u32, (bo - y) as u32)
    });
    if let Some(d) = desktop {
        if let Some(intersection) = result.intersect(&d) {
            result = intersection;
        }
    }
    result
}
