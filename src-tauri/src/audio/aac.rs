//! AAC 编码输入准备（§7.3）
//!
//! 把 48kHz/stereo/f32 交错采样转换为 16-bit PCM，**实际 AAC 编码由
//! `Mp4Writer` 的 SinkWriter 完成**（输出媒体类型为 AAC）。

use anyhow::{anyhow, Result};

/// 送入 SinkWriter 的音频包（32-bit float PCM 交错）。
#[derive(Clone, Debug)]
pub struct EncodedAudioSample {
    /// 小端 f32 交错字节。
    pub bytes: Vec<u8>,
    pub timestamp_100ns: i64,
    pub duration_100ns: i64,
}

/// AAC 编码器前端（PCM 转换）。
pub struct AacEncoder;

impl AacEncoder {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }

    /// f32 交错 → f32 小端 PCM（AAC 输入），并换算时间戳。
    pub fn encode(
        &mut self,
        samples: &[f32],
        timestamp_qpc: i64,
        origin_qpc: i64,
        frequency: i64,
    ) -> Result<Vec<EncodedAudioSample>> {
        if samples.is_empty() || frequency <= 0 {
            return Ok(Vec::new());
        }
        let bytes = f32_to_le_bytes(samples);
        let t_100ns = timestamp_qpc
            .saturating_sub(origin_qpc)
            .saturating_mul(10_000_000)
            / frequency;
        // 48kHz / 2ch / 16bit：每帧 1024 采样，约 21.3ms。
        let frames = (samples.len() / 2) as i64;
        let duration_100ns = if frames > 0 {
            frames.saturating_mul(10_000_000) / 48_000
        } else {
            0
        };
        Ok(vec![EncodedAudioSample {
            bytes,
            timestamp_100ns: t_100ns,
            duration_100ns,
        }])
    }
}

/// f32 小端字节，带 tanh 软限幅（AAC 编码器输入格式）。
pub fn f32_to_le_bytes(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 4);
    for &s in samples {
        let clamped = s.tanh().clamp(-1.0, 1.0);
        out.extend_from_slice(&clamped.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_length() {
        let bytes = f32_to_le_bytes(&[0.0, 0.5, -0.5, 1.0]);
        assert_eq!(bytes.len(), 16);
    }

    #[test]
    fn pcm_silence() {
        let bytes = f32_to_le_bytes(&[0.0, 0.0]);
        assert_eq!(bytes, vec![0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn encoder_timestamp() {
        let mut enc = AacEncoder::new().unwrap();
        let out = enc.encode(&[0.0; 1920], 1_000_000, 0, 10_000_000).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].timestamp_100ns, 1_000_000);
    }
}
