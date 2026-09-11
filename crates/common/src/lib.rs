//! `common` — MinUI 基础设施层
//!
//! 本 crate 是 workspace 中唯一零第三方依赖的模块，为上层（minui、minarch）
//! 提供：
//!
//! - [`platform`]：`Platform` trait，定义硬件抽象接口，是上层与平台实现的唯一通道
//! - [`video`]：视频相关基础类型（`VideoBuffer`、`Rect`、`Point`、颜色常量等）
//! - [`input`]：输入状态类型（`InputState`）和按键位掩码常量（`BTN_*`）
//! - [`audio`]：音频类型（`AudioFrame`、`AudioRingBuffer`）和重采样器（`Resampler`）
//! - [`power`]：电源管理状态机（`PWR_*` 系列函数）
//! - [`utils`]：文件 I/O 和字符串工具函数
//! - [`paths`]：平台路径派生函数族（对应 C `defines.h` 的路径宏）
//!
//! ## 与原 C 代码的对比
//!
//! 本 crate 对应原版 MinUI 中的 `api.h` + `defines.h` + `utils.h`。
//! 原版使用 `#define` 宏和全局变量（如 `pad`、`gfx`），Rust 版改为通过
//! `Platform` trait 注入依赖，消除了全局可变状态。
//!

pub mod audio;
pub mod input;
pub mod paths;
pub mod platform;
pub mod power;
pub mod utils;
pub mod video;
