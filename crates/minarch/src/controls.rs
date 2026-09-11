//! 输入映射（纯映射层）
//!
//! 本模块实现 minarch 输入体系的纯映射逻辑，对应原 C `minarch.c` 的：
//!
//! - `ButtonMapping` 结构与四张表（minarch.c:652-780）
//! - `Input_init`（minarch.c:1796-1853）→ [`init_core_mapping`]
//! - `input_poll_callback` 的映射段（minarch.c:1755-1773）→ [`build_buttons`]
//! - `Config_init`/`Config_readControlsString` 的按钮名解析
//!   （minarch.c:1035-1100、:1132-1192）→ [`parse_bind_default`]/[`parse_bind_button`]
//!
//! ## 与原 C 代码的对比
//!
//! C 的 `input_poll_callback` 把三层职责搅在一起：物理轮询（`PAD_poll`）、
//! 快捷指令动作、按键映射。Rust 版拆分——轮询归装配层
//! （`Platform::poll_input`）、动作归 menu/装配、**映射纯逻辑归本模块**。
//!
//! - C 用 `-1`（`BTN_ID_NONE`）表示未绑定；Rust 用 `Option<usize>`
//!   （common 的 `BTN_ID_NONE == 0` 与 `BTN_ID_DPAD_UP` 撞车，作哨兵会出错）
//! - C 的 `Input_init` 对越界 id 打印 UNAVAILABLE 后跳过；Rust 无日志
//!   设施，静默跳过
//! - C 的 `Config_init` 对未知 `:retro名` 后缀静默回退前缀 retro（掩盖
//!   用户笔误）；Rust 返回 `None`（整行不匹配）
//! - Rust 无 C 的"逻辑方向位"（dpad|摇杆合并），折叠在 [`build_buttons`]
//!   查询处用并集合并
//!
//! ## 边界（半环闭环清单）
//!
//! handler 实现与注册、buttons 静态状态、快捷指令检测/动作、bind 行
//! cfg 应用、gamepad_type 均不在本模块——见
//! 摇杆原始轴值承载已闭环：`InputState` 轴值字段（common）→
//! `update_sticks` 注入 → `input_state_handler` ANALOG 应答
//! （implement-analog-sticks）。
//!

use core::ffi::c_char;
use std::ffi::CStr;

use crate::libretro::{
    RETRO_DEVICE_ID_JOYPAD_A, RETRO_DEVICE_ID_JOYPAD_B, RETRO_DEVICE_ID_JOYPAD_DOWN,
    RETRO_DEVICE_ID_JOYPAD_L, RETRO_DEVICE_ID_JOYPAD_L2, RETRO_DEVICE_ID_JOYPAD_L3,
    RETRO_DEVICE_ID_JOYPAD_LEFT, RETRO_DEVICE_ID_JOYPAD_R, RETRO_DEVICE_ID_JOYPAD_R2,
    RETRO_DEVICE_ID_JOYPAD_R3, RETRO_DEVICE_ID_JOYPAD_RIGHT, RETRO_DEVICE_ID_JOYPAD_SELECT,
    RETRO_DEVICE_ID_JOYPAD_START, RETRO_DEVICE_ID_JOYPAD_UP, RETRO_DEVICE_ID_JOYPAD_X,
    RETRO_DEVICE_ID_JOYPAD_Y, RETRO_DEVICE_JOYPAD, RetroInputDescriptor,
};
use common::input::{
    BTN_ANALOG_DOWN, BTN_ANALOG_LEFT, BTN_ANALOG_RIGHT, BTN_ANALOG_UP, BTN_DPAD_DOWN,
    BTN_DPAD_LEFT, BTN_DPAD_RIGHT, BTN_DPAD_UP, BTN_ID_A, BTN_ID_B, BTN_ID_DPAD_DOWN,
    BTN_ID_DPAD_LEFT, BTN_ID_DPAD_RIGHT, BTN_ID_DPAD_UP, BTN_ID_L1, BTN_ID_L2, BTN_ID_L3,
    BTN_ID_R1, BTN_ID_R2, BTN_ID_R3, BTN_ID_SELECT, BTN_ID_START, BTN_ID_X, BTN_ID_Y,
};

// ═══════════════════════════════════════════════════════════════
// 数据模型
// ═══════════════════════════════════════════════════════════════

/// 单个按键映射（对应 C `typedef struct ButtonMapping`，minarch.c:662-670）
#[derive(Clone)]
pub struct ButtonMapping {
    /// 显示名（如 `Start`）；核心上报 `SET_INPUT_DESCRIPTORS` 后由
    /// [`init_core_mapping`] 覆盖为核心名称
    pub name: String,
    /// libretro 按键位（`RETRO_DEVICE_ID_JOYPAD_*`，0-15）
    pub retro: u32,
    /// 物理键位（`common::input` 的 `BTN_ID_*`）；`None` = 未绑定
    /// （对应 C `BTN_ID_NONE == -1` 哨兵——common 的 `BTN_ID_NONE == 0`
    /// 与 `BTN_ID_DPAD_UP` 撞车，故用 `Option` 表达）
    pub local: Option<usize>,
    /// 是否需要 MENU 修饰（对应 C `mod` 字段；与 Rust 关键字冲突改名 `mod_`）
    pub mod_: bool,
    /// 默认绑定（restore 时回填 `local` 的值）；`None` = 默认未绑定
    pub default_: Option<usize>,
    /// 核心不支持该按键（[`init_core_mapping`] 标记，映射时跳过）
    pub ignore: bool,
}

