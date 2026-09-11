//! 系统设置：亮度/音量/静音/耳机状态（libmsettings 的 Rust 版）
//!
//! 本模块对应原版 C 的 `libmsettings/msettings.c`——"系统设置"领域：
//! 亮度、音量、静音、耳机状态的跨进程共享（POSIX 共享内存）、
//! 硬件操作（ioctl /dev/disp、amixer）与持久化（`msettings.bin`）。
//!
//! ## 四重角色（与原版一一对应）
//!
//! | 角色 | 说明 |
//! |------|------|
//! | 跨进程共享面 | `shm_open("/SharedSettings")` + `mmap`——keymon（写）与 minui/minarch（读）通过它对话。共享内存本质是 `/dev/shm`（tmpfs）上的文件 |
//! | 硬件操作封装 | `set_raw_brightness`（ioctl /dev/disp）、`set_raw_volume`（amixer 命令） |
//! | 持久化 | 每次变更写 `{USERDATA_PATH}/msettings.bin` + `sync()` |
//! | Host/Client 仲裁 | `O_CREAT\|O_EXCL` 先到先得：先创建者是 host（加载磁盘初始值），后到者是 client（连接现成内存） |
//!
//! ## 与 Platform trait 的边界
//!
//! 亮度/音量**不属于平台抽象**（C 中不属于 `PLAT_*`，属于 libmsettings）——
//! 本模块是 trait 之外的"第二个接口面"。minui/minarch 读取设置值时，
//! 在 `#[cfg(feature = "tg5040")]` 下直接调用本模块函数（与原版 minui.c
//! 链接 libmsettings 直接调函数的行为一致）。
//!

#[cfg(unix)]
use std::ffi::CString;
use std::sync::Mutex;

use common::video::{BRIGHTNESS_MAX, VOLUME_MAX};

// ── 常量 ─────────────────────────────────────────────────────

/// 共享内存对象名（POSIX shm，实际为 /dev/shm 下的文件）。
/// 对应原版 `SHM_KEY "/SharedSettings"`（msettings.c:40）。
const SHM_KEY: &str = "/SharedSettings";

/// 设置结构体版本号。对应原版 `SETTINGS_VERSION 3`（msettings.c:19）。
const SETTINGS_VERSION: i32 = 3;

/// 亮度 ioctl 命令。对应原版 `DISP_LCD_SET_BRIGHTNESS 0x102`。
const DISP_LCD_SET_BRIGHTNESS: libc::c_ulong = 0x102;

// ── Settings 结构体 ──────────────────────────────────────────

/// 共享内存中的设置数据（keymon 与 minui/minarch 通过它对话）
///
/// 字段顺序/类型与原版 C `Settings` 结构体一致——意义在于
/// `msettings.bin` 磁盘文件格式兼容（从原版 MinUI 升级时音量/亮度
/// 设置可保留），而非共享内存兼容。共享内存的读写双方（keymon 写、
/// minui/minarch 读）都是本 crate 的 Rust 代码（同一构建产物），
/// 布局一致性由编译器保证——无需 `#[repr(C)]`。
///
/// 注意：与 keymon crate（`platform-tg5040-keymon`）的 `evdev::InputEvent`
/// 形成对比——后者是内核 ABI（跨越 Rust/内核边界），必须 `#[repr(C)]`；
/// 本结构体无跨语言边界。
struct Settings {
    /// 结构体版本（SETTINGS_VERSION = 3，未来兼容）
    ///
    /// 原版同样不读此字段（msettings.c:66 有 TODO："use settings->version
    /// for future proofing?"）——当前仅写入不读取，属预留
    #[allow(dead_code)]
    version: i32,
    /// 亮度值 0-10
    brightness: i32,
    /// 耳机音量 0-20
    headphones: i32,
    /// 扬声器音量 0-20
    speaker: i32,
    /// 静音开关（0/1）
    mute: i32,
    /// 预留字段（未来使用）——与原版布局对齐，仅写入不读取
    #[allow(dead_code)]
    unused: [i32; 2],
    /// 耳机插拔状态（0/1）
    jack: i32,
}

