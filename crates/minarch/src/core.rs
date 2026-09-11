//! `core` — libretro 核心接口封装：会话生命周期、布局计算、
//! 视频 pending 帧管线、快进帧预算、薄静态层
//!
//! 本模块是 minarch 的运行时心脏，吸收 C 侧四个块：
//!
//! - **会话生命周期**（[`CoreSession`]）：对应 C `Core_init`/`Core_load`/
//!   `Core_reset`/`Core_quit`（minarch.c:2956-3004）
//! - **布局计算**（[`compute_layout`]）：对应 C `selectScaler` 对 tg5040
//!   生效的子集 + C `PLAT_flip` 的 dst 数学（tg5040 platform.c:405-434）
//! - **视频管线**（[`video_handler`] + 薄静态）：对应 C
//!   `video_refresh_callback_main`（minarch.c:2773-2847）——pending 帧
//!   统一模型：处理器只做纯计算写帧，flip 由装配层主循环执行
//! - **快进帧控**（[`ff_frame_budget`]/[`limit_ff`]）：对应 C `limitFF`
//!   （minarch.c:4620-4645）的纯函数化
//!
//! ## 跨模块边界
//!
//! - `CoreSession` 实例由 `main.rs` 装配层持有（与 game.rs 同边界）
//! - 渲染态薄静态（`OnceLock<Mutex<VideoState>>`）与 audio/environment
//!   同构：核心回调线程与主循环共享，Mutex 是唯一诚实载体
//! - 快进标志以本模块静态为唯一事实源，`audio::set_fast_forward` 由
//!   装配层同步调用（core SHALL NOT 反向调用 audio）
//!
//! ## 与原 C 代码的对比
//!
//! C tg5040 把缩放/宽高比全部交给 GPU `SDL_RenderCopy`（platform.c:373-446），
//! Rust tg5040 的 `flip` 是物理尺寸 1:1 上传——运行时画面缩放必须在
//! 软件层完成（本模块 + render crate 的缩放器）。C 的两条视频路径
//! （回调内 flip / 线程 backbuffer）在 Rust 折叠为 pending 帧一条。
//!

use crate::game::Game;
use crate::libretro::{
    Core, RETRO_DEVICE_JOYPAD, RETRO_MEMORY_RTC, RETRO_MEMORY_SAVE_RAM, RetroGameInfo,
};
use crate::sram;
use common::video::{RGB_BLACK, Rect, VideoBuffer};
use render::scaler::{BilinearScaler, IntegerScaler};
use std::cmp::min;
use std::ffi::CString;
use std::io;
use std::path::Path;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

// ── 会话生命周期 ──────────────────────────────────

/// 会话生命周期错误（对应 C 的「静默返回 + LOG_error」失败路径的 Result 化）
///
/// C 对 `retro_load_game` 的 bool 返回值与 fwrite 错误一律静默忽略
/// （minarch.c:2969/:407-430），Rust 显式传播——见 design 决策 2。
#[derive(Debug)]
pub enum SessionError {
    /// `retro_load_game` 返回 false（游戏无法加载）
    GameLoadFailed,
    /// SRAM 写档失败（打开/写入/同步错误，存档丢失不可接受）
    SramWrite { path: String, source: io::Error },
    /// RTC 写档失败
    RtcWrite { path: String, source: io::Error },
}

/// 会话生命周期（C `Core_init`/`Core_load`/`Core_reset`/`Core_quit`）
///
/// 实例由装配层（main.rs）持有；方法以参数接收 `&Core`/`&Game`
/// （config 规则「函数传递尽量使用变量而非结构体」）。
#[derive(Default)]
pub struct CoreSession {
    /// 核心已初始化标志（对应 C `core.initialized`，minarch.c:2959/:2999）
    initialized: bool,
}

impl CoreSession {
    /// 构造会话（未初始化状态）
    pub fn new() -> Self {
        Self { initialized: false }
    }

    /// 初始化核心（对应 C `Core_init`，minarch.c:2956-2960）
    ///
    /// # 参数
    ///
    /// - `core`: 已 `Core::open` 加载的核心
    pub fn init(&mut self, core: &Core) {
        // SAFETY: core 由 Core::open 加载成功，22 个必须符号已绑定非空
        unsafe { (core.init)() };
        self.initialized = true;
    }

    /// 加载游戏并填充信息缓存（对应 C `Core_load`，minarch.c:2961-2986）
    ///
    /// 严格保持 C 的调用顺序：`load_game` → SRAM 读 → RTC 读 →
    /// `get_system_av_info` → `set_controller_port_device(0, JOYPAD)` →
    /// 填充 fps/sample_rate/aspect_ratio。
    ///
    /// # 参数
    ///
    /// - `core`: 核心（`fps`/`sample_rate`/`aspect_ratio` 三个缓存字段由本方法填充）
    /// - `game`: 已打开的游戏（`tmp_path`/`data` 构造 `RetroGameInfo`）
    ///
    /// # 返回值
    ///
    /// - `Ok(())`: 加载成功（SRAM/RTC 读错误不中断，与 C 一致——偏离记录）
    /// - `Err(SessionError::GameLoadFailed)`: `retro_load_game` 返回 false
    pub fn load(&mut self, core: &mut Core, game: &Game) -> Result<(), SessionError> {
        // 构造 RetroGameInfo（C minarch.c:2963-2966：tmp_path 优先）
        let path_str = game
            .tmp_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| game.path.clone());
        let c_path = CString::new(path_str).expect("游戏路径含 NUL");
        let (data_ptr, size) = match &game.data {
            Some(data) => (data.as_ptr() as *const core::ffi::c_void, data.len()),
            None => (core::ptr::null(), 0),
        };
        let info = RetroGameInfo {
            path: c_path.as_ptr(),
            data: data_ptr,
            size,
            meta: core::ptr::null(),
        };

