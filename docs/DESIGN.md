# SnapCap 技术设计文档

> 版本：v0.1（设计稿） · 日期：2026-11-17
> 项目：Windows 平台轻便截图 + 录屏工具
> 技术栈：Tauri 2（Rust）+ Vue 3 + TypeScript

---

## 1. 文档目的与范围

本文档定义项目的**目标、架构、模块接口、线程模型、数据格式、构建与测试方案**，作为实现阶段的依据。

- **包含**：截图、录制、音频、编码、标注、全局热键、托盘、配置、打包。
- **不包含（本期）**：摄像头画中画、直播推流、WebM/VP9、移动/跨平台、云端同步、OCR。

---

## 2. 产品定义

### 2.1 一句话定位

开机常驻托盘的轻量工具，用全局热键完成「截图 → 标注 → 保存/复制」和「屏幕/窗口/区域录制 → 高清 MP4」。

### 2.2 设计目标

| 编号 | 目标 | 验收指标 |
|---|---|---|
| G1 | 小体积 | 安装包 ≤ 15MB（不含系统 WebView2 运行时） |
| G2 | 低占用 | 空闲内存 ≤ 80MB；空闲 CPU < 0.5% |
| G3 | 高清 | 1080p60 / 1440p60 / 4K30，H.264，CQP 18–23 |
| G4 | 快 | 截图热键响应 < 100ms，保存 < 80ms；录制启动 < 500ms |
| G5 | 稳 | 连续录制 2 小时不卡顿；崩溃后临时文件可恢复 |
| G6 | 隐私 | 默认零网络请求、零遥测；采集需用户主动触发 |

### 2.3 非目标

- 不追求专业剪辑能力（时间线、多轨）。
- 不打包 ffmpeg（体积代价过大），编码能力全部基于 Windows 自带 Media Foundation。
- 不做 DRM/受保护内容的绕过。

---

## 3. 总体架构

### 3.1 分层

```
┌──────────────────────────────────────────────────────────────┐
│ 表现层   ui/                                                  │
│   Vue 3 + Vite + TS；托盘菜单、设置页、截图编辑页、录制控制台   │
└───────────────────────────┬──────────────────────────────────┘
                            │ Tauri IPC（invoke / emit）
┌───────────────────────────▼──────────────────────────────────┐
│ 控制层   src-tauri/src/ipc.rs, hotkey.rs, config.rs           │
│   命令分发、会话状态机、配置持久化、错误映射                    │
└───────────────────────────┬──────────────────────────────────┘
┌───────────────────────────▼──────────────────────────────────┐
│ 领域层                                                        │
│  overlay/    选区窗口、光标、点击波纹、标注渲染                 │
│  capture/    WGC / DXGI / GDI 采集器 + 帧队列                  │
│  audio/      WASAPI 回环 / 麦克风 / 混音 / AAC                 │
│  encode/     帧管线、H.264 编码、MP4 封装、时间戳同步           │
│  image/      PNG/JPEG、剪贴板、标注合成                        │
└───────────────────────────┬──────────────────────────────────┘
┌───────────────────────────▼──────────────────────────────────┐
│ 平台层   windows-rs → Win32 / WinRT                           │
│  D3D11 · DXGI · WGC · WASAPI · Media Foundation · Direct2D    │
└──────────────────────────────────────────────────────────────┘
```

### 3.2 代码结构