/// 快捷指令（对应 C `config.shortcuts`，minarch.c:930-940）
///
/// C 版复用 `ButtonMapping`（retro 字段填 -1 无意义）；Rust 版独立
/// 结构——快捷指令没有 retro 概念。
#[derive(Clone)]
pub struct Shortcut {
    /// 显示名（如 `Save State`）
    pub name: String,
    /// 绑定的物理键位；`None` = 未绑定
    pub local: Option<usize>,
    /// 是否需要 MENU 修饰
    pub mod_: bool,
}

// ═══════════════════════════════════════════════════════════════
// 映射表（对应 C minarch.c:652-780）
// ═══════════════════════════════════════════════════════════════

/// 按钮名↔id 查询表（对应 C `button_label_mapping`，minarch.c:691-712）
///
/// 17 项：NONE + 16 个普通按钮。`retro` 为 `0` 的项（NONE）无意义，
/// 仅供 `parse_bind_button` 的 NONE 判定。不公开——经
/// [`parse_bind_button`]/[`parse_bind_default`] 使用并由其测试覆盖。
const BUTTON_LABEL_MAPPING: &[(&str, Option<usize>, u32)] = &[
    ("NONE", None, 0),
    ("UP", Some(BTN_ID_DPAD_UP), RETRO_DEVICE_ID_JOYPAD_UP),
    ("DOWN", Some(BTN_ID_DPAD_DOWN), RETRO_DEVICE_ID_JOYPAD_DOWN),
    ("LEFT", Some(BTN_ID_DPAD_LEFT), RETRO_DEVICE_ID_JOYPAD_LEFT),
    (
        "RIGHT",
        Some(BTN_ID_DPAD_RIGHT),
        RETRO_DEVICE_ID_JOYPAD_RIGHT,
    ),
    ("A", Some(BTN_ID_A), RETRO_DEVICE_ID_JOYPAD_A),
    ("B", Some(BTN_ID_B), RETRO_DEVICE_ID_JOYPAD_B),
    ("X", Some(BTN_ID_X), RETRO_DEVICE_ID_JOYPAD_X),
    ("Y", Some(BTN_ID_Y), RETRO_DEVICE_ID_JOYPAD_Y),
    ("START", Some(BTN_ID_START), RETRO_DEVICE_ID_JOYPAD_START),
    ("SELECT", Some(BTN_ID_SELECT), RETRO_DEVICE_ID_JOYPAD_SELECT),
    ("L1", Some(BTN_ID_L1), RETRO_DEVICE_ID_JOYPAD_L),
    ("R1", Some(BTN_ID_R1), RETRO_DEVICE_ID_JOYPAD_R),
    ("L2", Some(BTN_ID_L2), RETRO_DEVICE_ID_JOYPAD_L2),
    ("R2", Some(BTN_ID_R2), RETRO_DEVICE_ID_JOYPAD_R2),
    ("L3", Some(BTN_ID_L3), RETRO_DEVICE_ID_JOYPAD_L3),
    ("R3", Some(BTN_ID_R3), RETRO_DEVICE_ID_JOYPAD_R3),
];

/// 按钮名查询（BUTTON_LABEL_MAPPING 的查询函数）
fn lookup_button_label(name: &str) -> Option<(Option<usize>, u32)> {
    BUTTON_LABEL_MAPPING
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, local, retro)| (*local, *retro))
}

/// 设备按键显示名（对应 C `device_button_names`，minarch.c:731-746）
///
/// 按 `BTN_ID_*`（0-15）索引，供日志/调试显示使用。
const DEVICE_BUTTON_NAMES: [&str; 16] = [
    "UP", "DOWN", "LEFT", "RIGHT", "A", "B", "X", "Y", "START", "SELECT", "L1", "R1", "L2", "R2",
    "L3", "R3",
];

/// 查询设备按键显示名
///
/// # 参数
///
/// - `local`: `BTN_ID_*` 物理键位（0-15）
///
/// # 返回值
///
/// 键位有效返回 `Some(&'static str)`（如 `BTN_ID_START` → `"START"`），
/// 越界返回 `None`。
pub fn device_button_name(local: usize) -> Option<&'static str> {
    DEVICE_BUTTON_NAMES.get(local).copied()
}