/// 默认设置（共享内存不存在且无持久化文件时的初始值）。
/// 对应原版 `DefaultSettings`（msettings.c:30-37）。
const DEFAULT_SETTINGS: Settings = Settings {
    version: SETTINGS_VERSION,
    brightness: 2,
    headphones: 4,
    speaker: 8,
    mute: 0,
    unused: [0; 2],
    jack: 0,
};

// ── SettingsHandle ───────────────────────────────────────────

/// 系统设置句柄（libmsettings 的 Rust 版）
///
/// 持有共享内存映射 + 进程内锁。**跨进程无锁共享**（设计意图，同原版——
/// 字段级 4 字节对齐访问在 ARM64 上原子）；**进程内用 [`Mutex`] 隔离线程
/// 并发**（keymon 的 mute 监控线程与主循环并发访问）。
///
/// # Safety
///
/// 本结构体包含 unsafe 代码（`map` 裸指针、`unsafe impl Send/Sync`）。
/// 调用者需满足的前置条件：
/// - `map` 只能由 [`SettingsHandle::init`] 返回的有效映射，Drop 时 `munmap`
/// - 跨进程共享内存的并发访问是无锁的（设计意图）——同一字段可能被
///   另一进程并发读写，依赖 ARM64 上 4 字节对齐访问的原子性
/// - 进程内的所有字段访问必须经过内部 `lock`（Mutex 串行化）——
///   公开方法已保证，禁止绕过
pub struct SettingsHandle {
    /// mmap 映射指针（`init()` 填充，`Drop` 时 `munmap`）
    map: *mut Settings,
    /// 是否 host（O_EXCL 创建者）——`Drop` 时 host 执行 `shm_unlink`
    is_host: bool,
    /// 进程内锁——keymon 的 mute 监控线程与主循环并发访问
    lock: Mutex<()>,
}

// # Safety：进程内访问经 `lock` 串行化；跨进程无锁共享是设计意图
//（同原版 C——Settings 字段级对齐访问），见结构体 # Safety 章节
unsafe impl Send for SettingsHandle {}
unsafe impl Sync for SettingsHandle {}

