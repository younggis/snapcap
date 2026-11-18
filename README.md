# SnapCap

> Windows 平台轻便截图 + 录屏工具 · Tauri 2（Rust）+ Vue 3 + TypeScript

## 一句话定位

开机常驻托盘的轻量工具，用全局热键完成「截图 → 标注 → 保存/复制」和「屏幕/窗口/区域录制 → 高清 MP4」。

## 功能特性

- **截图**：全屏 / 窗口 / 自定义区域抓取，透明选区覆盖层，窗口边界吸附；PNG（无损）/ JPEG（质量可调）输出到文件或剪贴板。
- **标注**：矩形、箭头、文字、马赛克（box blur）、序号步骤；编辑结果与落盘 / 剪贴板一致。
- **录屏**：屏幕 / 窗口 / 区域录制，输出 H.264 MP4；可选光标与点击高亮、窗口跟随、多屏、暂停续录。
- **音频**：WASAPI 系统回环 + 麦克风，重采样 / 混音 / 限幅，AAC 192kbps；设备缺失自动降级。
- **全局热键**：区域 / 全屏 / 窗口截图、录制 / 暂停 / 标注，默认 `Alt+A/S/W/R/P/C`，可自定义。
- **托盘常驻**：托盘菜单快速触发，开机自启，最小化到托盘。
- **设置**：集中配置截图、视频、音频、热键与路径（持久化到 `%APPDATA%\SnapCap\settings.json`）。
- **隐私优先**：默认零网络请求、零遥测；采集完全本地处理。

## 技术栈

| 层 | 技术 |
|---|---|
| 表现层 | Vue 3 + Vite + TypeScript + Tauri 2 API |
| 控制层 | Tauri IPC（`invoke` / `emit`）、会话状态机、配置持久化 |
| 领域层 | 采集（WGC / DXGI / GDI）、音频（WASAPI / AAC）、编码（Media Foundation H.264 / MP4）、标注合成 |
| 平台层 | `windows-rs` → Win32 / WinRT：D3D11 · DXGI · WGC · WASAPI · Media Foundation · Direct2D |

> 编码能力全部基于 Windows 自带 Media Foundation，**不打包 ffmpeg**。

## 项目结构

```
snapcap/
├─ ui/                         # Vue 3 + Vite + TS 前端
│  ├─ src/
│  │  ├─ views/                # Overlay.vue / Editor.vue / Recorder.vue / Settings.vue
│  │  ├─ components/           # Toolbar.vue / RegionHint.vue / StatsBadge.vue
│  │  ├─ composables/          # useCapture.ts / useSettings.ts / useHotkey.ts
│  │  ├─ ipc.ts                # 类型化 Tauri invoke/emit 封装
│  │  └─ types.ts              # 与 Rust 共享的 DTO（手动镜像）
│  └─ package.json
├─ src-tauri/                  # Rust 后端
│  ├─ Cargo.toml
│  ├─ tauri.conf.json
│  ├─ capabilities/default.json
│  └─ src/
│     ├─ main.rs / error.rs / ipc.rs / config.rs / hotkey.rs
│     ├─ capture/              # CaptureSource trait + wgc / dxgi / gdi / frame
│     ├─ audio/                # loopback / mic / aac / mixer
│     ├─ encode/               # mf_h264 / mp4 / sync
│     ├─ overlay/              # region / cursor / annotate
│     └─ image/                # encode / clipboard / ops
├─ docs/
│  ├─ DESIGN.md                # 技术设计文档
│  └─ API.md                   # IPC 协议（UI ↔ Rust 契约）
└─ tests/
   ├─ fixture/                 # 合成测试帧生成器
   └─ acceptance/              # 验收脚本（开发/CI 用 ffprobe，不随包分发）
```

## 前置条件

- **Windows 10 1809（build 17763）+**（WGC 窗口采集要求；显示器采集建议 19041+）。
- **Node.js 20+**
- **pnpm 9+**
- **Rust 1.77+**，使用 **MSVC 工具链**（`x86_64-pc-windows-msvc`，安装 Visual Studio Build Tools 的 "Desktop development with C++"）。
- **WebView2 Runtime**（Evergreen，系统自带或由安装器引导安装）；Tauri 开发模式需要 WebView2。

## 开发

```powershell
pnpm install
pnpm tauri dev
```

## 构建

```powershell
pnpm tauri build
```

产物位于 `src-tauri/target/release/bundle/`（Windows x64 MSI / NSIS）。

## MVP 功能清单（M0 – M5）

| 阶段 | 交付 | 验收点 |
|---|---|---|
| **M0 骨架** | Tauri 工程、托盘、热键、设置 | 热键可触发事件，设置可持久化 |
| **M1 截图** | 选区层、WGC/GDI 抓帧、PNG/JPEG、剪贴板 | AC-1（热键选区 ≤ 100ms，保存 ≤ 80ms） |
| **M2 标注** | 框 / 箭头 / 文字 / 马赛克 / 序号 | 编辑结果与落盘一致 |
| **M3 录屏** | WGC 帧 + 音频回环 + MF 编码 + MP4 | AC-2 / AC-3（帧率、A/V 同步） |
| **M4 增强** | 光标 / 点击高亮、窗口跟随、多屏、暂停续录 | 兼容矩阵通过 |
| **M5 发布** | 硬编码适配、崩溃恢复、安装包、updater | AC-4 / AC-5（崩溃恢复、≤ 15MB / ≤ 1.5s 启动） |