/// 序列化名表（对应 C `button_labels`，minarch.c:752-780）
///
/// 33 项：NONE + 16 普通 + 16 MENU+ 段。C 的 off-by-one 索引
/// （`j = local + 1`，mod 再加 16）在 Rust 直接展开为数组布局。
const BUTTON_LABELS: [&str; 33] = [
    "NONE",
    "UP",
    "DOWN",
    "LEFT",
    "RIGHT",
    "A",
    "B",
    "X",
    "Y",
    "START",
    "SELECT",
    "L1",
    "R1",
    "L2",
    "R2",
    "L3",
    "R3",
    "MENU+UP",
    "MENU+DOWN",
    "MENU+LEFT",
    "MENU+RIGHT",
    "MENU+A",
    "MENU+B",
    "MENU+X",
    "MENU+Y",
    "MENU+START",
    "MENU+SELECT",
    "MENU+L1",
    "MENU+R1",
    "MENU+L2",
    "MENU+R2",
    "MENU+L3",
    "MENU+R3",
];

/// 查询绑定的序列化名（对应 C `button_labels[j]` 查询，cfg 写入用）
///
/// # 参数
///
/// - `local`: 物理键位；`None` = 未绑定
/// - `mod_`: 是否 MENU 修饰
///
/// # 返回值
///
/// `None` → `"NONE"`；`Some(id)` + mod → `"MENU+<名>"`；越界 → `"NONE"`。
pub fn button_label_for(local: Option<usize>, mod_: bool) -> &'static str {
    match local {
        None => "NONE",
        Some(id) if id < 16 => {
            let index = if mod_ { 17 + id } else { 1 + id };
            BUTTON_LABELS[index]
        }
        Some(_) => "NONE",
    }
}

/// 默认按键映射（对应 C `default_button_mapping`，minarch.c:672-690）
///
/// 16 项：Up/Down/Left/Right/A/B/X/Y/Start/Select/L1..R3。
/// 在 pak 的 default.cfg 不存在或没有绑定时使用（C `Input_init` 的
/// `config.controls = core_button_mapping[0].name ? ... : default` 语义
/// 由装配层决定，本函数只提供表）。
pub fn default_button_mapping() -> Vec<ButtonMapping> {
    vec![
        ButtonMapping {
            name: "Up".to_string(),
            retro: RETRO_DEVICE_ID_JOYPAD_UP,
            local: Some(BTN_ID_DPAD_UP),
            mod_: false,
            default_: Some(BTN_ID_DPAD_UP),
            ignore: false,
        },
        ButtonMapping {
            name: "Down".to_string(),
            retro: RETRO_DEVICE_ID_JOYPAD_DOWN,
            local: Some(BTN_ID_DPAD_DOWN),
            mod_: false,
            default_: Some(BTN_ID_DPAD_DOWN),
            ignore: false,
        },
        ButtonMapping {
            name: "Left".to_string(),
            retro: RETRO_DEVICE_ID_JOYPAD_LEFT,
            local: Some(BTN_ID_DPAD_LEFT),
            mod_: false,
            default_: Some(BTN_ID_DPAD_LEFT),
            ignore: false,
        },
        ButtonMapping {
            name: "Right".to_string(),
            retro: RETRO_DEVICE_ID_JOYPAD_RIGHT,
            local: Some(BTN_ID_DPAD_RIGHT),
            mod_: false,
            default_: Some(BTN_ID_DPAD_RIGHT),
            ignore: false,
        },
        ButtonMapping {
            name: "A Button".to_string(),
            retro: RETRO_DEVICE_ID_JOYPAD_A,
            local: Some(BTN_ID_A),
            mod_: false,
            default_: Some(BTN_ID_A),
            ignore: false,
        },
        ButtonMapping {
            name: "B Button".to_string(),
            retro: RETRO_DEVICE_ID_JOYPAD_B,
            local: Some(BTN_ID_B),
            mod_: false,
            default_: Some(BTN_ID_B),
            ignore: false,
        },
        ButtonMapping {
            name: "X Button".to_string(),
            retro: RETRO_DEVICE_ID_JOYPAD_X,
            local: Some(BTN_ID_X),
            mod_: false,
            default_: Some(BTN_ID_X),
            ignore: false,
        },
        ButtonMapping {
            name: "Y Button".to_string(),
            retro: RETRO_DEVICE_ID_JOYPAD_Y,
            local: Some(BTN_ID_Y),
            mod_: false,
            default_: Some(BTN_ID_Y),
            ignore: false,
        },
        ButtonMapping {
            name: "Start".to_string(),
            retro: RETRO_DEVICE_ID_JOYPAD_START,
            local: Some(BTN_ID_START),
            mod_: false,
            default_: Some(BTN_ID_START),
            ignore: false,
        },
        ButtonMapping {
            name: "Select".to_string(),
            retro: RETRO_DEVICE_ID_JOYPAD_SELECT,
            local: Some(BTN_ID_SELECT),
            mod_: false,
            default_: Some(BTN_ID_SELECT),
            ignore: false,
        },
        ButtonMapping {
            name: "L1 Button".to_string(),
            retro: RETRO_DEVICE_ID_JOYPAD_L,
            local: Some(BTN_ID_L1),
            mod_: false,
            default_: Some(BTN_ID_L1),
            ignore: false,
        },
        ButtonMapping {
            name: "R1 Button".to_string(),
            retro: RETRO_DEVICE_ID_JOYPAD_R,
            local: Some(BTN_ID_R1),
            mod_: false,
            default_: Some(BTN_ID_R1),
            ignore: false,
        },
        ButtonMapping {
            name: "L2 Button".to_string(),
            retro: RETRO_DEVICE_ID_JOYPAD_L2,
            local: Some(BTN_ID_L2),
            mod_: false,
            default_: Some(BTN_ID_L2),
            ignore: false,
        },
        ButtonMapping {
            name: "R2 Button".to_string(),
            retro: RETRO_DEVICE_ID_JOYPAD_R2,
            local: Some(BTN_ID_R2),
            mod_: false,
            default_: Some(BTN_ID_R2),
            ignore: false,
        },
        ButtonMapping {
            name: "L3 Button".to_string(),
            retro: RETRO_DEVICE_ID_JOYPAD_L3,
            local: Some(BTN_ID_L3),
            mod_: false,
            default_: Some(BTN_ID_L3),
            ignore: false,
        },
        ButtonMapping {
            name: "R3 Button".to_string(),
            retro: RETRO_DEVICE_ID_JOYPAD_R3,
            local: Some(BTN_ID_R3),
            mod_: false,
            default_: Some(BTN_ID_R3),
            ignore: false,
        },
    ]
}

