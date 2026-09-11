//! libretro ABI 地基
//!
//! 本模块是 minarch 的地基层：手写 libretro.h 子集的 FFI 定义。
//! 所有结构体布局、常量数值均以 vendored 权威头
//! `tests/fixtures/libretro.h` 为准（来源 commit
//! `b894fa4ed3a7ed6247dd4745cc2cc4f50042b7ee`）。
//!
//! ## 内容
//!
//! - **ABI 常量**：`RETRO_ENVIRONMENT_*` 命令码、设备/按键/内存/像素格式常量
//! - **ABI 类型**：17 个 `#[repr(C)]` 结构体 + 3 个 `#[repr(u32)]` 枚举
//! - **函数指针类型**：6 个前端回调 + 22 个核心导出符号的签名别名
//! - **`Core` 加载器**：dlopen/dlsym 的 libloading 等价物
//!   （对应 C `Core_open`，minarch.c:2891-2955）
//! - **静态回调桥**：6 个 `extern "C"` trampoline + 一次性注册
//!
//! ## 与原 C 代码的对比
//!
//! 原版 `minarch.c` 构建时 `git clone libretro-common` 获得 `libretro.h`，
//! 用 `dlopen`+`dlsym` 绑定符号、以全局变量承接核心回调。Rust 版：
//!
//! - `dlfcn` → `libloading`（跨平台、错误可诊断）
//! - 全局变量回调 → `OnceLock<FrontendState>` 静态桥（注册后只读，无锁）
//! - C 结构体 → `#[repr(C)]` 手写子集，布局由 `tests/libretro_abi.rs` 校验
//!
//! ## 安全边界
//!
//! 核心 `.so` 是外部的 C 代码，所有调用均发生在 `unsafe` 块内。
//! 字符串字段（`*const c_char`）所有权归核心：前端只读、不释放
//! （libretro.h 规定这些是静态字符串）。
//!

use core::ffi::{c_char, c_void};
use core::fmt;
use std::ffi::CStr;
use std::path::Path;
use std::sync::OnceLock;

use libloading::Library;

// ═══════════════════════════════════════════════════════════════
// 常量 —— RETRO_ENVIRONMENT（对应 libretro.h:741-2429）
// ═══════════════════════════════════════════════════════════════

/// 实验性环境命令标志位（`0x10000`）——与基础命令码按位或
pub const RETRO_ENVIRONMENT_EXPERIMENTAL: u32 = 0x10000;

/// 核心请求前端处理画面旋转（C 版注释掉未启用，保留定义）
pub const RETRO_ENVIRONMENT_SET_ROTATION: u32 = 1;
/// 查询过扫描区域是否可用（数据：`bool *`）
pub const RETRO_ENVIRONMENT_GET_OVERSCAN: u32 = 2;
/// 查询前端是否支持画面复制（数据：`bool *`）
pub const RETRO_ENVIRONMENT_GET_CAN_DUPE: u32 = 3;
/// 核心向前端发送提示消息（数据：`RetroMessage *`）
pub const RETRO_ENVIRONMENT_SET_MESSAGE: u32 = 6;
/// 核心请求性能等级（数据：`u32 *`）
pub const RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL: u32 = 8;
/// 查询 BIOS 目录（数据：`*const c_char *` 出参）
pub const RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY: u32 = 9;
/// 核心请求像素格式（数据：`RetroPixelFormat *`，仅支持 RGB565）
pub const RETRO_ENVIRONMENT_SET_PIXEL_FORMAT: u32 = 10;
/// 核心上报按键描述符（数据：`RetroInputDescriptor *` 数组）
pub const RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS: u32 = 11;
/// 核心上报换碟接口（数据：`RetroDiskControlCallback *`）
pub const RETRO_ENVIRONMENT_SET_DISK_CONTROL_INTERFACE: u32 = 13;
/// 核心查询选项值（数据：`RetroVariable *`，前端回填 value）
pub const RETRO_ENVIRONMENT_GET_VARIABLE: u32 = 15;
/// 核心上报变量定义（数据：`RetroVariable *` 数组，旧式 API）
pub const RETRO_ENVIRONMENT_SET_VARIABLES: u32 = 16;
/// 查询核心选项是否被修改（数据：`bool *` 出参）
pub const RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE: u32 = 17;
/// 核心声明支持无游戏运行（数据：`bool *`）
pub const RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME: u32 = 18;
/// 核心注册帧时间回调（C 版未使用，保留定义）
pub const RETRO_ENVIRONMENT_SET_FRAME_TIME_CALLBACK: u32 = 21;
/// 核心注册音频回调（C 版未使用，保留定义）
pub const RETRO_ENVIRONMENT_SET_AUDIO_CALLBACK: u32 = 22;
/// 查询振动接口（数据：`RetroRumbleInterface *` 出参）
pub const RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE: u32 = 23;
/// 查询输入设备能力（数据：`u32 *` 出参，位掩码）
pub const RETRO_ENVIRONMENT_GET_INPUT_DEVICE_CAPABILITIES: u32 = 24;
/// 查询日志接口（数据：`RetroLogCallback *` 出参）
pub const RETRO_ENVIRONMENT_GET_LOG_INTERFACE: u32 = 27;
/// 查询存档目录（数据：`*const c_char *` 出参）
pub const RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY: u32 = 31;
/// 核心上报控制器信息（数据：`RetroControllerInfo *` 数组）
pub const RETRO_ENVIRONMENT_SET_CONTROLLER_INFO: u32 = 35;
/// 查询前端持有的软件帧缓冲（实验性，数据：`RetroFramebuffer *`）
pub const RETRO_ENVIRONMENT_GET_CURRENT_SOFTWARE_FRAMEBUFFER: u32 =
    40 | RETRO_ENVIRONMENT_EXPERIMENTAL;
