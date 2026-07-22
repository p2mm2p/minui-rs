//! # Platform Trait — 硬件抽象层
//!
//! 这是 MinUI 支持多种掌机的核心设计。每个平台只需实现这个 trait，
//! 上层的 minui/minarch 逻辑代码完全不感知硬件差异。
//!
//! ## 对应原 C 代码的位置
//!
//! | 原 C 文件                 | Rust 对应                        |
//! |--------------------------|----------------------------------|
//! | `platform.h`（平台常量）   | 关联常量 (associated constants)   |
//! | `api.h`（GFX_/PAD_/PWR_） | trait 方法                       |
//! | `defines.h`（派生常量）    | 提供默认实现的方法 (default fn)   |
//!
//! ## 常量的三层关系
//!
//! 1. **原始常量** — 平台直接定义（如 `FIXED_WIDTH = 640`）
//! 2. **派生常量** — 由原始常量计算得出（如 `FIXED_PITCH = WIDTH * BPP`）
//! 3. **可选覆盖** — 有默认值，平台可以覆写（如 `BUTTON_POWEROFF` 默认 = `NA`）
//!
//! 在 Rust 中，第 1 类用关联常量，第 2、3 类用带默认实现的方法。

use crate::types::*;

// ============================================================================
// 标记值 —— 对应 C 中的 BUTTON_NA / CODE_NA / JOY_NA / AXIS_NA
// ============================================================================

/// 未映射的按钮/键值 —— 对应 C 中的 `BUTTON_NA = -1` 等
pub const NA: i32 = -1;

// ============================================================================
// Framebuffer —— 原始帧缓冲区
// ============================================================================

/// 帧缓冲区描述符 —— 对应 C 中的 `SDL_Surface` 的核心部分
///
/// 在嵌入式平台上，这通常是 mmap 映射的 ION 内存或 /dev/fb0 的直接映射。
/// 像素格式为 RGB565（大多数平台）或 RGB888。
///
/// ## 安全性
///
/// `pixels` 是原始指针，生命周期由平台实现管理。调用者不应释放此指针。
#[derive(Debug)]
pub struct Framebuffer {
    /// 指向像素数据的原始指针
    pub pixels: *mut u8,
    /// 帧缓冲宽度（像素）
    pub width: u32,
    /// 帧缓冲高度（像素）
    pub height: u32,
    /// 每行字节数（stride / pitch），可能大于 `width * bpp`
    pub pitch: u32,
    /// 每像素字节数（2 = RGB565, 4 = RGBA8888）
    pub bpp: u8,
}

// SAFETY: Framebuffer 可以跨线程传递（底层 mmap 内存本身就是共享的）
unsafe impl Send for Framebuffer {}
unsafe impl Sync for Framebuffer {}

impl Framebuffer {
    /// 帧缓冲区总字节数
    pub fn size(&self) -> usize {
        (self.pitch as usize) * (self.height as usize)
    }

    /// 以 u16（RGB565 像素）切片视图访问
    ///
    /// # Safety
    /// 调用者必须确保 bpp == 2 且 pixels 有效。
    pub unsafe fn as_u16_slice(&self) -> &[u16] {
        let len = self.size() / 2;
        std::slice::from_raw_parts(self.pixels as *const u16, len)
    }

    /// 以 u16（RGB565 像素）可变切片视图访问
    ///
    /// # Safety
    /// 调用者必须确保 bpp == 2 且 pixels 有效。
    pub unsafe fn as_u16_slice_mut(&mut self) -> &mut [u16] {
        let len = self.size() / 2;
        std::slice::from_raw_parts_mut(self.pixels as *mut u16, len)
    }
}

// ============================================================================
// Scaler —— 软件缩放器类型
// ============================================================================

/// 缩放器函数签名
///
/// 对应 C 中的 `typedef void (*scaler_t)(void*,void*,int,int,int,int,int,int)`
///
/// 参数：
/// - src: 源像素指针
/// - dst: 目标像素指针
/// - src_w, src_h: 源宽高
/// - src_pitch: 源每行字节数
/// - dst_w, dst_h: 目标宽高
/// - dst_pitch: 目标每行字节数
pub type ScalerFn = unsafe extern "C" fn(
    src: *const u8,
    dst: *mut u8,
    src_w: i32,
    src_h: i32,
    src_pitch: i32,
    dst_w: i32,
    dst_h: i32,
    dst_pitch: i32,
);

