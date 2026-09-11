//! game 模块集成测试：zip 解压/m3u 探测/close/换碟
//!
//! 「zip 压缩包探测与解压」「m3u 播放列表探测」「Game close 与振动清零」
//! 「Game change_disc 换碟」五个 Requirement 的全部场景。
//!
//! 夹具约定：
//! - zip 用手工构造的本地文件头（实现与 C 一致不读 central directory）
//! - 临时文件用 `std::env::temp_dir()` + 进程内计数（无 tempfile 依赖）
//! - CHANGE_DISC_PATH 是全局共享文件 → 换碟测试经 `DISC_LOCK` 串行

use std::ffi::{CStr, CString, c_char, c_void};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

use minarch::environment;
use minarch::game::{Game, GameError};
use minarch::libretro::{
    RETRO_ENVIRONMENT_SET_DISK_CONTROL_EXT_INTERFACE, RetroDiskControlExtCallback, RetroGameInfo,
};
use minarch::vibration;

use common::paths::CHANGE_DISC_PATH;

/// 测试临时根目录计数器（进程内唯一）
static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);
/// 换碟测试互斥（CHANGE_DISC_PATH 全局文件串行，minui test_util 先例）
///
/// 锁毒化恢复用 `into_inner`（与 environment/vibration 薄静态层同模式：
/// 测试内 assert 的 panic 不应毒死后续换碟测试）。
static DISC_LOCK: Mutex<()> = Mutex::new(());
/// 假 replace_image_index 的调用记录（fn 指针不可捕获状态，经静态中转）
static REPLACE_RECORD: Mutex<Vec<(u32, String)>> = Mutex::new(Vec::new());

/// 毒化恢复锁助手（薄静态层同款 `into_inner` 语义）
fn lock_disc() -> MutexGuard<'static, ()> {
    DISC_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

fn record_replace(index: u32, path: String) {
    REPLACE_RECORD
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push((index, path));
}

fn replace_calls() -> Vec<(u32, String)> {
    REPLACE_RECORD
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

fn clear_replace_calls() {
    REPLACE_RECORD
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clear();
}

/// 创建进程内唯一的测试临时目录
fn temp_root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "minarch-game-test-{}-{}-{name}",
        std::process::id(),
        TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).expect("创建测试临时目录失败");
    dir
}

/// 在 `dir` 下写文件并返回其路径字符串
fn write_file(dir: &Path, name: &str, contents: &[u8]) -> String {
    let p = dir.join(name);
    fs::write(&p, contents).expect("写测试文件失败");
    p.to_string_lossy().into_owned()
}

// ── zip 夹具：手工构造最小 zip（30 字节本地文件头，无 central directory）──

/// 构造单条目 zip 字节流
///
/// 布局：签名(4) + 版本(2) + 通用标志(2) + 方法(2) + 时间日期(4) +
/// crc32(4) + 压缩大小(4) + 未压缩大小(4) + 名长(2) + 扩展区长(2) +
/// 条目名 + 条目数据。
fn zip_entry(
    entry_name: &str,
    compressed: &[u8],
    uncompressed_size: u32,
    method: u16,
    gp_flag: u16,
) -> Vec<u8> {
    let name = entry_name.as_bytes();
    let mut out = Vec::new();
    out.extend_from_slice(b"PK\x03\x04");
    out.extend_from_slice(&20u16.to_le_bytes()); // version needed
    out.extend_from_slice(&gp_flag.to_le_bytes()); // 通用标志位（bit 3 = data descriptor）
    out.extend_from_slice(&method.to_le_bytes()); // 压缩方法
    out.extend_from_slice(&[0u8; 4]); // mod time/date
    out.extend_from_slice(&[0u8; 4]); // crc32（实现不读）
    out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
    out.extend_from_slice(&uncompressed_size.to_le_bytes());
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // extra len
    out.extend_from_slice(name);
    out.extend_from_slice(compressed);
    out
}

/// store 方法（方法 0）条目
fn zip_store(entry_name: &str, data: &[u8]) -> Vec<u8> {
    zip_entry(entry_name, data, data.len() as u32, 0, 0)
}

