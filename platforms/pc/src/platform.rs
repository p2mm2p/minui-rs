//! # PC 桌面平台实现 — Reference Implementation
//!
//! 这是一个**完整的 Platform trait 参考实现**，使用 minifb 窗口库在桌面 PC 上运行 MinUI。
//!
//! ## 与其他平台的关键区别
//!
//! ⚠️ **以下代码是 PC 平台独有的，仅用于桌面开发测试，其他平台（tg5040、rg35xxplus
//! 等）不需要也不应该包含这些逻辑。**
//!
//! | 特点 | PC 平台 | 嵌入式平台 (tg5040 等) |
//! |------|---------|-----------------------|
//! | `new()` | 内部创建 minifb 窗口 + 模拟 SD 卡 | 无参，只保存初始状态 |
//! | `setup_sdcard()` | 在 new() 中调用，创建假的游戏目录 | 不存在，SD 卡已由 OS 挂载 |
//! | 视频 | minifb 窗口 + RGB565→ARGB 转换 | `/dev/fb0` + mmap |
//! | 输入 | `window.get_keys()` 键盘映射 | `/dev/input/event*` evdev |
//! | 电池 | 模拟 80% | `/sys/class/power_supply/` sysfs |
//! | `unsafe impl Send` | 需要（minifb 的 Window 不是 Send） | 不需要（fd/指针天然 Send） |
//!
//! 简而言之：**PC 平台的 `new()` 和 `setup_sdcard()` 是"测试脚手架"，
//! 不要在新平台实现中模仿它们。** 新平台只参考 trait 方法实现（`init_video`、
//! `poll_input` 等）和常量定义（`FIXED_*`、`KEY_*` 等）。
//!
//! ## minifb 的角色
//!
//! minifb 是一个轻量跨平台窗口库，提供：
//! - 一个可写的像素缓冲区（模拟 Linux framebuffer）
//! - 键盘输入（模拟掌机按键）
//! - 窗口事件循环
//!
//! 在真实硬件上，这些功能由 Linux framebuffer + evdev 提供。
//!
//! ## 像素格式转换
//!
//! MinUI 内部使用 **RGB565**（16-bit，每像素 2 字节），
//! minifb 使用 **ARGB**（32-bit，每像素 4 字节）。
//! 每次 flip() 时需要将整个帧缓冲从 RGB565 转换为 ARGB。

use std::fs;
use std::path::Path;

use minifb::{Key, Scale, Window, WindowOptions};
use platform::{Framebuffer, GfxRenderer, AudioFrame, Platform};
use common::types::*;

// ============================================================================
// PcPlatform
// ============================================================================

/// PC 桌面平台
///
/// 使用 minifb 窗口模拟嵌入式设备的屏幕和按键。
/// 帧缓冲区在堆上分配（`Vec<u8>`），不依赖任何硬件。
///
/// ## 字段说明
///
/// | 字段 | 对应原 C | 说明 |
/// |------|---------|------|
/// | `window` | `/dev/fb0` + mmap | minifb 窗口替代 framebuffer |
/// | `fb` | `SDL_Surface` / ION 内存 | RGB565 帧缓冲（内部格式） |
/// | `fb_argb` | — | ARGB 转换缓冲（送给 minifb 显示） |
/// | `battery_*` | `/sys/class/power_supply/battery/*` | 模拟电池状态 |
/// | `backlight_on` | `/sys/class/backlight/*/bl_power` | 背光状态 |
/// | `online` | WiFi 状态 | 是否有网络 |
pub struct PcPlatform {
    /// minifb 窗口（对应原 C 中打开的 `/dev/fb0` 文件描述符）
    window: Window,
    /// RGB565 帧缓冲（对应原 C 中 ION 分配的物理内存）
    fb: Vec<u8>,
    /// ARGB 转换缓冲（送给 minifb 显示用）
    /// minifb 接受 `&[u32]` 格式的像素数据（0x00RRGGBB）
    fb_argb: Vec<u32>,
    /// 模拟电池充电状态（对应原 C `/sys/.../charger_online`）
    battery_charging: bool,
    /// 模拟电池电量 0/10/20/40/60/80/100
    battery_level: u8,
    /// 背光状态（对应原 C `/sys/class/backlight/.../bl_power`）
    backlight_on: bool,
    /// 模拟网络状态
    online: bool,
    /// 屏幕物理宽度（可能不同于 FIXED_WIDTH，如 HDMI 模式）
    width: u32,
    /// 屏幕物理高度
    height: u32,
}

