//! # MinUI Launcher
//!
//! 游戏列表浏览器 —— MinUI 的主启动器。
//! 使用共享的 common, platform, render, power 等 crate。

pub mod state;
pub mod scan;
pub mod launch;

pub use state::MinUi;
