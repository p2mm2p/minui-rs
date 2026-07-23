//! # MinUI Core
//!
//! 被 minui（启动器）和 minarch（模拟器前端）共享的基础类型和工具函数。
//!
//! ## 模块
//!
//! - [`types`] — 数据结构 (Entry, Directory, Recent, Button, PadContext, Color)
//! - [`utils`] — 字符串匹配、显示名提取、文件 I/O
//! - [`paths`] — SD 卡路径常量和派生函数

pub mod types;
pub mod utils;
pub mod paths;

// 重导出最常用类型
pub use types::{
    Entry, EntryType, Directory, Recent, Button, ButtonId, PadContext,
    Color, Axis, RenderMode, CpuSpeed, ScaleMode, Sharpness,
    ScreenEffect, VsyncMode,
};
