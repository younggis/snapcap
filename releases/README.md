# SnapCap 构建产物（v0.1.0）

本机使用 Rust 1.100.0（MSVC）+ Visual Studio Build Tools 2022 构建。

## ✨ v0.1.0 — Windows 11 / Fluent 2 原生设计

全系统界面已按 Windows 11 Fluent 2 美学重新设计（单一品牌蓝 #2F6BFF、红仅用于录制、表面+边框分层、无彩色发光/多余渐变）：

- **Segoe UI Variable Text / Microsoft YaHei UI** 字体栈、Fluent 2 系统色板；键帽用 Cascadia Mono
- **跟随系统（默认）/ 浅色 / 深色** 三种外观，已移入「偏好设置」，多窗口自动同步
- 标题栏 / 内容区 / 标签栏同源底色；入口卡片 4 列、区域为唯一实心焦点；Windows 双键帽 Alt + 字母
- 列表卡内缩分割线、统一 primary-tint 图标块；底栏选中用填充图标 + 胶囊底
- 全部颜色 / 间距 / 圆角 / 阴影 / 动效落地为 CSS 设计令牌，组件一律引用令牌
- 支持 hover / pressed / focus-visible / disabled 与 prefers-reduced-motion

窗口尺寸：主窗口 420×580、编辑器 900×680、录制台 360×660。滚动条默认隐藏，仅在鼠标移入时浮现。

第二轮主窗口修整（Fluent 2）：
- 列表行 width:100%、文字区 flex:1、chevron 绝对定位固定在右内边距（两行同 x）
- 分割线从文字左缘（64px）画到卡片右边缘（right:0）
- 内容区左右边距均 20px；删除内容区重复的「图标+SnapCap」品牌行及分割线
- 标题栏与内容区同一纯色底（配置 backgroundColor + 运行时随主题切换；系统原生标题栏区域由 Windows 绘制）
- 入口卡：48px 图标底块 / 24px 图标 / 16px 内边距 / 约 136px 高；单键帽「Alt + A」；全屏改为 Fluent 四角括号
- 底栏选中态胶囊包住「图标+标签」，标签 12px

首页已极简化：仅保留四个操作磁贴 +「偏好设置 / 录制控制台」两个入口，快捷键以蓝色高亮胶囊清晰标注；入口行文字左对齐、图标与副标题间距加大，不再拥挤。开机自启动、托盘、输出目录、语言、命名等设置收拢到二级「偏好设置」页（带返回按钮）。主窗口高度精确贴合首页内容，不再有大片空白。

## 最新修复（窗口截图全链路）

窗口截图此前存在一个关键断点：Rust 收到了 `mode=window`，但覆盖层窗口始终加载的是 `index.html?view=overlay`，前端因此一直按普通区域截图运行 —— 看不到窗口识别，Enter 后自然没有窗口图。

本次修复与增强：

- **模式进入 URL**：每次打开覆盖层都重新导航到 `view=overlay&mode=screenshot|record|window`，同时清空上一次选区状态；非法模式直接拒绝。
- **真实窗口选择**：覆盖层列出真实窗口，鼠标悬停高亮并即时显示窗口标题，单击选中；按 `window.devicePixelRatio` 在物理像素与 CSS 像素间换算，提交时还原真实窗口矩形。
- **Enter 全链路**：窗口模式未选窗口时 Enter 不再误提交；确认后立即抓取目标窗口、自动保存到输出目录，再打开标注编辑器。
- **保存可追踪**：编辑器通过 `screenshot:saved` 事件显示实际保存路径，并提供「打开文件位置」。
- **防重复提交**：重复 Enter / 重复点击不会产生第二张截图。
- **窗口图不再空白**：`GetWindowDC` + 32bpp DIB 的 `BitBlt(SRCCOPY)` 不写 alpha，窗口 DC 抓到的 alpha 常为 0，编码后 PNG 大片透明、编辑器里看似空白；现在 GDI 抓帧统一把 alpha 置为 255。
- **微信 / QQ 等 GPU 渲染窗口不再全黑**：这类窗口由 DWM 合成，`GetWindowDC` 只能抓到全黑位图；窗口截图改走屏幕 DC + 窗口屏幕矩形，抓的是用户实际看到的合成画面（抓帧前会临时恢复并前置目标窗口）。
- **透明穿透窗口不再干扰选窗**：带 `WS_EX_TRANSPARENT` 的鼠标穿透覆盖层（屏幕光标 / 标注工具窗口）不再列入可选窗口，否则它们置顶铺屏，总被 hover 命中而选不中真实目标。
- **窗口识别更准**：`zoneAt` 不再返回枚举顺序里第一个矩形包含光标的窗口 —— 最大化的浏览器 / 桌面外壳矩形也覆盖光标，会把 QQ 等小窗口挡住识别不到；现在改为在光标命中的窗口里选**面积最小（最具体）**那个。
- **文字标注 Enter 提交恢复**：Enter/Esc 全局快捷键原先常驻注册，会在系统层吞掉按键，导致标注编辑器输入框的回车无法提交文字（表现为「添加文字标注不成功 / 颜色没生效 / 回车没反应」）；现在仅在选区覆盖层打开期间注册，关闭后立即注销，键盘交还编辑器。

