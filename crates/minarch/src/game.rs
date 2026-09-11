//! 游戏文件组织
//!
//! 本模块负责游戏文件的打开与组织，对应原 C `minarch.c` 的
//! `Game_open`/`Game_close`/`Game_changeDisc`（:196-381）：
//!
//! - zip 压缩包探测与解压（核心不支持 zip 时解压到临时目录）
//! - m3u 播放列表探测（多碟游戏共享同一存档名）
//! - 换碟（配合 `retro_disk_control_ext_callback` 与
//!   `/tmp/change_disc.txt` 通知 minui 更新最近列表）
//!
//! 与 audio/vibration 不同，本模块**无薄静态层**：所有调用点都在
//! 主线程（`Game` 实例由装配层 main.rs 持有），不需要 `OnceLock`
//! 静态——见 design.md 决策 1。
//!

use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use common::paths::CHANGE_DISC_PATH;
use common::utils::{exists, put_file, suffix_match};

use crate::environment;
use crate::libretro::RetroGameInfo;
use crate::vibration;

/// 本地文件头长度（对应 C `ZIP_HEADER_SIZE`，minarch.c:112）
const ZIP_HEADER_SIZE: usize = 30;
/// 流式拷贝/解压的块大小（对应 C `ZIP_CHUNK_SIZE`，minarch.c:113）
const ZIP_CHUNK_SIZE: usize = 65536;

/// 打开中的游戏（C `struct Game`，minarch.c:187-195）
///
/// 实例由装配层（main.rs）持有并传 `&mut self`；本模块不维护
/// 进程级静态（与 audio/vibration 的薄静态层不同，调用点全在主线程）。
#[derive(Debug)]
pub struct Game {
    /// ROM 原始路径（换碟前的启动路径）
    pub path: String,
    /// 存档/配置用名（ROM basename；探测到 m3u 时为 m3u 文件名）
    pub name: String,
    /// 探测到的 m3u 播放列表路径（无则 `None`）
    pub m3u_path: Option<String>,
    /// zip 解压产物路径（未解压则 `None`）
    pub tmp_path: Option<PathBuf>,
    /// `need_fullpath=false` 时整文件读入内存（核心自行读盘则 `None`）
    pub data: Option<Vec<u8>>,
}

/// 打开/换碟失败的错误（对应 C `LOG_error + 静默返回` 的六条失败路径）
///
/// C 用「清空全局 + `is_open=0` + 静默返回」表示失败，Rust 改为显式
/// `Result`——不存在半开状态，错误日志属装配层职责（本模块保持纯逻辑）。
#[derive(Debug)]
pub enum GameError {
    /// 归档文件无法打开
    ArchiveOpen { path: String, source: io::Error },
    /// 临时目录创建失败（对应 C `mkdtemp` 返回 NULL，C 此处是 UB）
    TempDir { source: io::Error },
    /// 解压失败（目标文件创建/拷贝/deflate 流错误）
    Extract {
        archive_path: String,
        entry_name: String,
        source: io::Error,
    },
    /// 不支持的压缩方法（C 打印 errno 残留值，Rust 携带方法号精确诊断）
    ExtractMethod {
        archive_path: String,
        entry_name: String,
        method: u16,
    },
    /// 游戏文件打开失败
    GameFileOpen { path: String, source: io::Error },
    /// 游戏文件读取失败
    GameFileRead { path: String, source: io::Error },
    /// 换碟通知文件写失败（换碟已完成，仅通知失败；C `putFile` 忽略返回值）
    ChangeDiscNotify { source: io::Error },
}

/// 临时目录名计数器（进程内唯一，对应 C `mkdtemp("/tmp/minarch-XXXXXX")`）
static TMP_DIR_COUNTER: AtomicU32 = AtomicU32::new(0);

