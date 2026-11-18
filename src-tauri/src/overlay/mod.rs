//! 覆盖层与标注模块
//!
//! Rust 侧只提供几何 / 状态 / 合成模型；MVP 的选区交互在 Vue overlay 窗口
//! (Canvas) 完成，通过 `overlay_submit` 命令把结果交回 Rust。

pub mod annotate;
pub mod cursor;
pub mod region;