/// deflate 方法（方法 8）条目：测试侧生成 raw deflate 数据
fn zip_deflate(entry_name: &str, data: &[u8]) -> Vec<u8> {
    let mut compressor = flate2::Compress::new(flate2::Compression::default(), false);
    let mut compressed = Vec::new();
    let mut buf = [0u8; 1024];
    let mut in_pos = 0;
    // Compress::compress_vec 不会扩展空 Vec（实测 BufError + 0 字节）——
    // 用定长缓冲 + total_in/total_out 增量循环（flate2 文档模式）
    loop {
        let before_in = compressor.total_in();
        let before_out = compressor.total_out();
        let status = compressor
            .compress(&data[in_pos..], &mut buf, flate2::FlushCompress::Finish)
            .expect("测试夹具 deflate 压缩失败");
        in_pos += (compressor.total_in() - before_in) as usize;
        compressed.extend_from_slice(&buf[..(compressor.total_out() - before_out) as usize]);
        if status == flate2::Status::StreamEnd {
            break;
        }
    }
    zip_entry(entry_name, &compressed, data.len() as u32, 8, 0)
}

// ── 组 1：Game 数据模型与 open 基础 ─────────────────────────────

#[test]
fn open_plain_rom_reads_data_and_basename() {
    let dir = temp_root("plain");
    let rom = write_file(&dir, "game.gb", b"rom-bytes");

    let g = Game::open(&rom, "gb|gbc", false).expect("普通 ROM 应打开成功");
    assert_eq!(g.path, rom);
    assert_eq!(g.name, "game.gb");
    assert_eq!(g.m3u_path, None);
    assert_eq!(g.tmp_path, None);
    assert_eq!(g.data.as_deref(), Some(&b"rom-bytes"[..]));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn open_with_need_fullpath_skips_data_read() {
    let dir = temp_root("fullpath");
    let rom = write_file(&dir, "game.gb", b"rom-bytes");

    let g = Game::open(&rom, "gb|gbc", true).expect("need_fullpath 时应打开成功");
    assert_eq!(g.data, None);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn open_missing_rom_returns_game_file_open_error() {
    let dir = temp_root("missing");
    let rom = dir.join("nope.gb").to_string_lossy().into_owned();

    let err = Game::open(&rom, "gb", false).expect_err("不存在的 ROM 应返回 Err");
    match err {
        GameError::GameFileOpen { path, .. } => assert_eq!(path, rom),
        other => panic!("期望 GameFileOpen，实际 {other:?}"),
    }

    fs::remove_dir_all(&dir).unwrap();
}

// ── 组 2：zip 压缩包探测与解压 ───────────────────────────────────

#[test]
fn open_zip_skips_extraction_when_core_supports_zip() {
    let dir = temp_root("zip-native");
    let zip_path = write_file(&dir, "game.zip", &zip_store("game.gb", b"data"));

    let g = Game::open(&zip_path, "mc|zip", true).expect("核心支持 zip 时应打开成功");
    assert_eq!(g.tmp_path, None, "核心原生支持 zip 时不应解压");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn open_zip_store_entry_extracts_to_temp_dir() {
    let dir = temp_root("zip-store");
    let zip_path = write_file(&dir, "game.zip", &zip_store("game.gb", b"stored-data"));

    let g = Game::open(&zip_path, "gb", true).expect("store 条目应解压成功");
    let tmp = g.tmp_path.as_ref().expect("应有解压产物");
    assert_eq!(tmp.file_name().and_then(|n| n.to_str()), Some("game.gb"));
    assert_eq!(fs::read(tmp).unwrap(), b"stored-data");
    // 解压产物位于系统临时目录下的独立子目录（minarch-<pid>-<n>）
    assert!(tmp.starts_with(std::env::temp_dir()));

    fs::remove_dir_all(tmp.parent().unwrap()).unwrap();
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn open_zip_deflate_entry_extracts_correctly() {
    let dir = temp_root("zip-deflate");
    let payload = b"deflated-payload-with-some-repeat-data-deflated-payload";
    let zip_path = write_file(&dir, "game.zip", &zip_deflate("game.gb", payload));

    let g = Game::open(&zip_path, "gb", true).expect("deflate 条目应解压成功");
    let tmp = g.tmp_path.as_ref().expect("应有解压产物");
    assert_eq!(fs::read(tmp).unwrap(), payload);

    fs::remove_dir_all(tmp.parent().unwrap()).unwrap();
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn open_zip_unsupported_method_returns_extract_method_error() {
    let dir = temp_root("zip-method");
    let zip_path = write_file(&dir, "game.zip", &zip_entry("game.gb", b"x", 1, 12, 0));

    let err = Game::open(&zip_path, "gb", true).expect_err("方法 12 应报错");
    match err {
        GameError::ExtractMethod {
            method, entry_name, ..
        } => {
            assert_eq!(method, 12);
            assert_eq!(entry_name, "game.gb");
        }
        other => panic!("期望 ExtractMethod，实际 {other:?}"),
    }

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn open_zip_without_matching_entry_stays_unextracted() {
    let dir = temp_root("zip-nomatch");
    let zip_path = write_file(&dir, "game.zip", &zip_store("readme.txt", b"not-a-rom"));

    let g = Game::open(&zip_path, "gb", true).expect("无匹配条目时应打开成功");
    assert_eq!(g.tmp_path, None, "无匹配扩展名条目不应解压");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn open_zip_data_descriptor_entry_aborts_scan() {
    let dir = temp_root("zip-descriptor");
    // 首个条目 GP 标志位含 0x0008 → 实现放弃扫描（与 C break 语义一致）
    let zip_path = write_file(
        &dir,
        "game.zip",
        &zip_entry("game.gb", b"data", 4, 0, 0x0008),
    );

    let g = Game::open(&zip_path, "gb", true).expect("data descriptor 时打开成功");
    assert_eq!(g.tmp_path, None, "data descriptor 条目应放弃解压");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn open_missing_zip_returns_archive_open_error() {
    let dir = temp_root("zip-missing");
    let zip_path = dir.join("nope.zip").to_string_lossy().into_owned();

    let err = Game::open(&zip_path, "gb", true).expect_err("不存在的 zip 应报错");
    match err {
        GameError::ArchiveOpen { path, .. } => assert_eq!(path, zip_path),
        other => panic!("期望 ArchiveOpen，实际 {other:?}"),
    }

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn open_zip_truncated_store_data_returns_extract_error() {
    let dir = temp_root("zip-truncated");
    // 头部声明 8 字节、实际裁剩 4 字节 → store 拷贝短读失败
    let mut z = zip_entry("game.gb", b"halfhalf", 8, 0, 0);
    z.truncate(z.len() - 4);
    let zip_path = write_file(&dir, "game.zip", &z);

    let err = Game::open(&zip_path, "gb", true).expect_err("截断的 store 数据应报错");
    match err {
        GameError::Extract { entry_name, .. } => assert_eq!(entry_name, "game.gb"),
        other => panic!("期望 Extract，实际 {other:?}"),
    }

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn open_non_zip_content_stays_unextracted() {
    let dir = temp_root("zip-garbage");
    // 可读的非 zip 字节流：无签名校验（design 决策 8），扫描读坏即放弃
    let zip_path = write_file(
        &dir,
        "game.zip",
        b"this is not a zip file, just some text data",
    );

    let g = Game::open(&zip_path, "gb", true).expect("非 zip 内容应打开成功（不解压）");
    assert_eq!(g.tmp_path, None, "非 zip 内容不应解压");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn open_zip_truncated_deflate_data_returns_extract_error() {
    let dir = temp_root("zip-truncated-deflate");
    let payload = b"deflate-data-that-will-be-cut-in-half-before-writing";
    // 数据区（完整 raw deflate）占 zip 尾部——裁半后头部仍声明完整
    // 压缩大小，读取时读到 0 字节且流未结束 → Extract 错误
    let mut z = zip_deflate("game.gb", payload);
    let data_start = 30 + "game.gb".len();
    let cut = (z.len() - data_start) / 2;
    z.truncate(z.len() - cut);
    let zip_path = write_file(&dir, "game.zip", &z);

    let err = Game::open(&zip_path, "gb", true).expect_err("截断的 deflate 数据应报错");
    match err {
        GameError::Extract { entry_name, .. } => assert_eq!(entry_name, "game.gb"),
        other => panic!("期望 Extract，实际 {other:?}"),
    }

    fs::remove_dir_all(&dir).unwrap();
}

// ── 组 3：m3u 播放列表探测 ─────────────────────────────────────

#[test]
fn open_detects_m3u_and_renames() {
    let dir = temp_root("m3u");
    let game_dir = dir.join("Game (Disc 1)");
    fs::create_dir_all(&game_dir).unwrap();
    let cue = write_file(&game_dir, "Game (Disc 1).cue", b"cue");
    let m3u = write_file(&game_dir, "Game (Disc 1).m3u", b"m3u");

    let g = Game::open(&cue, "cue", true).expect("m3u 场景应打开成功");
    assert_eq!(g.m3u_path.as_deref(), Some(m3u.as_str()));
    assert_eq!(g.name, "Game (Disc 1).m3u", "多碟游戏名应为 m3u 文件名");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn open_without_m3u_keeps_rom_basename() {
    let dir = temp_root("no-m3u");
    let rom = write_file(&dir, "game.gb", b"rom");

    let g = Game::open(&rom, "gb", true).expect("无 m3u 时应打开成功");
    assert_eq!(g.m3u_path, None);
    assert_eq!(g.name, "game.gb");

    fs::remove_dir_all(&dir).unwrap();
}

// ── 组 4：close 与振动清零 ─────────────────────────────────────

#[test]
fn close_removes_extracted_dir_and_is_idempotent() {
    let _guard = lock_disc(); // close 触碰 vibration 静态，与其他 close 调用串行
    let dir = temp_root("close");
    let zip_path = write_file(&dir, "game.zip", &zip_store("game.gb", b"data"));

    let mut g = Game::open(&zip_path, "gb", true).expect("解压应成功");
    let tmp = g.tmp_path.clone().expect("应有解压产物");
    let tmp_dir = tmp.parent().unwrap().to_path_buf();
    assert!(tmp.exists() && tmp_dir.exists());

    g.close();
    assert!(!tmp.exists(), "解压文件应被删除");
    assert!(!tmp_dir.exists(), "解压目录应被删除（偏离 C 的目录泄漏）");
    assert_eq!(g.data, None);

    // 幂等：重复 close 不 panic
    g.close();

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn close_clears_vibration() {
    let _guard = lock_disc(); // 振动引擎静态状态断言，与所有 close 调用串行
    // 本测试二进制内唯一一次 vibration init（OnceLock）
    assert!(vibration::init().is_ok(), "vibration 应首次注册");
    vibration::set_strength(100);
    assert_eq!(vibration::tick(), Some(100), "关闭前应驱动到 100");

    let dir = temp_root("vib");
    let rom = write_file(&dir, "game.gb", b"rom");
    let mut g = Game::open(&rom, "gb", true).expect("应打开成功");
    g.close();
    // 振动引擎关断去抖 3 帧（vibration 既有语义）：4 次 tick 内必返回 Some(0)
    let mut turned_off = None;
    for _ in 0..4 {
        turned_off = vibration::tick();
        if turned_off == Some(0) {
            break;
        }
    }
    assert_eq!(turned_off, Some(0), "close 应清零振动（关断去抖 3 帧内）");

    fs::remove_dir_all(&dir).unwrap();
}

// ── 组 5：change_disc 换碟 ─────────────────────────────────────

#[test]
fn change_disc_same_or_missing_path_is_noop() {
    let _guard = lock_disc();
    let _ = fs::remove_file(CHANGE_DISC_PATH);
    let dir = temp_root("disc-noop");
    let rom = write_file(&dir, "game.gb", b"rom");

    let mut g = Game::open(&rom, "gb", true).expect("应打开成功");
    g.change_disc(&rom, "gb", true).expect("同路径应无操作成功");
    let missing = dir.join("nope.gb").to_string_lossy().into_owned();
    g.change_disc(&missing, "gb", true)
        .expect("不存在路径应无操作成功");
    assert!(
        !Path::new(CHANGE_DISC_PATH).exists(),
        "无操作不应写通知文件"
    );
    assert_eq!(g.path, rom, "无操作不应改路径");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn change_disc_without_disc_control_still_writes_notify() {
    let _guard = lock_disc();
    let _ = fs::remove_file(CHANGE_DISC_PATH);
    let dir = temp_root("disc-none");
    let rom1 = write_file(&dir, "d1.gb", b"disc1");
    let rom2 = write_file(&dir, "d2.gb", b"disc2");

    let mut g = Game::open(&rom1, "gb", false).expect("应打开成功");
    // 未注册 disc_control → 跳过 replace 调用（C 此处空指针崩溃），
    // 仍写通知文件（minui 侧 recents 需要）
    g.change_disc(&rom2, "gb", false)
        .expect("未注册 disc_control 应换碟成功");
    assert_eq!(g.path, rom2);
    assert_eq!(g.data.as_deref(), Some(&b"disc2"[..]), "新碟片应整读入内存");
    assert_eq!(
        fs::read_to_string(CHANGE_DISC_PATH).unwrap(),
        rom2,
        "通知文件应为新碟片路径"
    );

    fs::remove_file(CHANGE_DISC_PATH).unwrap();
    fs::remove_dir_all(&dir).unwrap();
}

// ── 假 disc_control 回调（fn 指针不可捕获状态，调用记录经静态中转）──

unsafe extern "C" fn fake_set_eject(_ejected: bool) -> bool {
    true
}
unsafe extern "C" fn fake_get_eject() -> bool {
    false
}
unsafe extern "C" fn fake_get_image_index() -> u32 {
    0
}
unsafe extern "C" fn fake_set_image_index(_index: u32) -> bool {
    true
}
unsafe extern "C" fn fake_get_num_images() -> u32 {
    1
}
unsafe extern "C" fn fake_replace_image_index(index: u32, info: *const RetroGameInfo) -> bool {
    let path = unsafe { CStr::from_ptr((*info).path) }
        .to_string_lossy()
        .into_owned();
    record_replace(index, path);
    true
}
unsafe extern "C" fn fake_add_image_index() -> bool {
    false
}
unsafe extern "C" fn fake_set_initial_image(_index: u32, _path: *const c_char) -> bool {
    true
}
unsafe extern "C" fn fake_get_image_path(_index: u32, _s: *mut c_char, _len: usize) -> bool {
    true
}
unsafe extern "C" fn fake_get_image_label(_index: u32, _s: *mut c_char, _len: usize) -> bool {
    true
}

#[test]
fn change_disc_calls_replace_image_index_and_writes_notify() {
    let _guard = lock_disc();
    let _ = fs::remove_file(CHANGE_DISC_PATH);
    clear_replace_calls();
    // 注册 disc_control 进环境层静态（本测试二进制内唯一一次 init）
    assert!(
        environment::init(
            CString::new("/bios").unwrap(),
            CString::new("/saves").unwrap()
        )
        .is_ok(),
        "environment 应首次注册"
    );
    let ext = RetroDiskControlExtCallback {
        set_eject_state: fake_set_eject,
        get_eject_state: fake_get_eject,
        get_image_index: fake_get_image_index,
        set_image_index: fake_set_image_index,
        get_num_images: fake_get_num_images,
        replace_image_index: fake_replace_image_index,
        add_image_index: fake_add_image_index,
        set_initial_image: fake_set_initial_image,
        get_image_path: fake_get_image_path,
        get_image_label: fake_get_image_label,
    };
    let registered = environment::handle(
        RETRO_ENVIRONMENT_SET_DISK_CONTROL_EXT_INTERFACE,
        &ext as *const RetroDiskControlExtCallback as *mut c_void,
    );
    assert!(registered, "注册 disc_control 应成功");
    assert!(environment::disc_control().is_some());

    let dir = temp_root("disc-ext");
    let rom1 = write_file(&dir, "d1.gb", b"disc1");
    let rom2 = write_file(&dir, "d2.gb", b"disc2");
    let mut g = Game::open(&rom1, "gb", false).expect("应打开成功");
    g.change_disc(&rom2, "gb", false).expect("换碟应成功");

    assert_eq!(g.path, rom2);
    let calls = replace_calls();
    assert_eq!(calls.len(), 1, "replace_image_index 应被调用一次");
    assert_eq!(calls[0].0, 0, "碟片索引应为 0");
    assert_eq!(calls[0].1, rom2, "game_info.path 应为新碟片路径");
    assert_eq!(
        fs::read_to_string(CHANGE_DISC_PATH).unwrap(),
        rom2,
        "通知文件应为新碟片路径"
    );

    fs::remove_file(CHANGE_DISC_PATH).unwrap();
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn change_disc_open_failure_writes_no_notify() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_disc();
    let _ = fs::remove_file(CHANGE_DISC_PATH);
    let dir = temp_root("disc-fail");
    let rom1 = write_file(&dir, "d1.gb", b"disc1");

    let mut g = Game::open(&rom1, "gb", false).expect("应打开成功");
    // 目标碟片存在但不可读（chmod 000）→ need_fullpath=false 时打开失败
    let target = write_file(&dir, "d2.gb", b"disc2");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o000)).unwrap();
    let err = g
        .change_disc(&target, "gb", false)
        .expect_err("不可读文件应失败");
    match err {
        GameError::GameFileOpen { path, .. } => assert_eq!(path, target),
        other => panic!("期望 GameFileOpen，实际 {other:?}"),
    }
    assert!(
        !Path::new(CHANGE_DISC_PATH).exists(),
        "打开失败不应写通知文件"
    );
    assert_eq!(g.path, rom1, "失败后应保持原碟片");

    fs::remove_dir_all(&dir).unwrap();
}