```
snapcap/
├─ ui/
│  ├─ src/
│  │  ├─ views/         # Overlay.vue / Editor.vue / Recorder.vue / Settings.vue
│  │  ├─ components/    # Toolbar.vue / RegionHint.vue / StatsBadge.vue
│  │  ├─ composables/   # useCapture.ts / useSettings.ts / useHotkey.ts
│  │  ├─ ipc.ts         # Tauri invoke/emit 类型化封装
│  │  └─ types.ts       # 与 Rust 共享的 DTO（手动镜像）
│  └─ package.json
├─ src-tauri/
│  ├─ Cargo.toml
│  ├─ tauri.conf.json
│  ├─ capabilities/default.json
│  └─ src/
│     ├─ main.rs
│     ├─ error.rs
│     ├─ ipc.rs
│     ├─ config.rs
│     ├─ hotkey.rs
│     ├─ capture/
│     │  ├─ mod.rs      # CaptureSource trait + select_source()
│     │  ├─ frame.rs    # VideoFrame / FrameRing
│     │  ├─ wgc.rs
│     │  ├─ dxgi.rs
│     │  └─ gdi.rs
│     ├─ audio/
│     │  ├─ mod.rs      # AudioFrame / Mixer
│     │  ├─ loopback.rs
│     │  ├─ mic.rs
│     │  └─ aac.rs
│     ├─ encode/
│     │  ├─ mod.rs      # Encoder trait
│     │  ├─ mf_h264.rs
│     │  ├─ mp4.rs
│     │  └─ sync.rs
│     ├─ overlay/
│     │  ├─ region.rs
│     │  ├─ cursor.rs
│     │  └─ annotate.rs
│     └─ image/
│        ├─ encode.rs
│        ├─ clipboard.rs
│        └─ ops.rs
├─ docs/
│  ├─ DESIGN.md          # 本文档
│  └─ API.md             # 后续补充：IPC 明细
└─ tests/
   ├─ fixture/           # 合成测试帧生成器
   └─ acceptance/        # 验收脚本（开发期使用 ffprobe，不随包分发）
```

---

## 4. 依赖清单（建议版本）

```toml
# src-tauri/Cargo.toml 关键依赖
[dependencies]
tauri = { version = "2", features = ["tray-icon", "image-png"] }
tauri-plugin-autostart = "2"
tauri-plugin-global-shortcut = "2"
tauri-plugin-dialog = "2"
tauri-plugin-updater = "2"
tauri-plugin-log = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["sync", "time", "rt-multi-thread"] }
crossbeam-channel = "0.5"
parking_lot = "0.12"
thiserror = "2"
anyhow = "1"
tracing = "0.1"
uuid = { version = "1", features = ["v4"] }
png = "0.17"
jpeg-encoder = "0.6"
arboard = "3"          # 剪贴板图片
dirs = "5"

[target.'cfg(windows)'.dependencies]
windows = { version = "0.6x", features = [
  "Win32_Foundation",
  "Win32_System_Com",
  "Win32_System_LibraryLoader",
  "Win32_System_Threading",
  "Win32_System_Time",
  "Win32_System_WindowsProgramming",
  "Win32_UI_WindowsAndMessaging",
  "Win32_UI_Shell",
  "Win32_Graphics_Gdi",
  "Win32_Graphics_Dxgi",
  "Win32_Graphics_Dxgi_Common",
  "Win32_Graphics_Direct3D11",
  "Win32_Graphics_Direct2D",
  "Win32_Graphics_Imaging",
  "Win32_Media_Audio",
  "Win32_Media_MediaFoundation",
  "Win32_Storage_FileSystem",
  "Graphics_Capture",
  "Graphics_DirectX",
  "Graphics_DirectX_Direct3D11",
] }
```

> 说明：`windows` crate 的 feature 名和 WinRT 投影 API 名称随版本有差异；实现时以实际锁定版本的生成文档为准，本文档只约定**能力边界**。

---

## 5. 核心数据模型

