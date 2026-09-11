//! `platform-tg5040` — TrimUI Smart Pro / Brick 平台实现
//!
//! 本 crate 为 tg5040 平台提供 `Platform` trait 的具体实现，
//! 内部使用 SDL2 进行视频渲染、输入轮询和音频播放。
//!
//! ## 目标设备
//!
//! - TrimUI Smart Pro（`smart` feature）
//! - TrimUI Brick（`brick` feature）
//!
//! 两个设备共享同一芯片平台（tg5040），差异**在编译期**通过 Cargo feature
//! 区分（分辨率、布局、输入映射、LED 路径等），不拆分为两个平台 crate。
//! `smart`/`brick` 互斥且必选（无默认）——由本文件顶部的 `compile_error!`
//! 断言强制，设备选择必须显式。
//!
//! ## 与原 C 代码的对比
//!
//! 原版 `workspace/tg5040/platform/` 通过 `#define` 宏定义按键码、
//! 屏幕尺寸等硬件参数，设备差异用运行时 `is_brick` 全局变量 +
//! `getenv("DEVICE")` 环境变量检测（一个二进制服务两台设备）。
//!
//! Rust 版用编译期 feature（`#[cfg(feature = "brick")]`）替代——
//! 每个设备编译出独立的二进制。详见 README「设备区分机制：运行时 vs 编译期」。
//!
//! ## SDL 初始化策略
//!
//! `Tg5040::new()` 零 SDL 调用。SDL 按子系统"用到时初始化"——
//! `init_video()` 初始化 VIDEO，`init_input()` 初始化 JOYSTICK，
//! `init_audio()` 初始化 AUDIO（对应 C 的分阶段初始化）。
//!

// ── 设备 feature 强制断言（平台 feature 系统规范）──
// Cargo feature 是加法模型，无法原生表达"恰好启用一个设备变体"，
// 用编译错误取代静默回落——设备选择必须显式（--features smart|brick）。
// 新增设备变体时需同步更新以下两条断言。
#[cfg(all(feature = "smart", feature = "brick"))]
compile_error!("platform-tg5040: smart 与 brick 设备 feature 互斥，只能启用一个");

#[cfg(not(any(feature = "smart", feature = "brick")))]
compile_error!("platform-tg5040: 必须且只能启用一个设备 feature（smart 或 brick）");

use std::sync::Arc;

use common::audio::{AudioFrame, AudioRingBuffer};
use common::input::{BTN_MENU, BTN_MINUS, BTN_NONE, BTN_PLUS, BTN_POWER, InputState};
use common::platform::Platform;
use common::power::{BatteryStatus, CpuSpeed};
use common::video::VideoBuffer;

/// 系统设置（libmsettings 的 Rust 版）——亮度/音量/静音/耳机的跨进程共享、
/// 硬件操作与持久化。属于 Platform trait 之外的"第二个接口面"
/// （对应原版 libmsettings.so）。详见模块文档与 tg5040 README「系统设置与 keymon」。
pub mod settings;

/// 应用级按键输入（SDL joystick → InputState）——双通道输入架构的
/// 通道 A。纯逻辑层（事件抽象 + 跨帧状态 + 帧处理），sdl2 事件转换
/// 在 Platform 实现中完成。详见模块文档与 tg5040 README「输入」章节。
pub mod input;

/// 电源与硬件操作（sysfs 读写 + 睡眠/关机流程）——纯函数层（可测）
/// + 高级函数（供 Platform 实现调用）。详见模块文档与
/// tg5040 README「电源与硬件」章节。
pub mod power;

/// TrimUI tg5040 平台实现
///
/// 封装 SDL2 上下文和硬件状态。SDL 的使用完全局限在此结构体内部，
/// 不泄露到 `Platform` trait 接口。
///
/// 设备差异（分辨率/布局/输入映射/LED）全部由 `#[cfg(feature)]` 编译期处理——
/// **无 `is_brick` 字段**，禁止运行时设备判断（对应 C 的 `is_brick` 全局变量，
/// 详见 README「设备区分机制」）。
pub struct Tg5040 {
    /// SDL 总开关（`init_video` 填充）。
    /// 必须活得比所有 SDL 对象久——drop 时自动 `SDL_Quit`（对应 C 的 `SDL_Quit`）。
    /// 注意：`sdl2::init()` = `SDL_Init(0)` 只初始化核心，不碰子系统——
    /// 子系统通过 `sdl.video()`/`sdl.audio()` 等按需初始化（C 的"用到时初始化"）。
    sdl: Option<sdl2::Sdl>,
    /// 视频画布（`init_video` 填充）——window + renderer 合体。
    /// sdl2 无安全 API 独立创建 renderer（`into_canvas()` 消费 window），
    /// 对应 C 的 `vid.window` + `vid.renderer` 合并为 `WindowCanvas`。
    canvas: Option<sdl2::render::WindowCanvas>,
    /// 视频纹理（`init_video` 填充，RGB565 流纹理，物理尺寸）。
    /// 来自 `canvas.texture_creator()`——对应 C `vid.texture`。
    texture: Option<sdl2::render::Texture>,
    /// 游戏手柄（`init_input` 填充）。对应 C `joystick` 全局变量。
    joystick: Option<sdl2::joystick::Joystick>,
    /// SDL 音频设备（`init_audio` 填充，`open_playback` 返回值）。
    /// 对应 C `SND_Context` 的音频设备——`drop` 时自动关闭（对应 `SDL_CloseAudio`）。
    audio_device: Option<sdl2::audio::AudioDevice<AudioCallbackImpl>>,
    /// 音频环形缓冲——SDL 音频回调线程（消费者）和主线程（生产者）跨线程访问，
    /// 用 `Mutex` 包裹保证线程安全。对应 C `snd.buffer` + `SDL_LockAudio`。
    ///
    /// **`Arc` 原因**：SDL 音频回调闭包（`AudioCallback` 结构体）在独立音频线程
    /// 执行——回调的生命周期独立于 `Tg5040` 借用，必须经 `Arc` 共享所有权
    /// 而非借用（`open_playback` 的闭包参数不能借用 `&self`）。
    audio_buffer: std::sync::Arc<std::sync::Mutex<common::audio::AudioRingBuffer>>,
    /// 模拟器产生采样率（`init_audio` 设置）。对应 C `snd.sample_rate_in`。
    sample_rate_in: u32,
    /// 硬件实际采样率（`init_audio` 设置）。对应 C `snd.sample_rate_out`。
    sample_rate_out: u32,
    /// 输入状态跨帧记忆（`poll_input` 使用）。对应 C `pad` 全局变量
    /// （`PAD_Context`，api.c:1131）——按键掩码 + 重复计时表 +
    /// 摇杆原始值整合在 `PadState` 内（见 `crate::input`）。
    pad: crate::input::PadState,
    /// SDL 事件泵（`init_input` 填充）——`poll_input` 与 `should_wake`
    /// 共用（对应原版全局 `SDL_PollEvent`，api.c:1186/1366）。
    /// `RefCell` 因 `should_wake(&self)` 是共享引用（trait 签名）而事件
    /// 泵消费需要可变借用——单线程主循环内 `RefCell` 安全且正好阻止
    /// 跨线程（SDL 事件队列要求主线程）。
    event_pump: Option<std::cell::RefCell<sdl2::EventPump>>,
}

