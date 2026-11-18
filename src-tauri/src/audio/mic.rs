//! WASAPI 采集端点（麦克风）实现（§7.1）
//!
//! 与 loopback 共用采集循环，仅把端点改为 eCapture、不启用 LOOPBACK 标志。
//! 麦克风被占用时由 `audio::spawn_audio` 按 §7.4 降级为“仅系统声”。

use super::loopback::{spawn_wasapi_capture, Endpoint};
use super::{AudioCaptureHandle, AudioFrame};
use anyhow::{anyhow, Result};
use crossbeam_channel::Sender;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// 启动麦克风采集（仅在 Windows 上真实工作）。
pub fn spawn_mic(tx: Sender<AudioFrame>, mute: Arc<AtomicBool>) -> Result<AudioCaptureHandle> {
    #[cfg(not(windows))]
    {
        let _ = (tx, mute);
        return Err(anyhow!("WASAPI 麦克风采集仅支持 Windows"));
    }

    #[cfg(windows)]
    {
        spawn_wasapi_capture(tx, mute, Endpoint::Capture, false)
    }
}