// ============================================================================
// GfxRenderer —— 渲染器状态
// ============================================================================

/// 渲染器上下文 —— 对应 C 中的 `typedef struct GFX_Renderer`
///
/// 管理从模拟器核心输出（`src`）到显示缓冲区（`dst`）的缩放和位块传输。
#[derive(Debug, Clone)]
pub struct GfxRenderer {
    /// 源像素数据（模拟器核心输出）
    pub src: *const u8,
    /// 目标像素数据（显示缓冲区）
    pub dst: *mut u8,
    /// 当前使用的缩放器函数
    pub blit: Option<ScalerFn>,
    /// 宽高比：0 = 整数倍缩放，-1 = 全屏，否则为实际宽高比
    pub aspect: f64,
    /// 缩放倍数
    pub scale: i32,

    /// 模拟器核心输出的真实宽度
    pub true_w: i32,
    /// 模拟器核心输出的真实高度
    pub true_h: i32,

    /// 源矩形 X 偏移（像素）
    pub src_x: i32,
    /// 源矩形 Y 偏移（像素）
    pub src_y: i32,
    /// 源矩形宽度（像素）
    pub src_w: i32,
    /// 源矩形高度（像素）
    pub src_h: i32,
    /// 源每行字节数（pitch / stride）
    pub src_p: i32,

    /// 目标矩形 X 偏移（像素）
    pub dst_x: i32,
    /// 目标矩形 Y 偏移（像素）
    pub dst_y: i32,
    /// 目标矩形宽度（像素）
    pub dst_w: i32,
    /// 目标矩形高度（像素）
    pub dst_h: i32,
    /// 目标每行字节数（pitch / stride）
    pub dst_p: i32,
}

// SAFETY: GfxRenderer 包含原始指针，但平台实现保证它们的生命周期
unsafe impl Send for GfxRenderer {}

// ============================================================================
// Audio —— 音频帧
// ============================================================================

/// 单帧立体声音频采样 —— 对应 C 中的 `typedef struct SND_Frame`
#[derive(Debug, Clone, Copy, Default)]
pub struct AudioFrame {
    /// 左声道采样值
    pub left: i16,
    /// 右声道采样值
    pub right: i16,
}

// ============================================================================
// Lid —— 翻盖状态（部分设备有）
// ============================================================================

/// 翻盖传感器上下文 —— 对应 C 中的 `LID_Context`
#[derive(Debug, Clone, Copy)]
pub struct LidContext {
    /// 设备是否有翻盖传感器（如 GBA SP 形态的掌机）
    pub has_lid: bool,
    /// 当前是否打开
    pub is_open: bool,
}

impl Default for LidContext {
    fn default() -> Self {
        Self { has_lid: false, is_open: true }
    }
}

// ============================================================================
// Platform Trait
// ============================================================================

/// 平台抽象 trait —— 每个支持的设备实现一次
///
/// ## 实现平台时需要提供的
///
/// 1. **关联常量**：所有 `const FOO: Type;` 声明
/// 2. **必需方法**：`init_video`, `init_input`, `poll_input` 等标记为不带 `default` 的方法
/// 3. **可选覆写**：带 `default` 实现的方法已有合理默认值，可按需覆写
///
/// ## 编译时平台选择
///
/// 通过 Cargo features 控制：
/// ```toml
/// [features]
/// platform-rg35xx = []
/// ```
///
/// 代码中使用 `#[cfg(feature = "platform-rg35xx")]` 选择实现。
pub trait Platform: Send + Sized {
    // ================================================================
    // 1. 屏幕参数（每个平台必须定义）
    // ================================================================

    /// 屏幕物理宽度（像素），缩放前
    const FIXED_WIDTH: u32;

    /// 屏幕物理高度（像素），缩放前
    const FIXED_HEIGHT: u32;

    /// 每像素字节数（2 = RGB565, 4 = RGBA8888）
    const FIXED_BPP: u8;

