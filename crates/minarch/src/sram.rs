//! 电池存档持久化：SRAM/RTC 路径与读写（对应 C `SRAM_*`/`RTC_*`，
//! minarch.c:385-482）
//!
//! 本模块是"电池存档的持久化桥梁"：模拟核心在内存里模拟卡带存档
//! 芯片（SRAM 电池存档 + RTC 实时时钟），游戏内"存档"写的是核心
//! 内存；前端在启动时把 SD 卡文件灌进核心内存、退出/重置/睡眠时
//! 把核心内存写回 SD 卡——本模块提供这条搬运链路的**纯逻辑层**。
//!
//! ## 边界
//!
//! - 零静态、零泛型、零 FFI、零平台依赖：FFI 取指针
//!   （`get_memory_data`/`get_memory_size` → 切片构造）与四个调用点
//!   （Core_init 读、Core_reset 写、菜单退出写、睡眠前写）在装配层，
//!   见 spec「minarch 半环闭环清单」
//! - 日志缺失期静默（C 的 `LOG_error` 不实现，与 environment
//!   design 决策 7 同约定）
//!
//! ## 与 C 的偏离
//!
//! - C 写后调系统级 `sync()`（minarch.c:426）→ Rust [`write_from`]
//!   用 `File::sync_all()` 只刷本文件
//! - C 空文件读 0 字节走 `LOG_error` → Rust 静默返回 `Loaded`（无害，
//!   日志缺失期）
//! - 无原子写：与 C 一致保持截断写（tmp+rename 被否决，见 design）
//!

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

/// 读存档文件的结果三态
///
/// 对应 C `SRAM_read`/`RTC_read` 的三条正常路径（minarch.c:388-405/
/// 437-456），只有真 IO 错误走 `Err`。
pub enum ReadOutcome {
    /// 文件存在且成功读入（含读入 0 字节的空文件——C 会 LOG_error，
    /// 日志设施缺失期静默，见模块文档偏离记录）
    Loaded,
    /// 文件不存在（首次运行——C 静默返回，minarch.c:395）
    NotFound,
    /// 空切片跳过（对应 C size==0 跳过，minarch.c:390，不打开文件）
    Skipped,
}

/// 计算电池存档文件路径（对应 C `SRAM_getPath`，minarch.c:385-387）
///
/// # 参数
///
/// - `saves_dir`: 存档目录（`Core::open` 的 `saves_dir` 参数，如
///   `/mnt/SDCARD/Saves/GBC`）
/// - `name`: **完整文件名含扩展名**（C `Game_open` 的
///   `strrchr(path,'/')+1`，minarch.c:199-200）——`Pokemon Red.gb`
///   而非 `Pokemon Red`
///
/// # 返回值
///
/// `"{saves_dir}/{name}.sav"`——注意扩展名**追加**在完整文件名之后
/// （`Pokemon Red.gb` → `Pokemon Red.gb.sav`）。
pub fn save_path(saves_dir: &str, name: &str) -> String {
    format!("{saves_dir}/{name}.sav")
}

/// 计算实时时钟文件路径（对应 C `RTC_getPath`，minarch.c:434-436）
///
/// # 参数
///
/// - `saves_dir`: 存档目录（同 [`save_path`]）
/// - `name`: 完整文件名含扩展名（同 [`save_path`] 的契约）
///
/// # 返回值
///
/// `"{saves_dir}/{name}.rtc"`
pub fn rtc_path(saves_dir: &str, name: &str) -> String {
    format!("{saves_dir}/{name}.rtc")
}

/// 把存档文件读入核心内存切片（对应 C `SRAM_read`/`RTC_read`）
///
/// # 参数
///
/// - `memory`: 核心内存切片（装配层经 `get_memory_data`/`get_memory_size`
///   构造，长度即 size）
/// - `path`: 存档文件路径（由 [`save_path`]/[`rtc_path`] 计算）
///
/// # 返回值
///
/// - `Ok(ReadOutcome::Skipped)`: `memory` 为空（C size==0 跳过）——
///   SHALL NOT 打开文件
/// - `Ok(ReadOutcome::NotFound)`: 文件不存在（C 首次运行静默返回）
/// - `Ok(ReadOutcome::Loaded)`: 读入至多 `memory.len()` 字节——文件
///   短于切片时只覆盖前段（C `fread(sram, 1, sram_size, ...)` 同语义）
/// - `Err`: 真 IO 错误（权限等）
///
/// # 错误
///
/// - 文件存在但读取失败（`io::Error`）
pub fn read_into(memory: &mut [u8], path: &Path) -> io::Result<ReadOutcome> {
    if memory.is_empty() {
        return Ok(ReadOutcome::Skipped);
    }
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(ReadOutcome::NotFound),
        Err(e) => return Err(e),
    };
    // 部分读是预期语义（C `fread` 至多 sram_size 字节，短文件只覆盖
    // 前段）——读入量有意忽略
    let _ = file.read(memory)?;
    Ok(ReadOutcome::Loaded)
}

/// 把核心内存切片写入存档文件（对应 C `SRAM_write`/`RTC_write`）
///
/// # 参数
///
/// - `memory`: 核心内存切片（装配层经 `get_memory_data` 构造）
/// - `path`: 存档文件路径（由 [`save_path`]/[`rtc_path`] 计算）
///
/// # 返回值
///
/// - `Ok(())`: 空切片跳过（不创建文件，C size==0 跳过语义）或写入成功
/// - `Err`: 打开失败（目录缺失等，对应 C `LOG_error` 分支，
///   minarch.c:412-414）或短写
///
/// ## 与 C 的偏离
///
/// C 写后调系统级 `sync()`（minarch.c:426）同步**整个系统**脏页；
/// Rust 用 [`File::sync_all`] 只刷本文件——意图（断电保护刚写的
/// 存档）忠实、范围更窄、性能更好。
///
/// # 错误
///
/// - 打开失败、写入失败、`sync_all` 失败（`io::Error`）
pub fn write_from(memory: &[u8], path: &Path) -> io::Result<()> {
    if memory.is_empty() {
        return Ok(());
    }
    let mut file = File::create(path)?;
    file.write_all(memory)?;
    file.sync_all()
}
