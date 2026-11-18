//! 光标与点击高亮模型（§9.4）
//!
//! WGC `IsCursorCaptureEnabled = false`，光标由本程序在合成阶段绘制。
//! 本模块维护点击波纹的状态机与光标形状缓存模型；实际像素绘制由
//! `capture::gdi::draw_cursor_arrow` / 合成线程完成。

use crate::capture::frame::{ClickRing, MouseButton};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// 点击波纹持续时间（毫秒）。
pub const CLICK_RING_LIFETIME_MS: u32 = 300;

/// 根据年龄计算一个波纹（半径随时间扩散，透明度随时间衰减）。
pub fn click_ring_from_age(age_ms: u32, button: MouseButton) -> ClickRing {
    let t = (age_ms.min(CLICK_RING_LIFETIME_MS) as f32) / CLICK_RING_LIFETIME_MS as f32;
    ClickRing {
        button,
        age_ms,
        radius_px: (6.0 + 18.0 * t).round() as u32,
        alpha: (255.0 * (1.0 - t)).round().clamp(0.0, 255.0) as u8,
    }
}

/// 点击波纹集合：每次点击加入一个，超过寿命的自动清理。
#[derive(Default)]
pub struct ClickRingState {
    pub rings: Vec<(u64, MouseButton)>, // (起始时间戳 ms, 按键)
}

impl ClickRingState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一次点击。
    pub fn push(&mut self, button: MouseButton) {
        self.rings.push((now_ms(), button));
    }

    /// 清理过期项，并把存活项转成可渲染的 ClickRing。
    pub fn active(&mut self) -> Vec<ClickRing> {
        let now = now_ms();
        self.rings
            .retain(|(start, _)| now.saturating_sub(*start) < CLICK_RING_LIFETIME_MS as u64);
        self.rings
            .iter()
            .map(|(start, button)| {
                click_ring_from_age(now.saturating_sub(*start) as u32, *button)
            })
            .collect()
    }
}

/// 光标形状缓存：HCURSOR 句柄 -> 已经光栅化的 BGRA 位图。
#[derive(Default)]
pub struct CursorShapeCache {
    entries: HashMap<isize, (u32, u32, Vec<u8>)>,
}

impl CursorShapeCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, handle: isize, width: u32, height: u32, bgra: Vec<u8>) {
        self.entries.insert(handle, (width, height, bgra));
    }

    pub fn get(&self, handle: isize) -> Option<(u32, u32, &[u8])> {
        self.entries
            .get(&handle)
            .map(|(w, h, b)| (*w, *h, b.as_slice()))
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}