        // SAFETY: info 各指针在 load_game 调用期间有效（c_path/data 均在栈上存活）
        let loaded = unsafe { (core.load_game)(&info) };
        if !loaded {
            return Err(SessionError::GameLoadFailed);
        }

        // SRAM/RTC 读（C minarch.c:2971-2972；读错误不中断加载——偏离记录）
        self.read_save(core, RETRO_MEMORY_SAVE_RAM, game);
        self.read_save(core, RETRO_MEMORY_RTC, game);

        // 必须在 load_game 之后（C minarch.c:2974 注释）
        let mut av_info = unsafe { core::mem::zeroed() };
        // SAFETY: av_info 出参由调用方提供存储
        unsafe { (core.get_system_av_info)(&mut av_info) };
        // SAFETY: mock 与真实核心均支持 JOYPAD 默认设备
        unsafe { (core.set_controller_port_device)(0, RETRO_DEVICE_JOYPAD) };

        core.fps = av_info.timing.fps;
        core.sample_rate = av_info.timing.sample_rate;
        let a = av_info.geometry.aspect_ratio;
        core.aspect_ratio = if a <= 0.0 {
            // C :2981-2982：aspect 非正时回落 base_w/base_h（f64 除法）
            av_info.geometry.base_width as f64 / av_info.geometry.base_height as f64
        } else {
            a as f64
        };

        Ok(())
    }

    /// 复位核心（对应 C `Core_reset`，minarch.c:2987-2989，无门控）
    ///
    /// # 参数
    ///
    /// - `core`: 核心
    pub fn reset(&self, core: &Core) {
        // SAFETY: 符号已绑定
        unsafe { (core.reset)() };
    }

    /// 退出会话：写档、卸载游戏、反初始化（对应 C `Core_quit`，minarch.c:2993-3001）
    ///
    /// 仅 `initialized` 为 true 时执行（C 的 `if (core.initialized)` 门控）；
    /// 写错误传播（C 忽略 fwrite 错误——偏离记录）。
    ///
    /// # 参数
    ///
    /// - `core`: 核心
    /// - `game`: 游戏（`name` 用于存档文件名）
    ///
    /// # 返回值
    ///
    /// - `Ok(())`: 未初始化（空操作）或正常退出
    /// - `Err(SessionError::SramWrite/RtcWrite)`: 写档失败
    pub fn quit(&mut self, core: &Core, game: &Game) -> Result<(), SessionError> {
        if !self.initialized {
            return Ok(());
        }

        self.write_save(core, RETRO_MEMORY_SAVE_RAM, game, SaveKind::Sram)?;
        self.write_save(core, RETRO_MEMORY_RTC, game, SaveKind::Rtc)?;

        // SAFETY: 符号已绑定
        unsafe { (core.unload_game)() };
        // SAFETY: 符号已绑定
        unsafe { (core.deinit)() };
        self.initialized = false;

        Ok(())
    }

    /// 读存档进核心内存（load 阶段；错误吞掉——C `LOG_error + 继续` 语义，
    /// 日志缺失期静默，见 design 决策 2）
    fn read_save(&self, core: &Core, id: u32, game: &Game) {
        // SAFETY: get_memory_data 返回核心内存指针，size 由 get_memory_size 给出
        unsafe {
            let size = (core.get_memory_size)(id);
            if size == 0 {
                return;
            }
            let ptr = (core.get_memory_data)(id) as *mut u8;
            let memory = core::slice::from_raw_parts_mut(ptr, size);
            let path = save_path_for(core, game, id);
            let _ = sram::read_into(memory, Path::new(&path));
        }
    }

    /// 写核心内存到存档文件（quit 阶段；错误传播）
    fn write_save(
        &self,
        core: &Core,
        id: u32,
        game: &Game,
        kind: SaveKind,
    ) -> Result<(), SessionError> {
        // SAFETY: 同 read_save
        unsafe {
            let size = (core.get_memory_size)(id);
            if size == 0 {
                return Ok(());
            }
            let ptr = (core.get_memory_data)(id) as *const u8;
            let memory = core::slice::from_raw_parts(ptr, size);
            let path = save_path_for(core, game, id);
            sram::write_from(memory, Path::new(&path)).map_err(|source| match kind {
                SaveKind::Sram => SessionError::SramWrite { path, source },
                SaveKind::Rtc => SessionError::RtcWrite { path, source },
            })
        }
    }

    /// 写 SRAM + RTC 存档（不卸载核心，对应 C `SRAM_write`/`RTC_write`
    /// 的菜单进入/睡眠前调用点，minarch.c:3114-3115/:4238-4239）
    ///
    /// 与 [`CoreSession::quit`] 的区别：只写档，不 `unload_game`/`deinit`——
    /// 游戏会话保持运行。写错误传播（与 `quit` 同偏离记录）。
    ///
    /// # 参数
    ///
    /// - `core`: 核心
    /// - `game`: 游戏（`name` 用于存档文件名）
    ///
    /// # 返回值
    ///
    /// - `Ok(())`: 写档成功（无内存时同样返回 Ok）
    /// - `Err(SessionError::SramWrite/RtcWrite)`: 写档失败
    pub fn write_saves(&self, core: &Core, game: &Game) -> Result<(), SessionError> {
        self.write_save(core, RETRO_MEMORY_SAVE_RAM, game, SaveKind::Sram)?;
        self.write_save(core, RETRO_MEMORY_RTC, game, SaveKind::Rtc)?;
        Ok(())
    }
}