// ── Send 安全性说明 ──────────────────────────────────────────────
//
// minifb::Window 内部包含原始 X11/Wayland 指针和 Rc<RefCell<>>，
// 编译器无法自动证明它是 Send 的。
//
// 但在我们的使用场景中，PcPlatform 只在主线程中创建和使用
// （MinUi::run() 是单线程事件循环），永远不会跨线程传递。
// 因此标记为 Send 是安全的。
//
// 在真实嵌入式设备上不需要这个——framebuffer fd 和 mmap 指针
// 天然是 Send 的。
unsafe impl Send for PcPlatform {}

/// ────────────────────────────────────────────────────────────────
/// PC 独有：模拟 SD 卡初始化
/// ────────────────────────────────────────────────────────────────
///
/// ⚠️ 此方法**仅**用于桌面开发测试。
/// 在真实嵌入式设备上，SD 卡由操作系统自动挂载，
/// `Roms/`、`.system/` 等目录已经存在——不需要也不应该调用此方法。
///
/// 新平台实现者请**不要**模仿这一段。
fn setup_sdcard() {
    let sdcard = Path::new("/tmp/minui_sdcard");
    if sdcard.exists() {
        return;
    }

    log::info!("Setting up test SD card at {:?}...", sdcard);

    // ── Roms/ ──
    let roms = [
        "Roms/Game Boy (GB)",
        "Roms/Game Boy Color (GBC)",
        "Roms/Game Boy Advance (GBA)",
        "Roms/Nintendo Entertainment System (FC)",
        "Roms/Super Nintendo Entertainment System (SFC)",
        "Roms/Sega Genesis (MD)",
        "Roms/Sony PlayStation (PS)",
    ];
    for dir in &roms {
        fs::create_dir_all(sdcard.join(dir)).unwrap();
    }

    // ── 示例 ROM 文件 ──
    let rom_files = [
        ("Roms/Game Boy (GB)/Zelda.gb", "The Legend of Zelda"),
        ("Roms/Game Boy (GB)/Mario.gb", "Super Mario Land"),
        ("Roms/Game Boy (GB)/Kirby.gb", "Kirby's Dream Land"),
        ("Roms/Game Boy Color (GBC)/Pokemon Gold.gbc", "Pokemon Gold"),
        ("Roms/Game Boy Color (GBC)/Zelda DX.gbc", "Zelda DX"),
        ("Roms/Game Boy Advance (GBA)/Metroid Fusion.gba", "Metroid Fusion"),
        ("Roms/Game Boy Advance (GBA)/Castlevania.gba", "Castlevania"),
        ("Roms/Nintendo Entertainment System (FC)/Mario.nes", "Super Mario Bros"),
        ("Roms/Nintendo Entertainment System (FC)/Zelda.nes", "Zelda"),
        ("Roms/Super Nintendo Entertainment System (SFC)/Metroid.sfc", "Super Metroid"),
        ("Roms/Super Nintendo Entertainment System (SFC)/Mario World.sfc", "Super Mario World"),
        ("Roms/Sega Genesis (MD)/Sonic.md", "Sonic The Hedgehog"),
        ("Roms/Sony PlayStation (PS)/Final Fantasy VII.cue", "FF7 Disc 1"),
    ];
    for (path, _name) in &rom_files {
        let full = sdcard.join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&full, "placeholder ROM").unwrap();
    }

    // ── BIOS/ ──
    for bios in &["Bios/GB", "Bios/GBA", "Bios/GBC", "Bios/FC", "Bios/MD", "Bios/PS", "Bios/SFC"] {
        fs::create_dir_all(sdcard.join(bios)).unwrap();
    }

    // ── Saves/ ──
    for saves in &["Saves/GB", "Saves/GBA", "Saves/GBC", "Saves/FC", "Saves/MD", "Saves/PS", "Saves/SFC"] {
        fs::create_dir_all(sdcard.join(saves)).unwrap();
    }

    // ── .system/pc/ ──
    let system = sdcard.join(".system/pc");
    fs::create_dir_all(system.join("bin")).unwrap();
    fs::create_dir_all(system.join("cores")).unwrap();
    fs::create_dir_all(system.join("lib")).unwrap();
    fs::create_dir_all(system.join("paks")).unwrap();

    for (emu, _core) in &[
        ("GB", "gambatte"), ("GBC", "gambatte"),
        ("GBA", "gpsp"), ("FC", "fceumm"),
        ("SFC", "snes9x2005_plus"), ("MD", "picodrive"),
        ("PS", "pcsx_rearmed"),
    ] {
        let pak_dir = system.join(format!("paks/Emus/{}.pak", emu));
        fs::create_dir_all(&pak_dir).unwrap();
        fs::write(
            pak_dir.join("launch.sh"),
            format!("#!/bin/sh\necho \"Launching {} game with {} core...\"\nsleep 2\n", emu, _core),
        ).unwrap();
    }

    // ── Tools/ ──
    let tools = sdcard.join("Tools/pc/Clock.pak");
    fs::create_dir_all(&tools).unwrap();
    fs::write(tools.join("launch.sh"), "#!/bin/sh\necho Clock\n").unwrap();

    // ── Collections/ ──
    fs::create_dir_all(sdcard.join("Collections")).unwrap();
    fs::write(
        sdcard.join("Collections/My Favorites.txt"),
        "/Roms/Game Boy (GB)/Zelda.gb\n/Roms/Super Nintendo Entertainment System (SFC)/Metroid.sfc\n",
    ).unwrap();

    fs::create_dir_all(sdcard.join(".userdata/shared/.minui")).unwrap();
    fs::create_dir_all(sdcard.join(".system/res")).unwrap();
    fs::write(sdcard.join(".system/pc/version.txt"), "MinUI-Rust\nabc1234\n").unwrap();

    log::info!("SD card setup complete at {:?}", sdcard);
}