impl SettingsHandle {
    /// 打开/创建共享内存并加载设置
    ///
    /// 对应原版 `InitSettings`（msettings.c:69-113）。先到先得仲裁：
    /// `O_CREAT|O_EXCL` 创建成功 → **host**（ftruncate 尺寸 + 从磁盘
    /// 加载初始值）；`EEXIST` → **client**（连接现成共享内存，不重新加载）。
    ///
    /// "keymon 必然最先启动"靠部署时序保证（keymon 是开机链组件，
    /// minui 经 launch.sh 晚数秒启动）——`O_EXCL` 竞争必然 keymon 赢。
    ///
    /// ## 错误处理
    ///
    /// 无 Result——shm 打开/mmap 失败时 panic（与 Platform trait 的
    /// "嵌入式 init 失败无回退"设计一致）。
    pub fn init() -> SettingsHandle {
        // 共享内存是 POSIX 概念——仅 Linux 目标可实现。
        // 开发机（Windows/macOS）上本模块编译通过但调用即 panic
        // （目标设备是 Linux，测试均为 #[cfg(target_os = "linux")]）。
        #[cfg(not(unix))]
        {
            let _ = SHM_KEY;
            panic!("settings 模块仅支持 Linux 目标（开发机不可用）");
        }

        #[cfg(unix)]
        {
            let name = CString::new(SHM_KEY).expect("SHM_KEY 含 NUL");

            // ① 尝试创建（O_EXCL：已存在则失败并置 errno=EEXIST）
            let mut fd = unsafe {
                libc::shm_open(
                    name.as_ptr(),
                    libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
                    0o644,
                )
            };
            let is_host;
            if fd == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST) {
                // ② 已存在 → client：连接现成共享内存
                fd = unsafe { libc::shm_open(name.as_ptr(), libc::O_RDWR, 0o644) };
                is_host = false;
            } else {
                // ① 创建成功 → host（或非 EEXIST 的其他失败——下方 assert 兜底）
                is_host = true;
            }
            assert!(
                fd >= 0,
                "shm_open 失败：{}",
                std::io::Error::last_os_error()
            );

            // ③ host：设定共享内存大小（client 不做——文件已有大小）
            if is_host {
                let ret = unsafe { libc::ftruncate(fd, size_of::<Settings>() as libc::off_t) };
                assert!(
                    ret == 0,
                    "ftruncate 失败：{}",
                    std::io::Error::last_os_error()
                );
            }

            // ④ 映射到进程地址空间（MAP_SHARED——跨进程共享）
            let map = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    size_of::<Settings>(),
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    fd,
                    0,
                )
            };
            assert!(
                map != libc::MAP_FAILED,
                "mmap 失败：{}",
                std::io::Error::last_os_error()
            );
            unsafe { libc::close(fd) };

            let handle = SettingsHandle {
                map: map as *mut Settings,
                is_host,
                lock: Mutex::new(()),
            };

            // ⑤ host：从磁盘加载初始值（无持久化文件则用默认值）
            if is_host {
                handle.load_from_disk();
                // mute 不持久化（对应原版：settings->mute = 0）
                handle.with_lock(|s| s.mute = 0);
            }

            handle
        }
    }

    /// 在进程内锁保护下访问共享内存字段
    ///
    /// 进程内并发（keymon 的 mute 线程与主循环）经 Mutex 串行化。
    fn with_lock<R>(&self, f: impl FnOnce(&mut Settings) -> R) -> R {
        let _g = self.lock.lock().expect("settings 锁被污染");
        // # Safety：map 由 init() 初始化、Drop 时 munmap——整个生命周期内有效；
        // 跨进程并发访问是无锁共享（设计意图），见结构体 # Safety 章节
        unsafe { f(&mut *self.map) }
    }

    /// 从 `{USERDATA_PATH}/msettings.bin` 加载设置（host 首次启动时）
    ///
    /// 对应原版 `InitSettings` 的磁盘读取分支（msettings.c:60-71）：
    /// - 文件存在 → 读取覆盖（原版有 TODO：未来用 `version` 做版本检查）
    /// - 文件不存在 → 写入默认值（对应原版 `memcpy(settings, &DefaultSettings, shm_size)`）
    ///
    /// Rust 版更防御：文件长度不符时同样回退默认值（原版 `read` 长度
    /// 不足会部分覆盖——文件损坏时宁可默认值）。
    fn load_from_disk(&self) {
        let bytes = match std::fs::read(settings_path()) {
            Ok(bytes) => bytes,
            Err(_) => {
                // 无持久化文件——写入默认值（对应原版 else 分支）。
                // 若不写，mmap 的新页面是全零内存：brightness=0（最低亮度）、
                // speaker=0（无声音）——正是原版避免的行为
                self.write_defaults();
                return;
            }
        };
        if bytes.len() == size_of::<Settings>() {
            self.with_lock(|s| {
                // # Safety：字节长度已校验（== size_of::<Settings>()），
                // Settings 无对齐要求之外的填充（全 i32 字段，无对齐洞）
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        bytes.as_ptr() as *const Settings,
                        s as *mut Settings,
                        1,
                    );
                }
            });
        } else {
            // 长度不符——文件损坏，回退默认值（不复制原版的"部分覆盖"行为）
            self.write_defaults();
        }
    }

    /// 将默认设置写入共享内存
    ///
    /// 对应原版 `memcpy(settings, &DefaultSettings, shm_size)`。
    fn write_defaults(&self) {
        self.with_lock(|s| {
            // # Safety：DEFAULT_SETTINGS 与 Settings 同为同一结构体的值，
            // 字节复制合法
            unsafe { std::ptr::copy_nonoverlapping(&DEFAULT_SETTINGS, s, 1) };
        });
    }

    /// 当前亮度（0-10）
    pub fn brightness(&self) -> u8 {
        self.with_lock(|s| s.brightness as u8)
    }

    /// 当前音量（0-20）
    ///
    /// 对应原版 `GetVolume`（msettings.c:168-171）：静音时返回 0；
    /// 耳机插入时用耳机音量，否则用扬声器音量。
    pub fn volume(&self) -> u8 {
        self.with_lock(|s| {
            if s.mute != 0 {
                0
            } else if s.jack != 0 {
                s.headphones as u8
            } else {
                s.speaker as u8
            }
        })
    }

    /// 是否静音
    pub fn mute(&self) -> bool {
        self.with_lock(|s| s.mute != 0)
    }

    /// 耳机是否插入
    pub fn jack(&self) -> bool {
        self.with_lock(|s| s.jack != 0)
    }

    /// 设置亮度（0-10，超出上限 clamp）
    ///
    /// 对应原版 `SetBrightness`（msettings.c:130-166）：
    /// 写入共享内存字段 + 硬件操作（ioctl /dev/disp）+ 持久化落盘。
    pub fn set_brightness(&self, value: u8) {
        let value = value.min(BRIGHTNESS_MAX as u8);
        self.with_lock(|s| s.brightness = value as i32);
        set_raw_brightness(raw_brightness(value));
        self.save();
    }

    /// 设置音量（0-20，超出上限 clamp）
    ///
    /// 对应原版 `SetVolume`（msettings.c:172-182）：静音时只静音、
    /// 不修改音量字段；耳机插入时改耳机音量，否则改扬声器音量；
    /// 硬件 raw 值 = 音量 × 5。
    pub fn set_volume(&self, value: u8) {
        self.with_lock(|s| {
            // 静音时只静音不记字段（对应原版 mute 分支）
            if s.mute != 0 {
                set_raw_volume(0);
                return;
            }
            let value = value.min(VOLUME_MAX as u8);
            if s.jack != 0 {
                s.headphones = value as i32;
            } else {
                s.speaker = value as i32;
            }
            set_raw_volume(value as i32 * 5);
        });
        self.save();
    }

    /// 设置静音开关
    ///
    /// 对应原版 `SetMute`（msettings.c:263-267）：
    /// 静音时硬件音量直接置 0；取消静音时恢复当前音量。
    pub fn set_mute(&self, value: bool) {
        self.with_lock(|s| {
            s.mute = value as i32;
            if value {
                set_raw_volume(0);
            } else {
                // 恢复音量（对应原版 SetMute 的 else 分支）
                let v = if s.jack != 0 { s.headphones } else { s.speaker };
                set_raw_volume(v * 5);
            }
        });
        // 静音状态不落盘（对应原版：mute 不持久化），
        // 但取消静音时音量恢复可能影响耳机/扬声器值——无需保存
    }

    /// 设置耳机插拔状态
    ///
    /// 对应原版 `SetJack`（msettings.c:239-244）：
    /// 更新字段后按新通道恢复音量（耳机/扬声器切换）。
    pub fn set_jack(&self, value: bool) {
        self.with_lock(|s| {
            s.jack = value as i32;
            let v = if s.jack != 0 { s.headphones } else { s.speaker };
            if s.mute == 0 {
                set_raw_volume(v * 5);
            }
        });
        self.save();
    }

    /// 将当前设置写入 `{USERDATA_PATH}/msettings.bin` 并同步到磁盘
    ///
    /// 对应原版 `SaveSettings`（msettings.c:118-125）。每次设置变更都
    /// 调用（含 `sync()`）——低频操作（按键时），非热路径。
    fn save(&self) {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let bytes = self.with_lock(|s| {
            // # Safety：Settings 全 i32 字段无 padding，字节视图即磁盘格式
            //（与原版一致，msettings.bin 升级兼容）
            unsafe {
                std::slice::from_raw_parts(s as *const Settings as *const u8, size_of::<Settings>())
            }
            .to_vec()
        });
        if let Ok(mut file) = std::fs::File::create(&path) {
            use std::io::Write;
            let _ = file.write_all(&bytes);
            // 对应原版 sync()：确保落盘（SD 卡写入后断电不丢）
            let _ = file.sync_all();
        }
    }
}