/// 查询音频/视频使能状态（实验性，数据：`i32 *` 出参）
pub const RETRO_ENVIRONMENT_GET_AUDIO_VIDEO_ENABLE: u32 = 47 | RETRO_ENVIRONMENT_EXPERIMENTAL;
/// 查询输入位掩码支持（实验性，数据：`bool *` 出参）
pub const RETRO_ENVIRONMENT_GET_INPUT_BITMASKS: u32 = 51 | RETRO_ENVIRONMENT_EXPERIMENTAL;
/// 查询核心选项 API 版本（数据：`u32 *` 出参，返回 1）
pub const RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION: u32 = 52;
/// 核心上报选项定义（数据：`RetroCoreOptionDefinition *` 数组）
pub const RETRO_ENVIRONMENT_SET_CORE_OPTIONS: u32 = 53;
/// 核心上报国际化选项定义（数据：`RetroCoreOptionsIntl *`）
pub const RETRO_ENVIRONMENT_SET_CORE_OPTIONS_INTL: u32 = 54;
/// 核心上报选项显示状态（数据：`RetroCoreOptionDisplay *`）
pub const RETRO_ENVIRONMENT_SET_CORE_OPTIONS_DISPLAY: u32 = 55;
/// 查询换碟接口版本（数据：`u32 *` 出参，返回 1）
pub const RETRO_ENVIRONMENT_GET_DISK_CONTROL_INTERFACE_VERSION: u32 = 57;
/// 核心上报扩展换碟接口（数据：`RetroDiskControlExtCallback *`）
pub const RETRO_ENVIRONMENT_SET_DISK_CONTROL_EXT_INTERFACE: u32 = 58;
/// 核心上报内容信息覆盖（数据：`RetroSystemContentInfoOverride *`）
pub const RETRO_ENVIRONMENT_SET_CONTENT_INFO_OVERRIDE: u32 = 65;
/// 核心上报单个变量定义（数据：`RetroVariable *` 或 `i32 *`）
pub const RETRO_ENVIRONMENT_SET_VARIABLE: u32 = 70;

// ═══════════════════════════════════════════════════════════════
// 常量 —— 设备与按键（对应 libretro.h:188-405）
// ═══════════════════════════════════════════════════════════════

/// 设备类型：标准手柄
pub const RETRO_DEVICE_JOYPAD: u32 = 1;
/// 设备类型：模拟摇杆
pub const RETRO_DEVICE_ANALOG: u32 = 5;

/// 手柄按键：B
pub const RETRO_DEVICE_ID_JOYPAD_B: u32 = 0;
/// 手柄按键：Y
pub const RETRO_DEVICE_ID_JOYPAD_Y: u32 = 1;
/// 手柄按键：Select
pub const RETRO_DEVICE_ID_JOYPAD_SELECT: u32 = 2;
/// 手柄按键：Start
pub const RETRO_DEVICE_ID_JOYPAD_START: u32 = 3;
/// 手柄按键：上
pub const RETRO_DEVICE_ID_JOYPAD_UP: u32 = 4;
/// 手柄按键：下
pub const RETRO_DEVICE_ID_JOYPAD_DOWN: u32 = 5;
/// 手柄按键：左
pub const RETRO_DEVICE_ID_JOYPAD_LEFT: u32 = 6;
/// 手柄按键：右
pub const RETRO_DEVICE_ID_JOYPAD_RIGHT: u32 = 7;
/// 手柄按键：A
pub const RETRO_DEVICE_ID_JOYPAD_A: u32 = 8;
/// 手柄按键：X
pub const RETRO_DEVICE_ID_JOYPAD_X: u32 = 9;
/// 手柄按键：L
pub const RETRO_DEVICE_ID_JOYPAD_L: u32 = 10;
/// 手柄按键：R
pub const RETRO_DEVICE_ID_JOYPAD_R: u32 = 11;
/// 手柄按键：L2
pub const RETRO_DEVICE_ID_JOYPAD_L2: u32 = 12;
/// 手柄按键：R2
pub const RETRO_DEVICE_ID_JOYPAD_R2: u32 = 13;
/// 手柄按键：L3（摇杆按下）
pub const RETRO_DEVICE_ID_JOYPAD_L3: u32 = 14;
/// 手柄按键：R3（摇杆按下）
pub const RETRO_DEVICE_ID_JOYPAD_R3: u32 = 15;
/// 手柄按键：全部按键位掩码（input_state 查询 id 为 MASK 时返回整组状态）
pub const RETRO_DEVICE_ID_JOYPAD_MASK: u32 = 256;

/// 模拟摇杆轴：X
pub const RETRO_DEVICE_ID_ANALOG_X: u32 = 0;
/// 模拟摇杆轴：Y
pub const RETRO_DEVICE_ID_ANALOG_Y: u32 = 1;
/// 模拟摇杆索引：左摇杆
pub const RETRO_DEVICE_INDEX_ANALOG_LEFT: u32 = 0;
/// 模拟摇杆索引：右摇杆
pub const RETRO_DEVICE_INDEX_ANALOG_RIGHT: u32 = 1;

// ═══════════════════════════════════════════════════════════════
// 常量 —— 内存 / 像素格式 / 杂项（对应 libretro.h:109-527）
// ═══════════════════════════════════════════════════════════════

/// 内存类型：电池存档（SRAM）
pub const RETRO_MEMORY_SAVE_RAM: u32 = 0;
/// 内存类型：实时时钟（RTC）
pub const RETRO_MEMORY_RTC: u32 = 1;

/// 像素格式：0RGB1555（弃用，仅兼容）
pub const RETRO_PIXEL_FORMAT_0RGB1555: u32 = 0;
/// 像素格式：XRGB8888
pub const RETRO_PIXEL_FORMAT_XRGB8888: u32 = 1;
/// 像素格式：RGB565（minarch 唯一支持的格式，对应设备 16 位屏幕）
pub const RETRO_PIXEL_FORMAT_RGB565: u32 = 2;