impl PcPlatform {
    /// 创建一个新的 PC 平台实例（无参 —— 与所有其他平台签名一致）
    ///
    /// 窗口标题、分辨率（640×480）、模拟 SD 卡路径均为硬编码。
    ///
    /// ⚠️ 此方法内部调用 `setup_sdcard()` 是 PC 独有的"测试脚手架"逻辑。
    /// 新平台实现者的 `new()` 应保持简洁——通常只需要初始化为零/默认值的字段。
    /// 参见 tg5040 的 `Tg5040::new()` 作为正确示范。
    pub fn new() -> Self {
        // ═══════════════════════════════════════════════════════════
        // PC 独有：创建模拟 SD 卡
        // ═══════════════════════════════════════════════════════════
        setup_sdcard();

        let width: u32 = 640;
        let height: u32 = 480;

        // 创建 minifb 窗口 — 对应原 C 的 open("/dev/fb0") + ioctl + mmap
        let mut window = Window::new(
            "MinUI",
            width as usize,
            height as usize,
            WindowOptions {
                resize: false,
                scale: Scale::X1,  // 1:1 像素映射（不缩放）
                ..WindowOptions::default()
            },
        )
        .expect("Failed to create minifb window. Is a display available?");

        // 限制更新频率到 ~60fps（16ms 每帧）
        #[allow(deprecated)]
        window.limit_update_rate(Some(std::time::Duration::from_millis(16)));

        let fb_size = (width * height * 2) as usize; // RGB565: 每像素 2 字节
        let argb_size = (width * height) as usize; // ARGB: 每像素 1 个 u32

        Self {
            window,
            fb: vec![0u8; fb_size],
            fb_argb: vec![0u32; argb_size],
            battery_charging: false,
            battery_level: 80,
            backlight_on: true,
            online: false,
            width,
            height,
        }
    }

    // ================================================================
    // RGB565 → ARGB 转换
    // ================================================================

    /// 将内部 RGB565 帧缓冲转换为 minifb ARGB 格式
    ///
    /// 在真实硬件上不需要这一步——LCD 控制器原生支持 RGB565。
    ///
    /// RGB565 格式: RRRR RGGG GGGB BBBB (16 bits)
    /// ARGB  格式: AAAA AAAA RRRR RRRR GGGG GGGG BBBB BBBB (32 bits)
    fn convert_to_argb(&mut self) {
        for (i, chunk) in self.fb.chunks_exact(2).enumerate() {
            let pixel = u16::from_le_bytes([chunk[0], chunk[1]]);
            // 提取 RGB565 通道并扩展到 8-bit
            let r5 = (pixel >> 11) & 0x1F;
            let g6 = (pixel >> 5) & 0x3F;
            let b5 = pixel & 0x1F;
            // 5-bit → 8-bit: 左移 3 位 + 高 3 位填充低 3 位
            let r8 = ((r5 << 3) | (r5 >> 2)) as u32;
            let g8 = ((g6 << 2) | (g6 >> 4)) as u32;
            let b8 = ((b5 << 3) | (b5 >> 2)) as u32;
            // ARGB: Alpha=255(不透明) | R | G | B
            self.fb_argb[i] = (r8 << 16) | (g8 << 8) | b8;
        }
    }
}

