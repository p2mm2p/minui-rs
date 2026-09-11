//! assembly.rs 装配层纯逻辑集成测试（implement-minarch-main）
//!
//! 覆盖 spec「存档序列化编排与 resume」的可测部分：`save_state`/
//! `load_state`/`read_resume_slot` 与 mock 核心（`tests/fixtures/mock_core.c`）
//! 的真实 dlopen 往返。
//!
//! 注：装配层的平台交互部分（menu_loop/handle_shortcut 的 Platform 调用）
//! 在 `main.rs` 且 feature 门控，无法在无平台 feature 的测试环境链接——
//! 与 minui 先例一致（装配层不直接测试），本文件覆盖可测的纯逻辑层。

mod test_common;

use std::path::PathBuf;

use minarch::assembly::{load_state, read_resume_slot, save_state};
use minarch::libretro::Core;
use minarch::menu::SaveStateIo;

/// 临时测试根目录
fn temp_root(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("minarch_assembly_it_{}_{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 构造装配测试的 SaveStateIo（states 目录 + minui 目录 + 单碟）
fn make_save_io(root: &std::path::Path, game_name: &str) -> SaveStateIo {
    let states_dir = root.join("states");
    let minui_dir = root.join("minui");
    std::fs::create_dir_all(&states_dir).unwrap();
    std::fs::create_dir_all(&minui_dir).unwrap();
    SaveStateIo::new(
        minui_dir.to_str().unwrap(),
        states_dir.to_str().unwrap(),
        game_name,
        Vec::new(),
        "",
    )
}

#[test]
fn save_state_writes_snapshot_file() {
    let root = temp_root("save");
    let path = test_common::build_mock_core("assembly_save", &[]);
    let core = Core::open(
        path.to_str().unwrap(),
        "TST",
        "/cfg",
        root.join("states").to_str().unwrap(),
        "/saves",
        "/bios",
    )
    .expect("mock 核心加载");

    let save_io = make_save_io(&root, "Game.mc");
    save_state(&core, &game(&save_io), 3, &save_io, false).expect("存档应成功");

    // 快照文件存在且内容为 mock 的 0xAB × 8
    let snapshot = root.join("states/Game.mc.st3");
    let bytes = std::fs::read(&snapshot).expect("快照文件应存在");
    assert_eq!(bytes.len(), 8);
    assert!(bytes.iter().all(|b| *b == 0xAB));

    // slot 记忆文件写入 3
    let slot_mem = root.join("minui/Game.mc.txt");
    assert_eq!(std::fs::read_to_string(slot_mem).unwrap().trim(), "3");
}

#[test]
fn save_state_ff_gate_restores() {
    let root = temp_root("ff");
    let path = test_common::build_mock_core("assembly_ff", &[]);
    let core = Core::open(
        path.to_str().unwrap(),
        "TST",
        "/cfg",
        root.join("states").to_str().unwrap(),
        "/saves",
        "/bios",
    )
    .expect("mock 核心加载");

    let save_io = make_save_io(&root, "Game.mc");
    // 快进开 → 存档 → 快进恢复
    minarch::core::init(1280, 720).ok();
    minarch::core::set_fast_forward(true);
    save_state(&core, &game(&save_io), 0, &save_io, true).expect("存档应成功");
    assert!(minarch::core::fast_forward(), "快进应被恢复");
}

#[test]
fn load_state_no_save_is_noop() {
    let root = temp_root("load_noop");
    let path = test_common::build_mock_core("assembly_load_noop", &[]);
    let core = Core::open(
        path.to_str().unwrap(),
        "TST",
        "/cfg",
        root.join("states").to_str().unwrap(),
        "/saves",
        "/bios",
    )
    .expect("mock 核心加载");

    let save_io = make_save_io(&root, "Game.mc");
    // 无快照 → 空操作（不 panic、不写文件）
    load_state(&core, &game(&save_io), 0, &save_io, false).expect("无存档应 Ok");
    assert!(!root.join("states/Game.mc.st0").exists());
}

#[test]
fn load_state_reads_and_unserializes() {
    let root = temp_root("load");
    let path = test_common::build_mock_core("assembly_load", &[]);
    let core = Core::open(
        path.to_str().unwrap(),
        "TST",
        "/cfg",
        root.join("states").to_str().unwrap(),
        "/saves",
        "/bios",
    )
    .expect("mock 核心加载");

    let save_io = make_save_io(&root, "Game.mc");
    // 先存档再读档
    save_state(&core, &game(&save_io), 1, &save_io, false).unwrap();
    load_state(&core, &game(&save_io), 1, &save_io, false).expect("读档应成功");
}

#[test]
fn resume_slot_path_states() {
    let root = temp_root("resume");
    let path = root.join("resume_slot.txt");

    // 无文件 → AUTO_RESUME_SLOT
    assert_eq!(
        read_resume_slot(path.to_str().unwrap()),
        common::paths::AUTO_RESUME_SLOT
    );

    // 内容 3 → 3，读后删除
    std::fs::write(&path, "3").unwrap();
    assert_eq!(read_resume_slot(path.to_str().unwrap()), 3);
    assert!(!path.exists());

    // 内容 8 → AUTO_RESUME_SLOT（slot 8 归一）
    std::fs::write(&path, "8").unwrap();
    assert_eq!(
        read_resume_slot(path.to_str().unwrap()),
        common::paths::AUTO_RESUME_SLOT
    );

    // 解析失败 → AUTO_RESUME_SLOT
    std::fs::write(&path, "garbage").unwrap();
    assert_eq!(
        read_resume_slot(path.to_str().unwrap()),
        common::paths::AUTO_RESUME_SLOT
    );
}

/// 构造最小 Game（name 与 save_io 一致）
fn game(save_io: &SaveStateIo) -> minarch::game::Game {
    minarch::game::Game {
        path: "/tmp/Game.mc".to_string(),
        name: save_io.game_name.clone(),
        m3u_path: None,
        tmp_path: None,
        data: Some(vec![0u8; 16]),
    }
}