/// AV 使能位：视频（GET_AUDIO_VIDEO_ENABLE 出参位）
pub const RETRO_AV_ENABLE_VIDEO: u32 = 1 << 0;
/// AV 使能位：音频（GET_AUDIO_VIDEO_ENABLE 出参位）
pub const RETRO_AV_ENABLE_AUDIO: u32 = 1 << 1;

/// libretro API 版本（核心选项 v1 布局的判别依据）
pub const RETRO_API_VERSION: u32 = 1;
/// 核心选项 values 数组容量（`RetroCoreOptionDefinition` 内嵌数组长度）
pub const RETRO_NUM_CORE_OPTION_VALUES_MAX: usize = 128;

// ═══════════════════════════════════════════════════════════════
// 枚举
// ═══════════════════════════════════════════════════════════════

/// 像素格式（对应 libretro.h `enum retro_pixel_format`）
///
/// minarch 仅支持 `Rgb565`——对应 C 版 `SET_PIXEL_FORMAT` 处理
/// （minarch.c:1899 拒绝非 RGB565）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RetroPixelFormat {
    /// 0RGB1555（弃用格式，仅兼容声明）
    Rgb01555 = RETRO_PIXEL_FORMAT_0RGB1555,
    /// XRGB8888
    Xrgb8888 = RETRO_PIXEL_FORMAT_XRGB8888,
    /// RGB565——设备 16 位屏幕的原生格式
    Rgb565 = RETRO_PIXEL_FORMAT_RGB565,
}

/// 振动强度等级（对应 libretro.h `enum retro_rumble_effect`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RetroRumbleEffect {
    /// 强振动
    Strong = 0,
    /// 弱振动
    Weak = 1,
}

/// 核心日志级别（对应 libretro.h `enum retro_log_level`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RetroLogLevel {
    /// 调试信息
    Debug = 0,
    /// 常规信息
    Info = 1,
    /// 警告
    Warn = 2,
    /// 错误
    Error = 3,
}

// ═══════════════════════════════════════════════════════════════
// 前端回调函数指针类型（对应 libretro.h:7891-8012）
// ═══════════════════════════════════════════════════════════════

/// 环境回调签名：核心用命令码 + 数据指针向前端提问/上报
///
/// # 参数
///
/// - `cmd`: `RETRO_ENVIRONMENT_*` 命令码
/// - `data`: 随命令语义变化的载荷指针（由核心提供，按命令解释）
///
/// # 返回值
///
/// 命令被处理返回 `true`，不支持返回 `false`。
pub type RetroEnvironmentFn = unsafe extern "C" fn(cmd: u32, data: *mut c_void) -> bool;

/// 视频刷新回调签名：核心每渲染一帧调用
///
/// # 参数
///
/// - `data`: 帧像素起始地址（`NULL` 表示空帧）
/// - `width`/`height`: 帧尺寸（像素）
/// - `pitch`: 每行字节数
pub type RetroVideoRefreshFn =
    unsafe extern "C" fn(data: *const c_void, width: u32, height: u32, pitch: usize);

/// 音频采样回调签名：单样本（左右声道各 1 个 `i16`）
pub type RetroAudioSampleFn = unsafe extern "C" fn(left: i16, right: i16);

/// 音频批量回调签名：一批立体声帧
///
/// # 参数
///
/// - `data`: 交错样本数据（左/右交替）
/// - `frames`: 帧数（每帧 2 个 `i16`）
///
/// # 返回值
///
/// 实际消费的帧数。
pub type RetroAudioSampleBatchFn = unsafe extern "C" fn(data: *const i16, frames: usize) -> usize;

/// 输入轮询回调签名：核心在读取输入前调用，前端在此刷新按键状态
pub type RetroInputPollFn = unsafe extern "C" fn();

/// 输入状态回调签名：核心查询具体按键/摇杆状态
///
/// # 参数
///
/// - `port`: 端口（minarch 只支持 0）
/// - `device`: `RETRO_DEVICE_*`
/// - `index`: 摇杆索引等（手柄为 0）
/// - `id`: `RETRO_DEVICE_ID_*` 按键/轴 id
///
/// # 返回值
///
/// 按键为 0/1，摇杆轴为 -32768..32767。
pub type RetroInputStateFn =
    unsafe extern "C" fn(port: u32, device: u32, index: u32, id: u32) -> i16;

/// 日志回调签名（C 可变参，Rust 侧仅声明类型不实现——见 design 决策 8）
pub type RetroLogPrintFn = unsafe extern "C" fn(level: RetroLogLevel, fmt: *const c_char, ...);

/// 振动状态回调签名（核心经 `RetroRumbleInterface` 调用前端）
pub type RetroSetRumbleStateFn =
    unsafe extern "C" fn(port: u32, effect: RetroRumbleEffect, strength: u16) -> bool;

/// 换碟：弹出/合入光盘
pub type RetroSetEjectStateFn = unsafe extern "C" fn(ejected: bool) -> bool;
/// 换碟：查询弹出状态
pub type RetroGetEjectStateFn = unsafe extern "C" fn() -> bool;
/// 换碟：查询当前碟片索引
pub type RetroGetImageIndexFn = unsafe extern "C" fn() -> u32;
/// 换碟：设置当前碟片索引
pub type RetroSetImageIndexFn = unsafe extern "C" fn(index: u32) -> bool;
/// 换碟：查询碟片总数
pub type RetroGetNumImagesFn = unsafe extern "C" fn() -> u32;
/// 换碟：替换指定索引的碟片
pub type RetroReplaceImageIndexFn =
    unsafe extern "C" fn(index: u32, info: *const RetroGameInfo) -> bool;
/// 换碟：追加一张碟片
pub type RetroAddImageIndexFn = unsafe extern "C" fn() -> bool;
/// 换碟：设置初始碟片索引
pub type RetroSetInitialImageFn = unsafe extern "C" fn(index: u32, path: *const c_char) -> bool;
/// 换碟：查询碟片路径
pub type RetroGetImagePathFn = unsafe extern "C" fn(index: u32, s: *mut c_char, len: usize) -> bool;
/// 换碟：查询碟片标签
pub type RetroGetImageLabelFn =
    unsafe extern "C" fn(index: u32, s: *mut c_char, len: usize) -> bool;