impl Tg5040 {
    /// 创建 tg5040 平台实例
    ///
    /// 不初始化 SDL——所有 SDL 资源在对应 `init_*` 方法中按需初始化。
    /// 无副作用，可在无 SDL 环境下构造（便于单元测试）。
    /// 注意：无 `is_brick` 字段——设备差异全部由 `#[cfg(feature)]` 编译期处理。
    pub fn new() -> Self {
        Self {
            sdl: None,
            canvas: None,
            texture: None,
            joystick: None,
            audio_device: None,
            // 容量 0 是占位——`init_audio` 时按 AUDIO_BUFFER_FRAMES 重建
            audio_buffer: std::sync::Arc::new(std::sync::Mutex::new(
                common::audio::AudioRingBuffer::new(0),
            )),
            sample_rate_in: 0,
            sample_rate_out: 0,
            pad: crate::input::PadState::new(),
            event_pump: None,
        }
    }
}

impl Default for Tg5040 {
    fn default() -> Self {
        Self::new()
    }
}

/// 将 `VideoBuffer` 的 RGB565 像素转换为 SDL 纹理上传所需的字节切片
///
/// 长度 = `pitch × height × BYTES_PER_PIXEL`（RGB565 = 2 字节/像素）。
/// `pitch` 可能大于 `width`（硬件对齐）——按 pitch 计算行字节数。
///
/// # Safety
///
/// 本函数包含 unsafe 代码（`slice::from_raw_parts` 指针转换）。
/// 调用者需满足的前置条件：
/// - `buffer.pitch × buffer.height × 2 ≤ buffer.pixels.len()`（数据长度足够）
/// - `buffer.pixels` 的布局与 SDL 纹理的 RGB565 布局一致（行连续、pitch 行距）
///
/// 将 SDL 事件转换为抽象输入事件（`crate::input::AppInputEvent`）
///
/// 适配层：sdl2 事件 → 纯逻辑层的事件枚举（解耦测试——状态机测试不依赖 SDL）。未映射的事件类型（窗口/鼠标/音频等）返回 `None` 忽略。
///
/// 对应原版 api.c:1186-1365 的事件循环分支（JOYBUTTON/HAT/AXIS/KEY）。
fn translate_event(event: &sdl2::event::Event) -> Option<crate::input::AppInputEvent> {
    use sdl2::event::Event;
    match event {
        Event::JoyButtonDown { button_idx, .. } => Some(crate::input::AppInputEvent::Button {
            button: *button_idx,
            pressed: true,
        }),
        Event::JoyButtonUp { button_idx, .. } => Some(crate::input::AppInputEvent::Button {
            button: *button_idx,
            pressed: false,
        }),
        Event::JoyHatMotion { state, .. } => {
            Some(crate::input::AppInputEvent::Hat(hat_dir(*state)))
        }
        Event::JoyAxisMotion {
            axis_idx, value, ..
        } => Some(crate::input::AppInputEvent::Axis {
            axis: *axis_idx,
            value: *value as i32,
        }),
        Event::KeyDown {
            scancode: Some(sc), ..
        } => Some(crate::input::AppInputEvent::Key {
            // Scancode 是 #[repr(i32)] 枚举（Copy）——解引用后 cast 为 u16
            //（枚举值即 SDL scancode 数值）
            scancode: *sc as u16,
            pressed: true,
        }),
        Event::KeyUp {
            scancode: Some(sc), ..
        } => Some(crate::input::AppInputEvent::Key {
            scancode: *sc as u16,
            pressed: false,
        }),
        _ => None,
    }
}

/// 音频环形缓冲容量（帧）——对应原版 4000 帧 ≈ 83ms@48kHz。
///
/// 原版 `buffer_seconds × sample_rate / frame_rate`（api.c:987）除
/// `frame_rate` 是"游戏帧计量"残留——缓冲容量与游戏帧率无关，
/// Rust 版固定值（不随采样率/fps 变化）。
const AUDIO_BUFFER_FRAMES: usize = 4000;

/// SDL 音频回调结构体——音频线程 pop 环形缓冲填充输出
///
/// `AudioCallback: Send` 约束由字段类型保证（`Arc<Mutex<...>>` 是 Send）。
/// 对应原版 `SND_audioCallback`（api.c:948-981）——回调在 SDL 音频线程执行，
/// 经 `Arc` 与主线程共享环形缓冲。
struct AudioCallbackImpl {
    /// 音频环形缓冲（与主线程共享——`Arc` 跨线程所有权）
    buffer: std::sync::Arc<std::sync::Mutex<AudioRingBuffer>>,
}

impl sdl2::audio::AudioCallback for AudioCallbackImpl {
    type Channel = i16;

    fn callback(&mut self, out: &mut [i16]) {
        // 锁竞争极小：回调每 SAMPLES（512）采样触发一次，与主线程
        // push 频率（模拟器帧率）相比低频——Mutex 短临界区足够
        let mut buffer = self.buffer.lock().unwrap();
        fill_output(&mut buffer, out);
    }
}

/// 音频回调填充逻辑（纯函数，可测）
///
/// 从环形缓冲 pop 帧到输出切片（`i16` 交错立体声：每 2 个 `i16` 一帧）。
/// 缓冲不足时剩余输出静音填充（`0`）。
///
/// 对应原版 `SND_audioCallback`（api.c:948-981）——**差异**：原版部分
/// 不足时"倒序回放"已输出的采样（`*--in` 伪回声，非标准做法），
/// Rust 版静音填充（标准做法，不复制）。
///
/// ## 参数
///
/// - `buffer`：音频环形缓冲（SDL 回调线程持有）
/// - `out`：SDL 输出切片（交错立体声 `i16`）
///
/// ## 返回
///
/// 实际填充的有效帧数（不足时少于 `out.len() / 2`）
fn fill_output(buffer: &mut common::audio::AudioRingBuffer, out: &mut [i16]) -> usize {
    let mut filled = 0;
    let mut i = 0;
    let mut frame = common::audio::AudioFrame { left: 0, right: 0 };
    while i + 1 < out.len() {
        // 每轮重置静音帧——pop 失败时输出静音
        frame.left = 0;
        frame.right = 0;
        if buffer.pop(std::slice::from_mut(&mut frame)) == 1 {
            filled += 1;
        }
        out[i] = frame.left;
        out[i + 1] = frame.right;
        i += 2;
    }
    filled
}