/// 写档错误归类用（[`CoreSession::write_save`] 内部）
enum SaveKind {
    Sram,
    Rtc,
}

/// 按内存 id 计算存档路径（sram.rs 纯函数接线）
fn save_path_for(core: &Core, game: &Game, id: u32) -> String {
    if id == RETRO_MEMORY_SAVE_RAM {
        sram::save_path(&core.saves_dir, &game.name)
    } else {
        sram::rtc_path(&core.saves_dir, &game.name)
    }
}

// ── 布局计算 ──────────────────────────────────────

/// 屏幕缩放模式（对应 C `screen_scaling` 选项，minarch.c:47）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scaling {
    /// 整数倍居中（源小于屏）
    Native,
    /// 保持核心宽高比的信箱/柱箱
    Aspect,
    /// 整数倍放大后裁掉超屏部分
    Cropped,
    /// 整帧拉伸整屏
    Fullscreen,
}

/// 帧布局：源裁剪矩形 + 目标矩形 + 缩放语义（对应 C renderer 被
/// tg5040 `PLAT_flip` 消费的六元组：src_x/src_y/src_w/src_h/src_p/scale/aspect）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// 源帧裁剪矩形（源空间）
    pub src: Rect,
    /// 目标矩形（屏幕空间，已与屏幕矩形求交）
    pub dst: Rect,
    /// 整数倍率；-1 = 拉伸任意倍（aspect/fullscreen），0 = forced crop
    pub scale: i32,
    /// 0 = native/cropped；>0 = 宽高比；-1 = fullscreen 拉伸
    pub aspect: f32,
}

/// 向上取整除法（CEIL_DIV，对应 C 宏语义）
fn ceildiv(a: u32, b: u32) -> u32 {
    a.div_ceil(b)
}

/// 矩形求交辅助（dst 收窄到屏幕内；求交只收窄不改 src）
fn intersect(r: Rect, screen_w: u32, screen_h: u32) -> Rect {
    let x2 = min(r.x + r.w, screen_w);
    let y2 = min(r.y + r.h, screen_h);
    Rect {
        x: r.x,
        y: r.y,
        w: x2.saturating_sub(r.x),
        h: y2.saturating_sub(r.y),
    }
}