// ═══════════════════════════════════════════════════════════════
// 核心导出符号函数指针类型（对应 libretro.h:8022-8351）
// ═══════════════════════════════════════════════════════════════

/// `retro_init`：核心初始化（加载游戏前调用一次）
pub type RetroInit = unsafe extern "C" fn();
/// `retro_deinit`：核心销毁
pub type RetroDeinit = unsafe extern "C" fn();
/// `retro_get_system_info`：查询核心名称/版本/扩展名/加载方式
pub type RetroGetSystemInfo = unsafe extern "C" fn(info: *mut RetroSystemInfo);
/// `retro_get_system_av_info`：查询画面几何与音视频时序（须在 load_game 后调用）
pub type RetroGetSystemAvInfo = unsafe extern "C" fn(info: *mut RetroSystemAvInfo);
/// `retro_set_controller_port_device`：设置端口的控制器类型
pub type RetroSetControllerPortDevice = unsafe extern "C" fn(port: u32, device: u32);
/// `retro_reset`：重置游戏
pub type RetroReset = unsafe extern "C" fn();
/// `retro_run`：运行一帧
pub type RetroRun = unsafe extern "C" fn();
/// `retro_serialize_size`：查询存档状态所需字节数
pub type RetroSerializeSize = unsafe extern "C" fn() -> usize;
/// `retro_serialize`：序列化存档状态
pub type RetroSerialize = unsafe extern "C" fn(data: *mut c_void, len: usize) -> bool;
/// `retro_unserialize`：恢复存档状态
pub type RetroUnserialize = unsafe extern "C" fn(data: *const c_void, len: usize) -> bool;
/// `retro_load_game`：加载游戏
pub type RetroLoadGame = unsafe extern "C" fn(game: *const RetroGameInfo) -> bool;
/// `retro_load_game_special`：批量加载（多文件/多碟）——预留符号
pub type RetroLoadGameSpecial =
    unsafe extern "C" fn(game_type: u32, info: *const RetroGameInfo, num_info: usize) -> bool;
/// `retro_unload_game`：卸载游戏
pub type RetroUnloadGame = unsafe extern "C" fn();
/// `retro_get_region`：查询游戏区域（NTSC/PAL）——预留符号
pub type RetroGetRegion = unsafe extern "C" fn() -> u32;
/// `retro_get_memory_data`：获取核心内存区域指针（SRAM/RTC 等）
pub type RetroGetMemoryData = unsafe extern "C" fn(id: u32) -> *mut c_void;
/// `retro_get_memory_size`：查询内存区域字节数
pub type RetroGetMemorySize = unsafe extern "C" fn(id: u32) -> usize;
/// `retro_set_environment`：核心注册环境回调
pub type RetroSetEnvironment = unsafe extern "C" fn(cb: RetroEnvironmentFn);
/// `retro_set_video_refresh`：核心注册视频回调
pub type RetroSetVideoRefresh = unsafe extern "C" fn(cb: RetroVideoRefreshFn);
/// `retro_set_audio_sample`：核心注册音频单样本回调
pub type RetroSetAudioSample = unsafe extern "C" fn(cb: RetroAudioSampleFn);
/// `retro_set_audio_sample_batch`：核心注册音频批量回调
pub type RetroSetAudioSampleBatch = unsafe extern "C" fn(cb: RetroAudioSampleBatchFn);
/// `retro_set_input_poll`：核心注册输入轮询回调
pub type RetroSetInputPoll = unsafe extern "C" fn(cb: RetroInputPollFn);
/// `retro_set_input_state`：核心注册输入状态回调
pub type RetroSetInputState = unsafe extern "C" fn(cb: RetroInputStateFn);

// ═══════════════════════════════════════════════════════════════
// ABI 结构体（#[repr(C)]，字段顺序与 libretro.h 一致）
// ═══════════════════════════════════════════════════════════════

/// 游戏画面几何（对应 libretro.h `struct retro_game_geometry`）
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct RetroGameGeometry {
    /// 名义宽度（像素）
    pub base_width: u32,
    /// 名义高度（像素）
    pub base_height: u32,
    /// 最大宽度（像素）
    pub max_width: u32,
    /// 最大高度（像素）
    pub max_height: u32,
    /// 名义宽高比；≤0 表示用 `base_width / base_height`
    pub aspect_ratio: f32,
}

/// 音视频时序（对应 libretro.h `struct retro_system_timing`）
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct RetroSystemTiming {
    /// 视频帧率（fps）
    pub fps: f64,
    /// 音频采样率（Hz）
    pub sample_rate: f64,
}

/// 音视频系统信息（对应 libretro.h `struct retro_system_av_info`）
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct RetroSystemAvInfo {
    /// 画面几何
    pub geometry: RetroGameGeometry,
    /// 音视频时序
    pub timing: RetroSystemTiming,
}

/// 核心系统信息（对应 libretro.h `struct retro_system_info`）
///
/// 字符串字段所有权归核心（静态字符串），前端只读、不释放。
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RetroSystemInfo {
    /// 库名（如 `gambatte`）
    pub library_name: *const c_char,
    /// 库版本
    pub library_version: *const c_char,
    /// 支持的扩展名列表（`|` 分隔，如 `gb|gbc|dmg`）
    pub valid_extensions: *const c_char,
    /// 是否要求完整文件路径（true 时 data/size 无效）
    pub need_fullpath: bool,
    /// 是否禁止前端解压归档
    pub block_extract: bool,
}

/// 游戏信息（对应 libretro.h `struct retro_game_info`）
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct RetroGameInfo {
    /// 游戏文件路径（UTF-8，可空）
    pub path: *const c_char,
    /// 游戏数据内存缓冲（need_fullpath 时为空）
    pub data: *const c_void,
    /// 数据缓冲字节数
    pub size: usize,
    /// 实现相关元数据（可空）
    pub meta: *const c_char,
}