/// sdl2 `HatState` → 抽象 hat 方向（`crate::input::HatDir`）
///
/// HatState 定义在 `sdl2::joystick`（event.rs 只是内部引用，不公开）
fn hat_dir(state: sdl2::joystick::HatState) -> crate::input::HatDir {
    use sdl2::joystick::HatState;
    match state {
        HatState::Centered => crate::input::HatDir::Centered,
        HatState::Up => crate::input::HatDir::Up,
        HatState::Down => crate::input::HatDir::Down,
        HatState::Left => crate::input::HatDir::Left,
        HatState::Right => crate::input::HatDir::Right,
        HatState::LeftUp => crate::input::HatDir::LeftUp,
        HatState::LeftDown => crate::input::HatDir::LeftDown,
        HatState::RightUp => crate::input::HatDir::RightUp,
        HatState::RightDown => crate::input::HatDir::RightDown,
    }
}

fn buffer_as_bytes(buffer: &VideoBuffer) -> &[u8] {
    let byte_len = buffer.pitch as usize * buffer.height as usize * 2;
    unsafe { std::slice::from_raw_parts(buffer.pixels.as_ptr() as *const u8, byte_len) }
}

impl Platform for Tg5040 {
    // ══════════════════════════════════════════════
    // 关联常量（编译期，按 feature 区分设备）
    // ══════════════════════════════════════════════

    // ══════════════════════════════════════════════
    // 画布 = 物理分辨率（fix-tg5040-resolution-design 修正）
    // ══════════════════════════════════════════════
    // SCREEN_WIDTH/HEIGHT 等于物理屏分辨率——flip 恒为 1:1 无缩放。
    // SCALE 语义 = 图集倍率 + 布局倍率（render 裁切 ASSET_RECTS×SCALE
    // 从 @SCALE 图集取像素、布局 PILL_SIZE×SCALE），与画布→屏幕无关。
    // 历史修正：第一层曾用逻辑画布 640×360/341×256（意图 flip GPU 放大），
    // 数学不自洽（物理元素 = 布局×SCALE×flip = C 的 2 倍）——已废弃，
    // 详见 README「设备区分机制」分辨率章节。
    //
    // cfg 正向双 feature：smart 与 brick 互斥且必选（无默认），
    // 由文件顶部 compile_error! 断言强制——此处两组常量恰好启用一组，
    // 不会出现无常量定义的编译失败。

    // smart：TrimUI Smart Pro，物理屏 1280×720，SCALE=2（@2x 图集）
    #[cfg(feature = "smart")]
    const SCREEN_WIDTH: u32 = 1280;
    #[cfg(feature = "smart")]
    const SCREEN_HEIGHT: u32 = 720;
    #[cfg(feature = "smart")]
    const SCALE: u32 = 2;

    // brick：TrimUI Brick，物理屏 1024×768，SCALE=3（@3x 图集）
    #[cfg(feature = "brick")]
    const SCREEN_WIDTH: u32 = 1024;
    #[cfg(feature = "brick")]
    const SCREEN_HEIGHT: u32 = 768;
    #[cfg(feature = "brick")]
    const SCALE: u32 = 3;

    // 两台设备共用
    const BYTES_PER_PIXEL: u8 = 2; // RGB565
    const HAS_HDMI: bool = true; // tg5040 支持 HDMI
    const HAS_POWER_BUTTON: bool = true; // tg5040 has BUTTON_POWER
    const HAS_POWEROFF_BUTTON: bool = false; // tg5040: BUTTON_POWEROFF = BUTTON_NA
    const SUPPORTS_OVERSCAN: bool = false; // tg5040: no overscan implementation

    // ── 设备按键能力（对应原 C platform.h 按键宏探测式——
    //    BUTTON_*/CODE_*/JOY_*/AXIS_* 任一非 NA 即有该键）──

    // L2/R2 为模拟触发键（platform.h:100-101 `AXIS_L2 2`/`AXIS_R2 5`）
    const HAS_L2: bool = true;
    const HAS_R2: bool = true;
    // L3/R3 仅 brick 有（platform.h:90-91 `JOY_L3 (is_brick?9:NA)`/`JOY_R3 (is_brick?10:NA)`）
    #[cfg(feature = "brick")]
    const HAS_L3: bool = true;
    #[cfg(feature = "smart")]
    const HAS_L3: bool = false;
    #[cfg(feature = "brick")]
    const HAS_R3: bool = true;
    #[cfg(feature = "smart")]
    const HAS_R3: bool = false;
    // 双摇杆（platform.h:103-106 `AXIS_LX 0`/`AXIS_LY 1`/`AXIS_RX 3`/`AXIS_RY 4`）
    const HAS_LS: bool = true;
    const HAS_RS: bool = true;
    // 音量键（platform.h:67-68 `CODE_PLUS 128`/`CODE_MINUS 129`，JOY_PLUS 亦有）
    const HAS_VOLUME: bool = true;
    // MENU 键（platform.h:93 `JOY_MENU 8`）
    const HAS_MENU: bool = true;

    // ── 视频 ─────────────────────────────────────

