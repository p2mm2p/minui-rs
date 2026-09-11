//! `render` — 像素渲染层
//!
//! 本 crate 汇集所有"画东西"的函数，按功能语义划分为 8 个子模块。
//! 所有渲染函数直接操作 `common::video::VideoBuffer`（`Vec<u16>` 像素缓冲区），
//! 不依赖任何 SDL 类型。
//!
//! ## 子模块
//!
//! | 模块 | 职责 |
//! |------|------|
//! | [`pill`] | 圆角药丸按钮、圆角矩形 |
//! | [`asset`] | 精灵图集裁切、图集加载 |
//! | [`text`] | 文字渲染、截断、换行、消息框 |
//! | [`button`] | 按钮 + 提示文字、按钮组布局 |
//! | [`battery`] | 电池图标绘制 |
//! | [`hardware`] | 状态栏（电池/WiFi/音量/亮度） |
//! | [`scaler`] | AA 像素缩放器 |
//! | [`thumbnail`] | PNG 缩略图解码 |
//!
//! ## 与原 C 代码的对比
//!
//! 原版 `api.c` 中约 800 行渲染函数直接操作 `SDL_Surface*` 和 `TTF_Font*`。
//! Rust 版将所有渲染逻辑集中到本 crate，使用 `fontdue` 替代 SDL_ttf，
//! 使用纯 Rust `png` crate 替代 SDL_image。这使 common 保持零依赖，
//! 同时渲染逻辑的修改不会影响核心接口层。
//!

pub mod asset;
pub mod battery;
pub mod button;
pub mod hardware;
pub mod pill;
pub mod scaler;
pub mod text;
pub mod thumbnail;
