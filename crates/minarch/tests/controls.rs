//! controls.rs 纯映射层测试
//!
//! 覆盖 spec「controls 输入映射模型与表」「init_core_mapping 核心映射
//! 初始化」「build_buttons 按键映射」「bind 解析」四个 Requirement 的
//! 全部 Scenario。FFI 路径用 libretro.rs 的 `RetroInputDescriptor`
//! 构造真实指针数组（CString 保活）。

use std::ffi::CString;

use common::input::{
    BTN_A, BTN_ANALOG_UP, BTN_B, BTN_DPAD_UP, BTN_ID_A, BTN_ID_B, BTN_ID_DPAD_UP, BTN_ID_R3,
    BTN_ID_SELECT, BTN_ID_START, BTN_START,
};
use minarch::controls::*;
use minarch::libretro::{RETRO_DEVICE_ID_JOYPAD_A, RETRO_DEVICE_JOYPAD, RetroInputDescriptor};

// ── 任务 1.x：模型与四张表 ──────────────────────────────────────

#[test]
fn default_mapping_matches_c() {
    let mappings = default_button_mapping();
    assert_eq!(mappings.len(), 16);

    // 抽查与 C `default_button_mapping`（minarch.c:672-690）逐项一致
    let up = &mappings[0];
    assert_eq!(up.name, "Up");
    assert_eq!(up.retro, minarch::libretro::RETRO_DEVICE_ID_JOYPAD_UP);
    assert_eq!(up.local, Some(BTN_ID_DPAD_UP));
    assert_eq!(up.default_, Some(BTN_ID_DPAD_UP));
    assert!(!up.mod_);
    assert!(!up.ignore);

    let a = &mappings[4];
    assert_eq!(a.name, "A Button");
    assert_eq!(a.retro, RETRO_DEVICE_ID_JOYPAD_A);
    assert_eq!(a.local, Some(BTN_ID_A));

    let start = &mappings[8];
    assert_eq!(start.name, "Start");
    assert_eq!(start.retro, minarch::libretro::RETRO_DEVICE_ID_JOYPAD_START);
    assert_eq!(start.local, Some(BTN_ID_START));

    let r3 = &mappings[15];
    assert_eq!(r3.name, "R3 Button");
    assert_eq!(r3.retro, minarch::libretro::RETRO_DEVICE_ID_JOYPAD_R3);
    assert_eq!(r3.local, Some(BTN_ID_R3));
}

#[test]
fn device_button_names_match_c() {
    // C `device_button_names`（minarch.c:731-746）按 BTN_ID 索引
    assert_eq!(device_button_name(BTN_ID_DPAD_UP), Some("UP"));
    assert_eq!(device_button_name(BTN_ID_A), Some("A"));
    assert_eq!(device_button_name(BTN_ID_B), Some("B"));
    assert_eq!(device_button_name(BTN_ID_START), Some("START"));
    assert_eq!(device_button_name(BTN_ID_SELECT), Some("SELECT"));
    assert_eq!(device_button_name(BTN_ID_R3), Some("R3"));
    assert_eq!(device_button_name(99), None);
}

#[test]
fn button_label_for_covers_none_and_menu_segments() {
    // C `button_labels` 33 项（minarch.c:752-780）：NONE + 16 普通 + 16 MENU+
    assert_eq!(button_label_for(None, false), "NONE");
    assert_eq!(button_label_for(Some(BTN_ID_START), false), "START");
    assert_eq!(button_label_for(Some(BTN_ID_START), true), "MENU+START");
    assert_eq!(button_label_for(Some(BTN_ID_DPAD_UP), true), "MENU+UP");
    assert_eq!(button_label_for(Some(99), false), "NONE");
}

#[test]
fn shortcuts_default_unbound() {
    let shortcuts = shortcuts_default_mapping();
    assert_eq!(shortcuts.len(), 8);
    for shortcut in &shortcuts {
        assert!(shortcut.local.is_none(), "{} 应默认未绑定", shortcut.name);
        assert!(!shortcut.mod_);
    }
    let names: Vec<&str> = shortcuts.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Save State",
            "Load State",
            "Reset Game",
            "Save & Quit",
            "Cycle Scaling",
            "Cycle Effect",
            "Toggle FF",
            "Hold FF"
        ]
    );
}

// ── 任务 2.x：init_core_mapping ─────────────────────────────────