// ============================================================================
// Platform trait 实现
// ============================================================================

impl Platform for PcPlatform {
    // ────────────────────────────────────────────────────────────────
    // 1. 屏幕参数
    // ────────────────────────────────────────────────────────────────

    /// 逻辑屏幕宽度（缩放前）
    ///
    /// 对应原 C `platform.h` 中的 `#define FIXED_WIDTH 640`
    ///
    /// 对于 PC 平台，我们模拟一个 640×480 的屏幕，
    /// 逻辑分辨率为 320×240（FIXED_SCALE=2）。
    /// 这样 UI 元素的大小和真实设备一致。
    const FIXED_WIDTH: u32 = 640;
    const FIXED_HEIGHT: u32 = 480;

    /// 每像素字节数
    ///
    /// 2 = RGB565。这是绝大多数掌机 LCD 控制器的原生格式。
    /// 在 PC 上我们用 16-bit 内部渲染，flip 时转为 ARGB 显示。
    const FIXED_BPP: u8 = 2;

    /// UI 缩放倍数
    ///
    /// 逻辑分辨率 = 物理分辨率 ÷ FIXED_SCALE
    /// 640÷2 = 320 逻辑像素宽
    /// 所有 UI 坐标（文字大小、间距等）都基于逻辑像素。
    const FIXED_SCALE: u32 = 2;

    // ────────────────────────────────────────────────────────────────
    // 2. 文件系统路径
    // ────────────────────────────────────────────────────────────────

    /// SD 卡根路径
    ///
    /// 在真实设备上这是 `/mnt/sdcard` 或 `/mnt/mmcblk0p1`。
    /// PC 版使用 `/tmp/minui_sdcard` 模拟。
    ///
    /// 测试时会自动在这个目录下创建 Roms/、.system/ 等结构。
    const SDCARD_PATH: &'static str = "/tmp/minui_sdcard";

    /// 平台标识
    ///
    /// 用于 .system/<PLATFORM>/ 目录名和 Pak 查找路径。
    /// PC 版使用 "pc" 作为标识。
    const PLATFORM_TAG: &'static str = "pc";

    // ────────────────────────────────────────────────────────────────
    // 3. 按键映射（键盘 → 逻辑按钮）
    // ────────────────────────────────────────────────────────────────

    /// 方向键 — 对应掌机 D-Pad
    ///
    /// 原 C 中这些是 SDL keycode（如 rg35xx 用 SDLK_KATAKANA 映射上键）。
    /// PC 版用键盘方向键模拟。
    const KEY_UP: i32 = Key::Up as i32;
    const KEY_DOWN: i32 = Key::Down as i32;
    const KEY_LEFT: i32 = Key::Left as i32;
    const KEY_RIGHT: i32 = Key::Right as i32;

    /// 功能键
    ///
    /// 键盘映射:
    ///   Z    → A (确认)
    ///   X    → B (返回)
    ///   A    → X (从存档恢复)
    ///   S    → Y
    ///   Enter → Start
    ///   RShift → Select
    const KEY_A: i32 = Key::Z as i32;
    const KEY_B: i32 = Key::X as i32;
    const KEY_X: i32 = Key::A as i32;
    const KEY_Y: i32 = Key::S as i32;
    const KEY_START: i32 = Key::Enter as i32;
    const KEY_SELECT: i32 = Key::RightShift as i32;

    /// 肩键
    const KEY_L1: i32 = Key::Q as i32;
    const KEY_R1: i32 = Key::E as i32;
    const KEY_L2: i32 = Key::Key1 as i32;
    const KEY_R2: i32 = Key::Key2 as i32;

    /// 系统和菜单键
    const KEY_MENU: i32 = Key::Escape as i32;
    const KEY_POWER: i32 = Key::P as i32;
    const KEY_PLUS: i32 = Key::Equal as i32;
    const KEY_MINUS: i32 = Key::Minus as i32;