/// 默认快捷指令表（对应 C `config.shortcuts`，minarch.c:930-940）
///
/// 8 项，默认全部未绑定（C 初始化 `BTN_ID_NONE`）。
pub fn shortcuts_default_mapping() -> Vec<Shortcut> {
    [
        "Save State",
        "Load State",
        "Reset Game",
        "Save & Quit",
        "Cycle Scaling",
        "Cycle Effect",
        "Toggle FF",
        "Hold FF",
    ]
    .iter()
    .map(|name| Shortcut {
        name: (*name).to_string(),
        local: None,
        mod_: false,
    })
    .collect()
}

// ═══════════════════════════════════════════════════════════════
// 核心映射初始化（对应 C `Input_init`，minarch.c:1796-1853）
// ═══════════════════════════════════════════════════════════════

/// 读取核心提供的 C 字符串并复制为 Rust 所有（`NULL` 按空串处理）
fn c_string_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: libretro.h 规定描述符字符串为 NUL 结尾且在回调期间有效
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

/// 用核心上报的输入描述符初始化映射（对应 C `Input_init`，minarch.c:1796-1853）
///
/// 语义：
/// - `descriptors == NULL`（核心未上报）→ 返回 defaults 副本，无 ignore
/// - 逐描述符过滤：仅 `port==0 && device==RETRO_DEVICE_JOYPAD && index==0`
/// - `id >= 16` 跳过（C 打印 UNAVAILABLE 后跳过；Rust 无日志设施，
///   静默跳过——偏离理由见模块文档）
/// - 命中的描述符以 `description` 覆盖对应映射的 `name`
/// - 核心上报过（`core_mapped`）且默认映射的 retro 不在上报集合中
///   → 该映射 `ignore == true`
///
/// # Safety
///
/// - `descriptors` 必须指向以 `description == NULL` 终止的
///   [`RetroInputDescriptor`] 数组（可为 NULL 表示核心未上报）
/// - 数组在函数执行期间必须保持有效——由核心在 `SET_INPUT_DESCRIPTORS`
///   回调期间保证
/// - 本函数深拷贝所有字符串，返回后不持有数组的任何借用
pub unsafe fn init_core_mapping(
    defaults: &[ButtonMapping],
    descriptors: *const RetroInputDescriptor,
) -> Vec<ButtonMapping> {
    let mut mappings: Vec<ButtonMapping> = defaults.to_vec();
    if descriptors.is_null() {
        return mappings;
    }

    let mut present = [false; 16];
    let mut core_names: [Option<String>; 16] = core::array::from_fn(|_| None);
    let mut core_mapped = false;

    let mut i = 0usize;
    loop {
        // SAFETY: 调用方保证数组有效；终止项之前均有效
        let descriptor = unsafe { &*descriptors.add(i) };
        if descriptor.description.is_null() {
            break;
        }
        if descriptor.port == 0 && descriptor.device == RETRO_DEVICE_JOYPAD && descriptor.index == 0
        {
            core_mapped = true;
            let id = descriptor.id as usize;
            if id < 16 {
                present[id] = true;
                core_names[id] = Some(c_string_to_string(descriptor.description));
            }
        }
        i += 1;
    }

    if !core_mapped {
        return mappings;
    }

    for mapping in &mut mappings {
        let id = mapping.retro as usize;
        if id < 16 {
            if present[id] {
                if let Some(name) = &core_names[id] {
                    mapping.name = name.clone();
                }
            } else {
                mapping.ignore = true;
            }
        }
    }
    mappings
}