impl Game {
    /// 打开游戏文件（对应 C `Game_open`，minarch.c:196-358）
    ///
    /// # 参数
    ///
    /// - `path`: ROM 文件路径（`.zip` 时按核心能力决定是否解压）
    /// - `core_extensions`: 核心上报的扩展名串（`|` 分隔，来自
    ///   `libretro::Core::open` 的 `extensions` 缓存）
    /// - `core_need_fullpath`: 核心是否自行读文件（`false` 时前端整读入
    ///   `data`，对应 C「some cores handle opening files themselves」）
    ///
    /// # 返回值
    ///
    /// - `Ok(game)`: 打开成功（zip 无匹配条目/扫描放弃时 `tmp_path` 为 `None`）
    /// - `Err(GameError)`: 各失败路径（见 [`GameError`] 变体文档）
    pub fn open(
        path: &str,
        core_extensions: &str,
        core_need_fullpath: bool,
    ) -> Result<Game, GameError> {
        let path = path.to_string();

        let mut tmp_path: Option<PathBuf> = None;
        if suffix_match(".zip", &path) {
            let extensions: Vec<&str> = core_extensions.split('|').collect();
            // 核心原生支持 zip（如 mock core 的 "mc|zip"）→ 前端不解压
            if !extensions.iter().any(|e| e.eq_ignore_ascii_case("zip")) {
                tmp_path = extract_zip(&path, &extensions)?;
            }
        }

        let mut data: Option<Vec<u8>> = None;
        if !core_need_fullpath {
            // C：`path = game.tmp_path[0]?game.tmp_path:game.path`——解压后
            // 读的是解压产物，不是 zip 本身
            let read_path = match &tmp_path {
                Some(p) => p.to_string_lossy().into_owned(),
                None => path.clone(),
            };
            let mut file = File::open(&read_path).map_err(|e| GameError::GameFileOpen {
                path: read_path.clone(),
                source: e,
            })?;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)
                .map_err(|e| GameError::GameFileRead {
                    path: read_path,
                    source: e,
                })?;
            data = Some(buf);
        }

        let m3u_path = detect_m3u(&path);
        // 多碟游戏：所有碟片以 m3u 文件名共享同一存档/配置名
        let name = m3u_path
            .as_deref()
            .map(basename)
            .unwrap_or_else(|| basename(&path));

        Ok(Game {
            path,
            name,
            m3u_path,
            tmp_path,
            data,
        })
    }

    /// 关闭游戏并清理解压产物（对应 C `Game_close`，minarch.c:359-364）
    ///
    /// - 释放 `data`
    /// - 删除整个解压目录（偏离：C 只删文件、空目录遗留在 tmpfs 靠重启
    ///   回收；Rust 测试环境会累积，改为目录级清理——design 决策 6）
    /// - 振动清零（闭环清单「换游戏振动清零」：C `VIB_setStrength(0)`）
    ///
    /// 幂等：可重复调用（`tmp_path` 经 `take` 后二次调用为空操作）。
    pub fn close(&mut self) {
        if let Some(tmp) = self.tmp_path.take() {
            // tmp_path = <临时目录>/<条目名>——删除整个解压目录（C 只删
            // 文件、空目录泄漏到重启；Rust 目录级清理，design 决策 6）
            if let Some(dir) = tmp.parent() {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
        self.data = None;
        vibration::set_strength(0);
    }

    /// 换碟（对应 C `Game_changeDisc`，minarch.c:366-381）
    ///
    /// # 参数
    ///
    /// - `path`: 新碟片路径（与当前路径相同或不存在时无操作）
    /// - `core_extensions`/`core_need_fullpath`: 重开新碟片所需的核心能力
    ///   （与 [`Game::open`] 同参数化规则——design 决策 2）
    ///
    /// # 返回值
    ///
    /// - `Ok(())`: 无操作短路或换碟完成（通知文件写失败除外）
    /// - `Err(GameError)`: 新碟片打开失败（不写 CHANGE_DISC_PATH），或
    ///   通知文件写失败 `ChangeDiscNotify`（换碟已完成）
    ///
    /// # Safety 边界说明
    ///
    /// `replace_image_index` 为同步调用：`retro_game_info` 与 NUL 结尾
    /// 路径仅在调用期间被核心读取（libretro 契约），调用返回后
    /// `self.data` 仍持有数据缓冲，指针不悬垂。
    pub fn change_disc(
        &mut self,
        path: &str,
        core_extensions: &str,
        core_need_fullpath: bool,
    ) -> Result<(), GameError> {
        if path == self.path || !exists(path) {
            return Ok(());
        }

        self.close();
        *self = Game::open(path, core_extensions, core_need_fullpath)?;

        // ROM 路径为 SD 卡路径，不可能含 NUL（C 同假设）
        let c_path = CString::new(path).expect("ROM 路径含 NUL");
        let info = RetroGameInfo {
            path: c_path.as_ptr(),
            data: self
                .data
                .as_ref()
                .map_or(std::ptr::null(), |d| d.as_ptr() as *const _),
            size: self.data.as_ref().map_or(0, Vec::len),
            meta: std::ptr::null(),
        };

        // C 无条件调用（minarch.c:379）——核心未注册时是空指针崩溃。
        // Rust 以 Option 承载：None 跳过调用，仍写通知文件（minui
        // 侧 recents 仍需要知道换碟）——design 决策 7
        if let Some(disc) = environment::disc_control() {
            // SAFETY: replace_image_index 同步调用；info 与 c_path 在调用
            // 期间存活；核心按 libretro 契约只在调用内读取 image 数据。
            unsafe {
                (disc.replace_image_index)(0, &info);
            }
        }

        // MinUI 读此文件更新 recents.txt（C minarch.c:380；消费方见
        // common::paths::CHANGE_DISC_PATH 文档）
        put_file(CHANGE_DISC_PATH, path).map_err(|e| GameError::ChangeDiscNotify { source: e })
    }
}

/// 创建进程内唯一的临时目录（对应 C `mkdtemp("/tmp/minarch-XXXXXX")`）
///
/// 手写 pid + 原子计数方案而非 `tempfile` crate：为一次 mkdtemp 引入
/// 新依赖不值得，且显式 `close()` 清理时机比 Drop 隐式清理更贴近 C
/// 语义——design 决策 5。
fn make_temp_dir() -> io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!(
        "minarch-{}-{}",
        std::process::id(),
        TMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir)?;
    Ok(dir)
}