/// 核心变量（对应 libretro.h `struct retro_variable`）
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RetroVariable {
    /// 选项键（如 `gambatte_colorization`）
    pub key: *const c_char,
    /// 选项值（GET_VARIABLE 时由前端回填）
    pub value: *const c_char,
}

/// 提示消息（对应 libretro.h `struct retro_message`）
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RetroMessage {
    /// 消息文本
    pub msg: *const c_char,
    /// 显示帧数（约 1/60 秒每帧）
    pub frames: u32,
}

/// 输入描述符（对应 libretro.h `struct retro_input_descriptor`）
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RetroInputDescriptor {
    /// 端口（minarch 只用 0）
    pub port: u32,
    /// 设备类型（`RETRO_DEVICE_*`）
    pub device: u32,
    /// 索引（手柄为 0）
    pub index: u32,
    /// 按键/轴 id（`RETRO_DEVICE_ID_*`）
    pub id: u32,
    /// 按键名称描述
    pub description: *const c_char,
}

/// 振动接口（对应 libretro.h `struct retro_rumble_interface`）
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RetroRumbleInterface {
    /// 前端提供的振动函数（核心调用）
    pub set_rumble_state: RetroSetRumbleStateFn,
}

/// 日志接口（对应 libretro.h `struct retro_log_callback`）
///
/// minarch 不提供 log 实现（Rust 无法稳定定义 C 可变参函数，见 design 决策 8），
/// 该类型仅为 ABI 布局完整性定义。
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RetroLogCallback {
    /// 前端提供的日志函数（C 可变参）
    pub log: RetroLogPrintFn,
}

/// 控制器描述（对应 libretro.h `struct retro_controller_description`）
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RetroControllerDescription {
    /// 人类可读描述（如 `dualshock`）
    pub desc: *const c_char,
    /// 设备类型 id（传入 `retro_set_controller_port_device`）
    pub id: u32,
}

/// 控制器信息（对应 libretro.h `struct retro_controller_info`）
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RetroControllerInfo {
    /// 支持的控制器类型数组
    pub types: *const RetroControllerDescription,
    /// 数组长度
    pub num_types: u32,
}

/// 核心选项值（对应 libretro.h `struct retro_core_option_value`）
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RetroCoreOptionValue {
    /// 序列化的选项值
    pub value: *const c_char,
    /// 显示标签（可空，空则显示 value）
    pub label: *const c_char,
}

/// 核心选项定义 v1（对应 libretro.h `struct retro_core_option_definition`）
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RetroCoreOptionDefinition {
    /// 选项键
    pub key: *const c_char,
    /// 选项名称
    pub desc: *const c_char,
    /// 选项详情说明（可空）
    pub info: *const c_char,
    /// 可选值数组（`RETRO_NUM_CORE_OPTION_VALUES_MAX` 容量，以空 value 结尾）
    pub values: [RetroCoreOptionValue; RETRO_NUM_CORE_OPTION_VALUES_MAX],
    /// 默认值
    pub default_value: *const c_char,
}

/// 国际化核心选项（对应 libretro.h `struct retro_core_options_intl`）
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RetroCoreOptionsIntl {
    /// 英文（美式）定义
    pub us: *const RetroCoreOptionDefinition,
    /// 本地化定义
    pub local: *const RetroCoreOptionDefinition,
}

/// 换碟接口（对应 libretro.h `struct retro_disk_control_callback`）
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RetroDiskControlCallback {
    /// 弹出/合入
    pub set_eject_state: RetroSetEjectStateFn,
    /// 查询弹出状态
    pub get_eject_state: RetroGetEjectStateFn,
    /// 查询当前碟片索引
    pub get_image_index: RetroGetImageIndexFn,
    /// 设置当前碟片索引
    pub set_image_index: RetroSetImageIndexFn,
    /// 查询碟片总数
    pub get_num_images: RetroGetNumImagesFn,
    /// 替换碟片
    pub replace_image_index: RetroReplaceImageIndexFn,
    /// 追加碟片
    pub add_image_index: RetroAddImageIndexFn,
}

/// 扩展换碟接口（对应 libretro.h `struct retro_disk_control_ext_callback`）
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RetroDiskControlExtCallback {
    /// 弹出/合入
    pub set_eject_state: RetroSetEjectStateFn,
    /// 查询弹出状态
    pub get_eject_state: RetroGetEjectStateFn,
    /// 查询当前碟片索引
    pub get_image_index: RetroGetImageIndexFn,
    /// 设置当前碟片索引
    pub set_image_index: RetroSetImageIndexFn,
    /// 查询碟片总数
    pub get_num_images: RetroGetNumImagesFn,
    /// 替换碟片
    pub replace_image_index: RetroReplaceImageIndexFn,
    /// 追加碟片
    pub add_image_index: RetroAddImageIndexFn,
    /// 设置初始碟片（可选）
    pub set_initial_image: RetroSetInitialImageFn,
    /// 查询碟片路径（可选）
    pub get_image_path: RetroGetImagePathFn,
    /// 查询碟片标签（可选）
    pub get_image_label: RetroGetImageLabelFn,
}

// ═══════════════════════════════════════════════════════════════
// CoreError
// ═══════════════════════════════════════════════════════════════

/// 核心加载错误
///
/// `Core::open` 可能返回的错误类型——分别对应 dlopen 失败与 dlsym 失败。
#[derive(Debug)]
pub enum CoreError {
    /// dlopen 失败：路径不存在、格式错误或权限不足
    LibraryOpenFailed {
        /// 尝试加载的 `.so` 路径
        path: String,
        /// libloading 的底层错误（含 dlerror 文本）
        source: libloading::Error,
    },
    /// dlsym 失败：核心缺少 minarch 必需的导出符号（20 个必需符号之一）
    SymbolMissing {
        /// 缺失的符号名（如 `retro_run`）
        name: String,
    },
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::LibraryOpenFailed { path, source } => {
                write!(f, "无法打开核心库 {path}: {source}")
            }
            CoreError::SymbolMissing { name } => {
                write!(f, "核心缺少必需符号 {name}")
            }
        }
    }
}

