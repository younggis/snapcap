# SnapCap IPC 协议（UI ↔ Rust）

> 本文定义 Vue 前端（`ui/`）与 Rust 后端（`src-tauri/`）之间的 Tauri 2 IPC 契约，对应设计文档 `DESIGN.md` §11。
> 命令使用 `invoke`，事件使用 `listen`。所有 DTO 由 serde 序列化；字段名为 **snake_case**。
>
> **标签约定（重要）**：`CaptureTarget` 使用 `#[serde(tag = "kind")]`，`AnnotationOp` 使用 `#[serde(tag = "type")]`，
> 即 JSON 对象内部以一个判别字段表达枚举变体，变体与字段名均为 snake_case。前端 `ui/src/types.ts` 需与此保持一致。

---

## 1. Commands（UI → Rust）

通过 `invoke(<command>, <args>)` 调用。参数以对象形式传入，键名与下表一致。

| 命令 | 参数 | 返回 |
|---|---|---|
| `list_displays` | — | `Vec<DisplayInfo>` |
| `list_windows` | — | `Vec<WindowInfo>` |
| `open_region_overlay` | `mode: "screenshot" \| "record" \| "window"` | `SelectionPayload`（`window` 模式返回用户点击的窗口 `hwnd`） |
| `screenshot_capture` | `target: CaptureTarget`, `options: ScreenshotOptions` | `ScreenshotResult` |
| `screenshot_output` | `id: string`, `destination: "file" \| "clipboard"`, `path?: string` | `()` |
| `annotation_apply` | `id: string`, `ops: Vec<AnnotationOp>` | `new_id: string` |
| `record_start` | `target: CaptureTarget`, `options: RecordingOptions` | `SessionId` |
| `record_pause` | `session: SessionId` | `()` |
| `record_resume` | `session: SessionId` | `()` |
| `record_stop` | `session: SessionId` | `RecordingResult` |
| `get_settings` | — | `Settings` |
| `set_settings` | `settings: Settings` | `Settings` |
| `open_output_dir` | — | `()` |

> 说明：`record_pause` / `record_resume` 在原表中合并为一行，实际为两个独立命令，参数相同。
> `get_settings` / `set_settings` 同理为两个独立命令。失败时命令以 `Err(AppError)` 拒绝（reject）。

---

## 2. Events（Rust → UI）

通过 `listen(<event>, callback)` 订阅；回调收到 `event.payload`。

| 事件 | 载荷 | 说明 |
|---|---|---|
| `capture:state` | `{ session: SessionId, state: SessionState }` | 会话状态变化 |
| `capture:stats` | `CaptureStats` | 周期性统计（约每 500ms） |
| `capture:error` | `AppError` | 错误 / 警告（含 `code`、`remediation`、`retryable`） |
| `overlay:selection` | `{ rect: Rect, hwnd?: isize \| null, cancelled: boolean }` | 选区覆盖层结果（取消时 `cancelled=true`，`rect` 尺寸可为 0） |
| `hotkey:triggered` | `{ action: string }` | 全局热键触发；action ∈ `region`/`full`/`window`/`record`/`pause`/`annotate` |
| `settings:changed` | `Settings` | 配置被修改（任意来源） |

---

## 3. 错误码

`AppError.code` 取值（字符串枚举）：

| 错误码 | 含义 |
|---|---|
| `E_CAPTURE_CONSENT_DENIED` | 用户拒绝 Windows 屏幕采集授权 |
| `E_CAPTURE_UNSUPPORTED` | 当前环境 / 版本不支持该采集方式 |
| `E_TARGET_GONE` | 目标窗口 / 显示器已消失 |
| `E_ENCODER_NOT_AVAILABLE` | 无可用 H.264 编码器 |
| `E_AUDIO_DEVICE_MISSING` | 缺少默认音频设备（警告，录制可继续） |
| `E_DISK_FULL` | 磁盘空间不足 |
| `E_TIMEOUT` | 操作超时 |
| `E_INTERNAL` | 内部错误 |

**每个错误必须携带** `remediation`（用户可执行的提示文案）与 `retryable`（是否可重试）。

---

## 4. 共享 DTO

### 4.1 Rect

物理像素坐标，相对虚拟桌面左上角（可跨多屏，允许负值）。

```ts
interface Rect { x: number; y: number; width: number; height: number } // i32, i32, u32, u32
```

### 4.2 CaptureTarget — `#[serde(tag = "kind")]`

