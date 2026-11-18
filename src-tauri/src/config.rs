//! 配置持久化
//!
//! `Settings` 严格镜像 §12 的 settings.json。存储位置为
//! `%APPDATA%\SnapCap\settings.json`，写入采用「临时文件 -> 原子替换」。

use crate::models::ImageFormat;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// 多显示器策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MultiMonitorMode {
    /// 把所有显示器拼成一块虚拟桌面。
    #[serde(rename = "virtual")]
    Virtual,
    /// 每个显示器独立采集（与前端 types.ts / API.md 的 "per-monitor" 一致）。
    #[serde(rename = "per-monitor")]
    PerMonitor,
}

/// 质量模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QualityMode {
    Cqp,
    Vbr,
}

/// 硬件加速偏好。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HardwareAccel {
    Auto,
    Nvidia,
    Intel,
    Amd,
    Software,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    pub autostart: bool,
    pub minimize_to_tray: bool,
    pub language: String,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            autostart: true,
            minimize_to_tray: true,
            language: "zh-CN".to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureSettings {
    pub fps: u32,
    pub cursor: bool,
    pub click_highlight: bool,
    pub multi_monitor: MultiMonitorMode,
}

impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            fps: 60,
            cursor: true,
            click_highlight: true,
            multi_monitor: MultiMonitorMode::Virtual,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct QualitySettings {
    pub mode: QualityMode,
    pub cqp: u32,
}

impl Default for QualitySettings {
    fn default() -> Self {
        Self {
            mode: QualityMode::Cqp,
            cqp: 20,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoSettings {
    pub codec: String,
    pub quality: QualitySettings,
    pub hardware: HardwareAccel,
    pub keyframe_sec: u32,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            codec: "h264".to_string(),
            quality: QualitySettings::default(),
            hardware: HardwareAccel::Auto,
            keyframe_sec: 2,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    pub system: bool,
    pub mic: bool,
    pub system_gain: f64,
    pub mic_gain: f64,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            system: true,
            mic: false,
            system_gain: 1.0,
            mic_gain: 1.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ScreenshotSettings {
    pub format: ImageFormat,
    pub jpeg_quality: u8,
    pub include_cursor: bool,
}

impl Default for ScreenshotSettings {
    fn default() -> Self {
        Self {
            format: ImageFormat::Png,
            jpeg_quality: 92,
            include_cursor: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeySettings {
    pub region: String,
    pub full: String,
    pub window: String,
    pub record: String,
    pub pause: String,
    pub annotate: String,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            region: "Alt+A".to_string(),
            full: "Alt+S".to_string(),
            window: "Alt+W".to_string(),
            record: "Alt+R".to_string(),
            pause: "Alt+P".to_string(),
            annotate: "Alt+C".to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PathSettings {
    pub output_dir: String,
    pub naming: String,
}

impl Default for PathSettings {
    fn default() -> Self {
        Self {
            output_dir: "~/Pictures/SnapCap".to_string(),
            naming: "{yyyyMMdd-HHmmss}".to_string(),
        }
    }
}

/// 顶层设置，字段顺序与 §12 JSON 一致。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub general: GeneralSettings,
    pub capture: CaptureSettings,
    pub video: VideoSettings,
    pub audio: AudioSettings,
    pub screenshot: ScreenshotSettings,
    pub hotkeys: HotkeySettings,
    pub paths: PathSettings,
}

/// 配置目录：%APPDATA%\SnapCap
///
/// `SNAPCAP_CONFIG_DIR` 可覆盖该目录（便携部署与验收测试使用），
/// 测试时把配置与输出目录指向临时目录，避免污染用户数据。
pub fn config_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("SNAPCAP_CONFIG_DIR") {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    let base = dirs::config_dir().context("无法定位系统配置目录 (APPDATA)")?;
    Ok(base.join("SnapCap"))
}

/// 配置文件完整路径。
pub fn settings_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("settings.json"))
}

/// 读取设置；文件不存在或解析失败时回退到默认值（保证应用始终可用）。
pub fn load_settings() -> Result<Settings> {
    let path = settings_path()?;
    if !path.exists() {
        return Ok(Settings::default());
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("读取设置失败: {}", path.display()))?;
    match serde_json::from_str::<Settings>(&text) {
        Ok(settings) => Ok(settings),
        Err(err) => {
            tracing::warn!("settings.json 解析失败，使用默认配置: {err}");
            Ok(Settings::default())
        }
    }
}

/// 保存设置：写临时文件后原子替换目标文件。
pub fn save_settings(settings: &Settings) -> Result<()> {
    let path = settings_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建配置目录失败: {}", parent.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(settings).context("序列化设置失败")?;
    fs::write(&tmp, json).with_context(|| format!("写入临时文件失败: {}", tmp.display()))?;
    atomic_replace(&tmp, &path)
}

/// 跨平台原子替换。
///
/// NOTE: 在 Windows 上 `std::fs::rename` 目标存在时会失败，因此先移除再重命名。
/// 更严格的原子替换可改用 `MoveFileExW(..., MOVEFILE_REPLACE_EXISTING)`，
/// 这里采用无 windows-rs 细节依赖的实现以保证可移植性。
fn atomic_replace(from: &Path, to: &Path) -> Result<()> {
    match fs::rename(from, to) {
        Ok(()) => return Ok(()),
        Err(_err) => {
            if to.exists() {
                fs::remove_file(to)
                    .with_context(|| format!("删除旧目标失败: {}", to.display()))?;
            }
            fs::rename(from, to)
                .with_context(|| format!("原子替换失败: {} -> {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_match_spec() {
        let s = Settings::default();
        assert_eq!(s.capture.fps, 60);
        assert_eq!(s.video.quality.cqp, 20);
        assert_eq!(s.screenshot.jpeg_quality, 92);
        assert!(s.general.autostart);
        assert_eq!(s.hotkeys.region, "Alt+A");
        assert_eq!(s.paths.naming, "{yyyyMMdd-HHmmss}");
    }

    #[test]
    fn multi_monitor_serde_matches_frontend() {
        assert_eq!(serde_json::to_string(&MultiMonitorMode::Virtual).unwrap(), "\"virtual\"");
        assert_eq!(
            serde_json::to_string(&MultiMonitorMode::PerMonitor).unwrap(),
            "\"per-monitor\""
        );
        let m: MultiMonitorMode = serde_json::from_str("\"per-monitor\"").unwrap();
        assert_eq!(m, MultiMonitorMode::PerMonitor);
    }

    #[test]
    fn settings_json_roundtrip() {
        let settings = Settings::default();
        let json = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.hotkeys.record, settings.hotkeys.record);
        assert_eq!(back.video.hardware as i32, settings.video.hardware as i32);
    }
}
