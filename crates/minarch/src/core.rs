//! # 模拟器核心抽象
//!
//! ## EmuCore trait
//!
//! 定义了模拟器核心必须实现的接口。
//! 对应原 C 代码中通过 dlsym 加载的 libretro 函数指针。
//!
//! ## LibretroCore
//!
//! 通过 `libloading` crate 动态加载 .so 文件，实现 EmuCore trait。
//! 未来可以添加 Rust 原生核心的实现（编译时链接）。

use minui_platform::AudioFrame;

/// 系统信息
pub struct SystemInfo {
    pub library_name: String,
    pub library_version: String,
    pub valid_extensions: String,
    pub need_fullpath: bool,
}

/// 音视频信息
pub struct AvInfo {
    pub base_width: u32,
    pub base_height: u32,
    pub aspect_ratio: f64,
    pub fps: f64,
    pub sample_rate: f64,
}

/// 游戏数据（ROM）
pub struct GameData {
    pub path: String,
    pub data: Option<Vec<u8>>,
    pub size: usize,
}

/// 内存类型（对应 libretro RETRO_MEMORY_*）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryType {
    SaveRam,
    Rtc,
    SystemRam,
    VideoRam,
}

/// 核心回调 — 核心运行时需要调用的外部函数
pub struct CoreCallbacks {
    /// 视频刷新回调
    pub video_refresh: Option<Box<dyn FnMut(&[u8], u32, u32, usize)>>,
    /// 音频采样回调
    pub audio_sample: Option<Box<dyn FnMut(i16, i16)>>,
    /// 批量音频采样回调
    pub audio_sample_batch: Option<Box<dyn FnMut(&[i16], usize) -> usize>>,
    /// 输入轮询回调
    pub input_poll: Option<Box<dyn FnMut()>>,
    /// 输入状态回调
    pub input_state: Option<Box<dyn FnMut(u32, u32, u32, u32) -> i16>>,
}

/// 模拟器核心 trait
pub trait EmuCore {
    fn init(&mut self) -> Result<(), String>;
    fn deinit(&mut self);
    fn get_system_info(&self) -> SystemInfo;
    fn get_system_av_info(&self) -> AvInfo;
    fn set_controller_port_device(&mut self, port: u32, device: u32);
    fn reset(&mut self);
    fn run(&mut self, callbacks: &mut CoreCallbacks) -> Result<(), String>;
    fn serialize_size(&self) -> usize;
    fn serialize(&self, data: &mut [u8]) -> Result<(), String>;
    fn unserialize(&mut self, data: &[u8]) -> Result<(), String>;
    fn load_game(&mut self, game: &GameData) -> Result<(), String>;
    fn unload_game(&mut self);
    fn get_memory_data(&self, id: MemoryType) -> Option<&[u8]>;
    fn get_memory_size(&self, id: MemoryType) -> usize;

    /// 设置环境回调 — 核心通过此回调向前端请求信息
    fn set_environment(
        &mut self,
        cb: Box<dyn FnMut(u32, *mut std::ffi::c_void) -> bool>,
    );
    /// 设置视频刷新回调
    fn set_video_refresh(
        &mut self,
        cb: Box<dyn FnMut(*const u8, u32, u32, usize)>,
    );
    /// 设置音频采样回调
    fn set_audio_sample(&mut self, cb: Box<dyn FnMut(i16, i16)>);
    /// 设置批量音频回调
    fn set_audio_sample_batch(
        &mut self,
        cb: Box<dyn FnMut(*const i16, usize) -> usize>,
    );
    /// 设置输入轮询回调
    fn set_input_poll(&mut self, cb: Box<dyn FnMut()>);
    /// 设置输入状态回调
    fn set_input_state(
        &mut self,
        cb: Box<dyn FnMut(u32, u32, u32, u32) -> i16>,
    );
}

/// 通过 libloading 动态加载的 libretro 核心
///
/// TODO: 实现 EmuCore trait，使用 FFI 调用 .so 中的 retro_* 函数
pub struct LibretroCore {
    // lib: libloading::Library,
    // ... 函数指针
}

impl LibretroCore {
    /// 从 .so 文件路径加载 libretro 核心
    pub fn open(_core_path: &str) -> Result<Self, String> {
        // TODO: dlopen + dlsym
        Err("LibretroCore not yet implemented".into())
    }
}