真实 UI 实测（WebView2 CDP + 真实键盘，非模拟）：
- GDI 窗口：受控 WinForms 窗口 538×367 → 覆盖层识别 10–17 个窗口 → 悬停/选中显示「已识别：SnapCap E2E Target」→ 真实 Enter → 编辑器画布实际绘制像素占比 38.5%、保存路径可见 → 输出 PNG 538×367，alpha=1、颜色数 64。修复前该 PNG 有 81% 像素 alpha≈0，正是「编辑器空白」的根因。
- GPU 窗口：真实 QQ 窗口 1075×924（125% DPI）→ 同样全链路 → 输出 PNG 1075×924，alpha=1、黑色占比 0.9%、颜色数 128。修复前 `GetWindowDC` 抓同一窗口是 100% 全黑。
- 窗口识别：目标点被 12 个窗口的矩形同时覆盖，`zoneAt` 正确选中面积最小的目标窗口（538×367），不再误选铺满屏幕的 maximize 窗口。
- 文字标注：选文字工具 + 蓝色，输入文字后按**真实 Enter** → 输入框关闭、蓝色像素落在画布（CDP Enter 337 → 真实 Enter 498，递增），颜色生效。修复前真实 Enter 完全无反应。

## 文件

| 文件 | 说明 | 大小 |
|---|---|---|
| `snapcap.exe` | 独立可执行文件，双击后显示「SnapCap 设置」窗口，关闭窗口隐藏到托盘。需 WebView2 Runtime。 | 6.57 MB |
| `SnapCap_0.1.0_x64-setup.exe` | NSIS 安装程序（x64）。 | 2.16 MB |

## 请务必使用此目录中的最新版 exe

旧版曾出现「全屏截图 / 窗口截图 / 开始录制点了没反应并卡死」，原因与修复：

1. **UI 被阻塞**：截图（GDI 抓屏 + PNG 编码）和录制启动是同步 Tauri 命令，跑在 async executor 上。本机双屏 5120×1440，一次全屏截图约 1.15s（debug），期间整个界面无响应。→ 已改为 `spawn_blocking` 在线程池执行。
2. **没有可见反馈**：截图结果原本只存到后端缓存，设置窗口不显示；录制状态显示在另一个窗口。→ 已加入 Toast 提示，并自动打开标注窗口 / 录制控制台。
3. **窗口截图找不到窗口**：UI 传 `hwnd=0` 哨兵值，原实现直接 `GetWindowRect(0)` 失败且无提示。→ 已改为解析 `GetForegroundWindow()`。
4. **WASAPI 初始化可能永久阻塞**：`recv()` 无超时，某些声卡会导致「开始录制」永远不返回。→ 已加 2 秒超时并降级为无声。
5. **选区窗口只有默认大小**：→ 已按虚拟桌面尺寸设置位置与大小。
6. **标注窗口重复抓屏**：隐藏窗口启动时也会加载，editor 自动抓屏会截到桌面自身。→ 改为 Rust 广播 `screenshot:ready` 事件传递同一张截图。

## 验收记录

一键验收：`powershell -ExecutionPolicy Bypass -File tests\acceptance\run_all.ps1`

2026-11-23 完整执行结果：**12 项检查全部通过**（exit 0）。