    /// 初始化视频子系统
    ///
    /// 对应 C `PLAT_initVideo`（platform.c:58-124）。流程：
    /// 1. `sdl2::init()`（= `SDL_Init(0)`，只初始化核心，不碰子系统）
    /// 2. `sdl.video()`（内部 `SDL_InitSubSystem(SDL_INIT_VIDEO)`——按需初始化）
    /// 3. 创建窗口（尺寸 = 物理分辨率；默认位置 Undefined、默认显示 = C 的 SHOWN）
    /// 4. `into_canvas()` 合体创建渲染器（ACCELERATED | PRESENTVSYNC）
    /// 5. 隐藏光标（`SDL_ShowCursor(0)`——掌机无鼠标）
    /// 6. 渲染质量提示（最近邻——与 C 的 `SDL_SetHint` 一致）
    /// 7. 创建纹理（RGB565 流纹理，物理尺寸——画布=物理，flip 1:1）
    ///
    /// 失败即 panic（Platform trait 设计：嵌入式 init 失败无回退方案）。
    fn init_video(&mut self) {
        // ① SDL 核心（C 无对应——C 直接 InitSubSystem；sdl2 分两步）
        let sdl = sdl2::init().expect("SDL 初始化失败");

        // ② 视频子系统（C: SDL_InitSubSystem(SDL_INIT_VIDEO)）
        let video = sdl.video().expect("SDL 视频子系统初始化失败");

        // ③ 窗口（C: SDL_CreateWindow("", UNDEFINED, UNDEFINED, W, H, SHOWN)）
        let window = video
            .window("", Self::SCREEN_WIDTH, Self::SCREEN_HEIGHT)
            .build()
            .expect("SDL 窗口创建失败");

        // ④ 渲染器（C: SDL_CreateRenderer(window, -1, ACCELERATED|PRESENTVSYNC)）
        //    into_canvas 消费 window——canvas = window + renderer 合体
        let canvas = window
            .into_canvas()
            .accelerated()
            .present_vsync()
            .build()
            .expect("SDL 渲染器创建失败");

        // ⑤ 隐藏光标（C: SDL_ShowCursor(0)——掌机无鼠标）
        sdl.mouse().show_cursor(false);

        // ⑥ 渲染质量提示（C: SDL_SetHint(RENDER_SCALE_QUALITY, "0")——最近邻）
        sdl2::hint::set("SDL_RENDER_SCALE_QUALITY", "0");

        // ⑦ 纹理（C: SDL_CreateTexture(renderer, RGB565, STREAMING, W, H)）
        //    尺寸 = 物理分辨率（画布=物理，flip 1:1 无缩放）
        let texture = canvas
            .texture_creator()
            .create_texture_streaming(
                sdl2::pixels::PixelFormatEnum::RGB565,
                Self::SCREEN_WIDTH,
                Self::SCREEN_HEIGHT,
            )
            .expect("SDL 纹理创建失败");

        // 存入结构体——Sdl 活得最久（drop 时 SDL_Quit），canvas 其次，texture 最早
        self.sdl = Some(sdl);
        self.canvas = Some(canvas);
        self.texture = Some(texture);
    }

    /// 销毁视频子系统
    ///
    /// 对应 C `PLAT_quitVideo`（platform.c:140-153）。三层清理：
    /// 1. 清屏保险：3 次 `clear` + `present`——清空 vsync 双缓冲，
    ///    防退出后屏幕残留最后一帧（对应 C 的 `clearVideo` 3 次清屏）
    /// 2. RAII 按序销毁：texture → canvas → sdl（`SdlDrop::drop` 自动 `SDL_Quit`，
    ///    对应 C 的 Destroy 链 + `SDL_Quit`）
    /// 3. 清 framebuffer：`cat /dev/zero > /dev/fb0`——防 fbcon 残留（最后保险）
    fn quit_video(&mut self) {
        // ① 清屏保险（对应 C clearVideo 的 3 次清屏——清空 vsync 双缓冲）
        if let Some(canvas) = &mut self.canvas {
            for _ in 0..3 {
                canvas.clear();
                canvas.present();
            }
        }

        // ② RAII 按序销毁（对应 C 的 Destroy 链 + SDL_Quit）
        self.texture = None;
        self.canvas = None;
        self.sdl = None;

        // ③ 清 framebuffer（对应 C: cat /dev/zero > /dev/fb0——防 SDL_Quit 后残留）
        let _ = std::process::Command::new("sh")
            .args(["-c", "cat /dev/zero > /dev/fb0 2>/dev/null"])
            .status();
    }

    /// 将 `VideoBuffer` 像素提交到物理屏幕
    ///
    /// 对应 C `PLAT_flip`（platform.c:373-446 的 UI 路径——无游戏 blit 时）。
    /// 1. `buffer_as_bytes` 转字节切片（unsafe 集中一处，见函数 # Safety）
    /// 2. `update_texture` 整帧上传（纹理 = 物理尺寸，1:1）
    /// 3. `copy` 1:1 呈现（画布 = 物理分辨率，无缩放）
    /// 4. `present`（PRESENTVSYNC 阻塞到垂直同步）
    ///
    /// `wait_vsync` 参数**忽略**——与 C 的 `PLAT_flip(screen, ignored)` 一致：
    /// vsync 由 `SDL_RENDERER_PRESENTVSYNC` 创建时固定（present 阻塞到垂直同步）；
    /// 帧率控制在上层主循环（`now_ms()` 测帧耗时 + 补帧）。
    /// 详见 README「wait_vsync 语义说明」。
    fn flip(&mut self, buffer: &VideoBuffer, _wait_vsync: bool) {
        let canvas = self.canvas.as_mut().expect("视频未初始化");
        let texture = self.texture.as_mut().expect("视频未初始化");

        // ① 整帧上传（1:1——纹理尺寸 = buffer 尺寸 = 物理分辨率）
        //    texture.update(rect, pixels, pitch)：rect=None = 整帧
        texture
            .update(None, buffer_as_bytes(buffer), buffer.pitch as usize * 2)
            .expect("纹理更新失败");

        // ② 1:1 呈现（画布 = 物理分辨率，无缩放）
        canvas.copy(texture, None, None).expect("渲染拷贝失败");

        // ③ 呈现（PRESENTVSYNC 阻塞到垂直同步）
        canvas.present();
    }

    // ── 输入 ─────────────────────────────────────

    fn init_input(&mut self) {
        // ① joystick 子系统 + 打开 0 号手柄（对应 C `PLAT_initInput`：
        // `SDL_InitSubSystem(SDL_INIT_JOYSTICK)` + `SDL_JoystickOpen(0)`）
        // 打开失败不 panic（链式 Option）——`poll_input` 的键盘兜底通道仍可用
        self.joystick = self
            .sdl
            .as_ref()
            .and_then(|sdl| sdl.joystick().ok())
            .and_then(|subsystem| subsystem.open(0).ok());
        // ② 事件泵（sdl2 要求主线程创建——`init_input` 在主循环前调用；
        // 注意：`EventPump::new` 是私有的，须经 `sdl.event_pump()` 创建）
        self.event_pump = self
            .sdl
            .as_ref()
            .and_then(|sdl| sdl.event_pump().ok())
            .map(std::cell::RefCell::new);
    }

    fn quit_input(&mut self) {
        // 关闭手柄 + 事件泵（对应 C `SDL_JoystickClose`——drop 即 close）
        self.joystick = None;
        self.event_pump = None;
    }

    fn poll_input(&mut self) -> InputState {
        // 本帧事件收集（对应原版 `while (SDL_PollEvent(&event))`，api.c:1186）
        let mut events = Vec::new();
        if let Some(pump) = &self.event_pump {
            let mut pump = pump.borrow_mut();
            for event in pump.poll_iter() {
                if let Some(ev) = translate_event(&event) {
                    events.push(ev);
                }
            }
        }
        // 状态机处理（repeat 扫描 + 事件 → 键位更新）
        // 先取 tick（&self）再借用 &mut self.pad——避免同一表达式内借用冲突
        let tick = self.now_ms();
        crate::input::process_frame(&mut self.pad, &events, tick)
    }

