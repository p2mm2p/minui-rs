//! # MinUI PC 桌面平台
//!
//! 本 crate 提供 `PcPlatform` — 基于 minifb 的完整 Platform trait 参考实现。

mod platform;
pub use platform::PcPlatform;