```rust
/// 物理像素坐标，相对虚拟桌面左上角（可跨多屏，允许负值）
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum CaptureTarget {
    Region(Rect),
    Window { hwnd: isize, include_border: bool },
    Display { monitor_handle: isize },
    FullDesktop,
}

#[derive(Clone, Debug)]
pub struct VideoFrame {
    pub texture: ID3D11Texture2D,   // GPU 纹理，零拷贝传递
    pub timestamp_qpc: i64,        // QueryPerformanceCounter 单调时钟
    pub content_rect: Rect,        // 实际内容区域（窗口可能有不可见边框）
    pub rotation: DxgiRotation,
    pub cursor: Option<CursorOverlay>,
}

#[derive(Clone, Debug)]
pub struct CursorOverlay {
    pub x: i32,
    pub y: i32,
    pub shape_handle: isize,       // HCURSOR / 位图键
    pub click_ring: Option<ClickRing>,
}

#[derive(Clone, Copy, Debug)]
pub struct ClickRing {
    pub button: MouseButton,
    pub age_ms: u32,
    pub radius_px: u32,
}

/// 音频帧：48kHz / stereo / f32 交错
#[derive(Clone, Debug)]
pub struct AudioFrame {
    pub samples: Vec<f32>,
    pub timestamp_qpc: i64,
    pub source: AudioSource,       // System | Mic
}

pub struct SessionId(pub uuid::Uuid);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Starting,
    Recording,
    Paused,
    Stopping,
    Done,
    Failed,
}
```

---

## 6. 采集模块（capture）

### 6.1 `CaptureSource` trait

```rust
pub trait CaptureSource: Send {
    /// 能力探测：是否支持目标类型、是否需要用户授权、最大帧率
    fn capabilities(&self) -> CaptureCapabilities;

    /// 启动采集，帧通过 channel 推送；返回会话句柄
    fn start(&mut self, target: &CaptureTarget, tx: Sender<VideoFrame>)
        -> Result<CaptureHandle>;

    /// 暂停/恢复（仅停止向通道推帧，保留会话与时间戳基准）
    fn set_paused(&mut self, paused: bool) -> Result<()>;

    fn stop(&mut self) -> Result<()>;
}
```

### 6.2 三种实现与选择策略

| 实现 | 适用 | Windows 版本要求 | 备注 |
|---|---|---|---|
| `WgcSource` | 窗口、显示器 | 窗口 ≥ 1809；显示器 ≥ 19041 | 首选；GPU 纹理；窗口被遮挡仍可捕获内容 |
| `DxgiSource` | 全屏/显示器 | ≥ Windows 8 | WGC 不可用或授权被拒时回退；拿整屏后按选区裁剪 |
| `GdiSource` | 静态截图兜底 | 全版本 | 远程桌面/虚拟机/驱动异常时启用；只用于截图，不用于录制 |

选择逻辑（伪代码）：

```text
fn select_source(target, build, consent):
    if target is Window && build >= 17763 && consent == granted:
        return WgcSource
    if target is Display/FullDesktop && build >= 19041 && consent == granted:
        return WgcSource
    if target is Region && (RDP detected || wgc_failed):
        return GdiSource          // 仅截图
    if target is Display/FullDesktop:
        return DxgiSource
    return GdiSource              // 最终兜底
```

### 6.3 WGC 实现要点

1. 初始化 D3D11 设备和 `IDXGIDevice`，通过 `CreateDirect3D11DeviceFromDXGIDevice` 得到 WinRT `IDirect3DDevice`。
2. 通过 `IGraphicsCaptureItemInterop` 从 `HWND` / `HMONITOR` 创建 `GraphicsCaptureItem`。
3. 创建 `Direct3D11CaptureFramePool::CreateFreeThreaded(device, format, capacity=4, size)`，注册 `FrameArrived` 事件。
4. `TryGetNextFrame()` 拿到 `IDirect3D11Texture2D`；从 `ContentSize` 判断窗口尺寸变化，必要时 `Recreate` frame pool。
5. 创建 `GraphicsCaptureSession`，按需设置 `IsCursorCaptureEnabled = false`（光标由本程序自己绘制，保证一致性和点击效果）。
6. 首用弹出系统授权提示；用户拒绝 → 发 `E_CAPTURE_CONSENT_DENIED` 事件并自动切到 DXGI（全屏）或 GDI（区域）。

### 6.4 DXGI 桌面复制要点