/// 计算帧布局（对应 C `selectScaler` 对 tg5040 生效子集 + `PLAT_flip` 的
/// dst 数学，minarch.c:2536-2772 / tg5040 platform.c:405-434）
///
/// 纯函数、无平台依赖：屏幕尺寸与核心宽高比经参数传入
/// （config 规则「函数传递尽量使用变量而非结构体」）。
///
/// # 参数
///
/// - `src_w`/`src_h`: 核心帧尺寸（像素）
/// - `core_aspect`: 核心宽高比（`Core.aspect_ratio`，session load 后填充）
/// - `screen_w`/`screen_h`: 屏幕物理尺寸（`Platform::SCREEN_WIDTH/HEIGHT`）
/// - `scaling`: 缩放模式（前端选项）
///
/// # 返回值
///
/// 源裁剪 + 目标矩形 + 缩放语义的完整布局。dst 恒与屏幕矩形求交
/// （C 靠 GPU RenderCopy 隐式裁剪，软件路径显式求交——等价语义偏离）。
pub fn compute_layout(
    src_w: u32,
    src_h: u32,
    core_aspect: f64,
    screen_w: u32,
    screen_h: u32,
    scaling: Scaling,
) -> Layout {
    let full_src = Rect {
        x: 0,
        y: 0,
        w: src_w,
        h: src_h,
    };
    let full_screen = Rect {
        x: 0,
        y: 0,
        w: screen_w,
        h: screen_h,
    };

    match scaling {
        Scaling::Fullscreen => Layout {
            src: full_src,
            dst: full_screen,
            scale: -1,
            aspect: -1.0,
        },
        Scaling::Aspect => {
            // C PLAT_flip aspect>0 分支（platform.c:419-434）
            let mut dst_h = screen_h;
            let mut dst_w = (dst_h as f64 * core_aspect) as u32;
            if dst_w > screen_w {
                dst_w = screen_w;
                dst_h = (dst_w as f64 / core_aspect) as u32;
            }
            Layout {
                src: full_src,
                dst: intersect(
                    Rect {
                        x: (screen_w - dst_w) / 2,
                        y: (screen_h - dst_h) / 2,
                        w: dst_w,
                        h: dst_h,
                    },
                    screen_w,
                    screen_h,
                ),
                scale: -1,
                aspect: core_aspect as f32,
            }
        }
        Scaling::Native | Scaling::Cropped => {
            let scale = min(screen_w / src_w, screen_h / src_h);
            if scale == 0 {
                // forced crop（C :2574-2588）：源大于屏，dst 整屏、居中裁剪
                let ox = (screen_w as i64 - src_w as i64) / 2;
                let oy = (screen_h as i64 - src_h as i64) / 2;
                let (mut src_x, mut src_y) = (0u32, 0u32);
                let (mut cw, mut ch) = (src_w, src_h);
                if ox < 0 {
                    src_x = ox.unsigned_abs() as u32;
                    cw = screen_w;
                }
                if oy < 0 {
                    src_y = oy.unsigned_abs() as u32;
                    ch = screen_h;
                }
                Layout {
                    src: Rect {
                        x: src_x,
                        y: src_y,
                        w: cw,
                        h: ch,
                    },
                    dst: full_screen,
                    scale: 0,
                    aspect: 0.0,
                }
            } else if scaling == Scaling::Cropped {
                // C :2593-2625：CEIL_DIV 上下取小后双侧对称裁剪
                let scale = min(ceildiv(screen_w, src_w), ceildiv(screen_h, src_h));
                let scaled_w = src_w * scale;
                let scaled_h = src_h * scale;
                let ox = (screen_w as i64 - scaled_w as i64) / 2;
                let oy = (screen_h as i64 - scaled_h as i64) / 2;
                let (mut src_x, mut src_y, mut dst_x, mut dst_y) = (0u32, 0u32, 0u32, 0u32);
                let (mut cw, mut ch) = (src_w, src_h);
                if ox < 0 {
                    src_x = ox.unsigned_abs() as u32 / scale;
                    cw -= src_x * 2;
                } else {
                    dst_x = ox as u32;
                }
                if oy < 0 {
                    src_y = oy.unsigned_abs() as u32 / scale;
                    ch -= src_y * 2;
                } else {
                    dst_y = oy as u32;
                }
                let dst = intersect(
                    Rect {
                        x: dst_x,
                        y: dst_y,
                        w: cw * scale,
                        h: ch * scale,
                    },
                    screen_w,
                    screen_h,
                );
                let (src_x, src_y, cw, ch, dst) = (src_x, src_y, cw, ch, dst);
                Layout {
                    src: Rect {
                        x: src_x,
                        y: src_y,
                        w: cw,
                        h: ch,
                    },
                    dst,
                    scale: scale as i32,
                    aspect: 0.0,
                }
            } else {
                // Native：整数倍居中（C :2627-2636）
                Layout {
                    src: full_src,
                    dst: Rect {
                        x: (screen_w - src_w * scale) / 2,
                        y: (screen_h - src_h * scale) / 2,
                        w: src_w * scale,
                        h: src_h * scale,
                    },
                    scale: scale as i32,
                    aspect: 0.0,
                }
            }
        }
    }
}

// ── 渲染态薄静态层与快进标志 ──────────────────────

/// 屏幕锐度（对应 C `screen_sharpness` 选项，minarch.c:48）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sharpness {
    /// 双线性插值（C tg5040 SOFT = SDL 纹理默认双线性过滤）
    Soft,
    /// 最近邻（C tg5040 CRISP = hard_scale target 的整数倍放大）
    Crisp,
}

/// 渲染态薄静态（核心回调线程与主循环共享的可变状态）
///
/// 字段与 spec「core 渲染态薄静态层与快进标志」逐字一致。对应 C 的
/// 一组全局：`renderer`（minarch.c:90 起）、`screen_scaling`/`screen_sharpness`
/// （:47-48）、`fast_forward`（:53）、`downsample buffer`（:2506）、
/// `core.aspect_ratio`（:107 起）。
pub struct VideoState {
    /// pending 帧（屏幕物理尺寸，`VideoBuffer::new(screen_w, screen_h)`）
    pub buffer: VideoBuffer,
    /// 有待 flip 的新帧
    pub pending: bool,
    /// 当前布局缓存（尺寸未变不重算）
    pub layout: Option<Layout>,
    /// 上次帧尺寸（变化检测）
    pub last_w: u32,
    /// 上次帧尺寸（变化检测）
    pub last_h: u32,
    /// 快进标志（唯一事实源，见 spec 跨模块边界）
    pub fast_forward: bool,
    /// 快进限速跨帧时钟（`limit_ff` 的 `last_us`；单/双线程共享，
    /// 对应 C `limitFF` 的 static `last_time`，minarch.c:4621）
    pub ff_last_us: u64,
    /// 屏幕缩放模式（对应 C `screen_scaling`）
    pub scaling: Scaling,
    /// 屏幕锐度（对应 C `screen_sharpness`）
    pub sharpness: Sharpness,
    /// 裁剪紧凑源复用缓冲（cropped 模式；对应 C downsample buffer 全局）
    pub scratch: Vec<u16>,
    /// 缩放结果复用缓冲（dst_w×dst_h 紧凑，热路径零分配）
    pub scaled: Vec<u16>,
    /// 核心宽高比（load 后由装配层同步自 `core.aspect_ratio`）
    pub core_aspect: f64,
}

