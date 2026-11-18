//! Windows.Graphics.Capture 采集器（§6.3）
//!
//! WGC 依赖 WinRT 投影与系统采集授权，接口随 `windows` crate 版本变化较大。
//! 本构建保留完整模块边界与签名；录制路径会明确返回「WGC 不可用」，由
//! `capture::spawn_capture` 回退到 GDI 轮询。截图链路始终走 GDI（真实可用）。

use super::frame::VideoFrame;
use super::CaptureHandle;
use crate::models::CaptureTarget;
use anyhow::{anyhow, Result};
use crossbeam_channel::Sender;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// 启动 WGC 采集会话。
/// 当前构建未启用 WinRT 采集实现，返回错误以触发上层回退。
pub fn spawn_wgc(
    target: &CaptureTarget,
    _paused: Arc<AtomicBool>,
    _tx: Sender<VideoFrame>,
) -> Result<CaptureHandle> {
    let _ = target;
    Err(anyhow!(
        "Windows.Graphics.Capture 在当前构建中未启用；将回退到 GDI 采集"
    ))
}

/// 窗口尺寸变化 / DPI 变化时的重配置探测点（保留接口）。
pub fn needs_recreate(content_size_changed: bool) -> bool {
    content_size_changed
}
