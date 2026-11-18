//! 编码与封装模块
//!
//! `Encoder` trait + 选择策略（§8.1）、能力探测、编码线程编排、MP4 封装与
//! 崩溃恢复。编码线程独占、串行写 sample；所有线程只通过 channel / Arc 通信。

pub mod mf_h264;
pub mod mp4;
pub mod sync;

pub use mf_h264::EncodedPacket;

use crate::audio::aac::AacEncoder;
use crate::audio::AudioFrame;
use crate::capture::frame::{FrameRing, VideoFrame};
use crate::config::HardwareAccel;
use crate::error::{to_event_payload, AppError, E_ENCODER_NOT_AVAILABLE};
use crate::models::{CaptureStats, RecordingResult, SessionId, SessionState};
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::Emitter;

/// 通用编码器 trait（§8.1）。
pub trait Encoder: Send {
    /// 编码一帧为 0..N 个包，时间戳为 100ns 单位。
    fn encode_frame(&mut self, frame: &VideoFrame, timestamp_100ns: i64) -> Result<Vec<EncodedPacket>>;
    fn flush(&mut self) -> Result<Vec<EncodedPacket>>;
}

impl Encoder for mf_h264::MfH264Encoder {
    fn encode_frame(&mut self, frame: &VideoFrame, timestamp_100ns: i64) -> Result<Vec<EncodedPacket>> {
        self.encode_bgra(&frame.bgra, frame.width, frame.height, timestamp_100ns, 0)
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>> {
        mf_h264::MfH264Encoder::flush(self)
    }
}

/// 编码线程所需的全部输入。
pub struct EncodeRequest {
    pub final_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub has_audio: bool,
    pub params: mf_h264::EncodeParams,
    pub pref: HardwareAccel,
    pub clock: Arc<sync::QpcClock>,
    pub stats: Arc<RwLock<CaptureStats>>,
    pub result: Arc<RwLock<Option<RecordingResult>>>,
    pub session_id: SessionId,
    pub app: tauri::AppHandle,
}

/// 编码运行体句柄。
pub struct EncodeRuntime {
    pub stop: Arc<AtomicBool>,
    pub join: Option<std::thread::JoinHandle<()>>,
}

impl Drop for EncodeRuntime {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// 启动编码线程：消费 FrameRing 的视频帧与音频通道，写 MP4，上报统计。
pub fn spawn_encode(
    req: EncodeRequest,
    frames: Arc<FrameRing>,
    audio_rx: crossbeam_channel::Receiver<AudioFrame>,
) -> Result<EncodeRuntime> {
    let encoder = mf_h264::MfH264Encoder::new(req.pref, req.params)
        .map_err(|e| anyhow!("{}: {e}", E_ENCODER_NOT_AVAILABLE))?;
    // 注意：SinkWriter 是 COM 接口，不可跨线程移动，因此在编码线程内创建。

    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let session = req.session_id;
    let app = req.app.clone();

    let join = std::thread::Builder::new()
        .name("snapcap-encode".to_string())
        .spawn(move || {
            let writer = mp4::Mp4Writer::new(
                &req.final_path,
                req.width,
                req.height,
                req.fps,
                req.has_audio,
            );
            let result = match writer {
                Ok(writer) => encode_loop(req, encoder, writer, frames, audio_rx, stop_thread),
                Err(err) => Err(err),
            };
            if let Err(err) = result {
                tracing::error!("编码线程失败: {err:#}");
                let app_err = AppError::new(
                    E_ENCODER_NOT_AVAILABLE,
                    format!("编码失败: {err}"),
                    "请检查磁盘空间与系统编码组件后重试。",
                    false,
                );
                let _ = app.emit("capture:error", to_event_payload(&app_err));
                let _ = app.emit(
                    "capture:state",
                    serde_json::json!({ "session": session, "state": SessionState::Failed }),
                );
            }
        })?;

    Ok(EncodeRuntime {
        stop,
        join: Some(join),
    })
}

#[allow(clippy::too_many_arguments)]
fn encode_loop(
    req: EncodeRequest,
    mut encoder: mf_h264::MfH264Encoder,
    mut writer: mp4::Mp4Writer,
    frames: Arc<FrameRing>,
    audio_rx: crossbeam_channel::Receiver<AudioFrame>,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    let mut aac = AacEncoder::new().ok();
    let mut drift = sync::DriftMonitor::default();
    let started = Instant::now();
    let mut last_backup = Instant::now();
    let mut last_audio_100ns = 0i64;
    let frame_interval_100ns = 10_000_000i64 / req.fps.max(1) as i64;
    let mut frame_index = 0u64;
    let mut written_samples = 0u64;

    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }

        // 先排空音频（以音频时钟为主）。
        while let Ok(audio) = audio_rx.try_recv() {
            if let Some(ref mut aac_encoder) = aac {
                match aac_encoder.encode(
                    &audio.samples,
                    audio.timestamp_qpc,
                    req.clock.origin(),
                    req.clock.frequency(),
                ) {
                    Ok(samples) => {
                        for s in samples {
                            last_audio_100ns = last_audio_100ns.max(s.timestamp_100ns);
                            if let Err(err) = writer.write_audio(&s) {
                                tracing::warn!("音频写入失败（继续录制）: {err:#}");
                            }
                        }
                    }
                    Err(err) => tracing::warn!("AAC 编码失败: {err:#}"),
                }
            }
        }

        // 视频帧：即使停止也排空队列（带超时语义）。
        let frame = match frames.pop() {
            Some(f) => f,
            None => {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
        };

        let ts = req.clock.to_100ns(frame.timestamp_qpc);
        // 只有存在有效音频时钟时才做 A/V 漂移控制。
        // 否则 last_audio_100ns=0 会让漂移随时间无限增大，导致所有帧被丢弃，
        // 最终 SinkWriter 因“没有收到任何 sample”而 Finalize 失败 (0xC00D4A44)。
        let drift_ms = if last_audio_100ns > 0 {
            drift.observe(ts, last_audio_100ns)
        } else {
            0
        };
        if drift.should_drop(drift_ms) {
            let mut stats = req.stats.write();
            stats.dropped_frames = stats.dropped_frames.saturating_add(1);
            continue;
        }

        let packets = encoder.encode_bgra(
            &frame.bgra,
            frame.width,
            frame.height,
            ts,
            frame_interval_100ns,
        )?;
        for packet in packets {
            // 写入失败必须暴露，不能静默吞掉（否则 Finalize 时才会报笼统错误）。
            writer.write_video(&packet)?;
            written_samples += 1;
        }
        frame_index += 1;

        // 统计：fps / 时长 / 码率。
        let elapsed = started.elapsed();
        {
            let mut stats = req.stats.write();
            stats.fps = if elapsed.as_secs() > 0 {
                (frame_index / elapsed.as_secs()) as u32
            } else {
                req.fps
            };
            stats.duration_ms = elapsed.as_millis() as u64;
            stats.dropped_frames = stats.dropped_frames.max(frames.dropped());
            if let Some(size) = std::fs::metadata(writer.tmp_path()).ok().map(|m| m.len()) {
                stats.bitrate_kbps = if elapsed.as_secs() > 0 {
                    (size * 8 / 1000) / elapsed.as_secs()
                } else {
                    0
                };
            }
        }

        if let Some(max_ms) = drift.maybe_report(ts) {
            tracing::debug!("A/V 漂移峰值: {max_ms}ms");
        }

        // 每 30s 备份（§8.6）。
        if last_backup.elapsed() >= Duration::from_secs(30) {
            if let Err(err) = writer.backup() {
                tracing::warn!("周期备份失败: {err:#}");
            }
            last_backup = Instant::now();
        }
    }

    let flushed = encoder.flush()?;
    for packet in flushed {
        writer.write_video(&packet)?;
        written_samples += 1;
    }
    if written_samples == 0 {
        return Err(anyhow!(
            "没有捕获到任何视频帧，无法生成 MP4。请检查采集是否正常工作。"
        ));
    }
    let (path, size) = writer.finalize()?;
    let duration_ms = started.elapsed().as_millis() as u64;
    let result = RecordingResult {
        path,
        duration_ms,
        size,
    };
    *req.result.write() = Some(result.clone());

    let _ = req.app.emit(
        "capture:state",
        serde_json::json!({ "session": req.session_id, "state": SessionState::Done }),
    );
    let _ = req.app.emit("capture:result", &result);

    Ok(())
}

/// 输出目录的恢复候选扫描（供 setup() 调用）。
pub fn scan_recovery(output_dir: &std::path::Path) -> Result<Vec<crate::models::RecoveryCandidate>> {
    mp4::scan_recovery(output_dir)
}

/// 录制链路诊断：真实 GDI 抓帧 -> 队列 -> 编码 -> MP4（无 UI / 无音频）。
/// 用法：snapcap.exe --record-test <out.mp4> [秒数] [fps]
pub fn record_test(
    output: &std::path::Path,
    seconds: u64,
    fps: u32,
) -> Result<crate::models::RecordingResult> {
    #[cfg(windows)]
    {
        use crate::capture::frame::FrameRing;
        use std::sync::Arc;
        unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED).ok(); }
        let _guard = SelftestCom;

        let target = crate::models::CaptureTarget::Full;
        let probe = crate::capture::capture_frame(&target, false)?;
        let (w, h) = (probe.width, probe.height);
        let frames = Arc::new(FrameRing::new(60));
        let paused = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cap = crate::capture::spawn_capture(&target, fps, paused, frames.producer())?;