impl std::error::Error for CoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CoreError::LibraryOpenFailed { source, .. } => Some(source),
            CoreError::SymbolMissing { .. } => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Core 加载器（对应 C minarch.c:68-107 struct Core + :2891-2955 Core_open）
// ═══════════════════════════════════════════════════════════════

/// 已加载的 libretro 核心
///
/// 持有 `.so` 的加载句柄 + 22 个导出符号函数指针 + 核心信息缓存。
/// 字段与 C `struct Core`（minarch.c:68-107）对应。
///
/// 函数指针字段为 `unsafe extern "C" fn`——调用需 `unsafe` 块：
/// 核心是外部 C 代码，其内存安全由 ABI 约定（布局测试）与核心自身保证。
///
/// 线程安全：`Library` 与 fn 指针均为 `Send + Sync`，
/// 满足 thread_video 模式（`retro_run` 在独立线程，minarch.c:4647）的跨线程约束。
pub struct Core {
    /// dlopen 句柄——保持 `.so` 驻留内存（drop 时自动 dlclose）
    pub handle: Library,
    /// `retro_init`
    pub init: RetroInit,
    /// `retro_deinit`
    pub deinit: RetroDeinit,
    /// `retro_get_system_info`
    pub get_system_info: RetroGetSystemInfo,
    /// `retro_get_system_av_info`
    pub get_system_av_info: RetroGetSystemAvInfo,
    /// `retro_set_controller_port_device`
    pub set_controller_port_device: RetroSetControllerPortDevice,
    /// `retro_reset`
    pub reset: RetroReset,
    /// `retro_run`
    pub run: RetroRun,
    /// `retro_serialize_size`
    pub serialize_size: RetroSerializeSize,
    /// `retro_serialize`
    pub serialize: RetroSerialize,
    /// `retro_unserialize`
    pub unserialize: RetroUnserialize,
    /// `retro_load_game`
    pub load_game: RetroLoadGame,
    /// `retro_load_game_special`——预留符号（C 版绑而不用），缺失为 `None`
    pub load_game_special: Option<RetroLoadGameSpecial>,
    /// `retro_unload_game`
    pub unload_game: RetroUnloadGame,
    /// `retro_get_region`——预留符号（C 版绑而不用），缺失为 `None`
    pub get_region: Option<RetroGetRegion>,
    /// `retro_get_memory_data`
    pub get_memory_data: RetroGetMemoryData,
    /// `retro_get_memory_size`
    pub get_memory_size: RetroGetMemorySize,
    /// `retro_set_environment`
    pub set_environment: RetroSetEnvironment,
    /// `retro_set_video_refresh`
    pub set_video_refresh: RetroSetVideoRefresh,
    /// `retro_set_audio_sample`
    pub set_audio_sample: RetroSetAudioSample,
    /// `retro_set_audio_sample_batch`
    pub set_audio_sample_batch: RetroSetAudioSampleBatch,
    /// `retro_set_input_poll`
    pub set_input_poll: RetroSetInputPoll,
    /// `retro_set_input_state`
    pub set_input_state: RetroSetInputState,

    /// 模拟器标签（如 `GBC`，用于目录命名与配置键）
    pub tag: String,
    /// 核心短名（`basename` 截去末尾 `_libretro`，如 `gambatte`）
    pub name: String,
    /// 展示版本（`library_name (library_version)` 格式）
    pub version: String,
    /// 支持的扩展名列表（`|` 分隔）
    pub extensions: String,
    /// 是否要求完整文件路径（true 时 data/size 无效）
    pub need_fullpath: bool,
    /// 核心配置目录（`USERDATA_PATH/<TAG>-<name>`，由装配层传入）
    pub config_dir: String,
    /// 存档状态目录（由装配层传入）
    pub states_dir: String,
    /// 存档目录（由装配层传入）
    pub saves_dir: String,
    /// BIOS 目录（由装配层传入）
    pub bios_dir: String,
    /// 帧率（初值 0.0，由后续 core.rs 在 `get_system_av_info` 后填充）
    pub fps: f64,
    /// 采样率（初值 0.0，同上）
    pub sample_rate: f64,
    /// 宽高比（初值 0.0，同上）
    pub aspect_ratio: f64,
}