/// 取路径 basename（对应 C `strrchr(path, '/') + 1`）
fn basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// 构造 m3u 候选路径并探测（对应 C minarch.c:328-355 的三步截断）
///
/// 候选路径为 `<rom 所在目录>/<rom 所在目录名>.m3u`。C 用 `strrchr`
/// 三步截断构造；Rust 用 `Path` 语义等价实现（design 决策 9）。C 在
/// 路径无父目录/目录名时是空指针崩溃，Rust 以 `Option` 链安全跳过。
fn detect_m3u(path: &str) -> Option<String> {
    let p = Path::new(path);
    let parent = p.parent()?;
    let parent_name = parent.file_name()?;
    let candidate = parent.join(format!("{}.m3u", parent_name.to_string_lossy()));
    exists(&candidate.to_string_lossy()).then(|| candidate.to_string_lossy().into_owned())
}

/// zip 条目扫描与解压（对应 C minarch.c:229-299 的 while 循环）
///
/// 返回解压产物路径；无匹配条目、data descriptor 放弃、头损坏时返回
/// `Ok(None)`（与 C「break 后不解压继续打开」一致）。
///
/// 最小解析（design 决策 8）：只读 30 字节本地文件头，不校验签名、
/// 不读 central directory、无文件名长度上限（C 的 MAX_PATH 缓冲限制
/// 在 Rust `String` 下不存在）。
fn extract_zip(archive_path: &str, extensions: &[&str]) -> Result<Option<PathBuf>, GameError> {
    let mut zip = File::open(archive_path).map_err(|e| GameError::ArchiveOpen {
        path: archive_path.to_string(),
        source: e,
    })?;
    let mut header = [0u8; ZIP_HEADER_SIZE];

    loop {
        // C：`fread(header, 1, 30) != 30 → break`——头读不满即放弃扫描
        if zip.read_exact(&mut header).is_err() {
            break;
        }
        // 通用标志位 bit 3（data descriptor）：C 不支持流式 zip，放弃
        if u16::from_le_bytes([header[6], header[7]]) & 0x0008 != 0 {
            break;
        }
        let name_len = u16::from_le_bytes([header[26], header[27]]) as usize;
        let mut name_bytes = vec![0u8; name_len];
        // C：`len != fread(filename, 1, len, zip) → break`——短读即放弃
        if zip.read_exact(&mut name_bytes).is_err() {
            break;
        }
        let entry_name = String::from_utf8_lossy(&name_bytes).into_owned();
        let compressed_size =
            u32::from_le_bytes([header[18], header[19], header[20], header[21]]) as u64;
        // 跳过扩展区（C `fseek(SEEK_CUR)`），失败即放弃
        if skip_bytes(
            &mut zip,
            u16::from_le_bytes([header[28], header[29]]) as u64,
        )
        .is_err()
        {
            break;
        }

        // 扩展名匹配：核心扩展名逐个做 `.ext` 后缀匹配（大小写不敏感）
        let matched = extensions
            .iter()
            .any(|ext| suffix_match(&format!(".{ext}"), &entry_name));
        if !matched {
            // 跳过该条目数据后继续扫下一个（C `continue`）
            if skip_bytes(&mut zip, compressed_size).is_err() {
                break;
            }
            continue;
        }

        let method = u16::from_le_bytes([header[8], header[9]]);
        let tmp_dir = make_temp_dir().map_err(|e| GameError::TempDir { source: e })?;
        // C `basename(filename)`：条目名取末段（".." 等无末段名时回退
        // 原名，File::create 对目录路径自然失败 → Extract 错误，与 C
        // 的 fopen 失败同语义）
        let file_name = Path::new(&entry_name)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| entry_name.clone());
        let out_path = tmp_dir.join(&file_name);

        let result = extract_entry(
            &mut zip,
            &out_path,
            method,
            compressed_size,
            archive_path,
            &entry_name,
        );
        if let Err(e) = result {
            // 不遗留半解压状态（spec「解压失败…tmp_path 不设置」；C 此处
            // 泄漏临时目录与半截文件）
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return Err(e);
        }
        return Ok(Some(out_path));
    }

    Ok(None)
}

