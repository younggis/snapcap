/**
 * SnapCap 前后端共享 DTO（手动镜像 Rust `models.rs`）。
 * 约定：CaptureTarget 使用 serde tag="kind"，AnnotationOp 使用 tag="type"，均为 snake_case。
 */

/** 物理像素坐标，相对虚拟桌面左上角（可跨多屏，允许负值）。 */
export interface Rect {
  x: number
  y: number
  width: number
  height: number
}

export type CaptureTarget =
  | { kind: 'region'; rect: Rect }
  | { kind: 'window'; hwnd: number; include_border: boolean }
  | { kind: 'display'; monitor_handle: number }
  | { kind: 'full' }

export type SessionState =
  | 'starting'
  | 'recording'
  | 'paused'
  | 'stopping'
  | 'done'
  | 'failed'

export interface DisplayInfo {
  handle: number
  name: string
  is_primary: boolean
  rect: Rect
  scale_factor: number
}

export interface WindowInfo {
  hwnd: number
  title: string
  class_name: string
  rect: Rect
  is_visible: boolean
}

export type ImageFormat = 'png' | 'jpeg'

export interface ScreenshotOptions {
  format: ImageFormat
  jpeg_quality: number
  include_cursor: boolean
}

export interface ScreenshotResult {
  id: string
  width: number
  height: number
  /** base64 data URL，供 UI 预览 */
  data_url: string
}

export interface RecordingOptions {
  fps: 30 | 60
  capture_cursor: boolean
  click_highlight: boolean
  system_audio: boolean
  mic_audio: boolean
}

export interface RecordingResult {
  path: string
  duration_ms: number
  size: number
}

export type AnnotationOp =
  | { type: 'rect'; x: number; y: number; w: number; h: number; color: number; width: number }
  | { type: 'arrow'; from: [number, number]; to: [number, number]; color: number; width: number }
  | { type: 'text'; x: number; y: number; text: string; color: number; size: number }
  | { type: 'blur'; x: number; y: number; w: number; h: number; radius: number }
  | { type: 'step'; x: number; y: number; number: number }

export type AnnotationTool = AnnotationOp['type'] | 'select'

export interface CaptureStats {
  fps: number
  dropped_frames: number
  bitrate_kbps: number
  duration_ms: number
  audio_dropped: number
}

export interface AppErrorPayload {
  code: string
  message: string
  remediation: string
  retryable: boolean
}

export interface HotkeyPayload {
  action: 'region' | 'full' | 'window' | 'record' | 'pause' | 'annotate' | 'stop'
}

export interface SelectionPayload {
  rect: Rect
  hwnd?: number | null
  cancelled: boolean
}

export interface CaptureStatePayload {
  session: string
  state: SessionState
}

export interface Settings {
  general: { autostart: boolean; minimize_to_tray: boolean; language: string }
  capture: {
    fps: 60 | 30
    cursor: boolean
    click_highlight: boolean
    multi_monitor: 'virtual' | 'per-monitor'
  }
  video: {
    codec: 'h264'
    quality: { mode: 'cqp' | 'vbr'; cqp: number; bitrate_kbps?: number }
    hardware: 'auto' | 'nvidia' | 'intel' | 'amd' | 'software'
    keyframe_sec: number
  }
  audio: { system: boolean; mic: boolean; system_gain: number; mic_gain: number }
  screenshot: { format: ImageFormat; jpeg_quality: number; include_cursor: boolean }
  hotkeys: {
    region: string
    full: string
    window: string
    record: string
    pause: string
    annotate: string
  }
  paths: { output_dir: string; naming: string }
}

/** 与设计文档 §12 一致的默认值。 */
export const DEFAULT_SETTINGS: Settings = {
  general: { autostart: true, minimize_to_tray: true, language: 'zh-CN' },
  capture: { fps: 60, cursor: true, click_highlight: true, multi_monitor: 'virtual' },
  video: {
    codec: 'h264',
    quality: { mode: 'cqp', cqp: 20 },
    hardware: 'auto',
    keyframe_sec: 2,
  },
  audio: { system: true, mic: false, system_gain: 1.0, mic_gain: 1.0 },
  screenshot: { format: 'png', jpeg_quality: 92, include_cursor: false },
  hotkeys: {
    region: 'Alt+A',
    full: 'Alt+S',
    window: 'Alt+W',
    record: 'Alt+R',
    pause: 'Alt+P',
    annotate: 'Alt+C',
  },
  paths: { output_dir: '~/Pictures/SnapCap', naming: '{yyyyMMdd-HHmmss}' },
}

export const ERROR_REMEDIATIONS: Record<string, string> = {
  E_CAPTURE_CONSENT_DENIED: '请在 Windows 隐私设置中允许本应用访问屏幕，或改用全屏/区域采集。',
  E_CAPTURE_UNSUPPORTED: '当前系统或显卡不支持该采集方式，请尝试区域截图。',
  E_TARGET_GONE: '目标窗口已关闭，请重新选择。',
  E_ENCODER_NOT_AVAILABLE: '未找到可用的 H.264 编码器，请更新显卡驱动或改用软件编码。',
  E_AUDIO_DEVICE_MISSING: '未检测到音频设备，录制将继续但无声音。',
  E_DISK_FULL: '磁盘空间不足，请清理后重试。',
  E_TIMEOUT: '操作超时，请重试。',
  E_INTERNAL: '发生内部错误，请稍后重试。',
}
