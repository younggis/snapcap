//! 时间戳与 A/V 同步（§8.5）
//!
//! 统一时间基为 QueryPerformanceCounter。暂停时记录暂停区间，恢复时把
//! `qpc_origin += paused_duration`，保证输出时间轴连续。以音频时钟为主，
//! 视频偏差 > 40ms 时由上层丢帧或重复上一帧。

use std::sync::atomic::{AtomicI64, Ordering};

/// QPC 时间基准与暂停补偿。
pub struct QpcClock {
    origin: AtomicI64,
    frequency: i64,
    paused_at: AtomicI64,
}

impl QpcClock {
    /// 以当前 QPC 作为原点；frequency 由平台提供（Windows 约 10,000,000/s）。
    pub fn new(origin_qpc: i64, frequency: i64) -> Self {
        Self {
            origin: AtomicI64::new(origin_qpc),
            frequency: if frequency > 0 { frequency } else { 10_000_000 },
            paused_at: AtomicI64::new(0),
        }
    }

    pub fn origin(&self) -> i64 {
        self.origin.load(Ordering::SeqCst)
    }

    pub fn frequency(&self) -> i64 {
        self.frequency
    }

    /// QPC -> 100ns 时间戳。
    pub fn to_100ns(&self, qpc: i64) -> i64 {
        qpc.saturating_sub(self.origin())
            .saturating_mul(10_000_000)
            / self.frequency
    }

    /// 当前时刻的 100ns 时间戳。
    pub fn now_100ns(&self) -> i64 {
        self.to_100ns(crate::capture::frame::now_qpc())
    }

    /// 进入暂停：记录暂停起点（0 表示未暂停）。
    pub fn pause(&self, qpc: i64) {
        self.paused_at.store(qpc.max(1), Ordering::SeqCst);
    }

    /// 恢复：把暂停区间补偿进原点，时间轴保持连续。
    pub fn resume(&self, qpc: i64) {
        let started = self.paused_at.swap(0, Ordering::SeqCst);
        if started > 0 {
            let paused_duration = qpc.saturating_sub(started).max(0);
            self.origin.fetch_add(paused_duration, Ordering::SeqCst);
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused_at.load(Ordering::SeqCst) > 0
    }
}

/// A/V 漂移监控：以音频时钟为准，记录视频偏差并给出丢/补帧建议。
pub struct DriftMonitor {
    threshold_ms: i64,
    last_report_100ns: i64,
    max_drift_ms: i64,
}

impl Default for DriftMonitor {
    fn default() -> Self {
        Self {
            threshold_ms: 40,
            last_report_100ns: 0,
            max_drift_ms: 0,
        }
    }
}

impl DriftMonitor {
    pub fn new(threshold_ms: i64) -> Self {
        Self {
            threshold_ms,
            ..Default::default()
        }
    }

    /// 返回视频相对音频的偏差（毫秒，正=视频超前）。
    pub fn observe(&mut self, video_100ns: i64, audio_100ns: i64) -> i64 {
        let drift_ms = video_100ns.saturating_sub(audio_100ns) / 10_000;
        self.max_drift_ms = self.max_drift_ms.max(drift_ms.abs());
        drift_ms
    }

    /// 是否需要丢帧（视频超前超过阈值）。
    pub fn should_drop(&self, drift_ms: i64) -> bool {
        drift_ms > self.threshold_ms
    }

    /// 是否需要重复上一帧（视频落后超过阈值）。
    pub fn should_repeat(&self, drift_ms: i64) -> bool {
        drift_ms < -self.threshold_ms
    }

    /// 每 5 秒回报一次统计（§8.5）。
    pub fn maybe_report(&mut self, now_100ns: i64) -> Option<i64> {
        if now_100ns.saturating_sub(self.last_report_100ns) >= 5_000_000 {
            self.last_report_100ns = now_100ns;
            let max = self.max_drift_ms;
            self.max_drift_ms = 0;
            Some(max)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qpc_to_100ns() {
        let clock = QpcClock::new(1_000_000, 10_000_000);
        assert_eq!(clock.to_100ns(1_500_000), 500_000);
    }

    #[test]
    fn pause_resume_keeps_timeline_continuous() {
        let clock = QpcClock::new(0, 10_000_000);
        assert!(!clock.is_paused());
        clock.pause(1_000_000);
        assert!(clock.is_paused());
        clock.resume(1_500_000); // paused 0.05s
        assert!(!clock.is_paused());
        assert_eq!(clock.origin(), 500_000);
        assert_eq!(clock.to_100ns(1_500_000), 1_000_000);
    }

    #[test]
    fn drift_thresholds() {
        let mut d = DriftMonitor::default();
        assert_eq!(d.observe(60_000, 30_000), 3);
        assert!(d.should_drop(50));
        assert!(d.should_repeat(-50));
        assert!(!d.should_drop(20));
        assert_eq!(d.maybe_report(6_000_000), Some(3));
    }
}