/// 解压单个匹配条目到 `out_path`
fn extract_entry(
    zip: &mut File,
    out_path: &Path,
    method: u16,
    compressed_size: u64,
    archive_path: &str,
    entry_name: &str,
) -> Result<(), GameError> {
    let mut dst = File::create(out_path).map_err(|e| GameError::Extract {
        archive_path: archive_path.to_string(),
        entry_name: entry_name.to_string(),
        source: e,
    })?;
    let result = match method {
        // 0 = store（C `Zip_copy`）
        0 => copy_stream(zip, &mut dst, compressed_size),
        // 8 = raw deflate（C `Zip_inflate`，`inflateInit2(-MAX_WBITS)`）
        8 => inflate_stream(zip, &mut dst, compressed_size),
        // C：extract 为 NULL → "Error extracting file"；Rust 携带方法号
        m => {
            return Err(GameError::ExtractMethod {
                archive_path: archive_path.to_string(),
                entry_name: entry_name.to_string(),
                method: m,
            });
        }
    };
    result.map_err(|e| GameError::Extract {
        archive_path: archive_path.to_string(),
        entry_name: entry_name.to_string(),
        source: e,
    })
}

/// 相对跳过 `size` 字节（C `fseek(SEEK_CUR)`）
fn skip_bytes(file: &mut File, size: u64) -> io::Result<()> {
    file.seek(SeekFrom::Current(size as i64)).map(|_| ())
}

/// store 方法：分块原样拷贝（对应 C `Zip_copy`，minarch.c:118-127）
fn copy_stream(reader: &mut File, writer: &mut File, mut size: u64) -> io::Result<()> {
    let mut buffer = [0u8; ZIP_CHUNK_SIZE];
    while size > 0 {
        let want = size.min(ZIP_CHUNK_SIZE as u64) as usize;
        let n = reader.read(&mut buffer[..want])?;
        // C：`sz != fread(...) → 返回 -1`——数据被截断即失败
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "zip 数据被截断",
            ));
        }
        writer.write_all(&buffer[..n])?;
        size -= n as u64;
    }
    Ok(())
}

/// deflate 方法：raw 流式解压（对应 C `Zip_inflate`，minarch.c:128-183）
///
/// C 用 zlib `inflateInit2(-MAX_WBITS)`；Rust 用 `flate2::Decompress::new(false)`
/// （`zlib_header=false` = raw deflate）。与 C 一致的语义：
/// 数据耗尽或流自然结束（StreamEnd）均视为成功——C 的
/// `if (!size || ret == Z_STREAM_END) return Z_OK`（Z_DATA_ERROR 分支
/// 实际不可达，见 design 决策 8 分析）。
fn inflate_stream(reader: &mut File, writer: &mut File, mut size: u64) -> io::Result<()> {
    let mut decompress = flate2::Decompress::new(false);
    let mut in_buf = [0u8; ZIP_CHUNK_SIZE];
    let mut out_buf = [0u8; ZIP_CHUNK_SIZE];
    let mut stream_ended = false;

    while size > 0 {
        let want = size.min(ZIP_CHUNK_SIZE as u64) as usize;
        let n = reader.read(&mut in_buf[..want])?;
        // 偏离：C 在此处会死循环（fread 返回 0 且无 ferror 时外层
        // while 条件不变）；Rust 显式报错（spec「截断的压缩数据」）
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "zip 压缩数据被截断",
            ));
        }
        size -= n as u64;

        let mut in_pos = 0;
        while in_pos < n {
            let before_in = decompress.total_in();
            let before_out = decompress.total_out();
            let status = decompress
                .decompress(
                    &in_buf[in_pos..],
                    &mut out_buf,
                    flate2::FlushDecompress::None,
                )
                .map_err(|e| {
                    io::Error::new(io::ErrorKind::InvalidData, format!("deflate 流错误: {e}"))
                })?;
            let consumed = (decompress.total_in() - before_in) as usize;
            let produced = (decompress.total_out() - before_out) as usize;
            in_pos += consumed;
            writer.write_all(&out_buf[..produced])?;

            if status == flate2::Status::StreamEnd {
                stream_ended = true;
                break;
            }
            // 防御：无进展且未结束的流会导致内层死循环（正常输入不可达）
            if consumed == 0 && produced == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "deflate 流无进展",
                ));
            }
        }
        if stream_ended {
            break;
        }
    }
    Ok(())
}