    fn reset_input(&mut self) {
        // 重置跨帧输入状态（对应原版 PAD_reset，api.c:1180-1186）——
        // `PadState::reset` 连重复计时表一并清零（与原版差异见该方法注释）。
        // 由 `faux_sleep` 在睡眠前/唤醒后各调用一次
        self.pad.reset();
    }

    // ── 音频 ─────────────────────────────────────

    fn init_audio(&mut self, sample_rate: u32) -> u32 {
        use std::sync::atomic::{AtomicU32, Ordering};

        // 对应原版 SND_init（api.c:1081-1113）：SDL audio 子系统 +
        // open_playback（callback 模式、AUDIO_S16、2 通道、SAMPLES=512）。
        // 失败（无 SDL/无 audio 驱动）返回 0——不 panic
        let Some(sdl) = &self.sdl else { return 0 };
        let Ok(audio) = sdl.audio() else { return 0 };

        // 重建 4000 帧缓冲（new() 时容量 0 占位——AUDIO_BUFFER_FRAMES 见常量注释）
        *self.audio_buffer.lock().unwrap() = AudioRingBuffer::new(AUDIO_BUFFER_FRAMES);

        // 实际采样率回传：open_playback 的回调闭包参数是 AudioSpec（含实际
        // freq），但闭包返回回调结构——经 Arc<AtomicU32> 共享单元写回
        //（对应原版 spec_out.freq 作为重采样基准）
        let actual_rate = Arc::new(AtomicU32::new(0));
        let actual_rate_cb = Arc::clone(&actual_rate);
        let buffer_cb = Arc::clone(&self.audio_buffer);

        let device = audio
            .open_playback(
                None,
                &sdl2::audio::AudioSpecDesired {
                    freq: Some(sample_rate as i32),
                    channels: Some(2),
                    samples: Some(512), // 原版 SAMPLES=512（api.c:922）
                },
                move |spec| {
                    actual_rate_cb.store(spec.freq as u32, Ordering::Relaxed);
                    AudioCallbackImpl { buffer: buffer_cb }
                },
            )
            .ok();
        self.audio_device = device;

        let rate = actual_rate.load(Ordering::Relaxed);
        self.sample_rate_in = sample_rate;
        self.sample_rate_out = rate;
        rate
    }

    fn quit_audio(&mut self) {
        // pause + drop 自动关闭（对应 SND_quit：SDL_PauseAudio(1) + SDL_CloseAudio）
        self.audio_device = None;
    }

    fn pause_audio(&mut self, pause: bool) {
        // 对应 SDL_PauseAudio（api.c:1106/1118）——睡眠/唤醒用
        if let Some(device) = &self.audio_device {
            if pause {
                device.pause();
            } else {
                device.resume();
            }
        }
    }

    fn push_audio(&mut self, frames: &[AudioFrame]) -> usize {
        // 主线程 push 已重采样帧（minarch 侧完成重采样——common Resampler）。
        // 满时返回实际接受帧数（0 = 满）——背压由调用方处理（trait 契约
        // "调用方可选择丢弃或重试"）；不复制原版等待 10ms 重试
        // （SND_batchSamples 的 tries 循环，api.c:1016-1022——minarch
        // 单线程时代的背压策略）
        self.audio_buffer.lock().unwrap().push(frames)
    }

    // ── 电源 ──────────────────────────────

    fn power_off(&mut self) {
        // 对应原版 PLAT_powerOff（platform.c:527-539）：
        // ① 删除 /tmp/minui_exec——break launch.sh 的 while 循环
        // ② 延时 2 秒（给存档/清理留时间）
        // ③ 静音 + 关背光 + 显式关音频（exit 不跑 Drop——礼貌清理）
        // ④ exit(0)——shutdown 由 PLATFORM/bin/shutdown 处理（原版注释
        //    "poweroff handled by PLATFORM/bin/shutdown"）
        let _ = std::fs::remove_file("/tmp/minui_exec");
        std::thread::sleep(std::time::Duration::from_secs(2));
        crate::settings::set_raw_volume(crate::power::MUTE_VOLUME_RAW);
        self.enable_backlight(false);
        self.quit_audio();
        std::process::exit(0);
    }

    fn get_battery_status(&self) -> BatteryStatus {
        // 对应原版 PLAT_getBatteryStatus（platform.c:475-487）
        crate::power::get_battery_status()
    }


    fn set_cpu_speed(&mut self, speed: CpuSpeed) {
        // 对应原版 PLAT_setCPUSpeed（platform.c:546-555）
        crate::power::set_cpu_speed(speed);
    }

    // ── 睡眠/唤醒 ────────────────────────────────

    fn prepare_sleep(&mut self) {
        // 对应原版 PWR_enterSleep（api.c:1641-1653）：
        // 暂停音频 → 硬件静音（不改 mute 状态）→ 关背光 → SIGSTOP keymon → sync
        self.pause_audio(true);
        crate::settings::set_raw_volume(crate::power::MUTE_VOLUME_RAW);
        self.enable_backlight(false);
        // killall 命令（原版 system() 的忠实对应）——keymon 未启动时失败静默
        // 进程名 keymon（无 .elf 后缀）：launch.sh 以 `keymon &` 启动，
        // 设备进程名随之无后缀（Rust 版统一命名约定）
        let _ = std::process::Command::new("killall")
            .args(["-STOP", "keymon"])
            .status();
        sync_filesystem();
    }

    fn should_wake(&self) -> bool {
        // 唤醒检测：吃 POWER 释放事件（对应原版 `PLAT_shouldWake`，
        // api.c:1366-1372）——吃掉事件防止残留到 `poll_input`
        let mut wake = false;
        if let Some(pump) = &self.event_pump {
            let mut pump = pump.borrow_mut();
            for event in pump.poll_iter() {
                match event {
                    sdl2::event::Event::JoyButtonUp { button_idx, .. }
                        if button_idx == crate::input::JOY_POWER =>
                    {
                        wake = true;
                    }
                    sdl2::event::Event::KeyUp {
                        scancode: Some(sc), ..
                    } => {
                        // scrutinee 是 Event 值（poll_iter）——sc 绑定为
                        // Scancode 值（Copy），直接 cast（translate_event
                        // 的 scrutinee 是 &Event，sc 为引用需 `*sc`——两者不同）
                        wake |= (sc as u16) == crate::input::CODE_POWER;
                    }
                    _ => {}
                }
            }
        }
        wake
    }

    fn complete_wake(&mut self) {
        // 对应原版 PWR_exitSleep（api.c:1654-1663）：
        // SIGCONT keymon → 开背光 → 恢复音量 → 恢复音频 → sync
        let _ = std::process::Command::new("killall")
            .args(["-CONT", "keymon"])
            .status();
        self.enable_backlight(true);
        // 恢复音量（对应原版 SetVolume(GetVolume())——settings 内部处理
        // mute/jack 分支）
        let settings = crate::settings::SettingsHandle::init();
        settings.set_volume(settings.volume());
        self.pause_audio(false);
        sync_filesystem();
    }

