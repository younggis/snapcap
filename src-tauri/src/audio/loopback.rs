//! WASAPI 采集（系统回环 + 麦克风，§7.1）
//!
//! 两个端点共用同一采集循环：Render 端点启用 LOOPBACK 抓系统声，Capture 端点抓麦克风。
//! 采集线程独占 COM 对象，经 channel 把 f32 帧送回混音/编码线程。

use super::{AudioCaptureHandle, AudioFrame, AudioSource};
use anyhow::{anyhow, Result};
use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use windows::core::GUID;
use windows::Win32::Media::Audio::{
    eCapture, eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
    WAVEFORMATEX,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
};

/// 终点类型（供 mic.rs 复用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endpoint {
    Render,
    Capture,
}

/// 启动系统声回环采集。
pub fn spawn_loopback(tx: Sender<AudioFrame>, mute: Arc<AtomicBool>) -> Result<AudioCaptureHandle> {
    spawn_wasapi_capture(tx, mute, Endpoint::Render, true)
}

/// 通用 WASAPI 采集入口（mic.rs 复用）。
pub(crate) fn spawn_wasapi_capture(
    tx: Sender<AudioFrame>,
    mute: Arc<AtomicBool>,
    endpoint: Endpoint,
    loopback: bool,
) -> Result<AudioCaptureHandle> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let (init_tx, init_rx) = std::sync::mpsc::channel::<Result<()>>();

    let join = std::thread::Builder::new()
        .name("snapcap-wasapi".into())
        .spawn(move || {
            let result = run_capture(tx, mute, stop_thread, endpoint, loopback);
            let _ = init_tx.send(result);
        })?;

    // 关键：绝不能无限等待 WASAPI 初始化（某些设备/虚拟声卡会卡住），
    // 否则 record_start 永远不返回，UI 表现为“点了没反应并卡死”。
    match init_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(())) => Ok(AudioCaptureHandle {
            stop,
            join: Some(join),
        }),
        Ok(Err(err)) => Err(err),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            stop.store(true, Ordering::SeqCst);
            Err(anyhow!("WASAPI 初始化超时（2s），已降级为无声"))
        }
        Err(_) => Err(anyhow!("WASAPI 采集线程启动失败")),
    }
}

fn run_capture(
    tx: Sender<AudioFrame>,
    mute: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    endpoint: Endpoint,
    loopback: bool,
) -> Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let result = (|| -> Result<()> {
            // CLSID_MMDeviceEnumerator = {BCDE0395-E52F-467C-8E3D-C4579291692E}
            let clsid = GUID::from_u128(0xbcde0395_e52f_467c_8e3d_c4579291692e);
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&clsid, None, CLSCTX_ALL)
                    .map_err(|e| anyhow!("创建设备枚举器失败: {e:?}"))?;

            let flow = if loopback || endpoint == Endpoint::Render {
                eRender
            } else {
                eCapture
            };
            let device = enumerator
                .GetDefaultAudioEndpoint(flow, eConsole)
                .map_err(|e| anyhow!("获取默认音频端点失败: {e:?}"))?;

            let client: IAudioClient = device
                .Activate::<IAudioClient>(CLSCTX_ALL, None)
                .map_err(|e| anyhow!("激活 IAudioClient 失败: {e:?}"))?;

            let mix = client.GetMixFormat().map_err(|e| anyhow!("GetMixFormat: {e:?}"))?;
            let fmt: &WAVEFORMATEX = &*mix;
            let channels = fmt.nChannels as usize;
            let bits = fmt.wBitsPerSample as usize;
            let rate = fmt.nSamplesPerSec;
            let block_align = fmt.nBlockAlign as usize;
            let is_float = fmt.wFormatTag == 3; // WAVE_FORMAT_IEEE_FLOAT
            let flags = if loopback { AUDCLNT_STREAMFLAGS_LOOPBACK } else { 0 };
            client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    flags,
                    200_000, // 20ms buffer (100ns units)
                    0,
                    mix,
                    None,
                )
                .map_err(|e| anyhow!("IAudioClient::Initialize 失败: {e:?}"))?;
            CoTaskMemFree(Some(mix as *const core::ffi::c_void));

            let capturer: IAudioCaptureClient = client
                .GetService::<IAudioCaptureClient>()
                .map_err(|e| anyhow!("GetService(IAudioCaptureClient): {e:?}"))?;
            client.Start().map_err(|e| anyhow!("IAudioClient::Start: {e:?}"))?;

            let source = if loopback || endpoint == Endpoint::Render {
                AudioSource::System
            } else {
                AudioSource::Mic
            };
            let qpc_freq = query_qpc_frequency();

            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
                let mut packet_size = 0u32;
                loop {
                    packet_size = match capturer.GetNextPacketSize() {
                        Ok(n) => n,
                        Err(_) => break,
                    };
                    if packet_size == 0 {
                        break;
                    }
                    let mut data: *mut u8 = std::ptr::null_mut();
                    let mut frames = 0u32;
                    let mut flags = 0u32;
                    if capturer
                        .GetBuffer(
                            &mut data,
                            &mut frames,
                            &mut flags,
                            None,
                            Some(std::ptr::null_mut()),
                        )
                        .is_err()
                    {
                        break;
                    }
                    let samples = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                        vec![0.0f32; frames as usize * channels]
                    } else if data.is_null() || frames == 0 {
                        Vec::new()
                    } else {
                        let byte_count = frames as usize * block_align;
                        let raw = std::slice::from_raw_parts(data, byte_count);
                        decode_pcm(raw, bits, channels, is_float)
                    };
                    let _ = capturer.ReleaseBuffer(frames);

                    if samples.is_empty() {
                        continue;
                    }
                    let samples = resample(&samples, rate, 48_000, channels);
                    let samples = if mute.load(Ordering::SeqCst) {
                        vec![0.0f32; samples.len()]
                    } else {
                        samples
                    };
                    let timestamp = now_qpc();
                    let _ = qpc_freq;
                    let _ = tx.send(AudioFrame {
                        samples,
                        timestamp_qpc: timestamp,
                        source,
                    });
                }
            }

            let _ = client.Stop();
            CoUninitialize();
            Ok(())
        })();
        result
    }
}

