//! # Video — 缩放器 + 画面效果
//!
//! 对应原 C 代码中的 `selectScaler()`, `video_refresh_callback()`。

use platform::GfxRenderer;

/// 缩放模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleMode {
    Native,
    Aspect,
    Fullscreen,
    Cropped,
}

/// 画面效果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenEffect {
    None,
    Line,
    Grid,
}

/// 画面锐度
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sharpness {
    Sharp,
    Crisp,
    Soft,
}

/// 视频渲染器
pub struct VideoRenderer {
    /// 渲染器状态
    pub renderer: GfxRenderer,
    /// 当前缩放模式
    pub scaling: ScaleMode,
    /// 当前效果
    pub effect: ScreenEffect,
    /// 当前锐度
    pub sharpness: Sharpness,
    /// 设备宽度
    pub device_w: u32,
    /// 设备高度
    pub device_h: u32,
    /// 防撕裂模式
    pub vsync: u32, // 0=off, 1=lenient, 2=strict
}

impl VideoRenderer {
    pub fn new(device_w: u32, device_h: u32) -> Self {
        Self {
            renderer: GfxRenderer {
                src: std::ptr::null(),
                dst: std::ptr::null_mut(),
                blit: None,
                aspect: 0.0,
                scale: 1,
                true_w: 0,
                true_h: 0,
                src_x: 0, src_y: 0, src_w: 0, src_h: 0, src_p: 0,
                dst_x: 0, dst_y: 0, dst_w: 0, dst_h: 0, dst_p: 0,
            },
            scaling: ScaleMode::Aspect,
            effect: ScreenEffect::None,
            sharpness: Sharpness::Soft,
            device_w,
            device_h,
            vsync: 1,
        }
    }

    /// 选择缩放器 — 核心输出尺寸变化时调用
    ///
    /// 对应原 C 的 `selectScaler()`
    pub fn select_scaler(
        &mut self,
        _src_w: i32,
        _src_h: i32,
        _src_p: i32,
        _aspect_ratio: f64,
    ) {
        // TODO: 实现完整的缩放算法
        // 四种模式：Native / Aspect / Fullscreen / Cropped
        // 处理 fit/oversized 两种设备类型
    }

    /// 视频刷新回调 — 核心每帧调用
    ///
    /// 对应原 C 的 `video_refresh_callback()`
    pub fn refresh(
        &mut self,
        _data: *const u8,
        _width: u32,
        _height: u32,
        _pitch: usize,
    ) {
        // TODO: 调用 select_scaler → GFX_blitRenderer → GFX_flip
    }
}
