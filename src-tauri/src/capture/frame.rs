//! 视频帧与帧队列
//!
//! 传输格式为 CPU 侧 BGRA (4 bytes/pixel, top-down)。GPU 纹理的零拷贝直通
//! 属于 Phase 2；MVP（§8.3 CPU 路径）在所有采集器内部把纹理/staging 映射为
//! BGRA 后再入队，因此 `VideoFrame` 本身就是可跨线程传递的拥有型像素缓冲。

use crate::models::Rect;
use crossbeam_channel::{Receiver, Sender, TrySendError};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// DXGI 旋转角。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DxgiRotation {
    Identity,
    Rotate90,
    Rotate180,
    Rotate270,
}

/// 鼠标按键（用于点击高亮）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// 点击波纹：按下后持续约 300ms 的扩散圆。
#[derive(Clone, Copy, Debug)]
pub struct ClickRing {
    pub button: MouseButton,
    pub age_ms: u32,
    pub radius_px: u32,
    pub alpha: u8,
}

/// 光标叠加信息（由本程序绘制，WGC 的 `IsCursorCaptureEnabled=false`）。
#[derive(Clone, Debug)]
pub struct CursorOverlay {
    pub x: i32,
    pub y: i32,
    /// HCURSOR 句柄或缓存位图键。
    pub shape_handle: isize,
    pub click_ring: Option<ClickRing>,
}

/// 一帧视频（拥有型 BGRA 像素）。
#[derive(Clone, Debug)]
pub struct VideoFrame {
    /// QueryPerformanceCounter 单调时钟。
    pub timestamp_qpc: i64,
    /// 内容在虚拟桌面中的实际矩形（窗口可能含不可见边框）。
    pub content_rect: Rect,
    pub rotation: DxgiRotation,
    pub cursor: Option<CursorOverlay>,
    pub width: u32,
    pub height: u32,
    /// BGRA 像素，长度 = width * height * 4。
    pub bgra: Arc<Vec<u8>>,
}

impl VideoFrame {
    pub fn new(width: u32, height: u32, bgra: Vec<u8>) -> Self {
        Self {
            timestamp_qpc: now_qpc(),
            content_rect: Rect::new(0, 0, width, height),
            rotation: DxgiRotation::Identity,
            cursor: None,
            width,
            height,
            bgra: Arc::new(bgra),
        }
    }

    /// 按虚拟桌面坐标裁剪出一帧（返回新的拥有型缓冲）。
    pub fn cropped(&self, rect: &Rect) -> Option<VideoFrame> {
        let x1 = rect.x.max(self.content_rect.x);
        let y1 = rect.y.max(self.content_rect.y);
        let x2 = (rect.x + rect.width as i32).min(self.content_rect.x + self.width as i32);
        let y2 = (rect.y + rect.height as i32).min(self.content_rect.y + self.height as i32);
        if x2 <= x1 || y2 <= y1 {
            return None;
        }
        let left = (x1 - self.content_rect.x) as usize;
        let top = (y1 - self.content_rect.y) as usize;
        let cw = (x2 - x1) as usize;
        let ch = (y2 - y1) as usize;
        let src_stride = self.width as usize * 4;
        let mut out = vec![0u8; cw * ch * 4];
        for row in 0..ch {
            let src = (top + row) * src_stride + left * 4;
            let dst = row * cw * 4;
            out[dst..dst + cw * 4].copy_from_slice(&self.bgra[src..src + cw * 4]);
        }
        Some(VideoFrame {
            timestamp_qpc: self.timestamp_qpc,
            content_rect: Rect::new(x1, y1, cw as u32, ch as u32),
            rotation: self.rotation,
            cursor: self.cursor.clone(),
            width: cw as u32,
            height: ch as u32,
            bgra: Arc::new(out),
        })
    }
}

/// 固定容量环形队列：满了丢弃最旧帧，绝不阻塞采集线程（§6.5）。
pub struct FrameRing {
    tx: Sender<VideoFrame>,
    rx: Receiver<VideoFrame>,
    capacity: usize,
    dropped: AtomicU64,
}

impl FrameRing {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        let (tx, rx) = crossbeam_channel::bounded(capacity);
        Self {
            tx,
            rx,
            capacity,
            dropped: AtomicU64::new(0),
        }
    }

    /// 生产一帧；队列满时丢弃最旧帧并累加 dropped。
    pub fn push(&self, frame: VideoFrame) {
        // 先尝试非阻塞发送；满则丢弃最旧帧再试。
        if let Err(TrySendError::Full(frame)) = self.tx.try_send(frame) {
            let _ = self.rx.try_recv(); // 丢最旧
            self.dropped.fetch_add(1, Ordering::Relaxed);
            // 第二次仍满则丢弃新帧（极端抖动下），不阻塞采集线程。
            if let Err(TrySendError::Full(_)) = self.tx.try_send(frame) {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
        let _ = self.capacity; // 显式使用字段，避免未来改动产生 dead_code 警告
    }

    /// 消费一帧（非阻塞）。
    pub fn try_pop(&self) -> Option<VideoFrame> {
        self.rx.try_recv().ok()
    }

    /// 阻塞消费，用于编码线程；通道关闭时返回 None。
    pub fn pop(&self) -> Option<VideoFrame> {
        self.rx.recv().ok()
    }

    /// 清空队列（暂停时调用，避免恢复后吐出旧帧）。
    pub fn clear(&self) {
        while self.rx.try_recv().is_ok() {}
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn producer(&self) -> Sender<VideoFrame> {
        self.tx.clone()
    }

    pub fn len(&self) -> usize {
        self.tx.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 读取 QPC 计数值（纳秒基准换算见 encode::sync）。
#[cfg(windows)]
pub fn now_qpc() -> i64 {
    // NOTE: QueryPerformanceCounter 位于 Win32::System::Performance，
    // windows-rs 把 BOOL 返回值映射为 Result<()>。
    let mut value = 0i64;
    let _ = unsafe { windows::Win32::System::Performance::QueryPerformanceCounter(&mut value) };
    value
}

#[cfg(not(windows))]
pub fn now_qpc() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or_default();
    nanos / 100 // 近似 QPC 刻度（100ns）
}