/// 渲染态静态（OnceLock：注册后只读；Mutex：核心回调线程与主循环共享）
static VIDEO_STATE: OnceLock<Mutex<VideoState>> = OnceLock::new();

/// 预渲染钩子（OnceLock：一次性注册后只读）
///
/// 对应 C `Special_render()`（GB DMG 调色板，minarch.c:1388-1390）的
/// 预留挂接位——本 change 不实现 Special 本体，钩子消费方归后续 change。
static PRE_RENDER_HOOK: OnceLock<fn()> = OnceLock::new();

/// 节流时钟（`Instant` 单调毫秒，无锁 CAS——对应 C `last_flip_time`，
/// minarch.c:2778；与 C `SDL_GetTicks` 同语义的单调时钟）
static LAST_PRESENT: AtomicU64 = AtomicU64::new(0);

/// 进程启动时刻（`now_millis` 的零点，语义等价 C `SDL_GetTicks` 的
/// 启动后毫秒数；OnceLock 惰性初始化）
static START_INSTANT: OnceLock<Instant> = OnceLock::new();

/// 当前单调时钟毫秒（进程启动至今）
fn now_millis() -> u64 {
    let start = *START_INSTANT.get_or_init(Instant::now);
    start.elapsed().as_millis() as u64
}

/// 快进节流窗口（毫秒）
///
/// C :2783-2787 的 10ms 调参产物（"10 seems to be the sweet spot…"），
/// 保留为模块常量——实机调参只改这一处（design Open Questions）。
const FF_THROTTLE_MS: u64 = 10;

/// 初始化渲染态静态（装配层在启动时调用一次）
///
/// # 参数
///
/// - `screen_w`/`screen_h`: 屏幕物理尺寸（`Platform::SCREEN_WIDTH/HEIGHT`）
///
/// # 返回值
///
/// - `Ok(())`: 首次注册成功
/// - `Err(VideoState)`: 已注册过（重复注册是误用），携带本次传入的原值。
///   错误携带整个状态是 `OnceLock::set` 的原样语义（调用方通常只关心
///   `is_err()`）；`VideoState` 较大但仅出现在重复注册的误用路径，无
///   性能影响
#[allow(clippy::result_large_err)]
pub fn init(screen_w: u32, screen_h: u32) -> Result<(), VideoState> {
    match VIDEO_STATE.set(Mutex::new(VideoState {
        buffer: VideoBuffer::new(screen_w, screen_h),
        pending: false,
        layout: None,
        last_w: 0,
        last_h: 0,
        fast_forward: false,
        ff_last_us: 0,
        scaling: Scaling::Native,
        sharpness: Sharpness::Soft,
        scratch: Vec::new(),
        scaled: Vec::new(),
        core_aspect: 0.0,
    })) {
        Ok(()) => Ok(()),
        Err(state) => Err(state.into_inner().unwrap_or_else(|p| p.into_inner())),
    }
}

/// pending 帧一次性消费（主循环每帧调用；flip 由装配层经闭包执行）
///
/// 单次持锁完成「判 pending → 执行闭包 → 清 pending」——无检查-使用竞态
/// （design 决策 1）。
///
/// # 参数
///
/// - `f`: 收到 pending 帧引用的闭包（装配层在闭包内调 `Platform::flip`）
///
/// # 返回值
///
/// - `true`: 有 pending 帧，闭包已执行、pending 已清空
/// - `false`: 无 pending 帧，闭包未执行（未 `init` 时同样安全返回 false）
pub fn present_frame(f: impl FnOnce(&VideoBuffer)) -> bool {
    let Some(state) = VIDEO_STATE.get() else {
        return false;
    };
    let Ok(mut guard) = state.lock() else {
        return false;
    };
    if !guard.pending {
        return false;
    }
    f(&guard.buffer);
    guard.pending = false;
    true
}

/// 设置快进标志（装配层在快进切换时调用）
///
/// 本静态是会话级快进标志的唯一事实源；`audio::set_fast_forward` 由
/// 装配层同步调用（core SHALL NOT 反向调用 audio，模块单向依赖）。
///
/// 未 `init` 时为空操作（spec「未 init 时安全默认」）。
///
/// # 参数
///
/// - `enable`: 是否启用快进
pub fn set_fast_forward(enable: bool) {
    if let Some(state) = VIDEO_STATE.get()
        && let Ok(mut guard) = state.lock()
    {
        guard.fast_forward = enable;
    }
}

/// 读取快进标志（视频节流与 `limit_ff` 消费）
///
/// 未 `init` 时返回 `false`（安全默认）。
pub fn fast_forward() -> bool {
    VIDEO_STATE
        .get()
        .and_then(|state| state.lock().ok())
        .is_some_and(|guard| guard.fast_forward)
}

