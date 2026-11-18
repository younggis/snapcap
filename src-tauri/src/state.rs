//! 全局共享状态
//!
//! 线程模型（§10）：跨线程只共享不可变句柄与原子标志；
//! `config` / `session` 用 parking_lot::RwLock，读写都不跨越 await。

use crate::audio::AudioRuntime;
use crate::capture::frame::FrameRing;
use crate::capture::CaptureHandle;
use crate::config::Settings;
use crate::encode::sync::QpcClock;
use crate::encode::EncodeRuntime;
use crate::models::{CaptureStats, ImageFormat, SelectionPayload, SessionId, SessionState};
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::{Emitter, Manager};

/// 缓存中的截图（BGRA 像素），按 id 索引，供 output / annotation 命令使用。
#[derive(Clone, Debug)]
pub struct ScreenshotEntry {
    pub bgra: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    /// 已编码的字节（与预览一致），落盘时直接写入。
    pub encoded: Arc<Vec<u8>>,
    pub format: ImageFormat,
}

/// 当前录制会话句柄。持有采集/音频/编码运行体、abort 句柄与统计快照。
pub struct SessionHandle {
    pub id: SessionId,
    pub state: SessionState,
    /// 控制 tokio 统计上报任务的取消。
    pub abort: tokio::task::AbortHandle,
    /// 由编码线程更新、由控制线程读取上报的统计。
    pub stats: Arc<RwLock<CaptureStats>>,
    /// 暂停标志，采集线程读取，暂停时不再推帧。
    pub paused: Arc<AtomicBool>,
    /// 最终 MP4 输出路径。
    pub output_path: Option<PathBuf>,
    /// 会话开始的 QPC 时间值。
    pub started_qpc: i64,
    /// 帧队列（暂停时清空）。
    pub frames: Arc<FrameRing>,
    /// 统一时间基准（暂停补偿）。
    pub clock: Arc<QpcClock>,
    /// 编码结果由编码线程写回。
    pub result: Arc<RwLock<Option<crate::models::RecordingResult>>>,
    /// 各线程运行体，保持其生命周期；停止时按 §10 顺序 drop。
    pub capture: Option<CaptureHandle>,
    pub audio: Option<AudioRuntime>,
    pub encoder: Option<EncodeRuntime>,
}

/// 由 Tauri `manage()` 注入到所有命令 / 事件的共享状态。
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Settings>>,
    pub session: Arc<RwLock<Option<SessionHandle>>>,
    pub system_mute: Arc<AtomicBool>,
    pub mic_mute: Arc<AtomicBool>,
    pub screenshots: Arc<RwLock<HashMap<String, ScreenshotEntry>>>,
    /// `open_region_overlay` 挂起的选区结果通道；`overlay_submit` 负责兑现。
    pub pending_selection: Arc<Mutex<Option<tokio::sync::oneshot::Sender<SelectionPayload>>>>,
    /// setup() 中注入的 AppHandle，供只有 `State` 参数的命令隐藏窗口 / 发事件。
    pub app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        Self {
            config: Arc::new(RwLock::new(settings)),
            session: Arc::new(RwLock::new(None)),
            system_mute: Arc::new(AtomicBool::new(false)),
            mic_mute: Arc::new(AtomicBool::new(false)),
            screenshots: Arc::new(RwLock::new(HashMap::new())),
            pending_selection: Arc::new(Mutex::new(None)),
            app_handle: Arc::new(RwLock::new(None)),
        }
    }

    /// 由 setup() 注入 AppHandle 副本。
    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        *self.app_handle.write() = Some(handle);
    }

    /// 兑现挂起的选区（若存在），返回是否确实有等待者。
    pub fn submit_selection(&self, payload: SelectionPayload) -> bool {
        let sender = self.pending_selection.lock().take();
        match sender {
            Some(tx) => tx.send(payload).is_ok(),
            None => false,
        }
    }

    /// 隐藏覆盖层并广播 `overlay:selection`，供观察者 UI 使用。
    pub fn publish_selection(&self, payload: &SelectionPayload) {
        let handle_guard = self.app_handle.read();
        if let Some(app) = handle_guard.as_ref() {
            if let Some(window) = app.get_webview_window("overlay") {
                let _ = window.hide();
            }
            let _ = app.emit("overlay:selection", payload);
        }
    }

    /// 读取当前会话状态的快照。
    pub fn session_state(&self) -> Option<SessionState> {
        self.session.read().as_ref().map(|s| s.state)
    }

    /// 设置当前会话状态，并同步返回新状态。
    pub fn set_session_state(&self, state: SessionState) {
        if let Some(session) = self.session.write().as_mut() {
            session.state = state;
        }
    }

    /// 当前统计快照。
    pub fn stats_snapshot(&self) -> CaptureStats {
        self.session
            .read()
            .as_ref()
            .map(|s| *s.stats.read())
            .unwrap_or_default()
    }
}
