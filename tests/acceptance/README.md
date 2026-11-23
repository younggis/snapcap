# 验收脚本

对应设计文档 `docs/DESIGN.md` §16.3 的验收用例；`verify_recording.ps1` 重点覆盖编码管线产物校验（§16.1 / AC-2 / AC-3）。

## 真实 UI 端到端：窗口截图（e2e_window.ps1）

`e2e_window.ps1` 用 WebView2 CDP 驱动**已构建的 release exe** 走完整真实 UI 链路，验证的不只是 Rust 单元逻辑：

1. 启动受控 WinForms 窗口（`window_target.ps1`，置顶，写入 HWND 与物理矩形）
2. 以隔离配置/输出目录启动 SnapCap（`SNAPCAP_CONFIG_DIR`），并开启 `--remote-debugging-port`
3. CDP 点击主界面「窗口」入口
4. 断言覆盖层 URL 为 `view=overlay&mode=window`，且识别到 ≥1 个窗口
5. 在目标窗口中心 hover + 单击，断言选中 `hwnd` 与测试窗口一致
6. 通过真实键盘 `SendKeys('{ENTER}')` 提交
7. 断言标注编辑器画布已显示图像，并收到自动保存路径
8. 回读编辑器 canvas 像素：非透明像素占比须 > 10%（防止「画布存在但图是空白」的假阳性）
9. 断言 PNG 尺寸与目标窗口物理尺寸完全一致、文件位于隔离输出目录，且 alpha=255 的像素占比 ≥ 95%、颜色数 ≥ 3（防止 GDI alpha  bug 导致图被渲染成透明）

```powershell
# 默认验证 releases\snapcap.exe
powershell -ExecutionPolicy Bypass -File .\e2e_window.ps1

# 验证指定构建产物
powershell -ExecutionPolicy Bypass -File .\e2e_window.ps1 `
  -Exe ..\..\src-tauri\target\release\snapcap.exe
```

依赖：Node.js（CDP 驱动）、Windows PowerShell 5.1。测试配置与截图都写入 `%TEMP%\snapcap-e2e-*`，**不会写入用户的 `Pictures\SnapCap`**；成功时自动清理临时目录，失败时保留并打印 `WORKDIR` 便于排查。

成功后会在 `tests/acceptance/artifacts/` 生成两张视觉证据：

- `window-overlay-proof.png`：覆盖层中的窗口识别高亮与「已识别：…」提示
- `window-capture-proof.png`：最终抓取的窗口图（应与测试窗口物理尺寸完全一致）

`run_all.ps1` 已包含该检查项（`E2E window capture UI`）。其中还断言了「窗口识别准确性」：目标点被多个窗口矩形覆盖时，被高亮/选中的必须是面积最小（最具体）的窗口，防止最大化窗口（浏览器 / 桌面外壳）挡住 QQ 等小窗口导致识别不到。

### 文字标注（e2e_text_annotation.ps1）

`cdp_text_flow.mjs` + `e2e_text_annotation.ps1` 在真实 UI 中回归文字标注：主窗口「全屏」截图 → 编辑器选文字工具 + 蓝色 → 输入文字 → 分别用 CDP 键盘和**真实 SendKeys Enter** 提交 → 断言输入框关闭且画布出现蓝色像素（颜色生效）。

重点覆盖曾出现的回归：Enter/Esc 全局快捷键原先常驻注册，会在系统层吞掉按键，导致标注输入框回车无法提交文字。现在这两个键只在选区覆盖层打开期间注册。`run_all.ps1` 已包含该检查项（`E2E text annotation`）。

### GPU 渲染窗口（gpu_window.ps1）

`e2e_window.ps1` 的目标窗口是 WinForms（GDI 渲染）。微信 / QQ / Chromium 等由 DWM 合成，GDI `GetWindowDC` 抓到的是全黑位图。`gpu_window.ps1` 用同一套 CDP 真实 UI 链路抓一个**正在运行的真实 GPU 窗口**（默认找 QQ，可用 `-ProcessName` / `-Hwnd` 指定），并断言：

- PNG 物理尺寸与目标窗口一致（脚本内启用进程 DPI 感知，避免 125% 缩放下逻辑/物理像素混淆）
- alpha=255 占比 ≥ 95%（不能透明空白）
- 黑色像素占比 ≤ 50%、颜色数 ≥ 8（不能全黑）
- 目标窗口临时置顶以保证 hover 命中与抓图内容正确，结束后恢复原 z-order

找不到指定进程时打印 `SKIP` 并以 0 退出（便于在未安装 QQ 的机器上运行）。已接入 `run_all.ps1`（`GPU window capture`）。

## 依赖：ffprobe（仅开发 / CI）

`verify_recording.ps1` 依赖 `ffprobe`（FFmpeg 附带的媒体探测工具）。它**仅用于开发与 CI，不随 SnapCap 安装包分发**，SnapCap 运行时也不依赖 FFmpeg（编码完全基于 Windows Media Foundation）。

- 确保 `ffprobe` 在 `PATH` 中（例如安装 FFmpeg 并将 `bin` 加入 PATH）。
- 若未找到，脚本会打印 `FAIL: ffprobe not found in PATH` 并以非零码退出。

## 运行

```powershell
# 基本校验：编码格式 + 音轨
powershell -ExecutionPolicy Bypass -File .\verify_recording.ps1 -Path .\out.mp4

# 完整校验：分辨率、帧率、时长、音轨（对应 1080p60 录制 60 秒）
powershell -ExecutionPolicy Bypass -File .\verify_recording.ps1 `
  -Path .\out.mp4 `
  -ExpectWidth 1920 -ExpectHeight 1080 `
  -ExpectFps 60 -MinDurationSec 60 -ExpectAudio
```

参数（除 `-Path` 外均可选，提供才校验）：`-Path`、`-ExpectWidth`、`-ExpectHeight`、`-ExpectFps`、`-MinDurationSec`、`-ExpectAudio`。

脚本逐项打印 `PASS:` / `FAIL:`，最后输出 `RESULT: PASS` 或 `RESULT: FAIL`。任一断言失败则以退出码 `1` 结束（便于 CI 判定）。

## 校验项与验收用例映射

| 检查 | 对应用例 |
|---|---|
| H.264 视频流、分辨率、帧率、AAC 音轨、时长 ≥ 阈值 | **AC-2**（1080p60 / ≥ 3500 帧 / 丢帧统计）、**AC-3**（A/V 同步，帧率与时长校验；偏差的精确测量仍需人工/专门工具） |
| 热键选区延迟 ≤ 100ms、截图保存 ≤ 80ms（AC-1） | 需人工计时 / `pnpm tauri dev` 下手测，本脚本不覆盖 |
| 强杀进程后 `.tmp` 恢复（AC-4） | 需手工注入崩溃场景验证，本脚本不覆盖 |
| 安装包 ≤ 15MB、冷启动 ≤ 1.5s（AC-5） | 由构建产物大小与冷启动计时验证，本脚本不覆盖 |

> 合成测试帧由 `../fixture/generate_frames.py` 生成，供编码管线测试作为输入（纯色 / 棋盘格 / 渐变 BMP → 纹理）。