#[test]
fn init_without_descriptors_passthrough() {
    let defaults = default_button_mapping();
    let result = unsafe { init_core_mapping(&defaults, core::ptr::null()) };
    assert_eq!(result.len(), 16);
    for mapping in &result {
        assert!(!mapping.ignore, "未上报描述符不应有 ignore");
    }
    assert_eq!(result[4].name, "A Button");
}

#[test]
fn init_with_partial_descriptors_overrides_and_ignores() {
    let desc_a = CString::new("A (Core)").unwrap();
    let desc_b = CString::new("B (Core)").unwrap();
    let descriptors = [
        RetroInputDescriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_A,
            description: desc_a.as_ptr(),
        },
        RetroInputDescriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: minarch::libretro::RETRO_DEVICE_ID_JOYPAD_B,
            description: desc_b.as_ptr(),
        },
        RetroInputDescriptor {
            port: 0,
            device: 0,
            index: 0,
            id: 0,
            description: core::ptr::null(), // 终止项
        },
    ];

    let result = unsafe { init_core_mapping(&default_button_mapping(), descriptors.as_ptr()) };
    assert_eq!(result[4].name, "A (Core)");
    assert_eq!(result[5].name, "B (Core)");
    assert!(!result[4].ignore);
    assert!(!result[5].ignore);
    // 其余 14 项：核心上报过但未包含 → ignore
    let ignored = result.iter().filter(|m| m.ignore).count();
    assert_eq!(ignored, 14);
}

#[test]
fn init_filters_port_device_and_out_of_range_ids() {
    let desc_valid = CString::new("Only Button").unwrap();
    let desc_bad = CString::new("Wrong Port").unwrap();
    let descriptors = [
        RetroInputDescriptor {
            port: 1, // 非目标端口
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_A,
            description: desc_bad.as_ptr(),
        },
        RetroInputDescriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: 17, // 越界 id（C 打印 UNAVAILABLE 后跳过）
            description: desc_bad.as_ptr(),
        },
        RetroInputDescriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_A,
            description: desc_valid.as_ptr(),
        },
        RetroInputDescriptor {
            port: 0,
            device: 0,
            index: 0,
            id: 0,
            description: core::ptr::null(),
        },
    ];

    let result = unsafe { init_core_mapping(&default_button_mapping(), descriptors.as_ptr()) };
    assert_eq!(result[4].name, "Only Button");
    assert!(!result[4].ignore);
}

// ── 任务 3.x：build_buttons ─────────────────────────────────────

#[test]
fn build_single_and_combo() {
    let mappings = default_button_mapping();
    let single = build_buttons(&mappings, BTN_A, false);
    assert_eq!(single, 1 << RETRO_DEVICE_ID_JOYPAD_A);

    let combo = build_buttons(&mappings, BTN_A | BTN_START, false);
    assert_eq!(
        combo,
        (1 << RETRO_DEVICE_ID_JOYPAD_A) | (1 << minarch::libretro::RETRO_DEVICE_ID_JOYPAD_START)
    );
}

#[test]
fn build_folds_analog_into_dpad_bindings() {
    let mappings = default_button_mapping();
    // 仅摇杆方向按下 → Up 映射（local=BTN_ID_DPAD_UP）被触发（折叠）
    let buttons = build_buttons(&mappings, BTN_ANALOG_UP, false);
    assert_eq!(buttons, 1 << minarch::libretro::RETRO_DEVICE_ID_JOYPAD_UP);
}

#[test]
fn build_respects_menu_modifier_and_skips() {
    let mappings = vec![
        ButtonMapping {
            name: "Modded".to_string(),
            retro: RETRO_DEVICE_ID_JOYPAD_A,
            local: Some(BTN_ID_A),
            mod_: true,
            default_: Some(BTN_ID_A),
            ignore: false,
        },
        ButtonMapping {
            name: "Unbound".to_string(),
            retro: minarch::libretro::RETRO_DEVICE_ID_JOYPAD_B,
            local: None,
            mod_: false,
            default_: None,
            ignore: false,
        },
        ButtonMapping {
            name: "Ignored".to_string(),
            retro: minarch::libretro::RETRO_DEVICE_ID_JOYPAD_X,
            local: Some(BTN_ID_B),
            mod_: false,
            default_: Some(BTN_ID_B),
            ignore: true,
        },
    ];

    assert_eq!(
        build_buttons(&mappings, BTN_A, false),
        0,
        "MENU 修饰未按下不触发"
    );
    assert_eq!(
        build_buttons(&mappings, BTN_A, true),
        1 << RETRO_DEVICE_ID_JOYPAD_A
    );
    // None/ignore 项永不出现在结果中
    assert_eq!(build_buttons(&mappings, BTN_B, true), 0);
    // 无命中返回 0
    assert_eq!(build_buttons(&mappings, BTN_DPAD_UP, true), 0);
}