- `IDXGIOutput::DuplicateOutput` 获取 `IDXGIOutputDuplication`。
- 循环 `AcquireNextFrame(timeout=33ms)`，处理 `DXGI_ERROR_ACCESS_LOST`（重新创建）和 `DXGI_ERROR_WAIT_TIMEOUT`（正常）。
- `GetFrameBounds` 得到脏矩形；`GetMouse` 获取光标位置与形状（可选，MVP 先用自己的绘制）。
- `ReleaseFrame` 必须严格配对；异常路径用 RAII guard 保证。
- 多显示器：每个 monitor 一个 duplication 实例，分别采集后按虚拟桌面坐标合成/裁剪。

### 6.5 帧队列与背压

```rust
pub struct FrameRing {
    tx: Sender<VideoFrame>,
    rx: Receiver<VideoFrame>,
    capacity: usize,     // 默认 60（@60fps 约 1 秒）
    dropped: AtomicU64,
}
```

- 满了丢弃**最旧**帧并累加 `dropped`，绝不阻塞采集线程。
- 每 500ms 把 `dropped` 通过 `capture:stats` 事件上报 UI。
- 暂停时清空队列，避免恢复后吐出旧帧。

---

## 7. 音频模块（audio）

### 7.1 采集

- **系统声音**：`IMMDeviceEnumerator::GetDefaultAudioEndpoint(eRender, eConsole)`，激活 `IAudioClient`，`Initialize(..., AUDCLNT_STREAMFLAGS_LOOPBACK, ...)`，用 `IAudioCaptureClient` 拉取。
- **麦克风**：`eCapture` 端点，同样走 `IAudioClient`。
- 两路各自独占一个线程，回调里只做「读取 → 端点缓冲 → 发事件」，不做重活。

### 7.2 格式统一与混音

- 统一目标：**48kHz / 16-bit 等效 / stereo / f32 交错**。
- 重采样：源采样率 ≠ 48kHz 时做线性插值（质量足够且零依赖）；如后续要更高品质，可替换为 `rubberband`（体积增加约 1MB，作为 feature gate）。
- 混音：

```text
out[i] = clamp( sys[i] * sys_gain * sys_mute + mic[i] * mic_gain * mic_mute )
```

- 用 `tanh` 软限幅避免爆音；静音标志由 UI 实时控制（通过 `Arc<AtomicBool>` 传入混音线程，无需重建设备）。
- 混音线程用 20ms 一帧（960 samples）推送到 AAC 编码器。

### 7.3 AAC 编码

- Media Foundation AAC MFT：`48kHz / 2ch / 192kbps`；可用硬件 MFT 则优先。
- 输出 `IMFSample` 交给了 MP4 封装器，时间戳与视频统一用 QPC 基准。

### 7.4 失败降级

| 情况 | 行为 |
|---|---|
| 无默认渲染设备 | 录制继续，视频无声，发 `E_AUDIO_DEVICE_MISSING` 警告 |
| 回环初始化失败 | 提示检查「立体声混音/共享模式」，允许仅录麦克风 |
| 麦克风被占用 | 仅录系统声，提示可稍后重试 |
| 音频缓冲区溢出 | 重置该路环形缓冲，记录 `audio_dropped` 计数 |

---

## 8. 编码与封装（encode）

### 8.1 编码器选择

启动时能力探测，按优先级：

1. NVIDIA NVENC（MFT）
2. Intel Quick Sync（MFT）
3. AMD AMF（MFT）
4. Windows 自带软件 H.264 MFT

探测方式：枚举 `MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SYNCMFT`，过滤 `MFVideoFormat_H264` + NV12 输入，读取厂商标识；同时检查 `MF_SA_D3D11_AWARE` 判断能否走 GPU 直通。

> 若 SinkWriter 无法强制指定硬件 MFT，则采用「自定义 `IMFTransform` 编码 + SinkWriter 仅封装」的两段式；通过 `MF_SINK_WRITER_ENCODER_CONFIG` 传入偏好作为首选尝试。

### 8.2 编码参数