/// 同步前端选项（装配层从前端选项表调用，对应 C 全局
/// `screen_scaling`/`screen_sharpness` 的 Rust 化，design 决策 13）
///
/// 未 `init` 时为空操作。
///
/// # 参数
///
/// - `scaling`: 屏幕缩放模式
/// - `sharpness`: 屏幕锐度
pub fn set_frontend(scaling: Scaling, sharpness: Sharpness) {
    if let Some(state) = VIDEO_STATE.get()
        && let Ok(mut guard) = state.lock()
    {
        guard.scaling = scaling;
        guard.sharpness = sharpness;
    }
}

/// 同步核心宽高比（装配层在会话 load 后调用，对应 C `core.aspect_ratio`）
///
/// 未 `init` 时为空操作。
///
/// # 参数
///
/// - `aspect`: `Core.aspect_ratio`（load 后已填充/回落）
pub fn set_core_aspect(aspect: f64) {
    if let Some(state) = VIDEO_STATE.get()
        && let Ok(mut guard) = state.lock()
    {
        guard.core_aspect = aspect;
    }
}

/// 注册预渲染钩子（一次性，对应 C `Special_render` 的未来挂接位）
///
/// `video_handler` 在节流检查**之前**调用该钩子（C :2776 顺序：钩子
/// 每帧执行、不受快进节流影响）。未注册时跳过。与 environment 的
/// `RUMBLE_HOOK` 同构。
///
/// # 参数
///
/// - `hook`: 每帧渲染前执行的钩子
///
/// # 返回值
///
/// - `Ok(())`: 首次注册成功
/// - `Err(fn())`: 已注册过，携带本次传入的钩子原值
pub fn set_pre_render_hook(hook: fn()) -> Result<(), fn()> {
    PRE_RENDER_HOOK.set(hook)
}

/// 视频刷新处理器（注册进 `FrontendState.video_refresh` 的入口）
///
/// 执行顺序与 C `video_refresh_callback_main`（minarch.c:2773-2847）一致：
/// 预渲染钩子 → 快进 10ms 节流 → 空帧返回 → 尺寸变化重算+清黑 →
/// 裁剪缩放写 pending。帧数据仅在回调期间有效，本函数在返回前完成
/// 全部拷贝——禁止存裸指针（design 决策 1）。
///
/// # Safety
///
/// `data` 指针由 libretro 核心经回调传入：仅在本回调执行期间有效，
/// 长度 SHALL 为 `pitch × height` 个 `u16`（RGB565），`width`/`height`/
/// `pitch` 与核心上报一致。核心保证该契约（libretro 规范），本函数
/// 在返回前完成拷贝，不持有指针。
pub fn video_handler(data: *const core::ffi::c_void, width: u32, height: u32, pitch: usize) {
    // ① 预渲染钩子（C :2776，先于节流——DMG 调色板刷新必须帧帧跑）
    if let Some(hook) = PRE_RENDER_HOOK.get() {
        hook();
    }

    // ② 快进 10ms 节流（C :2787）：在拷贝前 return，省掉全部缩放开销。
    // 时钟戳：进程启动至今的单调毫秒（`Instant` 语义等价 C `SDL_GetTicks`，
    // design 决策 1）；入口处一次性记录，空帧与节流帧不更新（与 C 一致）
    let now_ms = now_millis();
    let throttle_ok = fast_forward()
        && LAST_PRESENT.load(Ordering::Relaxed) != 0
        && now_ms.saturating_sub(LAST_PRESENT.load(Ordering::Relaxed)) < FF_THROTTLE_MS;
    if throttle_ok {
        return;
    }
    LAST_PRESENT.store(now_ms, Ordering::Relaxed);
    // ③ 空帧直接返回（C :2799）——不更新节流时钟与 pending 标志
    if data.is_null() {
        return;
    }

    let Some(state) = VIDEO_STATE.get() else {
        return;
    };
    let Ok(mut guard) = state.lock() else {
        return;
    };

    // ④ 尺寸变化检测（C :2807-2810）：重算布局并清空 pending 帧为黑色
    if width != guard.last_w || height != guard.last_h {
        let layout = compute_layout(
            width,
            height,
            guard.core_aspect,
            guard.buffer.width,
            guard.buffer.height,
            guard.scaling,
        );
        let (bw, bh) = (guard.buffer.width, guard.buffer.height);
        guard.buffer.fill_rect(
            Rect {
                x: 0,
                y: 0,
                w: bw,
                h: bh,
            },
            RGB_BLACK,
        );
        guard.layout = Some(layout);
        guard.last_w = width;
        guard.last_h = height;
    }
    let Some(layout) = guard.layout else {
        return;
    };

    // ⑤ 裁剪缩放：源帧（RGB565、pitch 行距）→ scratch（必要时）→
    //    scaler → scaled → 逐行拷入 pending 帧的 dst 矩形
    let src_w = layout.src.w;
    let src_h = layout.src.h;
    let dst_w = layout.dst.w;
    let dst_h = layout.dst.h;
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return;
    }

    // SAFETY: data 仅在回调期间有效（本函数返回前完成全部读取）；
    // 长度 = pitch × height 个 u16（libretro 规范，见 # Safety 章节）。
    let frame = unsafe { core::slice::from_raw_parts(data.cast::<u16>(), pitch * height as usize) };

    // 裁剪紧凑源（有 src 偏移或 sp != sw 时经 scratch 重排）。
    // 解构为不相交的字段借用：scratch 承载裁剪紧凑源、scaled 承载缩放结果、
    // buffer 承载最终 pending 帧——三者互不重叠，可同时借用（design 决策 12
    // 零分配热路径：复用 scratch/scaled，不逐帧 to_vec）。
    let VideoState {
        buffer,
        scratch,
        scaled,
        sharpness,
        ..
    } = &mut *guard;

    let sw = width;
    let sp = pitch as u32;
    let needs_crop = layout.src.x != 0 || layout.src.y != 0 || sp != sw;
    if needs_crop {
        scratch.clear();
        scratch.reserve((src_w * src_h) as usize);
        for row in 0..src_h {
            let src_start = ((layout.src.y + row) * sp + layout.src.x) as usize;
            let end = src_start + src_w as usize;
            scratch.extend_from_slice(&frame[src_start..end]);
        }
    }
    let compact_src: &[u16] = if needs_crop {
        scratch.as_slice()
    } else {
        &frame[..(src_w * src_h) as usize]
    };

    // 缩放器选择（design 决策 4 映射表）
    type ScaleFn = Box<dyn Fn(&[u16], &mut [u16])>;
    let scaler: ScaleFn = match (*sharpness, layout.scale) {
        (Sharpness::Crisp, s) if s > 0 => {
            let s = s as u32;
            Box::new(move |src: &[u16], dst: &mut [u16]| {
                IntegerScaler::new(s, s).scale(src, src_w, src_h, src_w, dst, dst_w);
            })
        }
        _ => {
            let (sw, sh) = (src_w, src_h);
            Box::new(move |src: &[u16], dst: &mut [u16]| {
                BilinearScaler::new(sw, sh, dst_w, dst_h).scale(src, sw, sh, sw, dst, dst_w);
            })
        }
    };

    // 缩放结果复用缓冲（dst_w×dst_h 紧凑，热路径零分配）
    let scaled_len = (dst_w * dst_h) as usize;
    if scaled.len() < scaled_len {
        scaled.resize(scaled_len, 0);
    }
    let dst = &mut scaled[..scaled_len];
    scaler(compact_src, dst);

    // 逐行拷入 pending 帧的 dst 矩形（buffer.pitch 行距）
    let bp = buffer.pitch as usize;
    for row in 0..dst_h as usize {
        let buf_start = (layout.dst.y as usize + row) * bp + layout.dst.x as usize;
        let src_start = row * dst_w as usize;
        buffer.pixels[buf_start..buf_start + dst_w as usize]
            .copy_from_slice(&dst[src_start..src_start + dst_w as usize]);
    }

    // ⑥ 置 pending 标志（C :2846；节流时钟已在入口处记录）
    guard.pending = true;
}