## 隐私与安全

- 默认**零网络请求、零遥测、零崩溃上报**（除非用户显式开启）。
- 截屏 / 录屏内容全部本地处理，**不上传**；采集需用户主动触发。
- 系统采集授权弹窗由 Windows 提供，程序不试图绕过。
- Tauri capabilities 最小化：仅允许写入用户选择目录、剪贴板写、对话框、自启、更新检查。
- 屏幕内容被视为**不可信数据**（本版本不做 OCR，不把捕获文字当指令执行）。

## 关于原生平台代码

> **重要**：截图采集、录屏编码与音频回环为 **Windows 原生代码**，基于 Win32 / WinRT（WGC、DXGI、WASAPI、Media Foundation），通过 `windows-rs` 调用。该部分代码仅面向 Windows，且**必须使用 Rust/MSVC 工具链编译**；在 Linux/macOS 上无法构建原生采集与编码模块。安装/运行需要 Windows 10 1809+ 与 WebView2 Runtime。

## 界面设计

全系统采用 Windows 11 / Fluent 2 原生设计语言（单一品牌蓝、表面+边框分层、无彩色发光、无多余渐变）。所有颜色、间距、圆角、阴影、动效均落地为 `styles.css` 中的设计令牌，组件一律引用令牌：

- **SF Pro 字体栈**、iOS 系统色板（系统蓝 `#007AFF` / 绿 `#34C759` / 红 `#FF3B30` 等）
- **浅色 / 深色 / 跟随系统** 三种外观，可在「关于」或导航栏一键切换；多窗口自动同步
- 毛玻璃导航栏与标签栏、分组列表（iOS Grouped List）、51×31 开关、分段控件
- 极简首页：四个图标磁贴（快捷键以高亮胶囊清晰标注）+「偏好设置 / 录制控制台」入口；文字采用小字号 + Regular/Medium 字重的克制排版（非粗体），层次细腻；启动、托盘、输出、语言等设置收拢到带「返回」的二级「偏好设置」页，初始界面无滚动条
- 弹簧微交互与页面入场动画
- 标注编辑器采用悬浮毛玻璃工具栏与 iOS 调色板
- 选区覆盖层使用 iOS 风格提示胶囊与圆角选区框

## 实现状态（重要）

本仓库已包含 M0–M5 的**完整工程骨架与全部模块源码**，但不同部分的验证程度不同：

| 部分 | 状态 |
|---|---|
| Vue 3 前端（4 个视图 / 3 个组件 / 3 个 composable） | ✅ 已在本机 `pnpm --dir ui build` 通过类型检查与打包；浏览器实测 4 个视图零控制台错误，mock 录制流程实时统计正常。 |
| Tauri 配置 / 图标 / capabilities / CI | ✅ 已生成并做静态核对（含 256×256 PNG + ICO 图标）。 |
| Rust 纯逻辑层（DTO、配置持久化、错误模型、状态机、标注合成、帧队列、混音、时间戳、PNG/JPEG 编码、剪贴板、崩溃恢复扫描） | ✅ 已在本机编译通过。 |
| Windows GDI 截图采集 | ✅ 真实实现（`GetDC` / `CreateDIBSection` / `BitBlt` → BGRA → PNG/JPEG），截图链路端到端可用。 |
| H.264 / MP4 编码（Media Foundation SinkWriter） | ✅ 真实实现并用 ffprobe 验收通过：合成 90 帧 → H.264 MP4，320×240 / 30fps / 3s / 90 帧。默认 CPU 路径（BGRA→NV12 BT.709），SinkWriter 自动选择编码器。 |
| WASAPI 系统回环 / 麦克风采集 | ✅ 真实实现（IMMDeviceEnumerator / IAudioClient / IAudioCaptureClient），含 24/16/32-bit 解码与线性重采样到 48kHz。 |
| AAC 音频封装 | ⚠️ 采集与 f32→AAC 输入准备已实现；本机 SinkWriter 拒绝 AAC 输入媒体类型，录制自动降级为**无声视频**（不中断、不产坏文件）。 |
| WGC / DXGI 采集 | ⚠️ 保留模块边界；当前录制回退到 GDI 轮询采集（真实抓屏），截图始终走 GDI。 |

> **本机已完成工具链安装与构建验证**（Rust 1.100.0 + MSVC 14.44 + Windows SDK）：`cargo check` / `cargo build` / `cargo test`（33 项通过）与 `pnpm tauri build` 均通过，产物见 `releases/`。端到端验收脚本：`powershell -ExecutionPolicy Bypass -File tests\acceptance\run_all.ps1`。也可运行 `snapcap.exe --selftest <out.mp4>` 生成测试视频并用 ffprobe 校验。执行 `pnpm tauri dev` 可进行真机调试。

详细接口契约见 [`docs/API.md`](docs/API.md)，设计与测试方案见 [`docs/DESIGN.md`](docs/DESIGN.md)。
