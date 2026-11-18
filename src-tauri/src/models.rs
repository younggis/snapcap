//! 共享数据模型 (DTO)
//!
//! 本模块定义 Rust 与 Vue 前端 (`ui/src/types.ts`) 之间手动镜像的传输对象。
//! serde 的 tag/rename 规则与前端约定严格一致：
//!   - `CaptureTarget` 用 `kind` 判别，snake_case 变体名
//!   - `AnnotationOp`  用 `type` 判别，snake_case 变体名
//!   - 枚举状态一律 snake_case 序列化

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 物理像素坐标，相对虚拟桌面左上角（跨屏时允许负值）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// 判断点是否落在矩形内（边界包含）。
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && py >= self.y
            && px < self.x.saturating_add(self.width as i32)
            && py < self.y.saturating_add(self.height as i32)
    }

    /// 与另一个矩形求交集，无交集返回 None。
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = (self.x + self.width as i32).min(other.x + other.width as i32);
        let y2 = (self.y + self.height as i32).min(other.y + other.height as i32);
        if x2 <= x1 || y2 <= y1 {
            return None;
        }
        Some(Rect::new(x1, y1, (x2 - x1) as u32, (y2 - y1) as u32))
    }
}

/// 截图 / 录制的目标。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CaptureTarget {
    /// 屏幕上的一块矩形区域（虚拟桌面坐标）。
    Region { rect: Rect },
    /// 顶层窗口，`hwnd` 为 HWND 的整数值。
    Window { hwnd: isize, include_border: bool },
    /// 单个显示器，`monitor_handle` 为 HMONITOR 的整数值。
    Display { monitor_handle: isize },
    /// 整个虚拟桌面。
    Full,
}

/// 标注操作（编辑在 UI 侧完成，导出时回传 Rust 合成到最终位图）。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnnotationOp {
    Rect {
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        color: u32,
        width: u32,
    },
    Arrow {
        from: (i32, i32),
        to: (i32, i32),
        color: u32,
        width: u32,
    },
    Text {
        x: i32,
        y: i32,
        text: String,
        color: u32,
        size: u32,
    },
    Blur {
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        radius: u32,
    },
    Step {
        x: i32,
        y: i32,
        number: u32,
    },
}

/// 会话 / 采集状态机，snake_case 序列化给 UI。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Starting,
    Recording,
    Paused,
    Stopping,
    Done,
    Failed,
}

/// 显示器信息。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DisplayInfo {
    pub handle: isize,
    pub name: String,
    pub is_primary: bool,
    pub rect: Rect,
    pub scale_factor: f64,
}

/// 顶层窗口信息。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
    pub class_name: String,
    pub rect: Rect,
    pub is_visible: bool,
}

/// 截图输出格式。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    Png,
    Jpeg,
}

/// 截图选项。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScreenshotOptions {
    pub format: ImageFormat,
    pub jpeg_quality: u8,
    pub include_cursor: bool,
}

impl Default for ScreenshotOptions {
    fn default() -> Self {
        Self {
            format: ImageFormat::Png,
            jpeg_quality: 92,
            include_cursor: false,
        }
    }
}

/// 截图结果。`data_url` 形如 `data:image/png;base64,...`，供 UI 直接预览。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScreenshotResult {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub data_url: String,
}

/// 录制选项。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecordingOptions {
    pub fps: u32,
    pub capture_cursor: bool,
    pub click_highlight: bool,
    pub system_audio: bool,
    pub mic_audio: bool,
}

impl Default for RecordingOptions {
    fn default() -> Self {
        Self {
            fps: 60,
            capture_cursor: true,
            click_highlight: true,
            system_audio: true,
            mic_audio: false,
        }
    }
}

/// 录制结果。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecordingResult {
    pub path: String,
    pub duration_ms: u64,
    pub size: u64,
}

/// 采集统计（通过 `capture:stats` 事件上报）。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct CaptureStats {
    pub fps: u32,
    pub dropped_frames: u64,
    pub bitrate_kbps: u64,
    pub duration_ms: u64,
    pub audio_dropped: u64,
}

/// 区域选择覆盖层的提交载荷（UI -> Rust，经 `overlay_submit` 命令）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectionPayload {
    pub rect: Rect,
    pub hwnd: Option<isize>,
    pub cancelled: bool,
}

/// 兼容别名：`open_region_overlay` 解析后返回同样的结构。
pub type SelectionResult = SelectionPayload;

/// 会话标识（序列化为 UUID 字符串）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn to_string_simple(&self) -> String {
        self.0.to_string()
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

/// 录制临时文件恢复候选项（崩溃恢复用）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoveryCandidate {
    pub tmp_path: String,
    pub bak_path: Option<String>,
    pub size: u64,
    /// 该文件对应的原始输出路径（去掉 .tmp/.bak 后缀）。
    pub final_path: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_geometry() {
        let a = Rect::new(0, 0, 100, 100);
        assert!(a.contains(50, 50));
        assert!(!a.contains(100, 50));
        let b = Rect::new(50, 50, 100, 100);
        assert_eq!(a.intersect(&b), Some(Rect::new(50, 50, 50, 50)));
        assert_eq!(a.intersect(&Rect::new(200, 200, 10, 10)), None);
    }

    #[test]
    fn capture_target_serde_tag() {
        let t = CaptureTarget::Region { rect: Rect::new(1, 2, 3, 4) };
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"kind\":\"region\""));
        assert_eq!(serde_json::from_str::<CaptureTarget>(&json).unwrap().to_string(), t.to_string());
    }

    #[test]
    fn annotation_op_serde_tag() {
        let op = AnnotationOp::Step { x: 1, y: 2, number: 3 };
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("\"type\":\"step\""));
        let back: AnnotationOp = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, AnnotationOp::Step { number: 3, .. }));
    }

    #[test]
    fn session_state_serde_snake_case() {
        assert_eq!(serde_json::to_string(&SessionState::Recording).unwrap(), "\"recording\"");
        assert_eq!(serde_json::to_string(&SessionState::Paused).unwrap(), "\"paused\"");
    }

    impl CaptureTarget { fn to_string(&self) -> String { format!("{self:?}") } }
}
