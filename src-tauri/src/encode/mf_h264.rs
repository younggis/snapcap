//! H.264 编码器（§8.1 / §8.3）
//!
//! CPU 路径：BGRA → NV12（BT.709），由 `Mp4Writer` 的 SinkWriter 完成
//! H.264 编码与 MP4 封装。SinkWriter 会自动选择硬件或软件 H.264 MFT。

use crate::config::HardwareAccel;
use anyhow::{anyhow, Result};

/// 编码参数。
#[derive(Clone, Copy, Debug)]
pub struct EncodeParams {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub cqp: u32,
    pub keyframe_sec: u32,
}

/// 编码后的视频包（当前为 NV12 原始帧，由 SinkWriter 编码）。
#[derive(Clone, Debug)]
pub struct EncodedPacket {
    pub bytes: Vec<u8>,
    pub timestamp_100ns: i64,
    pub duration_100ns: i64,
    pub keyframe: bool,
}

/// H.264 编码器（CPU 侧格式转换 + 交由 SinkWriter 编码）。
pub struct MfH264Encoder {
    params: EncodeParams,
    #[allow(dead_code)]
    pref: HardwareAccel,
    frame_count: u64,
}

impl MfH264Encoder {
    pub fn new(pref: HardwareAccel, params: EncodeParams) -> Result<Self> {
        if params.width == 0 || params.height == 0 || params.fps == 0 {
            return Err(anyhow!("无效的编码参数"));
        }
        Ok(Self {
            params,
            pref,
            frame_count: 0,
        })
    }

    /// BGRA(top-down) → NV12，按关键帧间隔标记关键帧。
    pub fn encode_bgra(
        &mut self,
        bgra: &[u8],
        width: u32,
        height: u32,
        timestamp_100ns: i64,
        duration_100ns: i64,
    ) -> Result<Vec<EncodedPacket>> {
        let nv12 = bgra_to_nv12(bgra, width as usize, height as usize);
        if nv12.is_empty() {
            return Err(anyhow!("BGRA 转 NV12 失败"));
        }
        let key_interval = (self.params.keyframe_sec.max(1) * self.params.fps.max(1)) as u64;
        let keyframe = self.frame_count % key_interval == 0;
        self.frame_count += 1;
        Ok(vec![EncodedPacket {
            bytes: nv12,
            timestamp_100ns,
            duration_100ns,
            keyframe,
        }])
    }

    pub fn flush(&mut self) -> Result<Vec<EncodedPacket>> {
        Ok(Vec::new())
    }
}

/// BGRA(top-down, 4BPP) → NV12（BT.709 矩阵）。
/// 布局：Y 平面 width*height，随后 UV 交错平面 (width/2)*(height/2)*2。
pub fn bgra_to_nv12(bgra: &[u8], width: usize, height: usize) -> Vec<u8> {
    if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
        return Vec::new();
    }
    let expected = width
        .checked_mul(height)
        .and_then(|v| v.checked_mul(4))
        .unwrap_or(0);
    if bgra.len() < expected {
        return Vec::new();
    }
    let y_plane = width * height;
    let uv_bytes = (width / 2) * (height / 2) * 2;
    let mut out = vec![0u8; y_plane + uv_bytes];

    // BT.709 有限范围（studio swing）转换，与媒体类型 MF_MT_YUV_MATRIX=BT709 一致。
    // 系数：Kr=0.2126 Kg=0.7152 Kb=0.0722；
    //   Y' = 16  + 219 * (Kr*R + Kg*G + Kb*B) / 255
    //   Cb = 128 + 224 * (-Kb/(1-Kr)*R - Kg/(1-Kr)*G + 0.5*B) / 255
    //   Cr = 128 + 224 * (0.5*R - Kg/(1-Kb)*G - Kr/(1-Kb)*B) / 255
    for py in 0..height {
        let row = py * width;
        for px in 0..width {
            let i = (row + px) * 4;
            let b = bgra[i] as f32;
            let g = bgra[i + 1] as f32;
            let r = bgra[i + 2] as f32;
            let y = 16.0 + 219.0 * (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0;
            out[row + px] = y.clamp(16.0, 235.0) as u8;
        }
    }

    for by in 0..(height / 2) {
        for bx in 0..(width / 2) {
            let mut bs = 0f32;
            let mut gs = 0f32;
            let mut rs = 0f32;
            for dy in 0..2 {
                for dx in 0..2 {
                    let i = (((by * 2 + dy) * width) + (bx * 2 + dx)) * 4;
                    bs += bgra[i] as f32;
                    gs += bgra[i + 1] as f32;
                    rs += bgra[i + 2] as f32;
                }
            }
            let (b, g, r) = (bs * 0.25, gs * 0.25, rs * 0.25);
            // 注意：Cb 的 -0.1146 作用于 R，+0.5 作用于 B；Cr 相反。
            let u = 128.0 + 224.0 * (-0.1146 * r - 0.3854 * g + 0.5 * b) / 255.0;
            let v = 128.0 + 224.0 * (0.5 * r - 0.4542 * g - 0.0458 * b) / 255.0;
            let base = y_plane + (by * (width / 2) + bx) * 2;
            out[base] = u.clamp(16.0, 240.0) as u8;
            out[base + 1] = v.clamp(16.0, 240.0) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nv12_output_size() {
        let bgra = vec![128u8; 4 * 4 * 4];
        let nv12 = bgra_to_nv12(&bgra, 4, 4);
        assert_eq!(nv12.len(), 4 * 4 + (4 / 2) * (4 / 2) * 2);
    }

    #[test]
    fn nv12_rejects_odd_dimensions() {
        assert!(bgra_to_nv12(&vec![0u8; 4 * 3 * 4], 3, 3).is_empty());
    }

    #[test]
    fn nv12_y_plane_gray() {
        let bgra = vec![100u8, 100, 100, 255, 100, 100, 100, 255,
                        100, 100, 100, 255, 100, 100, 100, 255];
        let nv12 = bgra_to_nv12(&bgra, 2, 2);
        // 有限范围：Y = 16 + 219*100/255 ≈ 101.9 → 101
        assert!(nv12[..4].iter().all(|&y| y == 101));
    }

    #[test]
    fn nv12_red_vs_blue_are_distinct() {
        // 纯红 (BGRA: 0,0,255) 与纯蓝 (255,0,0) 的 Cr/Cb 必须明显不同且方向正确。
        // 若 R/B 系数写反，两者会互换。
        let red = [0u8, 0, 255, 255];
        let blue = [255u8, 0, 0, 255];
        let mk = |p: [u8; 4]| -> Vec<u8> {
            let mut v = Vec::with_capacity(2 * 2 * 4);
            for _ in 0..4 { v.extend_from_slice(&p); }
            bgra_to_nv12(&v, 2, 2)
        };
        let nv_red = mk(red);
        let nv_blue = mk(blue);
        let y_plane = 4usize;
        let red_u = nv_red[y_plane];
        let red_v = nv_red[y_plane + 1];
        let blue_u = nv_blue[y_plane];
        let blue_v = nv_blue[y_plane + 1];
        // 红色 Cb(U) 应 < 128、Cr(V) 应 > 128；蓝色相反。
        assert!(red_u < 128 && red_v > 128, "red U={red_u} V={red_v}");
        assert!(blue_u > 128 && blue_v < 128, "blue U={blue_u} V={blue_v}");
    }
}