// ── 任务 4.x：bind 解析 ─────────────────────────────────────────

#[test]
fn parse_bind_button_three_forms() {
    assert_eq!(
        parse_bind_button("START"),
        Some((Some(BTN_ID_START), false))
    );
    assert_eq!(
        parse_bind_button("MENU+START"),
        Some((Some(BTN_ID_START), true))
    );
    assert_eq!(parse_bind_button("NONE"), Some((None, false)));
    assert_eq!(parse_bind_button("GARBAGE"), None);
    assert_eq!(parse_bind_button(""), None);
}

#[test]
fn parse_bind_default_two_segments() {
    let up = (
        Some(BTN_ID_DPAD_UP),
        minarch::libretro::RETRO_DEVICE_ID_JOYPAD_UP,
    );
    assert_eq!(parse_bind_default("UP"), Some(up));
    assert_eq!(parse_bind_default("UP:UP"), Some(up));
    // 后缀未知 → 整体 None（C 版静默回退前缀 retro，Rust 不继承）
    assert_eq!(parse_bind_default("UP:GARBAGE"), None);
    assert_eq!(parse_bind_default("GARBAGE"), None);
    assert_eq!(parse_bind_default("NONE"), None);
}

// ── 任务 2.x：handler 与 buttons 静态（implement-minarch-main）────

use common::input::{BTN_ID_L1, BTN_ID_L2, BTN_ID_X, BTN_L1, BTN_L2, InputState};

/// 进程级 `BUTTONS` 静态迫使与 buttons 相关的测试**在同一测试函数内
/// 串行**（与 environment 静态层测试同约束——`init_buttons` 只能成功
/// 一次，跨测试并行会互相污染掩码）。
#[test]
fn buttons_static_serial_tests() {
    // ① init 生命周期
    assert!(init_buttons().is_ok());
    let err = init_buttons().unwrap_err();
    assert_eq!(err.into_inner(), 0);

    // ② 写入读回
    set_buttons(0b1010);
    assert_eq!(get_buttons(), 0b1010);
    set_buttons(0);
    assert_eq!(get_buttons(), 0);

    // ③ input_state 查询
    set_buttons(
        (1 << minarch::libretro::RETRO_DEVICE_ID_JOYPAD_A)
            | (1 << minarch::libretro::RETRO_DEVICE_ID_JOYPAD_B),
    );
    // MASK 查询返回当前掩码（A=8, B=0 → bit8|bit0 = 257）
    let mask = input_state_handler(
        0,
        RETRO_DEVICE_JOYPAD,
        0,
        minarch::libretro::RETRO_DEVICE_ID_JOYPAD_MASK,
    );
    assert_eq!(mask, 0b100000001);
    // 单键查询 0/1
    assert_eq!(input_state_handler(0, RETRO_DEVICE_JOYPAD, 0, 8), 1); // A
    assert_eq!(input_state_handler(0, RETRO_DEVICE_JOYPAD, 0, 0), 1); // B
    assert_eq!(input_state_handler(0, RETRO_DEVICE_JOYPAD, 0, 2), 0); // X
    // 越界 id 返回 0
    assert_eq!(input_state_handler(0, RETRO_DEVICE_JOYPAD, 0, 17), 0);
    // ANALOG 返回 0（摇杆轴值留独立 change）
    assert_eq!(
        input_state_handler(
            0,
            minarch::libretro::RETRO_DEVICE_ANALOG,
            0,
            minarch::libretro::RETRO_DEVICE_ID_ANALOG_X
        ),
        0
    );
    // 其他设备返回 0
    assert_eq!(input_state_handler(0, 999, 0, 0), 0);

    // ④ update_buttons 写入
    let mappings = default_button_mapping();
    update_buttons(&mappings, BTN_A | BTN_START, false);
    let mask = get_buttons();
    assert_ne!(mask & (1 << RETRO_DEVICE_ID_JOYPAD_A), 0);
    assert_ne!(
        mask & (1 << minarch::libretro::RETRO_DEVICE_ID_JOYPAD_START),
        0
    );
    update_buttons(&mappings, 0, false);
    assert_eq!(get_buttons(), 0);
}