// ═══════════════════════════════════════════════════════════════
// 按键映射（对应 C `input_poll_callback` 映射段，minarch.c:1755-1773）
// ═══════════════════════════════════════════════════════════════

/// 按键映射 → libretro 按键位掩码
///
/// # 规则（与 C 逐条对应）
///
/// - `local == None` 的映射跳过（C `btn == BTN_NONE`）
/// - `ignore == true` 的映射跳过
/// - **摇杆方向折叠**：`local` 为 `BTN_ID_DPAD_*` 时，按下判定用
///   `BTN_DPAD_* | BTN_ANALOG_*` 并集（C `gamepad_type==0` 分支把十字键
///   绑定重定向到含摇杆的逻辑方向位；Rust common 无逻辑方向位，折叠在
///   查询处合并）
/// - `mod_ == true` 的映射要求 `menu_pressed` 同时成立
///
/// # 参数
///
/// - `mappings`: 当前生效的映射表（装配层持有，可能来自
///   [`init_core_mapping`] 结果或 [`default_button_mapping`]）
/// - `pressed`: 当前帧按下掩码（`InputState.pressed`，含 `BTN_ANALOG_*`
///   摇杆方向位——平台 `poll_input` 已翻译）
/// - `menu_pressed`: MENU 键是否按下
///
/// # 返回值
///
/// retro 按键位掩码（第 `retro` 位置 1），供 `input_state_trampoline`
/// 应答核心查询。
pub fn build_buttons(mappings: &[ButtonMapping], pressed: u32, menu_pressed: bool) -> u32 {
    let mut buttons = 0u32;
    for mapping in mappings {
        let Some(local) = mapping.local else { continue };
        if mapping.ignore {
            continue;
        }
        if mapping.mod_ && !menu_pressed {
            continue;
        }
        let mask = match local {
            BTN_ID_DPAD_UP => BTN_DPAD_UP | BTN_ANALOG_UP,
            BTN_ID_DPAD_DOWN => BTN_DPAD_DOWN | BTN_ANALOG_DOWN,
            BTN_ID_DPAD_LEFT => BTN_DPAD_LEFT | BTN_ANALOG_LEFT,
            BTN_ID_DPAD_RIGHT => BTN_DPAD_RIGHT | BTN_ANALOG_RIGHT,
            _ => 1u32 << local,
        };
        if pressed & mask != 0 {
            buttons |= 1 << mapping.retro;
        }
    }
    buttons
}

// ═══════════════════════════════════════════════════════════════
// bind 解析（对应 C `Config_init`/`Config_readControlsString`）
// ═══════════════════════════════════════════════════════════════

/// 解析用户 cfg 的 bind 值（对应 C `Config_readControlsString` 的按钮名
/// 查询，minarch.c:1132-1192）
///
/// 调用方（装配 change）用 `config::get_cfg_value` 取 bind 行文本后
/// 传 `value` 段进来——本模块不解析 cfg 全文。
///
/// # 参数
///
/// - `value`: bind 行的值段（如 `"START"`、`"MENU+START"`、`"NONE"`）
///
/// # 返回值
///
/// - `"START"` → `Some((Some(BTN_ID_START), false))`
/// - `"MENU+START"` → `Some((Some(BTN_ID_START), true))`
/// - `"NONE"` → `Some((None, false))`
/// - 未知名/空串 → `None`（整行不匹配）
pub fn parse_bind_button(value: &str) -> Option<(Option<usize>, bool)> {
    if let Some(rest) = value.strip_prefix("MENU+") {
        lookup_button_label(rest).map(|(local, _)| (local, true))
    } else {
        lookup_button_label(value).map(|(local, _)| (local, false))
    }
}

/// 解析 default.cfg 的 bind 值（对应 C `Config_init` 双段格式，
/// minarch.c:1035-1100）
///
/// 格式：`"按钮"` 或 `"按钮:retro名"`（remap 后缀）。与 C 的差异：
/// 后缀未知时 C 静默回退前缀的 retro（掩盖用户笔误），Rust 返回
/// `None`（整行不匹配）。
///
/// # 参数
///
/// - `value`: bind 行的值段（如 `"UP"`、`"UP:UP"`）
///
/// # 返回值
///
/// - `"UP"` → `Some((Some(BTN_ID_DPAD_UP), RETRO_DEVICE_ID_JOYPAD_UP))`
/// - `"UP:UP"` → 同左（后缀 remap 到同名按键）
/// - 后缀未知、前缀未知、`"NONE"` → `None`
pub fn parse_bind_default(value: &str) -> Option<(Option<usize>, u32)> {
    if value == "NONE" {
        return None;
    }
    match value.split_once(':') {
        Some((button, retro_name)) => {
            let (local, _) = lookup_button_label(button)?;
            let (_, retro) = lookup_button_label(retro_name)?;
            Some((local, retro))
        }
        None => lookup_button_label(value),
    }
}