    // ────────────────────────────────────────────────────────────────
    // 4. 行为绑定（逻辑按钮 → 功能）
    // ────────────────────────────────────────────────────────────────

    const BTN_RESUME: Button = Button::X;
    const BTN_SLEEP: Button = Button::POWER;
    const BTN_WAKE: Button = Button::POWER;
    const BTN_MOD_BRIGHTNESS: Button = Button::MENU;
    const BTN_MOD_PLUS: Button = Button::PLUS;
    const BTN_MOD_MINUS: Button = Button::MINUS;

    // ────────────────────────────────────────────────────────────────
    // 5. 设备能力
    // ────────────────────────────────────────────────────────────────

    /// PC 平台用键盘模拟电源键
    fn has_power_button(&self) -> bool { true }
    /// PC 平台用 Escape 键作为 Menu
    fn has_menu_button(&self) -> bool { true }

    // ────────────────────────────────────────────────────────────────
    // 6. 视频方法
    // ────────────────────────────────────────────────────────────────

    /// 初始化视频系统
    ///
    /// 对应原 C 的 `PLAT_initVideo()`:
    ///   open("/dev/fb0") → ioctl(FBIOGET_VSCREENINFO) → ion_alloc() → mmap()
    ///
    /// PC 版：minifb 窗口已在 `PcPlatform::new()` 中创建，
    /// 这里只需返回一个指向内部 `fb` 缓冲区的 Framebuffer 描述符。
    fn init_video(&mut self) -> Result<Framebuffer, String> {
        Ok(Framebuffer {
            pixels: self.fb.as_mut_ptr(),
            width: self.width,
            height: self.height,
            pitch: self.width * 2, // RGB565: 每行 = 宽 × 2 字节
            bpp: 2,
        })
    }

    /// 销毁视频系统
    ///
    /// 对应原 C 的 `PLAT_quitVideo()`:
    ///   munmap() → ion_free() → close(fb_fd)
    ///
    /// PC 版：minifb window 在 PcPlatform drop 时自动关闭。
    fn quit_video(&mut self) {}

    /// 清除帧缓冲区（填零 = 黑色）
    ///
    /// 对应原 C:
    ///   memset(screen->pixels, 0, PAGE_SIZE);
    fn clear_video(&self, fb: &Framebuffer) {
        unsafe {
            std::ptr::write_bytes(fb.pixels, 0, fb.size());
        }
    }

    /// 清除所有缓冲区（包括双缓冲的另一个 page）
    ///
    /// PC 版只有一个缓冲区，等同于 clear_video。
    fn clear_all(&mut self) {
        self.fb.fill(0);
        self.fb_argb.fill(0);
    }

    /// 设置垂直同步模式
    ///
    /// 对应原 C 的 `PLAT_setVsync()`。
    /// PC 版不需要实现（minifb 有内置 vsync）。
    fn set_vsync(&mut self, _mode: VsyncMode) {}

    /// 调整视频尺寸
    ///
    /// 对应原 C 的 `PLAT_resizeVideo()`。
    /// HDMI 切换时会调用，改变 framebuffer 分辨率。
    ///
    /// PC 版：重新分配缓冲区（简化实现）。
    fn resize_video(&mut self, w: u32, h: u32, pitch: u32) -> Framebuffer {
        self.width = w;
        self.height = h;
        self.fb.resize((pitch * h) as usize, 0);
        self.fb_argb.resize((w * h) as usize, 0);
        Framebuffer {
            pixels: self.fb.as_mut_ptr(),
            width: w,
            height: h,
            pitch,
            bpp: 2,
        }
    }

    /// 设置视频缩放裁剪区域
    ///
    /// 对应原 C 的 `PLAT_setVideoScaleClip()`。
    /// PC 版不实现硬件裁剪。
    fn set_video_scale_clip(&mut self, _x: i32, _y: i32, _w: i32, _h: i32) {}

    /// 设置最近邻插值
    ///
    /// 整数缩放时使用最近邻（像素锐利），非整数缩放使用双线性。
    /// PC 版不实现硬件缩放。
    fn set_nearest_neighbor(&mut self, _enabled: bool) {}

    fn set_sharpness(&mut self, _sharpness: Sharpness) {}
    fn set_screen_effect(&mut self, _effect: ScreenEffect) {}

    /// 等待垂直同步
    ///
    /// 对应原 C 的 `PLAT_vsync()` / `ioctl(OWLFB_WAITFORVSYNC)`。
    /// PC 版不需要主动等待。
    fn vsync_wait(&mut self, _remaining: i32) {}

