//! 存档快照持久化：快照路径命名与读写（对应 C `State_getPath`/
//! `State_read`/`State_write`，minarch.c:484-568 的文件层）
//!
//! 本模块是"状态快照（存档）的持久化桥梁"：模拟核心把当前游戏状态
//! 序列化成一段字节（像给游戏瞬间拍一张拍立得），前端把这段字节存成
//! SD 卡上的 `.st{slot}` 文件；读档时反向——文件读回缓冲，核心反序列化
//! 恢复现场。本模块提供这条链路的**纯逻辑层**（命名规则 + 文件搬运）。
//!
//! ## 边界
//!
//! - 零静态、零泛型、零 FFI、零平台依赖、零 crate 内部模块依赖：
//!   序列化 FFI 调用（`serialize_size`/`serialize`/`unserialize`）、
//!   快进门控（关→序列化→恢复）、autosave/resume 槽位编排都在装配层，
//!   见 spec「minarch 半环闭环清单」
//! - 缓冲由调用方提供：写档时装配层把核心 `serialize` 出的字节传给
//!   [`write_from`]，读档时装配层用 `vec![0; size]`（对应 C `calloc`
//!   零填充）先建缓冲再传 [`read_into`]——短读只覆盖前段，后段保持
//!   零填充，与 C 行为等价
//! - 日志缺失期静默（C 的 `LOG_error` 不实现，与 sram/environment
//!   同约定）；真 IO 错误以 `Result` 传播
//!
//! ## 与 C 的偏离
//!
//! - C 写后调系统级 `sync()`（minarch.c:565）→ Rust [`write_from`]
//!   用 `File::sync_all()` 只刷本文件（与 sram 同偏离）
//! - C 的「slot 8 不存在时不报错」（minarch.c:505-506）是日志豁免，
//!   日志缺失期所有 slot 统一静默——模块不感知 slot 语义，无行为差异
//! - C `State_read` 的 `state_size < fread(...)` 判断（minarch.c:513）
//!   是恒假死分支（`fread` 上限即 `state_size`），不移植——直接实现
//!   真实语义：读入至多缓冲大小字节
//! - 无原子写：与 C 一致保持截断写（tmp+rename 被否决，见 design）
//!

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

/// 读快照文件的结果三态
///
/// 对应 C `State_read` 的三条正常路径（minarch.c:487-528），只有真
/// IO 错误走 `Err`。
pub enum ReadOutcome {
    /// 文件存在且成功读入（文件短于缓冲时仅覆盖前段——C `fread` 至多
    /// `state_size` 字节 + `calloc` 零填充语义）
    Loaded,
    /// 文件不存在（首次运行——C 静默返回，minarch.c:504-509）
    NotFound,
    /// 空缓冲跳过（对应 C size==0 跳过，不打开文件）
    Skipped,
}

/// 计算状态快照文件路径（对应 C `State_getPath`，minarch.c:484-486）
///
/// # 参数
///
/// - `states_dir`: 状态快照目录（`Core::open` 的 `states_dir` 参数，
///   如 `/mnt/SDCARD/Saves/GBC`）
/// - `name`: **完整文件名含扩展名**（C `Game_open` 的
///   `strrchr(path,'/')+1`，minarch.c:199-200）——`Pokemon Red.gb`
///   而非 `Pokemon Red`
/// - `slot`: 槽位号（0-8 手动槽位 / 9 自动存档槽位——本模块不感知
///   槽位语义，只做字符串拼接）
///
/// # 返回值
///
/// `"{states_dir}/{name}.st{slot}"`——注意扩展名**追加**在完整文件名
/// 之后（`Pokemon Red.gb` → `Pokemon Red.gb.st3`）。
pub fn state_path(states_dir: &str, name: &str, slot: i32) -> String {
    format!("{states_dir}/{name}.st{slot}")
}

/// 把快照文件读入缓冲（对应 C `State_read` 的文件层，minarch.c:487-528）
///
/// # 参数
///
/// - `buffer`: 目标缓冲（装配层用 `vec![0; size]` 提供——`size` 来自
///   `serialize_size`，零填充对应 C `calloc`）
/// - `path`: 快照文件路径（由 [`state_path`] 计算）
///
/// # 返回值
///
/// - `Ok(ReadOutcome::Skipped)`: `buffer` 为空（C size==0 跳过）——
///   SHALL NOT 打开文件
/// - `Ok(ReadOutcome::NotFound)`: 文件不存在（C 首次运行静默返回）
/// - `Ok(ReadOutcome::Loaded)`: 读入至多 `buffer.len()` 字节——文件
///   短于缓冲时只覆盖前段（C `fread` 至多 `state_size` 字节 + calloc
///   零填充；unserialize 传缓冲大小而非实际读入量，minarch.c:513
///   的行为由零填充保证）
/// - `Err`: 真 IO 错误（权限等）
///
/// # 错误
///
/// - 文件存在但读取失败（`io::Error`）
pub fn read_into(buffer: &mut [u8], path: &Path) -> io::Result<ReadOutcome> {
    if buffer.is_empty() {
        return Ok(ReadOutcome::Skipped);
    }
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(ReadOutcome::NotFound),
        Err(e) => return Err(e),
    };
    // 部分读是预期语义（C `fread` 至多 state_size 字节，短文件只覆盖
    // 前段、后段保持 calloc 零填充）——读入量有意忽略
    let _ = file.read(buffer)?;
    Ok(ReadOutcome::Loaded)
}

/// 把缓冲写入快照文件（对应 C `State_write` 的文件层，minarch.c:529-568）
///
/// # 参数
///
/// - `buffer`: 序列化出的状态字节（装配层经核心 `serialize` 填充）
/// - `path`: 快照文件路径（由 [`state_path`] 计算）
///
/// # 返回值
///
/// - `Ok(())`: 空缓冲跳过（不创建文件，C size==0 跳过语义）或写入成功
/// - `Err`: 打开失败（目录缺失等，对应 C `LOG_error` 分支，
///   minarch.c:547-549）或短写
///
/// ## 与 C 的偏离
///
/// C 写后调系统级 `sync()`（minarch.c:565）同步**整个系统**脏页；
/// Rust 用 [`File::sync_all`] 只刷本文件——意图（断电保护刚写的
/// 快照）忠实、范围更窄、性能更好。
///
/// # 错误
///
/// - 打开失败、写入失败、`sync_all` 失败（`io::Error`）
pub fn write_from(buffer: &[u8], path: &Path) -> io::Result<()> {
    if buffer.is_empty() {
        return Ok(());
    }
    let mut file = File::create(path)?;
    file.write_all(buffer)?;
    file.sync_all()
}