impl Drop for SettingsHandle {
    /// 释放共享内存映射；host 时删除共享内存对象（`shm_unlink`）
    ///
    /// 对应原版 `QuitSettings`（msettings.c:114-117）。
    /// `shm_unlink` 只删除名字——已映射的进程（client）不受影响，
    /// 下次开机 /dev/shm（tmpfs）自动清空。
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            // # Safety：map 由 init() 分配、本 Drop 是唯一释放点（所有权的
            // 对应），munmap 后不再有任何引用
            unsafe { libc::munmap(self.map as *mut libc::c_void, size_of::<Settings>()) };
            if self.is_host {
                let name = CString::new(SHM_KEY).expect("SHM_KEY 含 NUL");
                unsafe { libc::shm_unlink(name.as_ptr()) };
            }
        }
        // 非 unix：init() 在非 unix 直接 panic，不会构造出实例——无需清理
    }
}

// ── 硬件操作 ─────────────────────────────────────────────────

/// 亮度值 → 硬件 raw 值映射（smart）
///
/// 对应原版 msettings.c:148-162 的 switch 分支（smart）。
/// 原版亮度曲线为非线性（低档位步进小、高档位步进大）。
#[cfg(feature = "smart")]
fn raw_brightness(value: u8) -> i32 {
    match value {
        0 => 4,
        1 => 6,
        2 => 10,
        3 => 16,
        4 => 32,
        5 => 48,
        6 => 64,
        7 => 96,
        8 => 128,
        9 => 192,
        10 => 255,
        _ => 255, // clamp 后的兜底（BRIGHTNESS_MAX 已限制）
    }
}