#[test]
fn detect_shortcuts_actions() {
    let shortcuts = shortcuts_default_mapping();
    // 绑定：Save State=A、Reset Game=X、Toggle FF=L1、Hold FF=R3
    let mut s = shortcuts;
    s[0].local = Some(BTN_ID_A);
    s[2].local = Some(BTN_ID_X);
    s[6].local = Some(BTN_ID_L1);
    s[7].local = Some(BTN_ID_R3);

    let mut state = ShortcutState::default();

    // Save State 触发
    let mut input = InputState::new();
    input.pressed = BTN_A;
    input.just_pressed = BTN_A;
    let actions = detect_shortcuts(&s, &input, false, &mut state);
    assert_eq!(actions, vec![ShortcutAction::SaveState]);

    // Reset Game 触发
    let mut input = InputState::new();
    input.pressed = common::input::BTN_X;
    input.just_pressed = common::input::BTN_X;
    let actions = detect_shortcuts(&s, &input, false, &mut state);
    assert_eq!(actions, vec![ShortcutAction::ResetGame]);

    // Toggle FF 按下
    let mut input = InputState::new();
    input.pressed = BTN_L1;
    input.just_pressed = BTN_L1;
    let actions = detect_shortcuts(&s, &input, false, &mut state);
    assert_eq!(actions, vec![ShortcutAction::ToggleFf]);

    // Hold FF 按下 → on=true
    let mut input = InputState::new();
    input.pressed = common::input::BTN_R3;
    input.just_pressed = common::input::BTN_R3;
    let actions = detect_shortcuts(&s, &input, false, &mut state);
    assert_eq!(actions, vec![ShortcutAction::HoldFf { on: true }]);

    // Hold FF 释放（独立 state，未触发过 Toggle → toggled_ff_on=false）→ on=false
    let mut input = InputState::new();
    input.just_released = common::input::BTN_R3;
    let actions = detect_shortcuts(&s, &input, false, &mut ShortcutState::default());
    assert_eq!(actions, vec![ShortcutAction::HoldFf { on: false }]);

    // MENU 修饰：mod_ shortcut 需要 menu_pressed
    let mut modded = shortcuts_default_mapping();
    modded[4].local = Some(BTN_ID_A);
    modded[4].mod_ = true;
    let mut input = InputState::new();
    input.pressed = BTN_A;
    input.just_pressed = BTN_A;
    // 无 menu → 不触发
    assert!(detect_shortcuts(&modded, &input, false, &mut ShortcutState::default()).is_empty());
    // 有 menu → 触发 CycleScaling
    let actions = detect_shortcuts(&modded, &input, true, &mut ShortcutState::default());
    assert_eq!(actions, vec![ShortcutAction::CycleScaling]);
}

#[test]
fn detect_shortcuts_hold_ff_toggle_interaction() {
    // C 语义：TOGGLE 打开后，HOLD 的释放不得关闭快进（toggled_ff_on）
    let mut s = shortcuts_default_mapping();
    s[6].local = Some(BTN_ID_L1); // Toggle FF
    s[7].local = Some(BTN_ID_L2); // Hold FF

    let mut state = ShortcutState::default();
    // Toggle 按下 → toggled_ff_on = true
    let mut input = InputState::new();
    input.pressed = BTN_L1;
    input.just_pressed = BTN_L1;
    detect_shortcuts(&s, &input, false, &mut state);

    // Hold FF 释放（toggled_ff_on=true）→ 不产生 HoldFf{on:false}
    let mut input = InputState::new();
    input.just_released = BTN_L2;
    let actions = detect_shortcuts(&s, &input, false, &mut state);
    assert!(actions.is_empty());

    // 新按住周期：Toggled 释放后（下一帧 just_released L1），toggled_ff_on 仍 true
    // （C 的 static 不随释放清零——装配层在消费 ToggleFf 时自行管理快进状态）
    let mut input = InputState::new();
    input.just_released = BTN_L1;
    let actions = detect_shortcuts(&s, &input, false, &mut state);
    assert!(actions.is_empty());
}