    /// 缩放倍数 —— 逻辑分辨率 ÷ 物理分辨率
    ///
    /// 例如 RG35XX 的 640×480 LCD 对应 320×240 逻辑分辨率，FIXED_SCALE = 2
    const FIXED_SCALE: u32;

    /// 每行像素字节数 = `FIXED_WIDTH × FIXED_BPP`
    fn fixed_pitch() -> u32 {
        Self::FIXED_WIDTH * Self::FIXED_BPP as u32
    }

    /// 帧缓冲区总大小（字节）= `fixed_pitch × FIXED_HEIGHT`
    fn fixed_size() -> u32 {
        Self::fixed_pitch() * Self::FIXED_HEIGHT
    }

    /// 像素深度（位）= `FIXED_BPP × 8`
    fn fixed_depth() -> u32 {
        Self::FIXED_BPP as u32 * 8
    }

    /// 可见行数（缩放后的逻辑行数）
    ///
    /// 对应 C 中的 `MAIN_ROW_COUNT`，默认值 6。
    /// 公式：`FIXED_HEIGHT / (PILL_SIZE * FIXED_SCALE) - 2`
    const MAIN_ROW_COUNT: usize = 6;

    /// 可见区域两侧留白（逻辑像素）
    const PADDING: u32 = 10;

    /// 每行高度（逻辑像素，缩放前）
    const PILL_SIZE: u32 = 30;

    /// 按钮标签字体大小
    const BUTTON_SIZE: u32 = 20;

    // ================================================================
    // 2. 文件系统路径
    // ================================================================

    /// SD 卡挂载点
    const SDCARD_PATH: &'static str;

    /// 平台标识字符串（用于 .system/<PLATFORM>/ 目录名）
    const PLATFORM_TAG: &'static str;

    // ================================================================
    // 3. 按钮映射（SDL keycode → 逻辑按钮）
    // ================================================================

    // 每个平台将物理按键映射到 SDL keycode
    // 对于不使用 SDL 的平台，这些是虚拟 keycode，由 input 层自己模拟

    const KEY_UP: i32;
    const KEY_DOWN: i32;
    const KEY_LEFT: i32;
    const KEY_RIGHT: i32;
    const KEY_SELECT: i32;
    const KEY_START: i32;
    const KEY_A: i32;
    const KEY_B: i32;
    const KEY_X: i32;
    const KEY_Y: i32;
    const KEY_L1: i32;
    const KEY_R1: i32;
    const KEY_MENU: i32;
    const KEY_POWER: i32;
    const KEY_PLUS: i32;
    const KEY_MINUS: i32;

    // 可选按键 —— 有默认值 NA
    const KEY_L2: i32 = NA;
    const KEY_R2: i32 = NA;
    const KEY_L3: i32 = NA;
    const KEY_R3: i32 = NA;
    const KEY_POWER_OFF: i32 = NA;
    const KEY_MENU_ALT: i32 = NA;

    // ================================================================
    // 4. 行为绑定（逻辑按钮 → 功能）
    // ================================================================

    /// 哪个按钮触发从存档恢复（通常是 X）
    const BTN_RESUME: Button = Button::X;

    /// 哪个按钮触发休眠
    const BTN_SLEEP: Button = Button::POWER;

    /// 哪个按钮触发唤醒
    const BTN_WAKE: Button = Button::POWER;

    /// 调节音量的修饰键（与 PLUS/MINUS 组合）
    const BTN_MOD_VOLUME: Button = Button::NONE;

    /// 调节亮度的修饰键（与 PLUS/MINUS 组合，默认 MENU）
    const BTN_MOD_BRIGHTNESS: Button = Button::MENU;

    /// 增加键的绑定
    const BTN_MOD_PLUS: Button = Button::PLUS;

    /// 减少键的绑定
    const BTN_MOD_MINUS: Button = Button::MINUS;

    // ================================================================
    // 5. 设备能力标志
    // ================================================================

    /// 是否有物理电源键
    fn has_power_button(&self) -> bool {
        Self::KEY_POWER != NA
    }