// ── 快进帧预算（limit_ff 纯函数化） ──────────────

/// 计算快进帧预算（微秒/帧）
///
/// 对应 C `limitFF` 的 `1000000 / (core.fps * (max_ff_speed + 1))`
/// （minarch.c:4626）。C 用 static 缓存预算，Rust 纯函数化——预算
/// 重算上移为调用方职责（fps/max_speed 变化时重算传入，偏离记录）。
///
/// # 参数
///
/// - `fps`: 核心帧率（`Core.fps`，load 后填充）
/// - `max_speed`: 快进上限倍率（前端选项 `max_ff_speed`）
///
/// # 返回值
///
/// 每帧允许的微秒数（`1_000_000 / (fps × (max_speed+1))`，整数除法）
pub fn ff_frame_budget(fps: f64, max_speed: u32) -> u64 {
    1_000_000 / (fps * (max_speed + 1) as f64) as u64
}

/// 快进限速纯函数（对应 C `limitFF`，minarch.c:4620-4645 的纯化）
///
/// 时钟与 sleep 由装配层注入/执行（Platform 无 sleep 方法——纯函数化
/// 消除该缺口，design 决策 9）。
///
/// # 参数
///
/// - `now_us`: 当前时钟（微秒，装配层经 `Platform::now_ms` 注入）
/// - `last_us`: 上次推进时钟（微秒）
/// - `budget_us`: 帧预算（[`ff_frame_budget`] 计算结果）
/// - `fast_forward`: 快进标志（本模块 `fast_forward()` 或装配层缓存值）
/// - `max_speed`: 快进上限倍率（0 = 不生效，直通）
///
/// # 返回值
///
/// - `.0`: 建议延迟毫秒（封顶 16ms，C :4636 `delay<17`；不生效时为 0）
/// - `.1`: 下一次的 `last_us`（生效时按预算累计推进，否则重置为 `now_us`）
///
/// 仅在 `fast_forward && max_speed > 0` 时生效；`elapsed >= budget` 或
/// 时钟异常（elapsed 为 0/异常大）时 `next_last = now`（C 的
/// `elapsed>0 && elapsed<0x80000` 门控）。
pub fn limit_ff(
    now_us: u64,
    last_us: u64,
    budget_us: u64,
    fast_forward: bool,
    max_speed: u32,
) -> (u32, u64) {
    if !fast_forward || max_speed == 0 {
        return (0, now_us);
    }
    if last_us == 0 {
        return (0, now_us);
    }
    let elapsed = now_us.saturating_sub(last_us);
    if elapsed == 0 || elapsed >= 0x80000 {
        return (0, now_us);
    }
    if elapsed < budget_us {
        let delay = (budget_us - elapsed) / 1000;
        let delay = min(delay, 16) as u32;
        (delay, last_us + budget_us)
    } else {
        (0, now_us)
    }
}