```ts
type CaptureTarget =
  | { kind: "region";  rect: Rect }
  | { kind: "window";  hwnd: number; include_border: boolean }
  | { kind: "display"; monitor_handle: number }
  | { kind: "full" };
```

> Rust 侧为 `Region { rect: Rect }` 结构体变体（内部标签模式下保留 `rect` 嵌套）；「整个虚拟桌面」的变体名为 `full`。

### 4.3 AnnotationOp — `#[serde(tag = "type")]`

```ts
type AnnotationOp =
  | { type: "rect";  x: number; y: number; w: number; h: number; color: number; width: number }
  | { type: "arrow"; from: [number, number]; to: [number, number]; color: number; width: number }
  | { type: "text";  x: number; y: number; text: string; color: number; size: number }
  | { type: "blur";  x: number; y: number; w: number; h: number; radius: number }
  | { type: "step";  x: number; y: number; number: number };
```

> `color` 为 `u32`（`0xAARRGGBB`）；`from`/`to` 元组序列化为二元数组。

### 4.4 SessionState

```ts
type SessionState = "starting" | "recording" | "paused" | "stopping" | "done" | "failed";
```

### 4.5 Settings

持久化到 `%APPDATA%\SnapCap\settings.json`（写临时文件 → 原子替换）。

```ts
interface Settings {
  general: { autostart: boolean; minimize_to_tray: boolean; language: string };
  capture: { fps: 30 | 60; cursor: boolean; click_highlight: boolean; multi_monitor: "virtual" | "per-monitor" };
  video: {
    codec: "h264";
    quality: { mode: "cqp" | "vbr"; cqp: number; bitrate_kbps?: number };
    hardware: "auto" | "nvidia" | "intel" | "amd" | "software";
    keyframe_sec: number;
  };
  audio: { system: boolean; mic: boolean; system_gain: number; mic_gain: number };
  screenshot: { format: "png" | "jpeg"; jpeg_quality: number; include_cursor: boolean };
  hotkeys: {
    region: string; full: string; window: string;
    record: string; pause: string; annotate: string;
  };
  paths: { output_dir: string; naming: string };
}
```

### 4.6 ScreenshotResult

```ts
interface ScreenshotResult {
  id: string;       // UUID 字符串
  width: number;    // u32
  height: number;   // u32
  data_url: string; // data:image/png;base64,... 或 data:image/jpeg;base64,...，供 UI 直接预览
}
```

### 4.7 RecordingResult

```ts
interface RecordingResult {
  path: string;        // 最终 MP4 绝对路径
  duration_ms: number; // u64
  size: number;        // u64，字节
}
```

### 4.8 CaptureStats

```ts
interface CaptureStats {
  fps: number;            // 实际帧率
  dropped_frames: number; // 累计丢帧
  bitrate_kbps: number;   // 当前码率
  duration_ms: number;    // 录制时长
  audio_dropped: number;  // 音频溢出丢弃计数
}
```

### 4.9 AppError

```ts
type ErrorCode =
  | "E_CAPTURE_CONSENT_DENIED" | "E_CAPTURE_UNSUPPORTED" | "E_TARGET_GONE"
  | "E_ENCODER_NOT_AVAILABLE" | "E_AUDIO_DEVICE_MISSING" | "E_DISK_FULL"
  | "E_TIMEOUT" | "E_INTERNAL";

interface AppError {
  code: ErrorCode;
  message: string;
  remediation: string;
  retryable: boolean;
}
```

### 4.10 辅助 DTO（命令表引用，实现可细化）

```ts
type ImageFormat = "png" | "jpeg";

interface DisplayInfo { handle: number; name: string; is_primary: boolean; rect: Rect; scale_factor: number }
interface WindowInfo  { hwnd: number; title: string; class_name: string; rect: Rect; is_visible: boolean }
interface SelectionResult { rect: Rect | null; hwnd?: number | null; cancelled: boolean }
interface ScreenshotOptions { format: ImageFormat; jpeg_quality: number; include_cursor: boolean }
interface RecordingOptions {
  fps: 30 | 60;
  capture_cursor: boolean;
  click_highlight: boolean;
  system_audio: boolean;
  mic_audio: boolean;
}
type SessionId = string; // UUID 字符串
interface RecoveryCandidate { tmp_path: string; bak_path: string | null; size: number; final_path: string }
```

---

## 5. TypeScript 示例

### 5.1 调用命令（invoke）

