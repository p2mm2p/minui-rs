//! # MinUI — Rust Rewrite
//!
//! MinUI 是一个运行在复古掌机上的极简游戏启动器（Launcher）。
//!
//! 此 crate 是原 [MinUI](https://github.com/shauninman/MinUI) C 项目的 Rust 重写。
//!
//! ## 架构分层
//!
//! ```text
//! ┌──────────────────────────────┐
//! │   launcher (minui 逻辑)      │  ← 目录浏览、最近游戏、游戏启动
//! ├──────────────────────────────┤
//! │   Platform trait             │  ← 抽象硬件差异
//! ├──────────────────────────────┤
//! │   platform impl (rg35xx等)   │  ← framebuffer, input, power
//! └──────────────────────────────┘
//! ```
//!
//! ## 模块索引
//!
//! - [`types`] — 所有数据结构 (Entry, Directory, Recent, 按钮, 颜色等)
//! - [`platform`] — Platform trait 及其关联常量/类型
//! - [`state`] — MinUi 全局状态机
//! - [`paths`] — SD 卡路径常量（从 Platform 派生）
//! - [`utils`] — 工具函数（字符串匹配、文件 I/O、显示名提取）
//! - [`scan`] — 文件系统扫描（目录遍历、最近游戏、收藏、多碟）

pub mod types;
pub mod platform;
pub mod state;
pub mod paths;
pub mod utils;
pub mod scan;

// 重导出最常用的类型
pub use types::{Entry, EntryType, Directory, Recent, Button, PadContext, Color};
pub use platform::Platform;
pub use state::MinUi;