/// 亮度值 → 硬件 raw 值映射（brick）
///
/// 对应原版 msettings.c:133-147 的 switch 分支（`is_brick`）。
#[cfg(feature = "brick")]
fn raw_brightness(value: u8) -> i32 {
    match value {
        0 => 1,
        1 => 8,
        2 => 16,
        3 => 32,
        4 => 48,
        5 => 72,
        6 => 96,
        7 => 128,
        8 => 160,
        9 => 192,
        10 => 255,
        _ => 255,
    }
}

/// 写硬件亮度寄存器（`/dev/disp` ioctl）——**不修改** brightness 状态字段
///
/// 对应原版 `SetRawBrightness`（msettings.c:185-196）。`enable_backlight(false)`
/// （睡眠关屏）与 `power_off` 静音路径使用——与 `set_brightness`
/// （改字段+落盘）区分。
///
/// # Safety
///
/// 本函数包含 unsafe 代码（FFI）。调用者需满足的前置条件：
/// - `/dev/disp` 存在且当前用户可读写（设备特有，不可在 CI 验证）
/// - `param` 为 4 个 `c_ulong` 的数组，与内核 ioctl 协议一致
pub fn set_raw_brightness(raw: i32) {
    // ioctl 是 Linux 专属——开发机（非 unix）上无操作
    //（目标设备 /dev/disp 节点也不存在，静默跳过）
    #[cfg(unix)]
    {
        // 设备节点不存在时静默跳过（如开发机/CI 环境）
        let fd = unsafe { libc::open(c"/dev/disp".as_ptr(), libc::O_RDWR) };
        if fd >= 0 {
            let mut param: [libc::c_ulong; 4] = [0, raw as libc::c_ulong, 0, 0];
            unsafe {
                libc::ioctl(
                    fd,
                    DISP_LCD_SET_BRIGHTNESS,
                    &mut param as *mut _ as *mut libc::c_void,
                );
                libc::close(fd);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = raw;
    }
}

/// 写硬件音量（amixer 命令）
///
/// 对应原版 `SetRawVolume`（msettings.c:197-233）：
/// - 'digital volume' 映射是反向的（`100 - raw`）
/// - 音量为 0 时 'DAC volume' 置 0（否则仍会小声播放），
///   非零时置 160（=0dB=最大）
///
/// 原版用 `system()` 执行 amixer——Rust 版用 `std::process::Command`
/// （无 shell 解释，参数直接传递，避免引号转义问题）。
/// 写硬件音量（amixer 命令）——**不修改** mute 状态字段。
///
/// 原版 `SetRawVolume` 的对应——`prepare_sleep`/`power_off` 的硬件静音
/// 路径使用（睡眠期间临时静音，与 `set_mute`（改静音状态字段）区分）。
pub fn set_raw_volume(raw: i32) {
    let pct = 100 - raw;
    let _ = std::process::Command::new("amixer")
        .args(["sset", "digital volume", "-M", &format!("{pct}%")])
        .status();
    if raw == 0 {
        let _ = std::process::Command::new("amixer")
            .args(["sset", "DAC volume", "0"])
            .status();
    } else {
        let _ = std::process::Command::new("amixer")
            .args(["sset", "DAC volume", "160"])
            .status();
    }
}

// ── 路径 ─────────────────────────────────────────────────────

/// 设置持久化文件路径（`{USERDATA_PATH}/msettings.bin`）
///
/// 对应原版 `sprintf(SettingsPath, "%s/msettings.bin", getenv("USERDATA_PATH"))`
/// （msettings.c:73）。`USERDATA_PATH` 由 launch.sh 导出（SD 卡 .userdata）；
/// 未设置时（如开发机测试）回退设备默认路径。
fn settings_path() -> std::path::PathBuf {
    let userdata = std::env::var("USERDATA_PATH")
        .unwrap_or_else(|_| "/mnt/SDCARD/.userdata/tg5040".to_string());
    std::path::Path::new(&userdata).join("msettings.bin")
}

// ── 测试 ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Settings 布局（跨平台可测）──

    #[test]
    fn settings_size_matches_original() {
        // 原版 C Settings：8 个 int（含 unused[2]）= 32 字节
        assert_eq!(size_of::<Settings>(), 32);
    }

    #[test]
    fn default_settings_match_original() {
        // 对应原版 DefaultSettings（msettings.c:30-37）
        assert_eq!(DEFAULT_SETTINGS.version, 3);
        assert_eq!(DEFAULT_SETTINGS.brightness, 2);
        assert_eq!(DEFAULT_SETTINGS.headphones, 4);
        assert_eq!(DEFAULT_SETTINGS.speaker, 8);
        assert_eq!(DEFAULT_SETTINGS.mute, 0);
        assert_eq!(DEFAULT_SETTINGS.jack, 0);
    }

    // ── 亮度 raw 映射表（跨平台可测，与硬件 ioctl 无关）──

    #[test]
    fn raw_brightness_mapping_matches_original() {
        // 对应原版 msettings.c 的 switch 分支值
        let table: [(u8, i32); 11] = if cfg!(feature = "brick") {
            [
                (0, 1),
                (1, 8),
                (2, 16),
                (3, 32),
                (4, 48),
                (5, 72),
                (6, 96),
                (7, 128),
                (8, 160),
                (9, 192),
                (10, 255),
            ]
        } else {
            [
                (0, 4),
                (1, 6),
                (2, 10),
                (3, 16),
                (4, 32),
                (5, 48),
                (6, 64),
                (7, 96),
                (8, 128),
                (9, 192),
                (10, 255),
            ]
        };
        for (value, expected) in table {
            assert_eq!(
                raw_brightness(value),
                expected,
                "亮度 {value} 的 raw 值不符"
            );
        }
    }

    // ── init host/client 仲裁（Linux 专属——Windows 无 shm_open）──

    /// 清理测试残留的共享内存对象（测试隔离）
    ///
    /// 仅 Linux 测试使用——macOS/Windows 上无 shm_open，相关测试被
    /// `#[cfg(target_os = "linux")]` 排除后本函数无调用方
    #[cfg(target_os = "linux")]
    fn cleanup_shm() {
        let name = CString::new(SHM_KEY).unwrap();
        unsafe { libc::shm_unlink(name.as_ptr()) };
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn init_host_creates_and_loads_defaults() {
        cleanup_shm(); // 前置清理：确保 /SharedSettings 不存在
        let handle = SettingsHandle::init();
        assert!(handle.is_host, "首次 init 应是 host");
        // 无持久化文件 → 默认值
        assert_eq!(handle.brightness(), 2);
        assert_eq!(handle.volume(), 8); // 默认 speaker=8
        assert!(!handle.mute());
        drop(handle);
        cleanup_shm();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn init_client_connects_existing() {
        cleanup_shm();
        {
            let host = SettingsHandle::init();
            host.set_brightness(7);

            // 第二个进程（client）连接现成共享内存
            let client = SettingsHandle::init();
            assert!(!client.is_host, "第二次 init 应是 client");
            assert_eq!(client.brightness(), 7, "client 应读到 host 写入的值");
            // client 可写（写权限不分 host/client）
            client.set_brightness(3);
            assert_eq!(host.brightness(), 3, "host 应读到 client 写入的值");
        }
        cleanup_shm();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn host_drop_unlinks_shm() {
        cleanup_shm();
        {
            let host = SettingsHandle::init();
            assert!(host.is_host);
        } // host drop → shm_unlink
        // 之后再 init 应重新成为 host（shm 已被删除）
        let again = SettingsHandle::init();
        assert!(again.is_host, "shm_unlink 后 init 应再次成为 host");
        drop(again);
        cleanup_shm();
    }

    // ── 读写语义（Linux 专属——依赖 init）──

    #[test]
    #[cfg(target_os = "linux")]
    fn brightness_roundtrip() {
        cleanup_shm();
        {
            let handle = SettingsHandle::init();
            handle.set_brightness(8);
            assert_eq!(handle.brightness(), 8);
        }
        cleanup_shm();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn volume_mute_returns_zero() {
        cleanup_shm();
        {
            let handle = SettingsHandle::init();
            handle.set_volume(10);
            handle.set_mute(true);
            assert_eq!(handle.volume(), 0, "静音时 GetVolume 应返回 0");
            assert!(handle.mute());
        }
        cleanup_shm();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn volume_jack_uses_headphones() {
        cleanup_shm();
        {
            let handle = SettingsHandle::init();
            handle.set_volume(8); // speaker = 8
            handle.set_jack(true); // 切到耳机通道
            handle.set_volume(4); // headphones = 4
            assert_eq!(handle.volume(), 4, "耳机插入时应用耳机音量");
            assert!(handle.jack());
        }
        cleanup_shm();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn set_volume_muted_does_not_change_fields() {
        cleanup_shm();
        {
            let handle = SettingsHandle::init();
            handle.set_volume(10);
            handle.set_mute(true);
            handle.set_volume(15); // 静音中——不应修改字段
            assert_eq!(handle.volume(), 0, "静音中 volume() 仍返回 0");
            handle.set_mute(false);
            assert_eq!(
                handle.volume(),
                10,
                "取消静音后音量应仍是 10（未被静音中的 set 修改）"
            );
        }
        cleanup_shm();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn brightness_clamped_at_max() {
        cleanup_shm();
        {
            let handle = SettingsHandle::init();
            handle.set_brightness(99); // 超出 BRIGHTNESS_MAX=10
            assert_eq!(handle.brightness(), 10, "亮度应 clamp 到 10");
        }
        cleanup_shm();
    }

    // ── 持久化（Linux 专属）──

    #[test]
    #[cfg(target_os = "linux")]
    fn set_brightness_persists_to_disk() {
        cleanup_shm();
        // 临时 USERDATA_PATH（测试隔离——不污染真实用户数据）
        let tmp = std::env::temp_dir().join("minui_settings_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::env::set_var("USERDATA_PATH", &tmp);
        {
            let handle = SettingsHandle::init();
            handle.set_brightness(5);
        }
        // 磁盘文件应包含 brightness=5（布局与原版一致：第 2 个 i32）
        let bytes = std::fs::read(tmp.join("msettings.bin")).expect("msettings.bin 应被写入");
        assert_eq!(bytes.len(), 32, "文件应为 32 字节（8 个 i32）");
        // 手工解析（无依赖）：version(4B) + brightness(4B)
        let version = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let brightness = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        assert_eq!(version, 3);
        assert_eq!(brightness, 5, "落盘的 brightness 应为 5");

        // 清理
        drop(handle);
        let _ = std::fs::remove_dir_all(&tmp);
        cleanup_shm();
    }
}