    // ── 时间 ──────────────────────────────────────

    /// 单调时钟，毫秒精度
    ///
    /// 调用 `SDL_GetTicks()`——无需任何 SDL 子系统初始化
    /// （SDL2 内部对 ticks 子系统懒初始化）。
    /// 对应原 C `SDL_GetTicks()`（api.c 中 `#define ms SDL_GetTicks`）。
    ///
    /// # Safety
    ///
    /// 本函数包含 unsafe 代码（FFI 直接调用 `sdl2::sys::SDL_GetTicks`）。
    /// 调用者需满足的前置条件：
    /// - SDL2 运行时库已正确链接（由 `sdl2-sys` 构建时通过 pkg-config 保证）
    /// - 无需任何 SDL 子系统初始化——`SDL_GetTicks` 内部对 ticks 子系统懒初始化
    /// - 返回值是 `u32` 毫秒（约 49.7 天回绕），调用方需容忍回绕或使用相对差值
    fn now_ms(&self) -> u32 {
        unsafe { sdl2::sys::SDL_GetTicks() }
    }

    /// 设置系统时间（date + hwclock 命令）——对应原版 PLAT_setDateTime
    /// （api.c:1717-1722）
    fn set_date_time(&mut self, y: i32, m: i32, d: i32, h: i32, min: i32, sec: i32) {
        crate::power::set_date_time(y, m, d, h, min, sec);
    }

    // ── 关联常量（设备型号 / 平台路径） ──────────────────────────────────────

    /// 设备型号名称
    ///
    /// 编译期确定（feature 已固定设备）——关联常量形态，不读环境变量。
    /// 对应原 C `PLAT_getModel()`（读 `getenv("TRIMUI_MODEL")`，platform.c:566-570），
    /// Rust 版改为编译期常量（"编译期固定值 → 关联常量"的 trait 分层约定）。
    #[cfg(feature = "smart")]
    const DEVICE_MODEL: &str = "TrimUI Smart Pro";
    #[cfg(feature = "brick")]
    const DEVICE_MODEL: &str = "TrimUI Brick";

    // 平台路径原语（对应 C platform.h:135 的 SDCARD_PATH 宏与 makefile 的
    // PLATFORM 变量；派生路径由 common::paths 自由函数按需拼接）
    const SDCARD_PATH: &str = "/mnt/SDCARD";
    const PLATFORM: &str = "tg5040";

    // ── 设备语义键（对应 C platform.h:110-114 的 BTN_SLEEP/BTN_MOD_* 宏）──

    const BTN_SLEEP: u32 = BTN_POWER;
    const BTN_MOD_BRIGHTNESS: u32 = BTN_MENU;
    const BTN_MOD_VOLUME: u32 = BTN_NONE;
    const BTN_MOD_PLUS: u32 = BTN_PLUS;
    const BTN_MOD_MINUS: u32 = BTN_MINUS;

    // ── 布局（对应 platform.h:130-131 的 MAIN_ROW_COUNT/PADDING 宏）──

    /// 每页可显示行数。对应 platform.h:130
    /// `#define MAIN_ROW_COUNT (is_brick?7:8)`——smart=8、brick=7。
    /// `is_brick` 是运行时变量——Rust 版编译期 feature 替代（设备区分机制）。
    fn main_row_count(&self) -> u32 {
        #[cfg(feature = "brick")]
        {
            7
        }
        #[cfg(feature = "smart")]
        {
            8
        }
    }

    /// 页面边缘留白。对应 platform.h:131
    /// `#define PADDING (is_brick?5:40)`——smart=40、brick=5。
    fn padding(&self) -> u32 {
        #[cfg(feature = "brick")]
        {
            5
        }
        #[cfg(feature = "smart")]
        {
            40
        }
    }

    // ── 硬件 ──────────────────────────────────────

    fn enable_backlight(&mut self, enable: bool) {
        // 对应原版 PLAT_enableBacklight（platform.c:514-525）
        crate::power::enable_backlight(enable);
    }

    /// 是否在线（WiFi 已连接）——读 wlan0/operstate（对应原版
    /// getBatteryStatus 顺带的 wifi 检查，platform.c:488-489）
    fn is_online(&self) -> bool {
        crate::power::is_online()
    }

    /// 振动（gpio227，静音时抑制）——对应原版 PLAT_setRumble
    /// （platform.c:558-560）
    fn set_rumble(&mut self, strength: u8) {
        crate::power::set_rumble(strength);
    }

    /// 是否 HDMI 活跃——恒 false（tg5040 无 HDMI 输入检测，
    /// 原版 GetHDMI 返回 0）
    fn is_hdmi_active(&self) -> bool {
        false
    }

    // set_vsync: 使用默认实现（空操作——创建渲染器时已决定）
    // pick_sample_rate: 使用默认实现（requested.min(max)）
}