| 检查 | 结果 |
|---|---|
| `pnpm ui:build` | ✅ 通过 |
| `cargo test` | ✅ 40 项通过 |
| `--verify-all` | ✅ 12 项通过（新增覆盖层 `mode=window` URL 校验） |
| AC-1 截图链路 `--capture-test` | ✅ 5120×1440 PNG，约 1.7 MB，耗时约 1.1s（debug） |
| 真实录制链路 `--record-test` | ✅ 5120×1440，7–8 帧 / 2s，H.264 MP4 |
| 编码自测 `--selftest` | ✅ 生成真实 H.264 MP4 |
| AC-2 ffprobe | ✅ h264 / 320×240 / 30fps / 90 帧 / 3s |
| AC-5 安装包体积 | ✅ 2.16 MB ≤ 15 MB |
| AC-5 冷启动 | ✅ 进程常驻、主窗口可见、空闲内存约 35 MB |
| 窗口截图真实 UI E2E | ✅ 点击「窗口」→ 识别 10–17 个窗口 → 悬停/选中显示「已识别：SnapCap E2E Target」→ 选中 538×367 目标 → 真实 Enter → 编辑器画布像素占比 38.5% → PNG 538×367 落盘（alpha=1，颜色数 64） |
| GPU 窗口截图 E2E | ✅ 真实 QQ 窗口 1075×924 → 真实 Enter → PNG 1075×924 落盘（alpha=1、黑色占比 0.9%、颜色数 128），不再全黑 |
| 文字标注真实 UI E2E | ✅ 文字工具 + 蓝色 → 真实 Enter 提交 → 画布蓝色像素递增（337→498），输入框关闭，颜色生效 |

E2E 使用隔离配置与临时输出目录（`SNAPCAP_CONFIG_DIR`），不会向用户的 `Pictures\SnapCap` 写入测试图片。视觉证据：`../tests/acceptance/artifacts/window-overlay-proof.png`（窗口识别高亮）、`../tests/acceptance/artifacts/window-capture-proof.png`（最终 538×367 窗口图）、`../tests/acceptance/artifacts/gpu-window/window-capture-proof.png`（真实 QQ GPU 窗口图）。

## 功能状态

| 功能 | 状态 |
|---|---|
| 托盘、全局热键、设置持久化、四视图 UI、Toast 反馈 | ✅ |
| 截图（GDI）、PNG/JPEG、剪贴板、标注 | ✅ |
| 录制（GDI → NV12 → H.264 MP4） | ✅ ffprobe 验收通过 |
| WASAPI 系统声 / 麦克风采集（含超时降级） | ✅ |
| AAC 音频封装 | ⚠️ 本机 SinkWriter 拒绝 AAC 输入，自动降级为无声视频 |
| WGC / DXGI 采集 | ⚠️ 保留接口，当前回退 GDI 轮询 |

## 第四轮修复（录制滚动条 / 窗口截图 Enter / 界面变窄）

1. **录制面板滚动条**：录制台高度 520 → 660，宽度 380 → 360。实测含错误卡 + 结果卡的最坏情况下 `clientH == scrollH`，无滚动条。
2. **窗口截图 Enter 无图**：两个根因均已修：
   - 覆盖层是透明分层窗口，拿不到键盘焦点，DOM `keydown` 收不到 Enter。Rust 侧新增全局 `Enter` 快捷键兜底（发 `overlay:confirm` 事件，仅选区挂起时生效），前端 Overlay 监听该事件执行确认。
   - 编辑器在**隐藏状态**收到 `screenshot:ready` 时画布尺寸为 0，图片绘进 0×0 画布不可见；显示后无 resize 事件重绘。新增 `ResizeObserver` 监听画布容器，窗口显示时自动重布局重绘。
3. **界面太宽**：主窗 500 → 420、编辑器 980 → 900、录制台 380 → 360，更紧凑。

## 第三轮修复（窗口截图 / 输出目录 / 双界面）