| 项 | 默认 | 说明 |
|---|---|---|
| 分辨率 | 跟随目标，4K 时锁 30fps | 避免 4K60 过载 |
| 帧率 | 60 / 30 可选 | 采集与编码必须一致 |
| 质量 | CQP 20 | 或 VBR：1080p60 = 12Mbps，4K30 = 25Mbps |
| 关键帧 | 2 秒 | 便于拖动定位 |
| 像素格式 | NV12 / yuv420p | 兼容性最好 |
| 色彩矩阵 | BT.709 | 显式写入，避免播放器色偏 |
| Profile | High | 软编码回退时可降 Main |

### 8.3 GPU 与 CPU 两条路径

- **Phase 1（MVP，CPU 路径）**：D3D11 纹理映射到 staging texture → `Map` → SIMD BGRA→NV12 → `IMFSample`。1080p60 约 500MB/s 内存带宽，实测通常可接受。
- **Phase 2（零拷贝 GPU 路径）**：通过 `IMFDXGIBuffer` 把 D3D11 纹理直接包成 `IMFSample`，配合 `MFCreateDXGIDeviceManager` 与编码器共享设备；光标叠加也用 D3D11 渲染到同一纹理，全程不落 CPU。

### 8.4 MP4 封装

- 使用 `MFCreateSinkWriterFromURL` 输出 `.tmp` 文件。
- 视频媒体类型设置：`MF_MT_SUBTYPE(H264)`、`MF_MT_FRAME_SIZE`、`MF_MT_FRAME_RATE`、`MF_MT_AVG_BITRATE`、`MF_MT_PIXEL_ASPECT_RATIO(1:1)`、`MF_MT_YUV_MATRIX(BT709)`。
- 音频媒体类型：`MFAudioFormat_AAC`、`48000Hz`、`2ch`、`192kbps`。
- 每个 sample 用 `SetSampleTime` / `SetSampleDuration`（单位 100ns）。`Finalize()` 后原子重命名 `.tmp → .mp4`。

### 8.5 时间戳与同步

- 统一时间基：`QueryPerformanceCounter`。启动时记录 `qpc_origin`。
- 视频帧时间戳 = `frame_qpc - qpc_origin`；音频同理，换算为 100ns 写进 sample。
- 暂停：记录暂停期间经过的 QPC 时长，恢复时把 `qpc_origin += paused_duration`，保证时间轴连续。
- 漂移控制：以音频时钟为主，视频帧偏差 > 40ms 时丢帧或重复上一帧；每 5 秒统计一次漂移写入日志。

### 8.6 崩溃恢复

- 录制中每 30 秒对 MP4 做一次轻量备份（复制当前 `.tmp` 的已写部分到 `.bak`），或采用 fragmented MP4。
- 启动时扫描输出目录中的 `.tmp/.bak`，检测到未完成会话则提示用户「尝试恢复 / 删除」。

---

## 9. 截图与标注（image / overlay）

### 9.1 选区覆盖层

- 一个覆盖虚拟桌面的透明置顶窗口（WS_EX_LAYERED / WS_EX_TRANSPARENT 按状态切换）。
- **MVP**：Tauri 透明窗口 + HTML Canvas 绘制遮罩和选区（开发快）。
- **备选**：若 WebView2 合成延迟明显，替换为 Rust + Direct2D 原生覆盖层（接口不变，仅换实现）。
- 交互：拖拽画框；悬停窗口时调用 `WindowFromPoint` + `GetWindowRect` 吸附窗口边界；Esc 取消；Enter 确认。

### 9.2 截图编码

- 从帧队列取最新一帧，按选区裁剪（物理像素，不做二次缩放）。
- 默认 PNG（无损）；可选 JPEG（质量 92，可调）。
- 输出到：文件、剪贴板（`arboard::SetImage`）、编辑面板。

### 9.3 标注操作