        let mut encoder = mf_h264::MfH264Encoder::new(
            crate::config::HardwareAccel::Auto,
            mf_h264::EncodeParams { width: w, height: h, fps, cqp: 20, keyframe_sec: 2 },
        )?;
        let mut writer = mp4::Mp4Writer::new(output, w, h, fps, false)?;

        let duration = 10_000_000i64 / fps.max(1) as i64;
        let mut ts = 0i64;
        let mut written = 0u64;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
        while std::time::Instant::now() < deadline {
            match frames.pop() {
                Some(frame) => {
                    for packet in encoder.encode_bgra(&frame.bgra, frame.width, frame.height, ts, duration)? {
                        writer.write_video(&packet)?;
                        written += 1;
                    }
                    ts += duration;
                }
                None => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        }
        drop(cap); // 停采集
        let flushed = encoder.flush()?;
        for packet in flushed { writer.write_video(&packet)?; written += 1; }
        anyhow::ensure!(written > 0, "没有捕获到帧 (frames_written=0)");
        let (path, size) = writer.finalize()?;
        eprintln!("[RECORD TEST] source={w}x{h} frames_written={written}");
        Ok(crate::models::RecordingResult { path, duration_ms: seconds * 1000, size })
    }
    #[cfg(not(windows))]
    {
        let _ = (output, seconds, fps);
        Err(anyhow::anyhow!("录制诊断仅支持 Windows"))
    }
}

/// 生成 20ms 的 440Hz 正弦 PCM（48kHz / stereo / f32）。
fn tone_20ms(index: u32) -> Vec<f32> {
    const N: usize = 960; // 48kHz * 20ms
    let mut out = Vec::with_capacity(N * 2);
    for i in 0..N {
        let t = i as f32 / 48_000.0;
        let v = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.2;
        out.push(v);
        out.push(v);
    }
    let _ = index;
    out
}

/// 自测：用合成棋盘帧走完整编码管线，生成真实 MP4（供 `--selftest` 验收）。
pub fn selftest_encode(output: &std::path::Path, frames: u32, fps: u32, w: u32, h: u32) -> Result<crate::models::RecordingResult> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
        unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); }
        let _guard = SelftestCom;
        let _ = &_guard;

        let mut encoder = mf_h264::MfH264Encoder::new(
            crate::config::HardwareAccel::Auto,
            mf_h264::EncodeParams { width: w, height: h, fps, cqp: 20, keyframe_sec: 2 },
        )?;
        let mut writer = mp4::Mp4Writer::new(output, w, h, fps, true)?;
        let mut aac = crate::audio::aac::AacEncoder::new()?;
        let duration = 10_000_000i64 / fps.max(1) as i64;
        for i in 0..frames {
            let bgra = synthetic_frame(i, w, h);
            let ts = i as i64 * duration;
            for packet in encoder.encode_bgra(&bgra, w, h, ts, duration)? {
                writer.write_video(&packet)?;
            }
            // 每帧对应一段 440Hz 正弦 PCM（48kHz / stereo / 20ms）。
            let samples = tone_20ms(i);
            let audio_ts = ts;
            for sample in aac.encode(&samples, 0, 0, 10_000_000)? {
                let mut s = sample;
                s.timestamp_100ns = audio_ts;
                writer.write_audio(&s)?;
            }
        }
        let (path, size) = writer.finalize()?;
        Ok(crate::models::RecordingResult {
            path,
            duration_ms: frames as u64 * 1000 / fps.max(1) as u64,
            size,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = (output, frames, fps, w, h);
        Err(anyhow::anyhow!("编码自测仅支持 Windows"))
    }
}

#[cfg(windows)]
struct SelftestCom;
#[cfg(windows)]
impl Drop for SelftestCom {
    fn drop(&mut self) {
        unsafe { windows::Win32::System::Com::CoUninitialize() }
    }
}

/// 生成合成 BGRA 帧（棋盘格 + 移动色块），用于编码验收。
pub fn synthetic_frame(index: u32, w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let cell = ((x / 24) + (y / 24) + index / 8) % 2;
            let base = if cell == 0 { 40u8 } else { 90u8 };
            let dx = (x as i64 - (index * 4 % w) as i64).abs();
            let dy = (y as i64 - (index * 3 % h) as i64).abs();
            let moving = dx < 16 && dy < 16;
            let i = ((y * w + x) * 4) as usize;
            out[i] = if moving { 60 } else { base };
            out[i + 1] = base;
            out[i + 2] = if moving { 200 } else { 160 };
            out[i + 3] = 255;
        }
    }
    out
}
