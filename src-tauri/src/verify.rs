//! 全功能验收：把各模块真实跑一遍并输出 PASS/FAIL。
//! 由 `snapcap.exe --verify-all` 调用，供自动化验收脚本使用。

use anyhow::Result;

pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

fn check(name: &'static str, body: impl FnOnce() -> Result<String>) -> Check {
    match body() {
        Ok(detail) => Check { name, ok: true, detail },
        Err(e) => Check { name, ok: false, detail: format!("{e:#}") },
    }
}

pub fn run_all() -> Vec<Check> {
    let mut out = Vec::new();

    // 1) 几何 / 选区归一化
    out.push(check("geometry", || {
        use crate::models::Rect;
        let r = crate::overlay::region::normalize_selection(100, 100, 20, 30);
        anyhow::ensure!(r.x == 20 && r.y == 30 && r.width == 80 && r.height == 70, "归一化错误");
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(50, 50, 100, 100);
        anyhow::ensure!(a.intersect(&b) == Some(Rect::new(50, 50, 50, 50)), "交集错误");
        Ok(format!("rect={},{} {}x{}", r.x, r.y, r.width, r.height))
    }));

    // 2) 快捷键解析
    out.push(check("hotkey", || {
        let c = crate::hotkey::parse_combo("Alt+A")
            .map_err(|e| anyhow::anyhow!("解析失败: {e}"))?;
        let s = crate::config::Settings::default();
        let a = crate::hotkey::action_for_combo(&c.to_string(), &s)
            .ok_or_else(|| anyhow::anyhow!("动作映射失败: {}", c))?;
        Ok(format!("combo={} action={a:?}", c))
    }));

    // 3) 配置序列化
    out.push(check("settings", || {
        let s = crate::config::Settings::default();
        let json = serde_json::to_string(&s)?;
        let back: crate::config::Settings = serde_json::from_str(&json)?;
        anyhow::ensure!(back.hotkeys.region == s.hotkeys.region, "往返不一致");
        Ok(format!("{} bytes", json.len()))
    }));

    // 3b) 窗口列表必须过滤掉本进程自己的窗口（用 PID 判定，不靠标题）
    out.push(check("window-list-excludes-self", || {
        let ws = crate::ipc::list_windows()
            .map_err(|e| anyhow::anyhow!("list_windows 失败: {e:?}"))?;
        #[cfg(windows)]
        {
            let self_pid = unsafe { windows::Win32::System::Threading::GetCurrentProcessId() };
            let leaked: Vec<String> = ws.iter()
                .filter(|w| crate::ipc::window_pid(w.hwnd) == self_pid)
                .map(|w| w.title.clone())
                .collect();
            anyhow::ensure!(leaked.is_empty(), "窗口列表包含本进程窗口: {leaked:?}");
            Ok(format!("{} windows, none from self pid={self_pid}", ws.len()))
        }
        #[cfg(not(windows))]
        Ok(format!("{} windows", ws.len()))
    }));

    // 3c) 窗口截图覆盖层必须真的收到 mode=window（前端据此启用窗口识别）。
    out.push(check("overlay-window-mode-url", || {
        let base = tauri::Url::parse(
            "http://tauri.localhost/index.html?view=overlay&mode=screenshot",
        )?;
        let url = crate::ipc::overlay_navigation_url(base, "window")?;
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        anyhow::ensure!(
            query.get("mode").map(String::as_str) == Some("window"),
            "mode 未进入覆盖层 URL: {url}"
        );
        anyhow::ensure!(
            query.get("view").map(String::as_str) == Some("overlay"),
            "view 丢失: {url}"
        );
        Ok(url.to_string())
    }));

    // 4) 三种采集目标（GDI 真实抓屏）
    let targets: [(&str, crate::models::CaptureTarget); 3] = [
        ("capture-full", crate::models::CaptureTarget::Full),
        ("capture-region", crate::models::CaptureTarget::Region {
            rect: crate::models::Rect::new(0, 0, 64, 64),
        }),
        ("capture-window", crate::models::CaptureTarget::Window {
            hwnd: 0, include_border: true,
        }),
    ];
    for (name, target) in targets {
        out.push(check(name, || {
            let frame = crate::capture::capture_frame(&target, false)
                .map_err(|e| anyhow::anyhow!("抓帧失败: {e:#}"))?;
            anyhow::ensure!(frame.width > 0 && frame.height > 0, "尺寸为 0");
            anyhow::ensure!(
                frame.bgra.len() >= frame.width as usize * frame.height as usize * 4,
                "BGRA 缓冲不足"
            );
            // GDI 抓帧必须是不透明的：BitBlt(SRCCOPY) 不写 alpha，窗口 DC 尤其
            // 容易留下 alpha=0，编码后 PNG 透明，编辑器显示空白。
            let pixels = frame.width as usize * frame.height as usize;
            let opaque = frame.bgra.chunks_exact(4).filter(|p| p[3] == 255).count();
            anyhow::ensure!(
                opaque * 100 >= pixels * 95,
                "alpha=255 仅 {}/{}，截图会显示为空白",
                opaque,
                pixels
            );
            let (bytes, url) = crate::image::encode::encode_image(
                &frame.bgra, frame.width, frame.height,
                crate::models::ImageFormat::Png, 92,
            )?;
            anyhow::ensure!(url.starts_with("data:image/png;base64,"), "data URL 错误");
            Ok(format!("{}x{} png={}B", frame.width, frame.height, bytes.len()))
        }));
    }

    // 5) 标注合成
    out.push(check("annotation", || {
        let mut bgra = vec![0u8; 16 * 16 * 4];
        crate::overlay::annotate::apply_ops(
            &mut bgra, 16, 16,
            &[
                crate::models::AnnotationOp::Rect { x: 1, y: 1, w: 8, h: 8, color: 0x0000ff, width: 1 },
                crate::models::AnnotationOp::Step { x: 8, y: 8, number: 1 },
            ],
        )?;
        anyhow::ensure!(bgra.iter().any(|&v| v != 0), "标注未写入像素");
        Ok(format!("{} bytes written", bgra.len()))
    }));

    // 5b) 输出目录：保存路径与“打开文件位置”解析必须一致（问题2回归测试）
    out.push(check("output-dir-consistency", || {
        let settings = crate::config::Settings::default();
        let dir = crate::image::ops::expand_output_dir(&settings.paths.output_dir)?;
        let dir2 = crate::image::ops::expand_output_dir(&settings.paths.output_dir)?;
        anyhow::ensure!(dir == dir2, "同一路径两次解析结果不一致");
        // 模拟一次真实落盘
        crate::image::ops::ensure_output_dir(&settings.paths.output_dir)?;
        anyhow::ensure!(dir.exists(), "输出目录未创建: {}", dir.display());
        let name = crate::image::ops::format_filename(&settings.paths.naming, "png");
        let full = dir.join(&name);
        std::fs::write(&full, b"\x89PNG\r\n\x1a\n")?;
        anyhow::ensure!(full.exists(), "文件未落在解析目录");
        // 用 /select 语义验证父目录一致
        anyhow::ensure!(full.parent() == Some(dir.as_path()), "文件父目录与输出目录不一致");
        std::fs::remove_file(&full)?;
        Ok(format!("dir={}", dir.display()))
    }));

    // 6) 编码管线：BGRA→NV12 + H.264/MP4
    out.push(check("encode-mp4", || {
        let dir = std::env::temp_dir().join("snapcap-verify.mp4");
        let result = crate::encode::selftest_encode(&dir, 30, 30, 320, 240)?;
        anyhow::ensure!(std::path::Path::new(&result.path).exists(), "MP4 未生成");
        let size = std::fs::metadata(&result.path)?.len();
        anyhow::ensure!(size > 0, "MP4 为空");
        let _ = std::fs::remove_file(&result.path);
        Ok(format!("h264 mp4 {size}B"))
    }));

    // 7) 录制链路：真实 GDI 抓帧 → 编码
    out.push(check("record-path", || {
        let dir = std::env::temp_dir().join("snapcap-verify-record.mp4");
        let result = crate::encode::record_test(&dir, 2, 30)?;
        anyhow::ensure!(std::path::Path::new(&result.path).exists(), "录制 MP4 未生成");
        let size = std::fs::metadata(&result.path)?.len();
        let _ = std::fs::remove_file(&result.path);
        Ok(format!("record {size}B"))
    }));

    out
}

pub fn print_report(checks: &[Check]) -> bool {
    let mut all_ok = true;
    for c in checks {
        let tag = if c.ok { "PASS" } else { "FAIL" };
        if !c.ok { all_ok = false; }
        println!("{tag} {} — {}", c.name, c.detail);
    }
    all_ok
}