```ts
import { invoke } from '@tauri-apps/api/core';
import type {
  DisplayInfo, WindowInfo, CaptureTarget, ScreenshotOptions, ScreenshotResult,
  RecordingOptions, Settings, RecordingResult, SelectionResult, SessionId,
} from './types';

// 枚举显示器 / 窗口
const displays = await invoke<DisplayInfo[]>('list_displays');
const windows = await invoke<WindowInfo[]>('list_windows');

// 弹出选区覆盖层
const selection: SelectionResult = await invoke('open_region_overlay', { mode: 'screenshot' });
if (!selection.cancelled && selection.rect) {
  const target: CaptureTarget = { kind: 'region', rect: selection.rect };
  const options: ScreenshotOptions = { format: 'png', jpeg_quality: 92, include_cursor: false };

  // 截图
  const shot: ScreenshotResult = await invoke('screenshot_capture', { target, options });
  await invoke('screenshot_output', { id: shot.id, destination: 'clipboard' });
  // 或：await invoke('screenshot_output', { id: shot.id, destination: 'file', path: '/abs/out.png' });

  // 标注（AnnotationOp 以 tag = "type" 表达）
  const newId: string = await invoke('annotation_apply', {
    id: shot.id,
    ops: [{ type: 'rect', x: 10, y: 10, w: 100, h: 50, color: 0xFFFF0000, width: 2 }],
  });
}

// 窗口截图：mode=window 时由用户在覆盖层点击真实窗口，返回其 hwnd。
const picked: SelectionResult = await invoke('open_region_overlay', { mode: 'window' });
if (!picked.cancelled && picked.hwnd != null) {
  const target: CaptureTarget = { kind: 'window', hwnd: picked.hwnd, include_border: true };
  const shot: ScreenshotResult = await invoke('screenshot_capture', {
    target,
    options: { format: 'png', jpeg_quality: 92, include_cursor: false },
  });
  const savedPath: string = await invoke('screenshot_output', { id: shot.id, destination: 'file' });
  console.log(savedPath);
}

// 录制
const recOptions: RecordingOptions = {
  fps: 60,
  capture_cursor: true, click_highlight: true,
  system_audio: true, mic_audio: false,
};
const session: SessionId = await invoke('record_start', {
  target: { kind: 'full' }, options: recOptions,
});
await invoke('record_pause', { session });
await invoke('record_resume', { session });
const result: RecordingResult = await invoke('record_stop', { session });
console.log(result.path, result.duration_ms, result.size);

// 设置
const settings: Settings = await invoke('get_settings');
await invoke('set_settings', { settings: { ...settings, capture: { ...settings.capture, fps: 30 } } });
await invoke('open_output_dir');
```

### 5.2 订阅事件（listen）

```ts
import { listen } from '@tauri-apps/api/event';
import type { CaptureStats, SessionState, Settings, AppError, Rect } from './types';

const unlistenStats = await listen<CaptureStats>('capture:stats', ({ payload }) => {
  console.log(payload.fps, payload.dropped_frames, payload.duration_ms);
});

await listen<{ session: string; state: SessionState }>('capture:state', ({ payload }) => {
  console.log('state =', payload.state);
});

await listen<AppError>('capture:error', ({ payload }) => {
  showToast(payload.message, payload.remediation, payload.retryable);
});

await listen<{ rect: Rect | null; hwnd?: number | null; cancelled: boolean }>('overlay:selection', ({ payload }) => {
  if (!payload.cancelled && payload.rect) applySelection(payload.rect);
});

await listen<{ action: string }>('hotkey:triggered', ({ payload }) => {
  handleHotkey(payload.action);
});

await listen<Settings>('settings:changed', ({ payload }) => {
  store.setSettings(payload);
});

// 组件卸载时取消订阅
unlistenStats();
```

### 5.3 错误处理

```ts
try {
  await invoke('record_start', { target, options });
} catch (err) {
  const e = err as { code?: string; message?: string; remediation?: string; retryable?: boolean };
  // Tauri 将 Err(AppError) 序列化为上述结构
  showError(e.message ?? '未知错误', e.remediation ?? '', e.retryable ?? false);
}
```

---

## 6. 备注

- 命令失败均返回 `AppError`；事件 `capture:error` 用于传递警告（如 `E_AUDIO_DEVICE_MISSING`）与异步采集错误。
- `SessionId`、`ScreenshotResult.id` 均为 UUID 字符串。
- 该协议是 UI 与后端的**唯一契约**；修改需同步更新本文、`ui/src/types.ts` 与 Rust 侧结构定义。
