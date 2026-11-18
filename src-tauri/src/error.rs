//! 统一错误类型
//!
//! `AppError` 通过 IPC 返回给 UI，所有字段（code/message/remediation/retryable）
//! 都可直接展示给用户。错误码常量与 §11.3 对齐。

use serde::Serialize;

// ---- 错误码常量 (§11.3) ---------------------------------------------------
pub const E_CAPTURE_CONSENT_DENIED: &str = "E_CAPTURE_CONSENT_DENIED";
pub const E_CAPTURE_UNSUPPORTED: &str = "E_CAPTURE_UNSUPPORTED";
pub const E_TARGET_GONE: &str = "E_TARGET_GONE";
pub const E_ENCODER_NOT_AVAILABLE: &str = "E_ENCODER_NOT_AVAILABLE";
pub const E_AUDIO_DEVICE_MISSING: &str = "E_AUDIO_DEVICE_MISSING";
pub const E_DISK_FULL: &str = "E_DISK_FULL";
pub const E_TIMEOUT: &str = "E_TIMEOUT";
pub const E_INTERNAL: &str = "E_INTERNAL";

/// 面向用户的结构化错误。
#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct AppError {
    pub code: String,
    pub message: String,
    pub remediation: String,
    pub retryable: bool,
}

impl AppError {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        remediation: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            remediation: remediation.into(),
            retryable,
        }
    }

    /// 内部错误：不可重试。
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(
            E_INTERNAL,
            message,
            "请重试；若持续出现请查看日志或联系支持。",
            false,
        )
    }

    /// 采集能力不支持：不可重试。
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(
            E_CAPTURE_UNSUPPORTED,
            message,
            "当前系统或环境不支持该采集方式，请尝试其它捕获目标。",
            false,
        )
    }

    /// 采集授权被拒：可重试，引导用户在系统弹窗中允许。
    pub fn consent_denied() -> Self {
        Self::new(
            E_CAPTURE_CONSENT_DENIED,
            "屏幕采集授权被拒绝",
            "请在 Windows 弹出的授权对话框中勾选“允许”，然后重试。",
            true,
        )
    }

    /// 目标窗口/显示器已消失：可重试。
    pub fn target_gone(message: impl Into<String>) -> Self {
        Self::new(
            E_TARGET_GONE,
            message,
            "目标可能已关闭或最小化，请重新选择后重试。",
            true,
        )
    }

    /// 编码器不可用：不可重试。
    pub fn encoder_unavailable(message: impl Into<String>) -> Self {
        Self::new(
            E_ENCODER_NOT_AVAILABLE,
            message,
            "未找到可用的 H.264 编码器，请确认系统 Media Foundation 组件完整。",
            false,
        )
    }

    /// 音频设备缺失：可重试（仅影响声音）。
    pub fn audio_device_missing(message: impl Into<String>) -> Self {
        Self::new(
            E_AUDIO_DEVICE_MISSING,
            message,
            "请检查默认音频设备，或改为仅录制视频 / 仅麦克风。",
            true,
        )
    }

    /// 磁盘已满：不可重试。
    pub fn disk_full() -> Self {
        Self::new(
            E_DISK_FULL,
            "磁盘空间不足",
            "请清理磁盘或更换输出目录后重试。",
            false,
        )
    }

    /// 超时：可重试。
    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(E_TIMEOUT, message, "请稍后重试。", true)
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        // 保留底层错误信息，统一归类为内部错误。
        AppError::internal(format!("{err:#}"))
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        if err.kind() == std::io::ErrorKind::StorageFull {
            return AppError::disk_full();
        }
        AppError::internal(format!("I/O 错误: {err}"))
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::internal(format!("序列化错误: {err}"))
    }
}

/// 把 `anyhow::Error` 转成可 emit 的轻量载荷。
pub fn to_event_payload(err: &AppError) -> serde_json::Value {
    serde_json::json!({
        "code": err.code,
        "message": err.message,
        "remediation": err.remediation,
        "retryable": err.retryable,
    })
}