1. **窗口截图选区后无图**：根因是选区覆盖层置顶铺满屏幕，被 `list_windows()` 列进可选窗口，`zoneAt()` 总是先命中它 → 抓到透明的自身。`list_windows_impl` 现用 `GetWindowThreadProcessId` 按 PID 过滤掉本程序所有窗口；验收测试 `window-list-excludes-self` 用 PID 断言列表中无自身窗口。
2. **“在文件资源管理器中打开”目录对不上**：根因是 `~/Pictures/SnapCap` 展开后混用 `/` 与 `\`（`C:\Users\young\Pictures/SnapCap`）。`expand_output_dir` 现统一替换为平台原生分隔符并逐组件 join；`screenshot_output` 现返回真实保存路径，标注编辑器显示「打开文件位置」按钮直接 `explorer /select,` 到该文件；验收测试 `output-dir-consistency` 验证保存与打开解析一致。
3. **录制时两个界面干扰**：录制开始后自动隐藏主界面、只留录制控制台（单界面）；点按主界面「录制中」卡片可直接切到控制台（暂停/停止）；录制结束后自动恢复主界面。
4. **BitBlt 句柄失效重试**：`capture_dc` 在 BitBlt 返回 `ERROR_INVALID_HANDLE` 时重新获取屏幕 DC 重试一次。

## 验收测试说明

`--verify-all` 共 11 项。几何/热键/配置/窗口过滤/标注/输出目录/编码 7 项为纯逻辑测试，**全部 PASS**。截图/录制 4 项依赖 GDI BitBlt 到内存位图——在本会话末期系统级 GDI 状态损坏（连 .NET `CopyFromScreen`、原生 `BitBlt(src=屏幕DC, dst=兼容位图)` 都返回“句柄无效”，前台窗口为 0），与 SnapCap 代码无关；这些测试在本会话早期同一代码路径下曾全部 PASS。请在正常桌面会话中重新运行 `run_all.ps1` 完成这 4 项的最终确认。

## 四项体验问题的修复

1. **录制完成无法打开文件夹**：新增 `reveal_file` 命令（`explorer /select,<路径>`），结果卡增加「打开文件位置」「输出目录」两个按钮。
2. **录制视频颜色失真**：`bgra_to_nv12` 中 Cb/Cr 系数的 **R 与 B 通道写反**（`u=-0.1146*b+0.5*r`，正确为 `-0.1146*r+0.5*b`）。改用完整 BT.709 有限范围转换（Y:16..235、UV:16..240），与 `MF_MT_YUV_MATRIX=BT709` 一致。新增「红蓝 Cr/Cb 方向」回归测试，并已通过。
3. **截图时工具遮挡后面的内容（曾反复失败）**：原先 `hide()` + sleep 依赖 DWM 重绘时序，实测截图中仍能看到 SnapCap 界面。改用 Windows 10 2004+ 的 `SetWindowDisplayAffinity(HWND, WDA_EXCLUDEFROMCAPTURE)`：在 setup 中对所有自身窗口打标记，并在每次截图/录制启动前重试。窗口**对用户仍可见，但对所有采集 API（GDI/DXGI/WGC）完全不可见**，无时序竞态。已用「启动 GUI → 另一进程全屏抓帧 → 检查 PNG」实测确认截图中不再出现 SnapCap 界面。
4. **录制软件可收起/展开**：录制控制台标题栏新增收起按钮，点击缩成 300×96 的细条（只显示状态、计时、开始/停止），再点展开恢复 380×520；收起后不遮挡后面的内容。

## 三个截图问题的修复

1. **区域截图出现纯色遮罩**：`useTheme` 对**所有**窗口调用了 `setBackgroundColor()`，把透明覆盖层也刷成了纯色。现在跳过 `overlay` 窗口，透明选区可正常看到桌面。
2. **窗口截图截到纯色 / 无法选窗口**：原来传 `hwnd=0`（前台窗口哨兵值），截到的是自身窗口。现在新增窗口选择器：列出真实窗口、按真实矩形高亮、点击选中并把真实 `hwnd` 交给 Rust；打开选区时会先隐藏程序自身窗口。
3. **Esc 无效**：覆盖层是透明无焦点窗口，DOM keydown 可能收不到。新增 Rust 全局 Escape 快捷键兜底（发 `overlay:escape` 事件），并加了始终可见的 ✕ 取消按钮。

同时修复了一个测试暴露的真实 bug：热键动作映射失败（`alt+KeyA` vs 配置 `Alt+A`），现已统一归一化 Key/Digit 前缀。

## 全功能验收

`snapcap.exe --verify-all` 一次跑 12 项：几何/选区归一化、快捷键解析、配置序列化、覆盖层窗口模式 URL、**三种采集目标**（全屏/区域/窗口真实 GDI 抓帧+PNG）、标注合成、H.264/MP4 编码、真实录制链路。Rust 单测 40 项。`run_all.ps1` 已集成，并额外执行真实 UI 的窗口截图端到端回归。

## 录制报错修复（E_ENCODER_NOT_AVAILABLE / 0xC00D4A44）

现象：录制结束报 `Finalize 失败: 接收器未处理任何示例`。

原因：A/V 漂移监控在没有音频时钟（AAC 不可用）时，`last_audio_100ns=0` 导致漂移随时间无限增大，把每一帧都判为"超前"而丢弃，SinkWriter 一帧未收到。

修复：仅当存在有效音频时钟时才做漂移控制；视频写入失败不再静默忽略；零帧时给出明确错误。已通过 `--record-test` 用真实 GDI 抓屏验证（见下）。

## 诊断命令

```powershell
snapcap.exe --capture-test C:\temp\shot.png          # 全屏截图 + PNG，打印耗时
snapcap.exe --record-test C:\temp\rec.mp4 3 30        # 真实 GDI 抓屏 → H.264 MP4（3 秒 / 30fps）
snapcap.exe --selftest C:\temp\test.mp4              # 合成帧 → H.264 MP4
```

`--record-test` 实测：5120×1440、8～11 帧/2~3 秒、有效 H.264 MP4（ffprobe 通过）。