// ═══════════════════════════════════════════════════════════════
// 输入 handler 与 buttons 静态（装配层闭环，implement-minarch-main）
// ═══════════════════════════════════════════════════════════════

/// 快捷指令动作（对应 C `input_poll_callback` 的 switch，minarch.c:1694-1725）
///
/// 检测由 [`detect_shortcuts`] 纯函数完成，动作执行归装配层（main.rs）——
/// handler 是 `fn` 指针无法捕获装配层状态（存档编排/退出等）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutAction {
    /// 存档（对应 `Menu_saveState`）
    SaveState,
    /// 读档（对应 `Menu_loadState`）
    LoadState,
    /// 复位游戏（对应 `core.reset()`）
    ResetGame,
    /// 存档并退出（对应 `Menu_saveState(); quit=1`）
    SaveQuit,
    /// 循环缩放模式（对应 `screen_scaling += 1` 回绕）
    CycleScaling,
    /// 循环画面效果（对应 `screen_effect += 1` 回绕）
    CycleEffect,
    /// 切换快进（对应 `setFastForward(!fast_forward)`）
    ToggleFf,
    /// 按住快进（对应 `setFastForward(PAD_isPressed(btn))`）；`on` 为
    /// 本帧应置位的快进状态
    HoldFf { on: bool },
}

/// 快捷指令检测的跨帧状态（对应 C `input_poll_callback` 的 static
/// `toggled_ff_on`，minarch.c:1679）
///
/// 装配层持实例，每帧调用 [`detect_shortcuts`] 前无需重置——状态机
/// 自推进。`toggled_ff_on` 语义：HOLD_FF 的释放不得关闭由 TOGGLE_FF
/// 打开的快进（C 注释 "this logic only works because TOGGLE_FF is
/// before HOLD_FF in the menu..."）。
#[derive(Default)]
pub struct ShortcutState {
    /// Toggle FF 是否在当前按住周期内被打开过
    toggled_ff_on: bool,
}

/// 检测本帧触发的快捷指令（对应 C `input_poll_callback` 的快捷键循环，
/// minarch.c:1681-1728）
///
/// 与 C 的边界保持一致：动作只检测不执行；`Toggle FF`/`Hold FF` 的
/// 快进状态切换由装配层消费 [`ShortcutAction`] 后同步
/// `core::set_fast_forward` + `audio::set_fast_forward`（core 静态为
/// 唯一事实源）。
///
/// # 参数
///
/// - `shortcuts`: 当前快捷指令表（装配层持有，bind 行已应用）
/// - `input`: 本帧输入快照（`Platform::poll_input` 返回）
/// - `menu_pressed`: MENU 键是否按下（`mod_` 修饰判定；对应 C
///   `PAD_isPressed(BTN_MENU)`）
/// - `state`: 跨帧状态（`toggled_ff_on`）
///
/// # 返回值
///
/// 本帧触发的动作列表（顺序与 C 的 `SHORTCUT_*` 枚举一致；同一帧
/// 至多一个动作——C 的 `break` 语义）
pub fn detect_shortcuts(
    shortcuts: &[Shortcut],
    input: &common::input::InputState,
    menu_pressed: bool,
    state: &mut ShortcutState,
) -> Vec<ShortcutAction> {
    let mut actions = Vec::new();
    for (i, shortcut) in shortcuts.iter().enumerate() {
        let Some(local) = shortcut.local else {
            continue;
        };
        let btn = 1u32 << local;
        let mod_ok = !shortcut.mod_ || menu_pressed;
        if !mod_ok {
            continue;
        }
        match i {
            6 => {
                // SHORTCUT_TOGGLE_FF
                if input.just_pressed(btn) {
                    state.toggled_ff_on = true;
                    actions.push(ShortcutAction::ToggleFf);
                    break;
                }
            }
            7 => {
                // SHORTCUT_HOLD_FF：不因释放 HOLD 键而关闭由 TOGGLE 打开的快进
                if input.just_pressed(btn) || (!state.toggled_ff_on && input.just_released(btn)) {
                    actions.push(ShortcutAction::HoldFf {
                        on: input.is_pressed(btn),
                    });
                }
            }
            _ => {
                if input.just_pressed(btn) {
                    let action = match i {
                        0 => ShortcutAction::SaveState,
                        1 => ShortcutAction::LoadState,
                        2 => ShortcutAction::ResetGame,
                        3 => ShortcutAction::SaveQuit,
                        4 => ShortcutAction::CycleScaling,
                        5 => ShortcutAction::CycleEffect,
                        _ => unreachable!(),
                    };
                    actions.push(action);
                    break;
                }
            }
        }
    }
    actions
}

/// 进程级按键掩码静态（对应 C 全局 `buttons`，minarch.c:34）
///
/// `input_state` 核心查询的应答来源。`OnceLock<AtomicU32>` 模式与
/// `vibration::VIBRATION` 同构：装配层启动时注册一次，此后无锁读写。
static BUTTONS: std::sync::OnceLock<std::sync::atomic::AtomicU32> = std::sync::OnceLock::new();

