//! # TG5040 平台实现
//!
//! 覆盖: Trimui Smart Pro (1280×720, scale=2) 和 Trimui Brick (1024×768, scale=3)
//! SoC: Allwinner TG5040
//!
//! ## 状态
//! 🟡 开发中 — 常量已定义，IO 方法为最小存根。

use std::fs;
use std::io;

use minui_platform::*;
use common::types::*;

mod libmsettings;

// ============================================================================
// 平台常量 — evdev 扫描码
// ============================================================================

/// Smart Pro 的原始按键扫描码（来自 /dev/input/event* 的 evdev code）
mod evdev {
    pub const UP: u16     = 13;   // JOY_UP
    pub const DOWN: u16   = 16;   // JOY_DOWN
    pub const LEFT: u16   = 14;   // JOY_LEFT
    pub const RIGHT: u16  = 15;   // JOY_RIGHT
    pub const A: u16      = 1;    // JOY_A (注意: Smart Pro 上 A/B 是反的!)
    pub const B: u16      = 0;    // JOY_B
    pub const X: u16      = 3;    // JOY_X
    pub const Y: u16      = 2;    // JOY_Y
    pub const START: u16  = 7;    // JOY_START
    pub const SELECT: u16 = 6;    // JOY_SELECT
    pub const MENU: u16   = 8;    // JOY_MENU
    pub const L1: u16     = 4;    // JOY_L1
    pub const R1: u16     = 5;    // JOY_R1
    // L2/R2 通过 AXIS（模拟触发）
    pub const POWER: u16  = 116;  // BUTTON_POWER
    pub const PLUS: u16   = 128;  // JOY_PLUS
    pub const MINUS: u16  = 129;  // JOY_MINUS

    // 摇杆轴
    pub const LS_X: u16   = 0;    // AXIS_LX (ABS_X)
    pub const LS_Y: u16   = 1;    // AXIS_LY (ABS_Y)
    pub const RS_X: u16   = 3;    // AXIS_RX
    pub const RS_Y: u16   = 4;    // AXIS_RY
    pub const L2_AXIS: u16 = 2;   // AXIS_L2 (ABSZ, 模拟触发)
    pub const R2_AXIS: u16 = 5;   // AXIS_R2 (RABSZ)
}

// ============================================================================
// 平台结构体
// ============================================================================

pub struct Tg5040 {
    /// 帧缓冲区
    fb_fd: Option<std::fs::File>,
    fb_pixels: *mut u8,
    fb_width: u32,
    fb_height: u32,
    fb_pitch: u32,
    fb_page: u32,

    /// 输入设备（原始 fd，以 O_NONBLOCK 打开）
    input_fds: Vec<i32>,

    /// 设置
    settings: libmsettings::Settings,

    /// 是否是 Brick 变体 (1024×768)
    is_brick: bool,
}

impl Tg5040 {
    pub fn new() -> Self {
        let settings = libmsettings::Settings::new();

        // TODO: 运行时检测 Brick 变体
        let is_brick = false;

        Self {
            fb_fd: None,
            fb_pixels: std::ptr::null_mut(),
            fb_width: 0,
            fb_height: 0,
            fb_pitch: 0,
            fb_page: 0,
            input_fds: Vec::new(),
            settings,
            is_brick,
        }
    }
}

impl Platform for Tg5040 {
    // ── 屏幕参数 ──
    const FIXED_WIDTH: u32 = 1280;
    const FIXED_HEIGHT: u32 = 720;
    const FIXED_BPP: u8 = 2;
    const FIXED_SCALE: u32 = 2;

    const MAIN_ROW_COUNT: usize = 8;
    const PADDING: u32 = 40;

    // ── 路径 ──
    const SDCARD_PATH: &'static str = "/mnt/SDCARD";
    const PLATFORM_TAG: &'static str = "tg5040";

    // ── 按键映射 ──
    const KEY_UP: i32 = evdev::UP as i32;
    const KEY_DOWN: i32 = evdev::DOWN as i32;
    const KEY_LEFT: i32 = evdev::LEFT as i32;
    const KEY_RIGHT: i32 = evdev::RIGHT as i32;
    const KEY_SELECT: i32 = evdev::SELECT as i32;
    const KEY_START: i32 = evdev::START as i32;
    const KEY_A: i32 = evdev::A as i32;
    const KEY_B: i32 = evdev::B as i32;
    const KEY_X: i32 = evdev::X as i32;
    const KEY_Y: i32 = evdev::Y as i32;
    const KEY_L1: i32 = evdev::L1 as i32;
    const KEY_R1: i32 = evdev::R1 as i32;
    const KEY_L2: i32 = NA;       // 模拟触发，不通过 KEY 常量暴露
    const KEY_R2: i32 = NA;
    const KEY_L3: i32 = NA;       // Smart Pro 无 L3/R3（Brick 有，后续适配）
    const KEY_R3: i32 = NA;
    const KEY_MENU: i32 = evdev::MENU as i32;
    const KEY_POWER: i32 = evdev::POWER as i32;
    const KEY_PLUS: i32 = evdev::PLUS as i32;
    const KEY_MINUS: i32 = evdev::MINUS as i32;