```rust
pub enum AnnotationOp {
    Rect   { x: i32, y: i32, w: u32, h: u32, color: u32, width: u32 },
    Arrow  { from: (i32, i32), to: (i32, i32), color: u32, width: u32 },
    Text   { x: i32, y: i32, text: String, color: u32, size: u32 },
    Blur   { x: i32, y: i32, w: u32, h: u32, radius: u32 },
    Step   { x: i32, y: i32, number: u32 },
}
```

- 编辑在 UI 侧 Canvas 完成，导出时把 ops 回传 Rust 合成到最终位图（保证粘贴板与落盘一致）。
- 马赛克用 box blur 实现，避免使用高开销的高斯卷积。

### 9.4 光标与点击高亮（录制）

- WGC 设置 `IsCursorCaptureEnabled = false`，由本程序在 D3D11 合成阶段画光标（用 `GetCursorInfo` 取 HCURSOR 并缓存位图）。
- 鼠标左/右按下时生成 `ClickRing`，持续 300ms 的扩散圆；每帧按 `age_ms` 更新半径与透明度。
- 效果直接画进视频帧，因此导出的 MP4 中可见；截图默认不含光标（可配置）。

---

## 10. 线程模型

| 线程 | 职责 | 关键约束 |
|---|---|---|
| 主线程 | Tauri/WebView2 事件循环、窗口管理 | 不做重活 |
| 采集线程 | WGC FrameArrived / DXGI AcquireNextFrame | 只入队，不编码 |
| 合成线程 | 裁剪、光标/点击叠加、格式转换 | D3D11 设备上下文专用 |
| 编码线程 | MF 编码 + MP4 写入 | 独占，串行写 sample |
| 音频回环线程 | WASAPI 渲染端点回环采集 | 实时优先级优先 |
| 麦克风线程 | WASAPI 采集端点 | 同上 |
| 混音线程 | 重采样、增益、限幅、AAC 送入 | 20ms 帧 |
| 控制线程（tokio） | IPC 命令、状态机、定时器、事件上报 | 异步 |

**规则**

- 跨线程只传 `VideoFrame` / `AudioFrame` 所有权，不共享可变纹理。
- 唯一共享状态：`config: Arc<RwLock<Settings>>`、`session: Arc<RwLock<SessionState>>`、静音 `AtomicBool`。
- 任何线程不得持有锁跨越 `await` 或阻塞式 MF 调用。
- 退出顺序：停止采集 → 排空队列（带超时）→ 停止音频 → Finalize MP4 → 关窗口。

---

## 11. IPC 协议

### 11.1 Commands（UI → Rust）

| 命令 | 参数 | 返回 |
|---|---|---|
| `list_displays` | — | `Vec<DisplayInfo>` |
| `list_windows` | — | `Vec<WindowInfo>` |
| `open_region_overlay` | `mode: "screenshot" \| "record" \| "window"` | `SelectionResult`（`window` 返回选中的 `hwnd`） |
| `screenshot_capture` | `target, options` | `ScreenshotResult { id, width, height, bytes? }` |
| `screenshot_output` | `id, destination: "file" \| "clipboard", path?` | `()` |
| `annotation_apply` | `id, ops: Vec<AnnotationOp>` | `new_id` |
| `record_start` | `target, options` | `SessionId` |
| `record_pause` / `record_resume` | `session` | `()` |
| `record_stop` | `session` | `RecordingResult { path, duration_ms, size }` |
| `get_settings` / `set_settings` | — / `Settings` | `Settings` |
| `open_output_dir` | — | `()` |

### 11.2 Events（Rust → UI）

| 事件 | 载荷 |
|---|---|
| `capture:state` | `{ session, state }` |
| `capture:stats` | `{ fps, dropped_frames, bitrate_kbps, duration_ms, audio_dropped }` |
| `capture:error` | `{ code, message, remediation, retryable }` |
| `overlay:selection` | `{ rect, hwnd?, cancelled }` |
| `overlay:confirm` / `overlay:escape` | 全局 Enter / Esc 兜底（无聚焦覆盖层也可确认/取消） |
| `screenshot:saved` | 自动保存后的绝对路径（编辑器显示「打开文件位置」） |
| `hotkey:triggered` | `{ action }` |
| `settings:changed` | `Settings` |