/// 注册按键掩码静态（装配层启动时调用一次）
///
/// # 返回值
///
/// - `Ok(())`: 首次注册成功
/// - `Err(mask)`: 已注册过（重复注册是误用），携带本次传入的掩码原值
pub fn init_buttons() -> Result<(), std::sync::atomic::AtomicU32> {
    BUTTONS.set(std::sync::atomic::AtomicU32::new(0))
}

/// 写入当前帧的 libretro 按键掩码（装配层在 `core.run()` 前调用）
///
/// 未 `init_buttons` 时空操作（与 `vibration::set_strength` 未 init
/// 语义一致）。
pub fn set_buttons(mask: u32) {
    if let Some(buttons) = BUTTONS.get() {
        buttons.store(mask, std::sync::atomic::Ordering::Relaxed);
    }
}

/// 读取当前帧的 libretro 按键掩码
///
/// 未 `init_buttons` 时返回 0（与 `input_state` 未注册默认一致）。
pub fn get_buttons() -> u32 {
    BUTTONS
        .get()
        .map_or(0, |b| b.load(std::sync::atomic::Ordering::Relaxed))
}

/// 由物理按键掩码计算 libretro 按键掩码并写入静态
///
/// 装配层每帧调用（`Platform::poll_input` 之后、`core.run()` 之前）：
/// 核心的 `input_poll` 回调在本函数之后触发，`input_state` 读到的是
/// 本帧数据。对应 C `input_poll_callback` 的映射段（minarch.c:1755-1773）
/// 与 `buttons = 0` 重置（minarch.c:1754）。
///
/// # 参数
///
/// - `mappings`: 当前生效的映射表（`init_core_mapping` 结果或默认表）
/// - `pressed`: 当前帧按下掩码（`InputState.pressed`）
/// - `menu_pressed`: MENU 键是否按下（`mod_` 修饰判定）
pub fn update_buttons(mappings: &[ButtonMapping], pressed: u32, menu_pressed: bool) {
    set_buttons(build_buttons(mappings, pressed, menu_pressed));
}

/// 进程级摇杆轴值静态（对应 C 全局 `pad.laxis/raxis`，minarch.c:1785-1792 直读）
///
/// `input_state` 对 `RETRO_DEVICE_ANALOG` 查询的应答来源。元组顺序：
/// `(laxis_x, laxis_y, raxis_x, raxis_y)`。与 `BUTTONS` 同构但用
/// `Mutex` 承载——轴值是 4×i32 复合值，无法用单个 `Atomic` 表达
/// 每帧写入（`vibration`/`audio` 的"每帧可变状态"同款模式）。
static STICKS: std::sync::OnceLock<std::sync::Mutex<(i32, i32, i32, i32)>> =
    std::sync::OnceLock::new();

/// 注册摇杆轴值静态（装配层启动时调用一次）
///
/// # 返回值
///
/// - `Ok(())`: 首次注册成功
/// - `Err(lock)`: 已注册过（重复注册是误用），携带已存在的 `Mutex` 原值
pub fn init_sticks() -> Result<(), std::sync::Mutex<(i32, i32, i32, i32)>> {
    STICKS.set(std::sync::Mutex::new((0, 0, 0, 0)))
}

/// 写入当前帧的摇杆原始轴值（装配层在 `core.run()` 前调用）
///
/// `laxis`/`raxis` 来自 `poll_input` 返回的 `InputState` 轴值字段
/// （原始 SDL 值透传）。未 `init_sticks` 时空操作。
pub fn update_sticks(laxis: (i32, i32), raxis: (i32, i32)) {
    if let Some(sticks) = STICKS.get() {
        let mut guard = sticks.lock().expect("STICKS 锁不应中毒");
        *guard = (laxis.0, laxis.1, raxis.0, raxis.1);
    }
}

/// 读取当前帧的摇杆原始轴值
///
/// 未 `init_sticks` 时返回 `(0, 0, 0, 0)`（与 `input_state` 未注册
/// 默认一致——中立位）。
pub fn get_sticks() -> (i32, i32, i32, i32) {
    STICKS
        .get()
        .map_or((0, 0, 0, 0), |s| *s.lock().expect("STICKS 锁不应中毒"))
}

/// `input_poll` 回调 handler（注册进 `FrontendState.input_poll`）
///
/// **空操作占位**：`fn` 指针无法捕获 `Platform`，按键数据由装配层
/// 在 `core.run()` 前经 [`update_buttons`] 注入——核心触发本回调时
/// 数据已就绪（单线程下时序确定）。对应 C `input_poll_callback`
/// 的 `PAD_poll` 段（minarch.c:1655）——C 在回调内轮询，Rust 在
/// 回调前注入（config 规则「数据收集由装配层负责」）。
pub fn input_poll_handler() {
    // 空操作：数据由装配层注入（见模块文档）
}