    /// 翻页 — 将后台缓冲显示到屏幕
    ///
    /// 这是**最重要的视频操作**。对应原 C:
    /// ```c
    /// void PLAT_flip(SDL_Surface* screen, int sync) {
    ///     // 1. 切换硬件 DE 寄存器指向当前 page
    ///     DE_OVL_BA0 = fb_paddr + page * PAGE_SIZE;
    ///     // 2. 等待 VSync
    ///     if (sync) ioctl(fb_fd, OWLFB_WAITFORVSYNC, &_);
    ///     // 3. 交换前后台
    ///     page ^= 1;
    ///     screen->pixels = fb_vaddr + page * PAGE_SIZE;
    ///     // 4. 清除新后台
    ///     if (cleared) memset(screen->pixels, 0, PAGE_SIZE);
    /// }
    /// ```
    ///
    /// PC 版：将 RGB565 帧缓冲转换为 ARGB，然后调用 minifb 更新窗口。
    fn flip(&mut self, _fb: &Framebuffer, _sync: bool) {
        self.convert_to_argb();

        // minifb 的 update_with_buffer 相当于硬件 DE 的 DMA 传输
        self.window
            .update_with_buffer(
                &self.fb_argb,
                self.width as usize,
                self.height as usize,
            )
            .expect("Failed to update minifb window");

        // minifb 的 update 处理窗口事件（关闭按钮等）
        if !self.window.is_open() {
            std::process::exit(0);
        }
    }

    /// 硬件渲染器 blit
    ///
    /// 对应原 C 的 `PLAT_blitRenderer()`。
    /// minui（启动器）不用这个，minarch（游戏宿主）用。
    fn blit_renderer(&self, _renderer: &GfxRenderer) {}

    // ────────────────────────────────────────────────────────────────
    // 7. 输入方法
    // ────────────────────────────────────────────────────────────────