### 11.3 错误码

`E_CAPTURE_CONSENT_DENIED`、`E_CAPTURE_UNSUPPORTED`、`E_TARGET_GONE`、`E_ENCODER_NOT_AVAILABLE`、`E_AUDIO_DEVICE_MISSING`、`E_DISK_FULL`、`E_TIMEOUT`、`E_INTERNAL`。

每个错误必须携带 `remediation`（用户可执行的提示）和 `retryable`。

---

## 12. 配置

存储位置：`%APPDATA%\SnapCap\settings.json`，写入采用「写临时文件 → 原子替换」。便携部署与验收测试可用环境变量 `SNAPCAP_CONFIG_DIR` 覆盖配置目录。

```json
{
  "general": { "autostart": true, "minimize_to_tray": true, "language": "zh-CN" },
  "capture": { "fps": 60, "cursor": true, "click_highlight": true, "multi_monitor": "virtual" },
  "video": {
    "codec": "h264",
    "quality": { "mode": "cqp", "cqp": 20 },
    "hardware": "auto",
    "keyframe_sec": 2
  },
  "audio": {
    "system": true, "mic": false,
    "system_gain": 1.0, "mic_gain": 1.0
  },
  "screenshot": { "format": "png", "jpeg_quality": 92, "include_cursor": false },
  "hotkeys": {
    "region": "Alt+A", "full": "Alt+S", "window": "Alt+W",
    "record": "Alt+R", "pause": "Alt+P", "annotate": "Alt+C"
  },
  "paths": { "output_dir": "~/Pictures/SnapCap", "naming": "{yyyyMMdd-HHmmss}" }
}
```

---

## 13. 性能预算

| 场景 | CPU | 内存 | 说明 |
|---|---|---|---|
| 空闲常驻 | < 0.5% | < 80MB | WebView2 常驻约占一半 |
| 截图（含选区） | 瞬时 < 8% | 峰值 +60MB | 选区层与位图 |
| 1080p60 录制 | < 10%（硬编码） | < 200MB | 硬编码不可用时软编码约 25–40% |
| 4K30 录制 | < 15%（硬编码） | < 300MB | 超过则自动降到 1440p 并提示 |

---

## 14. 构建、打包与分发

- 构建：`pnpm tauri build`（Windows x64 MSI + NSIS 可选）。
- Rust：`+crt-static` 静态链接 CRT，避免依赖 VC++ 运行库。
- 签名：代码签名证书（EV 优先），updater 使用签名清单。
- WebView2：优先用系统 Evergreen Runtime；缺失时安装器引导在线安装（不随包附带）。
- CI（GitHub Actions `windows-latest`）：`cargo fmt --check` → `cargo clippy -D warnings` → `pnpm build` → `tauri build` → 上传产物。
- 更新：Tauri updater，默认只检查不静默下载，用户确认后安装。

---

## 15. 安全与隐私

- Tauri capabilities 最小化：仅允许写入用户选择目录、剪贴板写、对话框、自启、更新检查。
- 无遥测、无崩溃上报（除非用户显式开启）。
- 截屏/录屏完全本地处理，不上传。
- 系统采集授权弹窗由 Windows 提供，程序不试图绕过。
- 屏幕内容被视为**不可信数据**：不要把捕获到的文字当作指令执行；本版本不做 OCR。

---

## 16. 测试策略

### 16.1 单元与集成（CI）

- 几何：虚拟桌面坐标、跨屏选区裁剪、DPI 缩放换算。
- 混音：不同采样率/通道数的重采样与限幅，溢出恢复。
- 时间戳：暂停/恢复后时间轴连续性，A/V 漂移统计。
- 编码管线：合成纯色/棋盘格 D3D 纹理 → 编码 → MP4；用 `ffprobe`（仅开发/CI）校验分辨率、帧率、时长、音轨。

