//! DXGI Desktop Duplication 采集器（§6.4）
//!
//! 保留模块边界与签名。当前构建未启用 DXGI 实现，`spawn_capture` 会回退到 GDI
//! 轮询采集（真实抓屏）。WGC/DXGI 的 WinRT/COM 接口对 `windows` crate 版本敏感，
//! 可在具备对应 SDK 的环境中按生成文档补齐后重新启用。

use super::frame::VideoFrame;
use super::CaptureHandle;
use crate::models::CaptureTarget;
use anyhow::{anyhow, Result};
use crossbeam_channel::Sender;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// 启动 DXGI 桌面复制采集会话。
pub fn spawn_dxgi(
    target: &CaptureTarget,
    _fps: u32,
    _paused: Arc<AtomicBool>,
    _tx: Sender<VideoFrame>,
) -> Result<CaptureHandle> {
    let _ = target;
    Err(anyhow!(
        "DXGI 桌面复制在当前构建中未启用；将回退到 GDI 采集"
    ))
}

/// 处理 `DXGI_ERROR_ACCESS_LOST` 的重建策略开关（保留接口）。
pub fn should_recreate(hr: i32) -> bool {
    // DXGI_ERROR_ACCESS_LOST = 0x887A0005（有符号为负）。
    hr as u32 == 0x887A0005
}
