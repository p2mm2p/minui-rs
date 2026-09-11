//! savestate.rs 状态快照读写测试
//!
//! 覆盖 spec「快照文件路径与读写」Requirement 的全部 Scenario。
//!
//! 分层测试策略：纯函数 + 真文件系统（temp dir 铺文件，Vec 当"核心
//! 序列化缓冲"——与 sram 测试同模式），无 FFI、无静态、无 mock，
//! 各用例可并行执行（每测试独立 temp 目录）。

use std::fs;
use std::path::PathBuf;

use minarch::savestate::{ReadOutcome, read_into, state_path, write_from};

/// 为单个测试创建唯一 temp 目录（并行测试隔离）
fn temp_dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("minarch_savestate_{test}"));
    fs::remove_dir_all(&dir).ok(); // 清理上次残留
    fs::create_dir_all(&dir).unwrap();
    dir
}

// ── 路径命名契约 ───────────────────────────────────────────────

#[test]
fn state_path_appends_st_slot_after_full_filename() {
    assert_eq!(
        state_path("/mnt/SDCARD/Saves/GBC", "Pokemon Red.gb", 3),
        "/mnt/SDCARD/Saves/GBC/Pokemon Red.gb.st3"
    );
}

#[test]
fn state_path_auto_resume_slot_shape() {
    // slot 9（AUTO_RESUME_SLOT）形态一致——模块不感知槽位语义
    assert_eq!(
        state_path("/mnt/SDCARD/Saves/GBC", "Pokemon Red.gb", 9),
        "/mnt/SDCARD/Saves/GBC/Pokemon Red.gb.st9"
    );
}

// ── read_into 三态 ─────────────────────────────────────────────

#[test]
fn read_missing_file_returns_not_found() {
    let dir = temp_dir("read_missing");
    let path = dir.join("game.st0");
    let mut buf = [0u8; 16];
    let outcome = read_into(&mut buf, &path).unwrap();
    assert!(matches!(outcome, ReadOutcome::NotFound));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn read_loaded_matches_file_content() {
    let dir = temp_dir("read_loaded");
    let path = dir.join("game.st0");
    fs::write(&path, (0u8..16).collect::<Vec<u8>>()).unwrap();
    let mut buf = [0u8; 16];
    let outcome = read_into(&mut buf, &path).unwrap();
    assert!(matches!(outcome, ReadOutcome::Loaded));
    assert_eq!(
        buf,
        [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D,
            0x0E, 0x0F,
        ]
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn read_short_file_partial_fill() {
    // 8 字节文件 + 16 字节缓冲：仅前 8 字节被覆盖、后 8 字节保持
    // 调用方初始值（对应 C fread 至多 state_size 字节 + calloc 零填充）
    let dir = temp_dir("read_short");
    let path = dir.join("game.st0");
    fs::write(&path, [0xABu8; 8]).unwrap();
    let mut buf = [0x5Cu8; 16];
    let outcome = read_into(&mut buf, &path).unwrap();
    assert!(matches!(outcome, ReadOutcome::Loaded));
    assert_eq!(&buf[..8], &[0xABu8; 8]);
    assert_eq!(&buf[8..], &[0x5Cu8; 8]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn read_empty_slice_skips_without_touching_fs() {
    let dir = temp_dir("read_skip");
    let path = dir.join("nope.st0");
    let outcome = read_into(&mut [], &path).unwrap();
    assert!(matches!(outcome, ReadOutcome::Skipped));
    assert!(!path.exists()); // 未打开文件（C size==0 跳过语义）
    let _ = fs::remove_dir_all(&dir);
}

// ── write_from：截断写 + 刷盘 ──────────────────────────────────

#[test]
fn write_then_read_roundtrip() {
    let dir = temp_dir("write_roundtrip");
    let path = dir.join("game.st0");
    let data = [0x7Bu8; 16];
    write_from(&data, &path).unwrap();
    let mut buf = [0u8; 16];
    let outcome = read_into(&mut buf, &path).unwrap();
    assert!(matches!(outcome, ReadOutcome::Loaded));
    assert_eq!(buf, data);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn write_truncates_existing_file() {
    let dir = temp_dir("write_truncate");
    let path = dir.join("game.st0");
    write_from(&[0x11u8; 16], &path).unwrap();
    write_from(&[0x22u8; 4], &path).unwrap(); // 截断语义：fopen "w"
    assert_eq!(fs::metadata(&path).unwrap().len(), 4);
    let mut buf = [0u8; 4];
    read_into(&mut buf, &path).unwrap();
    assert_eq!(buf, [0x22u8; 4]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn write_empty_slice_skips_without_creating_file() {
    let dir = temp_dir("write_skip");
    let path = dir.join("nope.st0");
    write_from(&[], &path).unwrap();
    assert!(!path.exists()); // 空切片不创建文件（C size==0 跳过语义）
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn write_to_missing_directory_errors() {
    let dir = temp_dir("write_missing_dir");
    let path = dir.join("no_such_subdir").join("game.st0");
    let result = write_from(&[0u8; 4], &path);
    assert!(result.is_err()); // 目录缺失 fopen 失败（对应 C LOG_error 分支）
    let _ = fs::remove_dir_all(&dir);
}