    // ── 行为绑定 ──
    const BTN_RESUME: Button = Button::X;
    const BTN_SLEEP: Button = Button::POWER;
    const BTN_WAKE: Button = Button::POWER;
    const BTN_MOD_BRIGHTNESS: Button = Button::MENU;
    const BTN_MOD_PLUS: Button = Button::PLUS;
    const BTN_MOD_MINUS: Button = Button::MINUS;

    // ── 能力 ──
    fn has_power_button(&self) -> bool { true }
    fn has_menu_button(&self) -> bool { true }

    // ── 视频 ──
    fn init_video(&mut self) -> Result<Framebuffer, String> {
        // TODO: /dev/fb0 → mmap → ION 双缓冲
        Err("init_video not yet implemented for tg5040".into())
    }
    fn quit_video(&mut self) {}
    fn clear_video(&self, _fb: &Framebuffer) {}
    fn clear_all(&mut self) {}
    fn set_vsync(&mut self, _mode: VsyncMode) {}
    fn resize_video(&mut self, w: u32, h: u32, pitch: u32) -> Framebuffer {
        Framebuffer { pixels: self.fb_pixels, width: w, height: h, pitch, bpp: Self::FIXED_BPP }
    }
    fn set_video_scale_clip(&mut self, _x: i32, _y: i32, _w: i32, _h: i32) {}
    fn set_nearest_neighbor(&mut self, _enabled: bool) {}
    fn set_sharpness(&mut self, _sharpness: Sharpness) {}
    fn set_screen_effect(&mut self, _effect: ScreenEffect) {}
    fn vsync_wait(&mut self, _remaining: i32) {}
    fn flip(&mut self, _fb: &Framebuffer, _sync: bool) {
        // TODO: 写 DE 寄存器切换显示页
    }
    fn blit_renderer(&self, _renderer: &GfxRenderer) {}

    // ── 输入 ──
    fn init_input(&mut self) -> Result<(), String> {
        // TODO: 非阻塞打开 /dev/input/event0, event1
        Ok(())
    }
    fn quit_input(&mut self) {
        for fd in self.input_fds.drain(..) {
            unsafe { libc::close(fd); }
        }
    }
    fn poll_input(&mut self, _pad: &mut PadContext) {
        // TODO: 读取 evdev 事件 → 更新 pad（含 AXIS L2/R2 模拟触发）
    }

    // ── 电源 ──
    fn get_battery_status(&self) -> (bool, u8) {
        // TODO: 读取 /sys/class/power_supply/
        (false, 80)
    }
    fn enable_backlight(&mut self, enable: bool) {
        let _ = enable;
        // TODO: 写 /sys/class/backlight/
    }
    fn power_off(&self) -> ! {
        // TODO: 系统关机
        unsafe { libc::sync(); }
        loop { unsafe { libc::sleep(1); } }
    }
    fn set_cpu_speed(&mut self, speed: CpuSpeed) {
        let _ = speed;
        // TODO: 写 /sys/devices/system/cpu/cpu0/cpufreq/scaling_setspeed
    }

    // ── 音频 ──
    fn init_audio(&mut self, _sample_rate: f64, _frame_rate: f64) -> Result<(), String> { Ok(()) }
    fn batch_samples(&mut self, _frames: &[AudioFrame]) -> usize { 0 }
    fn quit_audio(&mut self) {}
    fn pick_sample_rate(&self, requested: i32, max: i32) -> i32 { requested.min(max) }

    // ── 信息 ──
    fn get_model(&self) -> &str { "Trimui Smart Pro" }
    fn is_hdmi(&self) -> bool { false }
    fn set_date_time(&self, y: i32, m: i32, d: i32, h: i32, min: i32, s: i32) -> bool {
        let cmd = format!("date -s '{}-{}-{} {}:{}:{}'", y, m, d, h, min, s);
        std::process::Command::new("sh").arg("-c").arg(cmd).output().is_ok()
    }
}