/// 从核心路径推导核心短名（对应 C `Core_getName`，minarch.c:2886-2890）
///
/// C 版取 `basename` 后在最后一个 `_` 处截断。Rust 版对无下划线的情况
/// 返回完整文件名（C 版此时会解引用 NULL 崩溃，属防御性差异）。
fn derive_core_name(core_path: &str) -> String {
    let file_name = Path::new(core_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(core_path);
    file_name
        .rsplit_once('_')
        .map(|(stem, _)| stem)
        .unwrap_or(file_name)
        .to_string()
}

/// 读取核心提供的 C 字符串（所有权归核心，只读不释放）
///
/// # 参数
///
/// - `ptr`: 核心字符串字段指针；`NULL` 按空字符串处理
fn c_string_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: libretro.h 规定这些字段为 NUL 结尾的静态 UTF-8 字符串，调用期间有效
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

/// 解析一个导出符号并复制出裸 fn 指针
///
/// `libloading::Symbol` 借用 `Library` 的生命周期；本函数立即解引用复制
/// （`Symbol` 是 Copy 的），使返回值与句柄生命周期解耦，避免自引用结构。
fn load_symbol<T: Copy>(handle: &Library, name: &str) -> Result<T, CoreError> {
    let mut symbol_name = name.as_bytes().to_vec();
    symbol_name.push(0);
    // SAFETY: 符号名以 NUL 结尾；T 为 fn 指针类型，与核心导出签名一致（布局测试保证）
    unsafe { handle.get::<T>(&symbol_name) }
        .map(|symbol| *symbol)
        .map_err(|_| CoreError::SymbolMissing {
            name: name.to_string(),
        })
}

impl Core {
    /// 加载并打开一个 libretro 核心（对应 C `Core_open`，minarch.c:2891-2955）
    ///
    /// 完成 dlopen → 22 符号 dlsym → `retro_get_system_info` 填充信息缓存。
    /// 20 个必需符号缺失返回 [`CoreError::SymbolMissing`]；
    /// 2 个预留符号（`retro_load_game_special`/`retro_get_region`）
    /// 缺失时对应字段为 `None`（C 版同样绑而不用）。
    ///
    /// # 参数
    ///
    /// - `core_path`: 核心 `.so` 路径（如 `/mnt/SDCARD/.system/tg5040/paks/GBC.pak/gambatte_libretro.so`）
    /// - `tag`: 模拟器标签（如 `GBC`）
    /// - `config_dir`/`states_dir`/`saves_dir`/`bios_dir`: 四个数据目录，
    ///   由装配层用 `common::paths` + 平台常量算好后传入（C 版在 Core_open 内
    ///   用宏拼接，Rust 版保持本模块平台无关）
    ///
    /// # 返回值
    ///
    /// - `Ok(Core)`：加载成功，信息缓存已填充（`fps`/`sample_rate`/`aspect_ratio`
    ///   仍为 0.0，待后续 core.rs 填充）
    /// - `Err(CoreError::LibraryOpenFailed)`: dlopen 失败
    /// - `Err(CoreError::SymbolMissing)`: 必需符号缺失
    pub fn open(
        core_path: &str,
        tag: &str,
        config_dir: &str,
        states_dir: &str,
        saves_dir: &str,
        bios_dir: &str,
    ) -> Result<Core, CoreError> {
        // SAFETY: dlopen 外部库是受控操作；路径来自装配层，加载失败由 CoreError 承载
        let handle =
            unsafe { Library::new(core_path) }.map_err(|source| CoreError::LibraryOpenFailed {
                path: core_path.to_string(),
                source,
            })?;

        let init: RetroInit = load_symbol(&handle, "retro_init")?;
        let deinit: RetroDeinit = load_symbol(&handle, "retro_deinit")?;
        let get_system_info: RetroGetSystemInfo = load_symbol(&handle, "retro_get_system_info")?;
        let get_system_av_info: RetroGetSystemAvInfo =
            load_symbol(&handle, "retro_get_system_av_info")?;
        let set_controller_port_device: RetroSetControllerPortDevice =
            load_symbol(&handle, "retro_set_controller_port_device")?;
        let reset: RetroReset = load_symbol(&handle, "retro_reset")?;
        let run: RetroRun = load_symbol(&handle, "retro_run")?;
        let serialize_size: RetroSerializeSize = load_symbol(&handle, "retro_serialize_size")?;
        let serialize: RetroSerialize = load_symbol(&handle, "retro_serialize")?;
        let unserialize: RetroUnserialize = load_symbol(&handle, "retro_unserialize")?;
        let load_game: RetroLoadGame = load_symbol(&handle, "retro_load_game")?;
        let unload_game: RetroUnloadGame = load_symbol(&handle, "retro_unload_game")?;
        let get_memory_data: RetroGetMemoryData = load_symbol(&handle, "retro_get_memory_data")?;
        let get_memory_size: RetroGetMemorySize = load_symbol(&handle, "retro_get_memory_size")?;
        let set_environment: RetroSetEnvironment = load_symbol(&handle, "retro_set_environment")?;
        let set_video_refresh: RetroSetVideoRefresh =
            load_symbol(&handle, "retro_set_video_refresh")?;
        let set_audio_sample: RetroSetAudioSample = load_symbol(&handle, "retro_set_audio_sample")?;
        let set_audio_sample_batch: RetroSetAudioSampleBatch =
            load_symbol(&handle, "retro_set_audio_sample_batch")?;
        let set_input_poll: RetroSetInputPoll = load_symbol(&handle, "retro_set_input_poll")?;
        let set_input_state: RetroSetInputState = load_symbol(&handle, "retro_set_input_state")?;

        // 预留符号：存在则绑定，缺失不报错
        let load_game_special =
            load_symbol::<RetroLoadGameSpecial>(&handle, "retro_load_game_special").ok();
        let get_region = load_symbol::<RetroGetRegion>(&handle, "retro_get_region").ok();

        let mut info: RetroSystemInfo = unsafe { core::mem::zeroed() };
        // SAFETY: mock/真实核心都会填充该结构；字段均为标量或字符串指针
        unsafe { (get_system_info)(&mut info) };

        let name = derive_core_name(core_path);
        let version = format!(
            "{} ({})",
            c_string_to_string(info.library_name),
            c_string_to_string(info.library_version)
        );

        Ok(Core {
            handle,
            init,
            deinit,
            get_system_info,
            get_system_av_info,
            set_controller_port_device,
            reset,
            run,
            serialize_size,
            serialize,
            unserialize,
            load_game,
            load_game_special,
            unload_game,
            get_region,
            get_memory_data,
            get_memory_size,
            set_environment,
            set_video_refresh,
            set_audio_sample,
            set_audio_sample_batch,
            set_input_poll,
            set_input_state,
            tag: tag.to_string(),
            name,
            version,
            extensions: c_string_to_string(info.valid_extensions),
            need_fullpath: info.need_fullpath,
            config_dir: config_dir.to_string(),
            states_dir: states_dir.to_string(),
            saves_dir: saves_dir.to_string(),
            bios_dir: bios_dir.to_string(),
            fps: 0.0,
            sample_rate: 0.0,
            aspect_ratio: 0.0,
        })
    }
}

// ═══════════════════════════════════════════════════════════════
// 静态回调桥（对应 C minarch.c 的全局变量回调 + environment/video/
// audio/input 回调函数）
// ═══════════════════════════════════════════════════════════════

/// 环境回调处理器签名：接收命令码与载荷指针，返回是否处理
pub type EnvironmentHandler = fn(u32, *mut c_void) -> bool;
/// 视频刷新处理器签名：接收帧数据/尺寸/行字节数
pub type VideoRefreshHandler = fn(*const c_void, u32, u32, usize);
/// 音频单样本处理器签名
pub type AudioSampleHandler = fn(i16, i16);
/// 音频批量处理器签名：返回实际消费的帧数
pub type AudioSampleBatchHandler = fn(*const i16, usize) -> usize;
/// 输入轮询处理器签名
pub type InputPollHandler = fn();
/// 输入状态处理器签名：返回按键 0/1 或摇杆轴值
pub type InputStateHandler = fn(u32, u32, u32, u32) -> i16;

/// 前端回调处理器注册表
///
/// 由 `main.rs` 装配层在核心加载前注册一次，此后只读。
/// 各字段为 `Option`：未注册时 trampoline 返回安全默认值。
/// 需要可变状态的处理器（如按键轮询）由其所属模块自持内部状态
/// （后续 change 决策），本结构体保持不可变。
#[derive(Default)]
pub struct FrontendState {
    /// 环境回调处理器（分发 `RETRO_ENVIRONMENT_*`）
    pub environment: Option<EnvironmentHandler>,
    /// 视频刷新处理器
    pub video_refresh: Option<VideoRefreshHandler>,
    /// 音频单样本处理器
    pub audio_sample: Option<AudioSampleHandler>,
    /// 音频批量处理器
    pub audio_sample_batch: Option<AudioSampleBatchHandler>,
    /// 输入轮询处理器
    pub input_poll: Option<InputPollHandler>,
    /// 输入状态处理器
    pub input_state: Option<InputStateHandler>,
}

/// 全局回调注册表（`OnceLock`：注册后只读，无锁设计——见 design 决策 3）
static FRONTEND_STATE: OnceLock<FrontendState> = OnceLock::new();

/// 注册前端回调处理器（一次性，装配层启动时调用）
///
/// # 参数
///
/// - `state`: 6 个处理器字段的注册表（可 `FrontendState::default()` 后逐项填充）
///
/// # 返回值
///
/// - `Ok(())`: 首次注册成功
/// - `Err(state)`: 已注册过（重复注册是误用），携带本次传入的原值
pub fn register_callbacks(state: FrontendState) -> Result<(), FrontendState> {
    FRONTEND_STATE.set(state)
}

/// 环境回调 trampoline（`retro_environment_t`）
///
/// 核心在任何阶段都可能调用。未注册处理器时返回 `false`（表示不支持）。
///
/// # Safety
///
/// 本函数由核心 C 代码经函数指针调用；`cmd`/`data` 的有效性由调用方
/// （核心）保证，本函数不解引用 `data`，仅原样转发给处理器。
pub unsafe extern "C" fn environment_trampoline(cmd: u32, data: *mut c_void) -> bool {
    FRONTEND_STATE
        .get()
        .and_then(|state| state.environment)
        .is_some_and(|handler| handler(cmd, data))
}

/// 视频刷新 trampoline（`retro_video_refresh_t`）
///
/// 未注册处理器时忽略帧（静默丢弃）。
///
/// # Safety
///
/// 本函数由核心 C 代码经函数指针调用；`data` 指针的有效性（非空、
/// 长度满足 width/height/pitch）由调用方（核心）保证，本函数仅转发。
pub unsafe extern "C" fn video_refresh_trampoline(
    data: *const c_void,
    width: u32,
    height: u32,
    pitch: usize,
) {
    if let Some(handler) = FRONTEND_STATE.get().and_then(|state| state.video_refresh) {
        handler(data, width, height, pitch);
    }
}

/// 音频单样本 trampoline（`retro_audio_sample_t`）
///
/// 未注册处理器时忽略样本。
///
/// # Safety
///
/// 本函数由核心 C 代码经函数指针调用；参数为标量，无前置条件。
pub unsafe extern "C" fn audio_sample_trampoline(left: i16, right: i16) {
    if let Some(handler) = FRONTEND_STATE.get().and_then(|state| state.audio_sample) {
        handler(left, right);
    }
}

/// 音频批量 trampoline（`retro_audio_sample_batch_t`）
///
/// 未注册处理器时返回 `frames`（声明全部消费），与 C 版 fast_forward 路径
///
/// # Safety
///
/// 本函数由核心 C 代码经函数指针调用；`data` 指针的有效性（`frames * 2`
/// 个 `i16` 可读）由调用方（核心）保证，本函数仅转发。
/// 行为一致（minarch.c:2879-2881）。
pub unsafe extern "C" fn audio_sample_batch_trampoline(data: *const i16, frames: usize) -> usize {
    FRONTEND_STATE
        .get()
        .and_then(|state| state.audio_sample_batch)
        .map_or(frames, |handler| handler(data, frames))
}

/// 输入轮询 trampoline（`retro_input_poll_t`）
///
/// 未注册处理器时为空操作。
///
/// # Safety
///
/// 本函数由核心 C 代码经函数指针调用；无参数，无前置条件。
pub unsafe extern "C" fn input_poll_trampoline() {
    if let Some(handler) = FRONTEND_STATE.get().and_then(|state| state.input_poll) {
        handler();
    }
}

/// 输入状态 trampoline（`retro_input_state_t`）
///
/// 未注册处理器时返回 `0`（未按下）。
///
/// # Safety
///
/// 本函数由核心 C 代码经函数指针调用；参数为标量，无前置条件。
pub unsafe extern "C" fn input_state_trampoline(
    port: u32,
    device: u32,
    index: u32,
    id: u32,
) -> i16 {
    FRONTEND_STATE
        .get()
        .and_then(|state| state.input_state)
        .map_or(0, |handler| handler(port, device, index, id))
}