    /// 是否有物理 MENU 键
    fn has_menu_button(&self) -> bool {
        Self::KEY_MENU != NA
    }

    /// 是否有独立的关机键（区别于电源键）
    fn has_poweroff_button(&self) -> bool {
        Self::KEY_POWER_OFF != NA
    }

    /// 是否是窄屏幕设备（宽度 < 320）
    fn has_skinny_screen() -> bool {
        Self::FIXED_WIDTH < 320
    }

    // ================================================================
    // 6. 视频方法
    // ================================================================

    /// 初始化视频系统，返回主帧缓冲区
    ///
    /// 对应 C 中的 `PLAT_initVideo()`
    fn init_video(&mut self) -> Result<Framebuffer, String>;

    /// 销毁视频系统
    fn quit_video(&mut self);

    /// 清除帧缓冲区（填零）
    fn clear_video(&self, fb: &Framebuffer);

    /// 清除所有缓冲区（包括前后台）
    fn clear_all(&mut self);

    /// 设置 VSync 模式
    fn set_vsync(&mut self, mode: VsyncMode);

    /// 调整视频输出尺寸
    fn resize_video(&mut self, w: u32, h: u32, pitch: u32) -> Framebuffer;

    /// 设置视频缩放裁剪区域
    fn set_video_scale_clip(&mut self, x: i32, y: i32, width: i32, height: i32);

    /// 设置最近邻插值（整数缩放时使用，避免模糊）
    fn set_nearest_neighbor(&mut self, enabled: bool);

    /// 设置画面锐度
    fn set_sharpness(&mut self, sharpness: Sharpness);

    /// 设置画面效果（扫描线/CRT网格/无）
    fn set_screen_effect(&mut self, effect: ScreenEffect);

    /// 设置效果叠加颜色
    fn set_effect_color(&mut self, color: u32) {
        // 大多数平台不需要覆写此方法
        let _ = color;
    }

    /// 等待 VSync（垂直同步）
    ///
    /// `remaining` 参数含义因平台而异，通常 0 表示阻塞等待。
    fn vsync_wait(&mut self, remaining: i32);

    /// 翻页 —— 将后台缓冲区显示到屏幕
    ///
    /// 对应 C 中的 `PLAT_flip()`
    fn flip(&mut self, fb: &Framebuffer, sync: bool);

    /// 是否支持过扫描（Overscan）调整
    fn supports_overscan(&self) -> bool {
        false
    }

    /// 获取当前平台对应的硬件缩放器
    fn get_scaler(&self, renderer: &GfxRenderer) -> Option<ScalerFn> {
        // 默认：没有硬件缩放器，使用软件缩放
        let _ = renderer;
        None
    }

    /// 执行硬件位块传输（将渲染结果写入显示缓冲区）
    fn blit_renderer(&self, renderer: &GfxRenderer);

    // ================================================================
    // 7. 覆盖层（Overlay）—— 部分平台支持
    // ================================================================

    /// 初始化硬件覆盖层（用于显示独立于主 framebuffer 的 UI 元素）
    fn init_overlay(&mut self) -> Option<Framebuffer> {
        None
    }

    /// 销毁覆盖层
    fn quit_overlay(&mut self) {}

    /// 启用/禁用覆盖层显示
    fn enable_overlay(&mut self, _enable: bool) {}

    // ================================================================
    // 8. 输入方法
    // ================================================================

    /// 初始化输入设备
    fn init_input(&mut self) -> Result<(), String>;

    /// 关闭输入设备
    fn quit_input(&mut self);

    /// 轮询输入状态，更新 `pad`
    ///
    /// 对应 C 中的 `PLAT_pollInput()`
    fn poll_input(&mut self, pad: &mut PadContext);

    /// 检查是否有唤醒事件（如按下电源键）
    fn should_wake(&self) -> bool {
        false
    }

    // ================================================================
    // 9. 电源和电池方法
    // ================================================================

    /// 获取电池状态
    ///
    /// 返回 `(is_charging, charge_level)`
    /// - `is_charging`: 是否在充电
    /// - `charge_level`: 电量 0/10/20/40/60/80/100
    fn get_battery_status(&self) -> (bool, u8);

