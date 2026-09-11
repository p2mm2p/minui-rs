//! sram.rs 电池存档读写测试
//!
//! 覆盖 spec「SRAM/RTC 路径与读写」Requirement 的全部 Scenario。
//!
//! 分层测试策略：纯函数 + 真文件系统（temp dir 铺文件，Vec 当"核心
//! 内存"——与 minui recents 测试同模式），无 FFI、无静态、无 mock，
//! 各用例可并行执行（每测试独立 temp 目录）。

use minarch::sram::{rtc_path, save_path};

#[test]
fn paths_use_full_filename_with_extension() {
    let dir = "/mnt/SDCARD/Saves/GBC";
    assert_eq!(
        save_path(dir, "Pokemon Red.gb"),
        "/mnt/SDCARD/Saves/GBC/Pokemon Red.gb.sav"
    );
    assert_eq!(
        rtc_path(dir, "Pokemon Red.gb"),
        "/mnt/SDCARD/Saves/GBC/Pokemon Red.gb.rtc"
    );
}

#[test]
fn paths_preserve_extension_in_name() {
    // name 契约：完整文件名含扩展名，不截断（C `strrchr(path,'/')+1`）
    assert_eq!(save_path("/s", "game.sfc"), "/s/game.sfc.sav");
    assert_eq!(rtc_path("/s", "game.gb"), "/s/game.gb.rtc");
}

// ── read_into 三态 ─────────────────────────────────────────────

use std::fs;
use std::path::PathBuf;

use minarch::sram::{ReadOutcome, read_into};

/// 为单个测试创建唯一 temp 目录（并行测试隔离）
fn temp_dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("minarch_sram_{test}"));
    fs::remove_dir_all(&dir).ok(); // 清理上次残留
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn read_missing_file_returns_not_found() {
    let dir = temp_dir("read_missing");
    let path = dir.join("none.sav");
    let mut mem = [0u8; 16];
    let outcome = read_into(&mut mem, &path).unwrap();
    assert!(matches!(outcome, ReadOutcome::NotFound));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn read_loaded_matches_file_content() {
    let dir = temp_dir("read_loaded");
    let path = dir.join("game.sav");
    fs::write(&path, [0x42u8; 16]).unwrap();
    let mut mem = [0u8; 16];
    let outcome = read_into(&mut mem, &path).unwrap();
    assert!(matches!(outcome, ReadOutcome::Loaded));
    assert_eq!(mem, [0x42u8; 16]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn read_short_file_partial_fill() {
    let dir = temp_dir("read_short");
    let path = dir.join("game.sav");
    fs::write(&path, [0xAAu8; 8]).unwrap();
    let mut mem = [0x5Cu8; 16];
    let outcome = read_into(&mut mem, &path).unwrap();
    assert!(matches!(outcome, ReadOutcome::Loaded));
    assert_eq!(&mem[..8], &[0xAAu8; 8]); // 前 8 字节被覆盖
    assert_eq!(&mem[8..], &[0x5Cu8; 8]); // 后 8 字节保持原值（C fread 至多 sram_size 字节）
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn read_empty_slice_skips_without_touching_fs() {
    let dir = temp_dir("read_skip");
    let path = dir.join("nope.sav");
    let outcome = read_into(&mut [], &path).unwrap();
    assert!(matches!(outcome, ReadOutcome::Skipped));
    assert!(!path.exists()); // 未打开文件（C size==0 跳过语义）
    let _ = fs::remove_dir_all(&dir);
}

// ── write_from：截断写 + 刷盘 ──────────────────────────────────

use minarch::sram::write_from;

#[test]
fn write_then_read_roundtrip() {
    let dir = temp_dir("write_roundtrip");
    let path = dir.join("game.sav");
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
    let path = dir.join("game.sav");
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
    let path = dir.join("nope.sav");
    write_from(&[], &path).unwrap();
    assert!(!path.exists()); // 空切片不创建文件（C size==0 跳过语义）
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn write_to_missing_directory_errors() {
    let dir = temp_dir("write_missing_dir");
    let path = dir.join("no_such_subdir").join("game.sav");
    let result = write_from(&[0u8; 4], &path);
    assert!(result.is_err()); // 目录缺失 fopen 失败（对应 C LOG_error 分支）
    let _ = fs::remove_dir_all(&dir);
}