/// 把 WASAPI 原始字节解为 f32 交错采样。
fn decode_pcm(raw: &[u8], bits: usize, channels: usize, is_float: bool) -> Vec<f32> {
    if channels == 0 {
        return Vec::new();
    }
    if is_float && bits == 32 {
        return raw
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
    }
    if bits == 16 {
        return raw
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
            .collect();
    }
    if bits == 24 {
        return raw
            .chunks_exact(3)
            .map(|c| {
                let v = (c[0] as i32) | ((c[1] as i32) << 8) | ((c[2] as i32) << 16);
                let v = if v & 0x80_0000 != 0 { v | !0xff_ffff } else { v };
                v as f32 / 8_388_608.0
            })
            .collect();
    }
    Vec::new()
}

/// 线性插值重采样到 48kHz。
fn resample(samples: &[f32], from: u32, to: u32, channels: usize) -> Vec<f32> {
    if from == to || channels == 0 || samples.len() < channels {
        return samples.to_vec();
    }
    let frames = samples.len() / channels;
    let out_frames = (frames as u64 * to as u64 / from as u64).max(1) as usize;
    let mut out = Vec::with_capacity(out_frames * channels);
    for i in 0..out_frames {
        let pos = i as f64 * from as f64 / to as f64;
        let idx = pos.floor() as usize;
        let frac = pos - idx as f64;
        let idx2 = (idx + 1).min(frames - 1);
        for ch in 0..channels {
            let a = samples[idx * channels + ch];
            let b = samples[idx2 * channels + ch];
            out.push(a + (b - a) * frac as f32);
        }
    }
    out
}

fn query_qpc_frequency() -> i64 {
    let mut f = 0i64;
    let _ = unsafe { windows::Win32::System::Performance::QueryPerformanceFrequency(&mut f) };
    f
}

fn now_qpc() -> i64 {
    let mut v = 0i64;
    let _ = unsafe { windows::Win32::System::Performance::QueryPerformanceCounter(&mut v) };
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_s16() {
        let raw = [0u8, 0, 0x00, 0x40]; // 0.0, 0.5
        let out = decode_pcm(&raw, 16, 1, false);
        assert_eq!(out.len(), 2);
        assert!((out[1] - 0.5).abs() < 0.001);
    }

    #[test]
    fn decode_float() {
        let raw = 0.25f32.to_le_bytes();
        let out = decode_pcm(&raw, 32, 1, true);
        assert!((out[0] - 0.25).abs() < 0.001);
    }

    #[test]
    fn resample_same_rate() {
        assert_eq!(resample(&[1.0, 2.0], 48000, 48000, 1), vec![1.0, 2.0]);
    }
}