    /// 启用/禁用屏幕背光
    fn enable_backlight(&mut self, enable: bool);

    /// 执行关机
    ///
    /// 此方法不应返回。实现应保存状态、静音、关闭背光，然后执行系统关机。
    fn power_off(&self) -> !;

    /// 设置 CPU 频率
    fn set_cpu_speed(&mut self, speed: CpuSpeed);

    /// 设置震动强度（0-65535 映射到 0-100%）
    fn set_rumble(&mut self, strength: i32) {
        let _ = strength;
        // 大多数设备没有震动，默认空实现
    }

    /// 从请求和最大采样率中选择一个平台支持的音频采样率
    fn pick_sample_rate(&self, requested: i32, max: i32) -> i32 {
        requested.min(max)
    }

    // ================================================================
    // 10. 音频方法
    // ================================================================

    /// 初始化音频系统
    ///
    /// `sample_rate`: 采样率（如 44100）
    /// `frame_rate`: 目标帧率（如 60）
    fn init_audio(&mut self, sample_rate: f64, frame_rate: f64) -> Result<(), String>;

    /// 向音频缓冲提交一组采样帧
    ///
    /// 返回实际写入的帧数
    fn batch_samples(&mut self, frames: &[AudioFrame]) -> usize;

    /// 关闭音频系统
    fn quit_audio(&mut self);

    // ================================================================
    // 11. 平台信息
    // ================================================================

    /// 获取设备型号名称（如 "Anbernic RG35XX"）
    fn get_model(&self) -> &str;

    /// 是否有网络连接（WiFi）
    fn is_online(&self) -> bool {
        false // 大多数设备没有 WiFi
    }

    /// 设置系统日期时间
    fn set_date_time(&self, _year: i32, _month: i32, _day: i32,
                     _hour: i32, _min: i32, _sec: i32) -> bool {
        false // 大多数设备没有 RTC
    }

    // ================================================================
    // 12. 翻盖传感器
    // ================================================================

    /// 初始化翻盖传感器（如果设备有）
    fn init_lid(&mut self) -> LidContext {
        LidContext::default()
    }

    /// 检查翻盖状态是否改变
    ///
    /// `state` 参数可选：传入 `Some(&mut val)` 会将当前翻盖状态（打开/合上）
    /// 写入 val，传入 `None` 则仅检查状态是否改变而不读取状态值。
    /// 对应 C 中 `PLAT_lidChanged(int* state)` — state 可以为 NULL。
    fn lid_changed(&self, state: Option<&mut i32>) -> bool {
        let _ = state;
        false
    }

    // ================================================================
    // 13. HDMI 输出（部分设备支持）
    // ================================================================

    /// HDMI 输出时的宽度
    fn hdmi_width() -> u32 {
        Self::FIXED_WIDTH
    }

    /// HDMI 输出时的高度
    fn hdmi_height() -> u32 {
        Self::FIXED_HEIGHT
    }

    /// 检查 HDMI 状态是否改变
    fn hdmi_changed(&self) -> bool {
        false
    }

    /// 当前是否通过 HDMI 输出
    fn is_hdmi(&self) -> bool {
        false
    }
}

// ============================================================================
// 测试用的模拟平台
// ============================================================================

#[cfg(test)]
pub mod test_platform {
    use super::*;

    /// 一个用于测试的假平台实现
    ///
    /// 所有操作在内存中完成，不访问真实硬件。
    pub struct TestPlatform {
        /// 帧缓冲区数据（RGB565 格式，640×480×2 字节）
        pub fb: Vec<u8>,
        /// 模拟的充电状态
        pub battery_charging: bool,
        /// 模拟的电量（0/10/20/40/60/80/100）
        pub battery_level: u8,
        /// 模拟的背光开关状态
        pub backlight_on: bool,
        /// 当前 CPU 速度等级
        pub cpu_speed: CpuSpeed,
        /// 模拟的网络连接状态
        pub online: bool,
        /// 模拟的 HDMI 输出状态
        pub hdmi: bool,
    }