/// 快进限速统一入口（装配层/核心线程每帧调用）
///
/// 在锁内完成「读 `ff_last_us` → `limit_ff` → 写回 `next_last_us`」，
/// 单线程（主线程调用）与线程化（核心线程调用）共享同一时钟——对应
/// C `limitFF` 的全局 static `last_time`（minarch.c:4621）在 Rust 的
/// 显式归属（design 决策 4）。
///
/// # 参数
///
/// - `budget_us`: 帧预算（调用方用 `ff_frame_budget(core.fps, max_speed)`
///   计算——fps 在 `Core.fps`，预算计算上移为调用方职责，偏离记录）
/// - `max_speed`: 快进上限倍率（前端选项 `max_ff_speed`；0 = 不生效）
///
/// # 返回值
///
/// - `.0`: 建议延迟毫秒（封顶 16ms；未 `init` 或直通时为 0）
/// - `.1`: 是否产生延迟（调用方据此 `sleep`；未 `init` 时为 false）
///
/// 未 `init` 时安全返回 `(0, false)`（与 `present_frame` 未 init 语义一致）。
pub fn limit_ff_now(budget_us: u64, max_speed: u32) -> (u32, bool) {
    let Some(state) = VIDEO_STATE.get() else {
        return (0, false);
    };
    let Ok(mut guard) = state.lock() else {
        return (0, false);
    };
    if !guard.fast_forward || max_speed == 0 {
        return (0, false);
    }
    let now_us = now_millis() * 1000;
    let (delay, next_last) = limit_ff(now_us, guard.ff_last_us, budget_us, true, max_speed);
    guard.ff_last_us = next_last;
    (delay, delay > 0)
}

// ── 核心线程（thread_video 线程模式，方案 A 优雅退出） ──────

/// 核心线程门控标志（对应 C 全局 `should_run_core`，minarch.c:30）
///
/// 归属装配层持有 `Arc<AtomicBool>` 实例（与 `RunState` 同层，非本模块
/// 静态），核心线程闭包捕获引用——菜单开关/线程切换/退出序列经它门控。
pub type ShouldRunCore = std::sync::Arc<std::sync::atomic::AtomicBool>;

/// 创建核心线程（对应 C `pthread_create(coreThread)`，minarch.c:4736）
///
/// 方案 A 优雅退出：闭包循环读 `should_run`，`false` 时自退出（取代 C
/// 的 `pthread_cancel` 粗暴中断——C 可能在 `core.run()` 执行中途杀线程，
/// 属未定义行为，Rust 不继承）。
///
/// 每轮循环：`should_run.load(Acquire)` → false 自退出；true → 调
/// `core.run()` → 快进限速（`limit_ff_now` 共享时钟，design 决策 4）→
/// 需要时 `sleep`。
///
/// **`Arc<Core>` 的必要性**：线程执行 `core.run()` 需要 `.so` 保持加载
/// （`Core.handle: Library` 持有 dlopen 句柄）。`Core` 为 `Send + Sync`
/// （spec 编译期断言），装配层以 `Arc` 共享——线程 clone 一份，`Library`
/// 引用计数保持 `.so` 存活。装配层 SHALL 在停线程 join 后才调
/// `CoreSession::quit`（`unload_game`/`deinit` 不与 `run` 并发）。
///
/// # 参数
///
/// - `core`: 已加载的核心（`Arc` 共享；线程 clone 保持 `.so` 存活）
/// - `should_run`: 门控标志（装配层持有）
/// - `budget_us`: 快进帧预算（`ff_frame_budget(core.fps, max_ff_speed)`）
/// - `max_speed`: 快进上限倍率
///
/// # 返回值
///
/// 线程句柄（装配层持有，切换/退出时 `join`）
pub fn spawn_core_thread(
    core: std::sync::Arc<Core>,
    should_run: ShouldRunCore,
    budget_us: u64,
    max_speed: u32,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("minarch-core".into())
        .spawn(move || {
            while should_run.load(std::sync::atomic::Ordering::Acquire) {
                // SAFETY: 符号已绑定（Core::open 保证）；装配层在 join
                // 后才调 unload/deinit（spec「退出序列」），线程存活
                // 期间符号与 .so 均有效（Arc 保持 Library 存活）
                unsafe { (core.run)() };
                let (delay_ms, need_sleep) = limit_ff_now(budget_us, max_speed);
                if need_sleep && delay_ms > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms as u64));
                }
            }
        })
        .expect("核心线程创建失败")
}
