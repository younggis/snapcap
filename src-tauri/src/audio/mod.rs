//! 音频模块
//!
//! 统一 48kHz / stereo / f32 交错；两路 WASAPI 采集各自独占线程，回调只读取并
//! 入队（§7.1）。混音线程做线性插值重采样、增益、tanh 软限幅，按 20ms 帧推送。

pub mod aac;
pub mod loopback;
pub mod mic;

use crate::models::RecordingOptions;
use anyhow::Result;
use crossbeam_channel::Sender;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
/// 20ms 帧 = 960 samples (48kHz)。
pub const FRAME_SAMPLES: usize = 960;

/// 音频帧来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioSource {
    System,
    Mic,
}

/// 一路采集的音频帧（f32 交错，48kHz/stereo 之前为原始采样率）。
#[derive(Clone, Debug)]
pub struct AudioFrame {
    pub samples: Vec<f32>,
    pub timestamp_qpc: i64,
    pub source: AudioSource,
}

/// 采集线程句柄（loopback / mic 共用）。
pub struct AudioCaptureHandle {
    pub stop: Arc<AtomicBool>,
    pub join: Option<std::thread::JoinHandle<()>>,
}

impl Drop for AudioCaptureHandle {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// 整个录制的音频运行体。
pub struct AudioRuntime {
    pub handles: Vec<AudioCaptureHandle>,
}

impl Drop for AudioRuntime {
    fn drop(&mut self) {
        for handle in self.handles.drain(..) {
            drop(handle);
        }
    }
}

/// 启动系统回环 / 麦克风线程（各一路）。任一路初始化失败按 §7.4 降级：
/// 只启用成功的那一路；两路都失败时返回 Err，由上层决定只录视频。
pub fn spawn_audio(
    options: &RecordingOptions,
    tx: Sender<AudioFrame>,
    system_mute: Arc<AtomicBool>,
    mic_mute: Arc<AtomicBool>,
) -> Result<AudioRuntime> {
    let mut handles = Vec::new();
    let mut any = false;

    if options.system_audio {
        match loopback::spawn_loopback(tx.clone(), system_mute) {
            Ok(handle) => {
                handles.push(handle);
                any = true;
            }
            Err(err) => tracing::warn!("系统回环音频不可用（降级）: {err:#}"),
        }
    }

    if options.mic_audio {
        match mic::spawn_mic(tx.clone(), mic_mute) {
            Ok(handle) => {
                handles.push(handle);
                any = true;
            }
            Err(err) => tracing::warn!("麦克风不可用（降级）: {err:#}"),
        }
    }

    if !any {
        return Err(anyhow::anyhow!("没有可用的音频输入设备"));
    }

    Ok(AudioRuntime { handles })
}

/// 混音器：重采样到 48kHz、增益、静音、tanh 限幅。
pub struct Mixer {
    system_gain: f32,
    mic_gain: f32,
    system_mute: Arc<AtomicBool>,
    mic_mute: Arc<AtomicBool>,
}

impl Mixer {
    pub fn new(
        system_gain: f64,
        mic_gain: f64,
        system_mute: Arc<AtomicBool>,
        mic_mute: Arc<AtomicBool>,
    ) -> Self {
        Self {
            system_gain: system_gain as f32,
            mic_gain: mic_gain as f32,
            system_mute,
            mic_mute,
        }
    }

    /// 把两路（可能不同采样率）混合为一个 20ms / 48kHz / stereo f32 帧。
    pub fn mix(
        &self,
        system: Option<(&[f32], u32)>,
        mic: Option<(&[f32], u32)>,
        target_samples: usize,
    ) -> Vec<f32> {
        let mut out = vec![0.0f32; target_samples.max(FRAME_SAMPLES) * CHANNELS];
        if let Some((samples, rate)) = system {
            let resampled = resample_linear(samples, rate, SAMPLE_RATE, target_samples);
            let gain = if self.system_mute.load(std::sync::atomic::Ordering::SeqCst) {
                0.0
            } else {
                self.system_gain
            };
            for (i, s) in resampled.iter().enumerate() {
                if i < out.len() {
                    out[i] += s * gain;
                }
            }
        }
        if let Some((samples, rate)) = mic {
            let resampled = resample_linear(samples, rate, SAMPLE_RATE, target_samples);
            let gain = if self.mic_mute.load(std::sync::atomic::Ordering::SeqCst) {
                0.0
            } else {
                self.mic_gain
            };
            for (i, s) in resampled.iter().enumerate() {
                if i < out.len() {
                    out[i] += s * gain;
                }
            }
        }
        for s in out.iter_mut() {
            // tanh 软限幅，避免爆音（§7.2）。
            *s = s.tanh();
        }
        out
    }
}

/// 线性插值重采样（零依赖，质量足够 §7.2）。按交错帧索引缩放。
pub fn resample_linear(input: &[f32], src_rate: u32, dst_rate: u32, target_samples: usize) -> Vec<f32> {
    if src_rate == 0 || dst_rate == 0 || target_samples == 0 {
        return Vec::new();
    }
    let src_frames = input.len() / CHANNELS;
    if src_frames == 0 {
        return vec![0.0; target_samples * CHANNELS];
    }
    let mut out = vec![0.0f32; target_samples * CHANNELS];
    let ratio = src_rate as f64 / dst_rate as f64;
    for dst_frame in 0..target_samples {
        let src_pos = dst_frame as f64 * ratio;
        let i0 = src_pos.floor() as usize;
        let frac = (src_pos - i0 as f64) as f32;
        let i1 = (i0 + 1).min(src_frames - 1);
        let i0 = i0.min(src_frames - 1);
        for ch in 0..CHANNELS {
            let a = input[i0 * CHANNELS + ch];
            let b = input[i1 * CHANNELS + ch];
            out[dst_frame * CHANNELS + ch] = a + (b - a) * frac;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    fn mixer() -> (Mixer, Arc<AtomicBool>, Arc<AtomicBool>) {
        let s = Arc::new(AtomicBool::new(false));
        let m = Arc::new(AtomicBool::new(false));
        (Mixer::new(1.0, 1.0, s.clone(), m.clone()), s, m)
    }

    #[test]
    fn mix_empty_is_silent() {
        let (mx, _, _) = mixer();
        let out = mx.mix(None, None, FRAME_SAMPLES);
        assert_eq!(out.len(), FRAME_SAMPLES * CHANNELS);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn mix_resamples_and_applies_gain() {
        let (mx, _, _) = mixer();
        let sys = vec![0.5f32; 24000 * CHANNELS]; // 0.5s @24kHz
        let out = mx.mix(Some((&sys, 24_000)), None, FRAME_SAMPLES);
        assert_eq!(out.len(), FRAME_SAMPLES * CHANNELS);
        // mix 末级做 tanh 软限幅：tanh(0.5) ≈ 0.462。
        assert!(out.iter().all(|&v| (v - 0.4621).abs() < 0.01));
    }

    #[test]
    fn mute_zeroes_channel() {
        let (mx, sys_mute, _) = mixer();
        sys_mute.store(true, Ordering::SeqCst);
        let sys = vec![1.0f32; 48_000 * CHANNELS];
        let out = mx.mix(Some((&sys, 48_000)), None, FRAME_SAMPLES);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn limiter_bounds() {
        let (mx, _, _) = mixer();
        let sys = vec![8.0f32; FRAME_SAMPLES * CHANNELS];
        let out = mx.mix(Some((&sys, 48_000)), None, FRAME_SAMPLES);
        assert!(out.iter().all(|&v| v.abs() <= 1.01));
    }
}