    impl TestPlatform {
        /// 创建一个用于测试的模拟平台实例
        pub fn new() -> Self {
            let fb_size = (640 * 480 * 2) as usize; // RGB565
            Self {
                fb: vec![0u8; fb_size],
                battery_charging: false,
                battery_level: 80,
                backlight_on: true,
                cpu_speed: CpuSpeed::Normal,
                online: false,
                hdmi: false,
            }
        }
    }

    impl Platform for TestPlatform {
        const FIXED_WIDTH: u32 = 640;
        const FIXED_HEIGHT: u32 = 480;
        const FIXED_BPP: u8 = 2;
        const FIXED_SCALE: u32 = 2;
        const SDCARD_PATH: &'static str = "/tmp/test_sdcard";
        const PLATFORM_TAG: &'static str = "test";

        const KEY_UP: i32 = 1;
        const KEY_DOWN: i32 = 2;
        const KEY_LEFT: i32 = 3;
        const KEY_RIGHT: i32 = 4;
        const KEY_SELECT: i32 = 5;
        const KEY_START: i32 = 6;
        const KEY_A: i32 = 7;
        const KEY_B: i32 = 8;
        const KEY_X: i32 = 9;
        const KEY_Y: i32 = 10;
        const KEY_L1: i32 = 11;
        const KEY_R1: i32 = 12;
        const KEY_MENU: i32 = 13;
        const KEY_POWER: i32 = 14;
        const KEY_PLUS: i32 = 15;
        const KEY_MINUS: i32 = 16;

        fn init_video(&mut self) -> Result<Framebuffer, String> {
            Ok(Framebuffer {
                pixels: self.fb.as_mut_ptr(),
                width: Self::FIXED_WIDTH,
                height: Self::FIXED_HEIGHT,
                pitch: Self::fixed_pitch(),
                bpp: Self::FIXED_BPP,
            })
        }

        fn quit_video(&mut self) {}

        fn clear_video(&self, fb: &Framebuffer) {
            unsafe {
                std::ptr::write_bytes(fb.pixels, 0, fb.size());
            }
        }

        fn clear_all(&mut self) {
            self.fb.fill(0);
        }

        fn set_vsync(&mut self, _mode: VsyncMode) {}

        fn resize_video(&mut self, w: u32, h: u32, pitch: u32) -> Framebuffer {
            Framebuffer {
                pixels: self.fb.as_mut_ptr(),
                width: w,
                height: h,
                pitch,
                bpp: Self::FIXED_BPP,
            }
        }

        fn set_video_scale_clip(&mut self, _x: i32, _y: i32, _w: i32, _h: i32) {}
        fn set_nearest_neighbor(&mut self, _enabled: bool) {}
        fn set_sharpness(&mut self, _sharpness: Sharpness) {}
        fn set_screen_effect(&mut self, _effect: ScreenEffect) {}

        fn vsync_wait(&mut self, _remaining: i32) {}

        fn flip(&mut self, _fb: &Framebuffer, _sync: bool) {
            // 测试平台：flip 是空操作
        }

        fn blit_renderer(&self, _renderer: &GfxRenderer) {}

        fn init_input(&mut self) -> Result<(), String> {
            Ok(())
        }

        fn quit_input(&mut self) {}

        fn poll_input(&mut self, _pad: &mut PadContext) {
            // 测试平台：没有输入
        }

        fn get_battery_status(&self) -> (bool, u8) {
            (self.battery_charging, self.battery_level)
        }

        fn enable_backlight(&mut self, enable: bool) {
            self.backlight_on = enable;
        }

        fn power_off(&self) -> ! {
            std::process::exit(0);
        }

        fn set_cpu_speed(&mut self, speed: CpuSpeed) {
            self.cpu_speed = speed;
        }

        fn init_audio(&mut self, _sample_rate: f64, _frame_rate: f64) -> Result<(), String> {
            Ok(())
        }

        fn batch_samples(&mut self, _frames: &[AudioFrame]) -> usize {
            0
        }

        fn quit_audio(&mut self) {}

        fn get_model(&self) -> &str {
            "Test Device"
        }

        fn is_online(&self) -> bool {
            self.online
        }

        fn is_hdmi(&self) -> bool {
            self.hdmi
        }
    }
}