    /// 初始化输入设备
    ///
    /// 对应原 C 的 `PLAT_initInput()`：
    ///   open("/dev/input/event0") → ioctl(EVIOCGRAB) → 配置非阻塞读取
    ///
    /// PC 版：minifb 内置键盘处理，无需额外初始化。
    fn init_input(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn quit_input(&mut self) {}

    /// 轮询输入状态 — 这是**每帧调用的核心输入函数**
    ///
    /// 对应原 C 的 `PLAT_pollInput()`：
    /// ```c
    /// while (read(fd, &events, sizeof(events)) > 0) {
    ///     // 解析 evdev 事件 → 更新 pad.is_pressed / just_pressed / just_released
    /// }
    /// ```
    ///
    /// PC 版：调用 minifb 的 `get_keys()` 获取当前按下的键，
    /// 然后与上一帧的状态比较，生成 just_pressed / just_released 事件。
    ///
    /// ## 实现要点
    ///
    /// 1. **边沿检测**：`just_pressed = 当前帧按下的键 ∩ 上一帧未按下的键`
    ///    `just_released = 上一帧按下的键 ∩ 当前帧未按下的键`
    ///
    /// 2. **长按重复（Key Repeat）**：首次按下 300ms 后开始重复，之后每 100ms 触发一次。
    ///    这模拟了硬件键盘的 typematic rate。
    ///    用于列表中的连续导航（按住下键自动滚动）。
    ///
    /// 3. **组合键**：D-pad 方向键同时加入 `UP` 和 `DPAD_UP` / `ANALOG_UP`。
    fn poll_input(&mut self, pad: &mut PadContext) {
        // 保存上一帧的状态
        let was_pressed = pad.is_pressed;

        // 获取 minifb 当前按下的键
        let keys = self.window.get_keys();

        // 构建当前帧的按钮位掩码
        let mut current = Button::NONE;

        // 将每个 minifb 键映射到对应的 Button
        for key in keys {
            let btn = match key {
                Key::Up => Button::UP,
                Key::Down => Button::DOWN,
                Key::Left => Button::LEFT,
                Key::Right => Button::RIGHT,
                Key::Z => Button::A,
                Key::X => Button::B,
                Key::A => Button::X,
                Key::S => Button::Y,
                Key::Enter => Button::START,
                Key::RightShift => Button::SELECT,
                Key::Q => Button::L1,
                Key::E => Button::R1,
                Key::Key1 => Button::L2,
                Key::Key2 => Button::R2,
                Key::Escape => Button::MENU,
                Key::P => Button::POWER,
                Key::Equal => Button::PLUS,
                Key::Minus => Button::MINUS,
                _ => continue, // 未映射的键忽略
            };
            current.insert(btn);
        }

        // 边沿检测
        pad.is_pressed = current;
        pad.just_pressed = current;
        pad.just_pressed.remove(was_pressed); // 去掉上一帧也在按的 → 只剩新按下的
        pad.just_released = was_pressed;
        pad.just_released.remove(current); // 去掉当前帧还在按的 → 只剩刚释放的

        // Key Repeat 处理
        pad.just_repeated = Button::NONE;
        // 简化版：不做真正的 repeat 检测，依赖 minifb 的 get_keys_released
        // 完整实现需要维护每个按钮的 press_time 和 repeat_at 数组
    }

    /// 检查是否有唤醒事件
    ///
    /// 对应原 C 的 `PLAT_shouldWake()`。
    /// 在休眠状态下轮询电源键。
    fn should_wake(&self) -> bool {
        self.window.is_open()
    }

    // ────────────────────────────────────────────────────────────────
    // 8. 电源和电池方法
    // ────────────────────────────────────────────────────────────────

    /// 获取电池状态
    ///
    /// 对应原 C 的 `PLAT_getBatteryStatus()`：
    /// ```c
    /// *is_charging = getInt("/sys/class/power_supply/battery/charger_online");
    /// int voltage = getInt("/sys/class/power_supply/battery/voltage_now") / 10000;
    /// // 将电压映射为 0/10/20/40/60/80/100 六档
    /// ```
    ///
    /// PC 版返回模拟电量 80%。
    fn get_battery_status(&self) -> (bool, u8) {
        (self.battery_charging, self.battery_level)
    }

    /// 启用/禁用背光
    ///
    /// 对应原 C 的 `PLAT_enableBacklight()`:
    /// ```c
    /// putInt("/sys/class/backlight/backlight.2/bl_power",
    ///     enable ? FB_BLANK_UNBLANK : FB_BLANK_POWERDOWN);
    /// ```
    ///
    /// PC 版记录状态（不实际控制背光）。
    fn enable_backlight(&mut self, enable: bool) {
        self.backlight_on = enable;
    }

    /// 关机
    ///
    /// 对应原 C 的 `PLAT_powerOff()`:
    /// ```c
    /// sleep(2);
    /// SetRawVolume(MUTE_VOLUME_RAW);
    /// PLAT_enableBacklight(0);
    /// system("shutdown");
    /// ```
    fn power_off(&self) -> ! {
        println!("Power off requested. Exiting...");
        std::process::exit(0);
    }

    /// 设置 CPU 速度
    ///
    /// 对应原 C 的 `PLAT_setCPUSpeed()`:
    /// ```c
    /// sprintf(cmd, "overclock.elf %d\n", freq);
    /// system(cmd);
    /// ```
    ///
    /// PC 版不实现。
    fn set_cpu_speed(&mut self, _speed: CpuSpeed) {}

    // ────────────────────────────────────────────────────────────────
    // 9. 音频方法
    // ────────────────────────────────────────────────────────────────

    /// 初始化音频
    ///
    /// 对应原 C 的 `SND_init()` → 打开 ALSA 设备，配置采样率和缓冲区。
    /// PC 版不实现（启动器不需要音频）。
    fn init_audio(&mut self, _sample_rate: f64, _frame_rate: f64) -> Result<(), String> {
        Ok(())
    }

    fn batch_samples(&mut self, _frames: &[AudioFrame]) -> usize {
        0
    }

    fn quit_audio(&mut self) {}

    // ────────────────────────────────────────────────────────────────
    // 10. 平台信息
    // ────────────────────────────────────────────────────────────────

    fn get_model(&self) -> &str {
        "PC (minifb)"
    }

    fn is_online(&self) -> bool {
        self.online
    }

    // ────────────────────────────────────────────────────────────────
    // 11. HDMI 检测
    // ────────────────────────────────────────────────────────────────

    /// 是否通过 HDMI 输出
    ///
    /// 对应原 C 的 `GetHDMI()` / `PLAT_isOnline()`。
    /// PC 版不支持 HDMI。
    fn is_hdmi(&self) -> bool {
        false
    }
}