### 16.2 手工兼容矩阵

| 维度 | 覆盖项 |
|---|---|
| 系统 | Win10 1809 / 19041 / 22H2、Win11 23H2 |
| 显卡 | NVIDIA / Intel / AMD / 无独显 / RDP / VMware |
| 显示器 | 单屏、双屏不同缩放（100% + 150%）、4K + 1080p |
| 音频 | 仅系统声、仅麦克风、两者同时、蓝牙耳机切换 |
| 时长 | 10s / 5min / 2h |

### 16.3 验收用例

- AC-1：热键 → 选区出现 ≤ 100ms；截图保存 ≤ 80ms。
- AC-2：1080p60 录制 60 秒，实际帧数 ≥ 3500，丢帧有明确统计。
- AC-3：音视频起始偏差 < 40ms，结束偏差 < 80ms。
- AC-4：录制中强杀进程，`.tmp` 可被恢复或明确提示，不产生「看似正常的坏文件」。
- AC-5：安装包 ≤ 15MB，冷启动到托盘 ≤ 1.5s。

---

## 17. 里程碑

| 阶段 | 交付 | 验收点 |
|---|---|---|
| M0 骨架 | Tauri 工程、托盘、热键、设置 | 热键可触发事件，设置可持久化 |
| M1 截图 | 选区层、WGC/GDI 抓帧、PNG/JPEG、剪贴板 | AC-1 |
| M2 标注 | 框/箭头/文字/马赛克/序号 | 编辑结果与落盘一致 |
| M3 录屏 | WGC 帧 + 音频回环 + MF 编码 + MP4 | AC-2 / AC-3 |
| M4 增强 | 光标/点击高亮、窗口跟随、多屏、暂停续录 | 兼容矩阵通过 |
| M5 发布 | 硬编码适配、崩溃恢复、安装包、updater | AC-4 / AC-5 |

---

## 18. 风险与对策

| 风险 | 等级 | 对策 |
|---|---|---|
| Win11 屏幕采集授权被拒 | 高 | 明确引导；全屏回退 DXGI，区域回退 GDI |
| 硬件编码器驱动差异 | 高 | 启动时能力枚举 + 逐级回退软编码；记录所用编码器 |
| WGC 窗口尺寸变化导致池失效 | 中 | 监听 `ContentSize`，`Recreate` frame pool |
| 4K60 过载 | 中 | 自动降帧到 30；环形队列丢弃最旧帧并统计 |
| WebView2 缺失/合成延迟 | 中 | 安装器引导安装；必要时覆盖层切 Direct2D 原生实现 |
| DRM 黑帧 | 中 | 检测并提示；明确不绕过 |
| 音频独占模式漏采 | 低 | 提示切共享模式；允许只录视频 |
| MP4 崩溃损坏 | 中 | 写 `.tmp` + 周期备份 + 启动时恢复提示 |

---

## 19. 待确认问题

1. 是否需要「摄像头画中画」？（会引入摄像头采集与合成，约 +1 周）
2. 输出格式是否只要 MP4/H.264？后续是否要 WebM/VP9 或 HEVC/HDR？
3. 命名与品牌（当前占位名 SnapCap）。
4. 是否需要命令行接口或本地 HTTP 控制（供自动化调用）？
5. 是否需要多语言（当前默认 zh-CN，结构已预留）。

---

## 20. 附录：术语

- **WGC**：Windows.Graphics.Capture，Win10 1809+ 的屏幕采集 API，输出 D3D11 纹理。
- **DXGI Desktop Duplication**：DXGI 桌面复制，全屏采集的通用回退方案。
- **WASAPI Loopback**：Windows 音频会话 API 的回环采集，用于抓系统播放声音。
- **MFT**：Media Foundation Transform，Windows 的媒体编码/解码组件（硬件或软件）。
- **QPC**：QueryPerformanceCounter，Windows 高精度单调时钟。
- **CQP**：Constant Quantization Parameter，恒定质量编码模式。