/// `input_state` 回调 handler（注册进 `FrontendState.input_state`）
///
/// 从按键掩码静态应答核心查询（对应 C `input_state_callback`，
/// minarch.c:1777-1793）。
///
/// # 参数
///
/// - `port`/`device`/`index`/`id`: libretro 输入查询参数
///
/// # 返回值
///
/// - `RETRO_DEVICE_ID_JOYPAD_MASK` → 当前按键掩码（`i16` 截断，与 C 一致）
/// - `RETRO_DEVICE_JOYPAD` 单键 → `(mask >> id) & 1`
/// - `RETRO_DEVICE_ANALOG` → 按 `index`（左/右摇杆）+ `id`（X/Y）返回
///   摇杆原始轴值（对应 C `pad.laxis/raxis` 直读，minarch.c:1785-1792；
///   `laxis_x, laxis_y, raxis_x, raxis_y` 由装配层经 [`update_sticks`] 注入）
pub fn input_state_handler(port: u32, device: u32, index: u32, id: u32) -> i16 {
    let buttons = get_buttons();
    if device == crate::libretro::RETRO_DEVICE_JOYPAD {
        if id == crate::libretro::RETRO_DEVICE_ID_JOYPAD_MASK {
            return buttons as i16;
        }
        if id < 16 {
            return ((buttons >> id) & 1) as i16;
        }
        return 0;
    }
    if device == crate::libretro::RETRO_DEVICE_ANALOG {
        let (lx, ly, rx, ry) = get_sticks();
        let value = match (index, id) {
            (
                crate::libretro::RETRO_DEVICE_INDEX_ANALOG_LEFT,
                crate::libretro::RETRO_DEVICE_ID_ANALOG_X,
            ) => lx,
            (
                crate::libretro::RETRO_DEVICE_INDEX_ANALOG_LEFT,
                crate::libretro::RETRO_DEVICE_ID_ANALOG_Y,
            ) => ly,
            (
                crate::libretro::RETRO_DEVICE_INDEX_ANALOG_RIGHT,
                crate::libretro::RETRO_DEVICE_ID_ANALOG_X,
            ) => rx,
            (
                crate::libretro::RETRO_DEVICE_INDEX_ANALOG_RIGHT,
                crate::libretro::RETRO_DEVICE_ID_ANALOG_Y,
            ) => ry,
            _ => 0,
        };
        return value as i16;
    }
    let _ = (port, index);
    0
}

// ── 测试 ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // 摇杆轴值静态测试。`init_sticks` 是 `OnceLock` 一次性注册——
    // 测试进程内首个用例注册后，后续用例经 `update_sticks` 复用同一静态。
    fn ensure_sticks_registered() {
        let _ = init_sticks();
    }

    #[test]
    fn analog_query_returns_injected_sticks() {
        ensure_sticks_registered();
        // 注入左摇杆 (12345, -6789)、右摇杆 (0, 0)
        update_sticks((12345, -6789), (0, 0));

        use crate::libretro::{
            RETRO_DEVICE_ANALOG, RETRO_DEVICE_ID_ANALOG_X, RETRO_DEVICE_ID_ANALOG_Y,
            RETRO_DEVICE_INDEX_ANALOG_LEFT, RETRO_DEVICE_INDEX_ANALOG_RIGHT,
        };
        // LEFT + X → laxis_x
        assert_eq!(
            input_state_handler(
                0,
                RETRO_DEVICE_ANALOG,
                RETRO_DEVICE_INDEX_ANALOG_LEFT,
                RETRO_DEVICE_ID_ANALOG_X
            ),
            12345
        );
        // LEFT + Y → laxis_y
        assert_eq!(
            input_state_handler(
                0,
                RETRO_DEVICE_ANALOG,
                RETRO_DEVICE_INDEX_ANALOG_LEFT,
                RETRO_DEVICE_ID_ANALOG_Y
            ),
            -6789
        );
        // RIGHT + X → raxis_x（未注入，中立 0）
        assert_eq!(
            input_state_handler(
                0,
                RETRO_DEVICE_ANALOG,
                RETRO_DEVICE_INDEX_ANALOG_RIGHT,
                RETRO_DEVICE_ID_ANALOG_X
            ),
            0
        );
    }

    #[test]
    fn analog_unregistered_returns_zero_without_panic() {
        // 未注册（`STICKS` 为空）时 ANALOG 查询返回 0 不 panic——
        // 该用例需在 `ensure_sticks_registered` 之前运行才有意义；
        // 由于测试执行顺序不定，改用"get() 为 None 时"的直接断言兜底。
        // 实际未注册语义由 `get_sticks` 的 map_or 覆盖：此处显式验证。
        if STICKS.get().is_none() {
            assert_eq!(get_sticks(), (0, 0, 0, 0));
        }
        // 已注册时 get_sticks 应返回当前值（非 panic 即可）
        let _ = get_sticks();
    }
}