/// 文件系统同步（`sync()` 系统调用）
///
/// 对应原版 `PWR_enterSleep`/`PWR_exitSleep` 末尾的 `sync()`（api.c:1653/1663）——
/// 睡眠前后确保文件系统落盘（SD 卡写入后断电不丢）。
///
/// # Safety
///
/// 本函数包含 unsafe 代码（FFI 调用 `libc::sync`）。调用者需满足的
/// 前置条件：无（`sync` 无参数、无返回值，进程内安全）
fn sync_filesystem() {
    unsafe { libc::sync() };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SDL 测试串行锁——SDL 是进程级全局状态，多个 SDL 冒烟测试并行
    /// 会互相干扰（窗口/事件泵/子系统初始化竞争）。每个 SDL 测试开头
    /// 获取此锁（`let _g = SDL_TEST_LOCK.lock().unwrap();`）
    static SDL_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    // ── new() 无 SDL 副作用（TDD 红阶段定义的行为）──

    #[test]
    fn new_has_no_sdl_side_effects() {
        // 在无 SDL 环境构造——不应 panic、不应调用任何 SDL 函数
        let platform = Tg5040::new();
        // 字段初始值验证
        assert!(platform.sdl.is_none());
        assert!(platform.canvas.is_none());
        assert!(platform.texture.is_none());
        assert!(platform.joystick.is_none());
        assert!(platform.audio_device.is_none());
        assert_eq!(platform.sample_rate_in, 0);
        assert_eq!(platform.sample_rate_out, 0);
    }

    #[test]
    fn new_audio_buffer_is_empty() {
        let platform = Tg5040::new();
        assert!(platform.audio_buffer.lock().unwrap().is_empty());
        assert_eq!(platform.audio_buffer.lock().unwrap().capacity(), 0);
    }

    // ── 回调填充纯函数（fill_output）──

    #[test]
    fn fill_output_empty_buffer_all_silence() {
        // 空缓冲 → 全静音 + 返回 0（无有效帧）
        let mut buffer = AudioRingBuffer::new(4000);
        let mut out = [0i16; 1024]; // 512 帧
        let filled = fill_output(&mut buffer, &mut out);
        assert_eq!(filled, 0);
        assert!(out.iter().all(|&s| s == 0), "空缓冲应全静音");
    }

    #[test]
    fn fill_output_partial_fill() {
        // 200 帧缓冲 → 前 200 帧内容 + 后 312 帧静音
        let mut buffer = AudioRingBuffer::new(4000);
        let frames: Vec<AudioFrame> = (0..200)
            .map(|i| AudioFrame {
                left: i as i16,
                right: (i as i16) * 2,
            })
            .collect();
        assert_eq!(buffer.push(&frames), 200);
        let mut out = [0i16; 1024]; // 512 帧
        let filled = fill_output(&mut buffer, &mut out);
        assert_eq!(filled, 200);
        // 前 200 帧 = 缓冲内容
        for i in 0..200 {
            assert_eq!(out[i * 2], i as i16);
            assert_eq!(out[i * 2 + 1], (i as i16) * 2);
        }
        // 后 312 帧静音
        for i in 200..512 {
            assert_eq!(out[i * 2], 0, "第 {i} 帧左声道应静音");
            assert_eq!(out[i * 2 + 1], 0);
        }
    }

    #[test]
    fn fill_output_full_buffer() {
        // 缓冲 ≥ 输出 → 全部有效
        let mut buffer = AudioRingBuffer::new(4000);
        let frames: Vec<AudioFrame> = (0..600).map(|_| AudioFrame { left: 1, right: 2 }).collect();
        assert_eq!(buffer.push(&frames), 600);
        let mut out = [0i16; 1024]; // 512 帧
        let filled = fill_output(&mut buffer, &mut out);
        assert_eq!(filled, 512, "输出满应全部有效");
        // 缓冲剩余 88 帧
        assert_eq!(buffer.len(), 88);
    }

    // ── 关联常量按 feature（TDD 红阶段定义的行为）──

    #[cfg(feature = "smart")]
    #[test]
    fn smart_constants_equal_physical_screen() {
        assert_eq!(Tg5040::SCREEN_WIDTH, 1280);
        assert_eq!(Tg5040::SCREEN_HEIGHT, 720);
        assert_eq!(Tg5040::SCALE, 2);
    }

    #[cfg(feature = "brick")]
    #[test]
    fn brick_constants_equal_physical_screen() {
        assert_eq!(Tg5040::SCREEN_WIDTH, 1024);
        assert_eq!(Tg5040::SCREEN_HEIGHT, 768);
        assert_eq!(Tg5040::SCALE, 3);
    }

    #[test]
    fn shared_constants() {
        assert_eq!(Tg5040::BYTES_PER_PIXEL, 2);
        assert!(Tg5040::HAS_HDMI);
        assert!(Tg5040::HAS_POWER_BUTTON);
        assert!(!Tg5040::HAS_POWEROFF_BUTTON);
        assert!(!Tg5040::SUPPORTS_OVERSCAN);
    }

    // ── 设备按键能力常量（对应原 C platform.h 按键宏探测式）──

    /// smart/brick 共用的能力常量：L2/R2/LS/RS/VOLUME/MENU 均 true
    ///
    /// 用 const 块做**编译期**断言——能力常量是编译期值，编译失败即测试失败
    #[test]
    fn capability_constants_shared_true() {
        const {
            assert!(Tg5040::HAS_L2); // AXIS_L2=2（ABSZ 触发键）
            assert!(Tg5040::HAS_R2); // AXIS_R2=5（RABSZ 触发键）
            assert!(Tg5040::HAS_LS); // AXIS_LX=0（左摇杆）
            assert!(Tg5040::HAS_RS); // AXIS_RX=3（右摇杆）
            assert!(Tg5040::HAS_VOLUME); // CODE_PLUS=128/JOY_PLUS
            assert!(Tg5040::HAS_MENU); // JOY_MENU=8
        }
    }

    /// smart 无 L3/R3（原 C `JOY_L3 (is_brick?9:NA)` 的 smart 分支）
    #[cfg(feature = "smart")]
    #[test]
    fn capability_constants_smart_no_l3_r3() {
        const {
            assert!(!Tg5040::HAS_L3);
            assert!(!Tg5040::HAS_R3);
        }
    }

    /// brick 有 L3/R3（原 C `JOY_L3 9`/`JOY_R3 10`）
    #[cfg(feature = "brick")]
    #[test]
    fn capability_constants_brick_has_l3_r3() {
        const {
            assert!(Tg5040::HAS_L3);
            assert!(Tg5040::HAS_R3);
        }
    }

    #[test]
    fn platform_path_constants() {
        // 对应 C platform.h:135 的 SDCARD_PATH 宏与 makefile 的 PLATFORM 变量
        assert_eq!(Tg5040::SDCARD_PATH, "/mnt/SDCARD");
        assert_eq!(Tg5040::PLATFORM, "tg5040");
    }

    // ── now_ms 无初始化可用（TDD 红阶段定义的行为）──

    #[test]
    fn now_ms_works_without_init() {
        // new() 后直接调用——无需 init_video，应返回合理时间值
        let platform = Tg5040::new();
        let t1 = platform.now_ms();
        let t2 = platform.now_ms();
        // 单调递增（同一毫秒内可能相等，但不回退）
        assert!(t2 >= t1);
    }

    // ── device_model 按 feature（TDD 红阶段定义的行为）──

    #[cfg(feature = "smart")]
    #[test]
    fn device_model_smart() {
        assert_eq!(Tg5040::DEVICE_MODEL, "TrimUI Smart Pro");
    }

    // ── 布局方法按 feature（TDD 红阶段定义的行为）──
    // 对应 platform.h:130-131 的 `MAIN_ROW_COUNT (is_brick?7:8)` /
    // `PADDING (is_brick?5:40)`

    #[cfg(feature = "smart")]
    #[test]
    fn smart_layout_values_match_original() {
        let platform = Tg5040::new();
        assert_eq!(platform.main_row_count(), 8);
        assert_eq!(platform.padding(), 40);
    }

    #[cfg(feature = "brick")]
    #[test]
    fn brick_layout_values_match_original() {
        let platform = Tg5040::new();
        assert_eq!(platform.main_row_count(), 7);
        assert_eq!(platform.padding(), 5);
    }

    // ── buffer_as_bytes（TDD 红阶段定义的行为）──

    #[test]
    fn buffer_bytes_len_normal() {
        // pitch=width：1280×720 → 1280×720×2 = 1,843,200 字节
        let buf = VideoBuffer::new(1280, 720);
        assert_eq!(buffer_as_bytes(&buf).len(), 1280 * 720 * 2);
    }

    #[test]
    fn buffer_bytes_len_pitch_greater() {
        // pitch > width（硬件对齐）：width=640, pitch=656, height=360
        let mut buf = VideoBuffer::new(640, 360);
        buf.pitch = 656; // 模拟硬件对齐（行距大于宽度）
        assert_eq!(buffer_as_bytes(&buf).len(), 656 * 360 * 2);
    }

    #[test]
    fn buffer_bytes_empty() {
        let buf = VideoBuffer::new(0, 0);
        assert!(buffer_as_bytes(&buf).is_empty());
    }

    // ── 睡眠冒烟测试（SDL dummy + Linux——settings/sysfs 依赖）──
    // prepare_sleep/complete_wake 调 SettingsHandle::init()（enable_backlight/
    // 音量恢复）——settings 是 Linux 专属（非 unix panic），故 Linux-gated
    //（与 settings 测试先例一致；macOS 上跳过，Linux CI/真机运行）

    #[test]
    #[cfg(target_os = "linux")]
    fn sleep_smoke_prepare_complete_wake() {
        let _g = SDL_TEST_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("SDL_VIDEODRIVER", "dummy");
            std::env::set_var("SDL_AUDIODRIVER", "dummy");
        }

        let mut platform = Tg5040::new();
        platform.init_video();
        platform.init_audio(48000);

        // prepare_sleep 不 panic（killall keymon 未运行——失败静默；
        // settings/sysfs 路径在 Linux 上可访问——真机节点不存在时 read/write 容错）
        platform.prepare_sleep();
        // complete_wake 不 panic（恢复音量/背光）
        platform.complete_wake();

        platform.quit_audio();
        platform.quit_video();
    }

    // ── 音频冒烟测试（SDL dummy 驱动——headless 可运行）──
    // 覆盖：init_audio 不 panic（返回采样率或 0——dummy 驱动可能拒绝，
    // 容错）、push_audio 返回接受帧数、pause/resume 不 panic、
    // quit_audio 后可重新初始化。
    // 音频无事件通道——不受 sdl2-compat 事件值错位影响（输入冒烟的坑）

    #[test]
    fn audio_smoke_init_quit() {
        // SDL 是全局状态——与其他 SDL 测试串行（SDL_TEST_LOCK）
        let _g = SDL_TEST_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("SDL_VIDEODRIVER", "dummy");
            std::env::set_var("SDL_AUDIODRIVER", "dummy");
        }

        let mut platform = Tg5040::new();
        platform.init_video(); // sdl 初始化（audio 依赖）

        // init_audio：dummy 驱动下可能返回 0（无音频硬件）——不 panic 即可
        let rate = platform.init_audio(48000);
        let _ = rate;

        // push_audio 不 panic，返回接受帧数（缓冲独立于设备——0 容量占位时
        // init_audio 已重建 4000 帧）
        let frames = vec![AudioFrame { left: 1, right: 2 }; 100];
        let accepted = platform.push_audio(&frames);
        assert!(accepted <= 100);

        // pause/resume 不 panic（device 可能为 None——容错）
        platform.pause_audio(true);
        platform.pause_audio(false);

        // quit_audio 清理 + 可重新初始化
        platform.quit_audio();
        assert!(platform.audio_device.is_none());
        platform.init_audio(44100);
        platform.quit_audio();

        platform.quit_video();
    }

    // ── 输入冒烟测试（SDL_VIDEODRIVER=dummy——headless 可运行）──
    // 覆盖：init_input 不 panic（dummy 下 joystick 子系统可用但 0 号
    // 手柄不存在——打开失败容错）、quit_input 清理。
    //
    // **环境限制**：事件消费（poll_input/should_wake）在此开发机
    // （Homebrew sdl2-compat 2.32.70）不可测——sdl2-compat 运行时发
    // SDL2 传统事件值（如 MOUSEBUTTONDOWN=0x207），而 sdl2-sys 0.36
    // 预生成绑定是 2.32 新值（0x401），sdl2 crate 转换事件时 panic
    // （non-unwinding abort）。真机/标准 SDL2 无此错位——事件通道的
    // 行为由 input 模块纯函数测试 + 真机验证覆盖

    #[test]
    fn input_smoke_init_poll_quit() {
        // SDL 是全局状态——与 video_smoke 串行（见 SDL_TEST_LOCK 注释）
        let _g = SDL_TEST_LOCK.lock().unwrap();
        unsafe { std::env::set_var("SDL_VIDEODRIVER", "dummy") };

        let mut platform = Tg5040::new();
        // event_pump 依赖 sdl（init_video 填充）
        platform.init_video();
        platform.init_input();
        assert!(platform.event_pump.is_some(), "dummy 下事件泵应创建成功");

        // 无手柄（dummy）——joystick 打开失败容错（不 panic，Option 保持 None）
        assert!(platform.joystick.is_none(), "dummy 下无手柄——打开失败容错");

        platform.quit_input();
        assert!(platform.joystick.is_none());
        assert!(platform.event_pump.is_none());
        platform.quit_video();
    }

    // ── 视频冒烟测试（SDL_VIDEODRIVER=dummy——headless 可运行）──
    // 已验证 dummy 驱动支持 renderer（探针测试 RENDERER OK）
    // 覆盖：init_video 不 panic + 字段填充、flip 不 panic、quit_video 清理

    #[test]
    fn video_smoke_init_quit_flip() {
        // SDL 是全局状态——与 input_smoke 串行（见 SDL_TEST_LOCK 注释）
        let _g = SDL_TEST_LOCK.lock().unwrap();
        // 必须在 SDL_Init 前设置 dummy 驱动（headless 环境）
        // Safety: 测试是单线程的，设置环境变量后立即使用，无并发读——无 UB
        unsafe { std::env::set_var("SDL_VIDEODRIVER", "dummy") };

        let mut platform = Tg5040::new();

        // init_video 不 panic + 字段填充
        platform.init_video();
        assert!(platform.sdl.is_some());
        assert!(platform.canvas.is_some());
        assert!(platform.texture.is_some());

        // flip 空 buffer 不 panic（调用序列正确）
        let buf = VideoBuffer::new(Tg5040::SCREEN_WIDTH, Tg5040::SCREEN_HEIGHT);
        platform.flip(&buf, true);
        platform.flip(&buf, false); // wait_vsync 两种值都应工作

        // quit_video 清理
        platform.quit_video();
        assert!(platform.sdl.is_none());
        assert!(platform.canvas.is_none());
        assert!(platform.texture.is_none());
    }

    #[cfg(feature = "brick")]
    #[test]
    fn device_model_brick() {
        assert_eq!(Tg5040::DEVICE_MODEL, "TrimUI Brick");
    }
}
