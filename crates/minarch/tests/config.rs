//! config.rs 纯逻辑层测试
//!
//! 覆盖 spec「OptionList 核心选项注册表」「前端选项表」「cfg 字符串
//! 解析与序列化」三个 Requirement 的全部 Scenario。FFI 适配器测试用
//! libretro.rs 的 repr(C) 类型构造真实指针结构（CString 保活）。

use std::ffi::CString;

use minarch::config::*;
use minarch::libretro::{
    RETRO_NUM_CORE_OPTION_VALUES_MAX, RetroCoreOptionDefinition, RetroCoreOptionValue,
    RetroVariable,
};

// ── 夹具辅助 ───────────────────────────────────────────────────

/// 构造一个空（NULL）的 values 数组——defs/vars 数组的终止项与
/// `info == NULL` 的项都基于它
fn null_values() -> [RetroCoreOptionValue; RETRO_NUM_CORE_OPTION_VALUES_MAX] {
    [RetroCoreOptionValue {
        value: core::ptr::null(),
        label: core::ptr::null(),
    }; RETRO_NUM_CORE_OPTION_VALUES_MAX]
}

/// 构造 key 为 NULL 的终止项
fn null_def() -> RetroCoreOptionDefinition {
    RetroCoreOptionDefinition {
        key: core::ptr::null(),
        desc: core::ptr::null(),
        info: core::ptr::null(),
        values: null_values(),
        default_value: core::ptr::null(),
    }
}

/// 构造一个 def 项；`info` 为 `None` 时对应 C 的 NULL info
fn make_def(
    key: &CString,
    desc: &CString,
    info: Option<&CString>,
    values: &[(&CString, Option<&CString>)],
    default_value: &CString,
) -> RetroCoreOptionDefinition {
    let mut v = null_values();
    for (i, (value, label)) in values.iter().enumerate() {
        v[i] = RetroCoreOptionValue {
            value: value.as_ptr(),
            label: label.map_or(core::ptr::null(), |l| l.as_ptr()),
        };
    }
    RetroCoreOptionDefinition {
        key: key.as_ptr(),
        desc: desc.as_ptr(),
        info: info.map_or(core::ptr::null(), |c| c.as_ptr()),
        values: v,
        default_value: default_value.as_ptr(),
    }
}

// ── 任务 1.x：数据模型与 defs 解析 ──────────────────────────────

#[test]
fn parses_core_option_definitions() {
    let key1 = CString::new("gambatte_colorization").unwrap();
    let desc1 = CString::new("Colorization").unwrap();
    let v_internal = CString::new("internal").unwrap();
    let v_gbc = CString::new("GBC").unwrap();
    let v_sgb = CString::new("SGB").unwrap();

    let key2 = CString::new("gambatte_gb_bootloader").unwrap();
    let desc2 = CString::new("GB Bootloader").unwrap();
    let info2 = CString::new("Show the boot logo on startup.").unwrap();
    let v_disabled = CString::new("disabled").unwrap();
    let v_enabled = CString::new("enabled").unwrap();

    let defs = [
        make_def(
            &key1,
            &desc1,
            None, // info == NULL → desc/full 为 None
            &[
                (&v_internal, Some(&v_internal)),
                (&v_gbc, None),
                (&v_sgb, Some(&v_sgb)),
            ],
            &v_gbc,
        ),
        make_def(
            &key2,
            &desc2,
            Some(&info2),
            &[
                (&v_disabled, Some(&v_disabled)),
                (&v_enabled, Some(&v_enabled)),
            ],
            &v_enabled,
        ),
        null_def(),
    ];

    let list = unsafe { OptionList::from_core_options(defs.as_ptr()) };
    assert_eq!(list.options.len(), 2);
    assert!(!list.changed);

    let first = &list.options[0];
    assert_eq!(first.key, "gambatte_colorization");
    assert_eq!(first.name, "Colorization");
    assert!(first.desc.is_none());
    assert!(first.full.is_none());
    assert_eq!(first.count, 3);
    assert_eq!(first.values, ["internal", "GBC", "SGB"]);
    // label 为 NULL 时回退为 value
    assert_eq!(first.labels, ["internal", "GBC", "SGB"]);
    assert_eq!(first.default_value, 1); // "GBC"
    assert_eq!(first.value, 1);
    assert!(!first.lock);

    let second = &list.options[1];
    assert_eq!(second.key, "gambatte_gb_bootloader");
    assert_eq!(
        second.desc.as_deref(),
        Some("Show the boot logo on startup.")
    );
    assert_eq!(
        second.full.as_deref(),
        Some("Show the boot logo on startup.")
    );
    assert_eq!(second.count, 2);
    assert_eq!(second.default_value, 1); // "enabled"
}

#[test]
fn truncates_values_at_128() {
    let key = CString::new("overflow_option").unwrap();
    let desc = CString::new("Overflow").unwrap();
    // 128 个全非 NULL 的值（核心违反 ABI 约定：数组无 NULL 终止）
    let values: Vec<CString> = (0..RETRO_NUM_CORE_OPTION_VALUES_MAX)
        .map(|i| CString::new(format!("value_{i}")).unwrap())
        .collect();
    let mut def = make_def(&key, &desc, None, &[], &values[0]);
    for (i, v) in values.iter().enumerate() {
        def.values[i] = RetroCoreOptionValue {
            value: v.as_ptr(),
            label: v.as_ptr(),
        };
    }

    let defs = [def, null_def()];
    let list = unsafe { OptionList::from_core_options(defs.as_ptr()) };
    let option = &list.options[0];
    // 安全截断为 128 项，不 panic、不越界
    assert_eq!(option.count, RETRO_NUM_CORE_OPTION_VALUES_MAX);
    assert_eq!(option.values.len(), RETRO_NUM_CORE_OPTION_VALUES_MAX);
}

// ── 任务 2.x：vars 解析 ─────────────────────────────────────────

#[test]
fn parses_variables_three_forms() {
    let key1 = CString::new("snes9x_region").unwrap();
    let value1 = CString::new("Region; auto|ntsc|pal").unwrap();
    let key2 = CString::new("gpsp_frameskip").unwrap();
    let value2 = CString::new("0|1|2").unwrap();
    let key3 = CString::new("fceumm_palette").unwrap();
    let value3 = CString::new("Palette;default|fire").unwrap(); // ';' 后非空格

    let vars = [
        RetroVariable {
            key: key1.as_ptr(),
            value: value1.as_ptr(),
        },
        RetroVariable {
            key: key2.as_ptr(),
            value: value2.as_ptr(),
        },
        RetroVariable {
            key: key3.as_ptr(),
            value: value3.as_ptr(),
        },
        RetroVariable {
            key: core::ptr::null(),
            value: core::ptr::null(),
        },
    ];

    let list = unsafe { OptionList::from_variables(vars.as_ptr()) };
    assert_eq!(list.options.len(), 3);

    // 标准格式
    assert_eq!(list.options[0].name, "Region");
    assert_eq!(list.options[0].values, ["auto", "ntsc", "pal"]);

    // 无分号：C 版 strchr(NULL) 崩溃，Rust 安全回退
    assert_eq!(list.options[1].name, "gpsp_frameskip");
    assert_eq!(list.options[1].values, ["0", "1", "2"]);

    // 分号后非空格：name 回退 key，首值从 ';' 开始（与 C 一致忠实呈现）
    assert_eq!(list.options[2].name, "fceumm_palette");
    assert_eq!(list.options[2].values, [";default", "fire"]);
}

#[test]
fn parses_variables_edge_cases() {
    let key1 = CString::new("empty_option").unwrap();
    let empty = CString::new("").unwrap();
    let key2 = CString::new("trailing_pipe").unwrap();
    let trailing = CString::new("a|b|").unwrap();

    let vars = [
        RetroVariable {
            key: key1.as_ptr(),
            value: empty.as_ptr(),
        },
        RetroVariable {
            key: key2.as_ptr(),
            value: trailing.as_ptr(),
        },
        RetroVariable {
            key: core::ptr::null(),
            value: core::ptr::null(),
        },
    ];

    let list = unsafe { OptionList::from_variables(vars.as_ptr()) };
    // 空串 → 1 个空值（与 C 一致）
    assert_eq!(list.options[0].values, [""]);
    // '|' 尾 → 尾随空值（split 语义，与 C strchr 循环一致）
    assert_eq!(list.options[1].values, ["a", "b", ""]);
}

// ── 任务 3.x：查询与修改 API ────────────────────────────────────

fn sample_list() -> OptionList {
    let key1 = CString::new("opt_a").unwrap();
    let desc1 = CString::new("A").unwrap();
    let v1 = CString::new("one").unwrap();
    let v2 = CString::new("two").unwrap();
    let defs = [
        make_def(
            &key1,
            &desc1,
            None,
            &[(&v1, Some(&v1)), (&v2, Some(&v2))],
            &v1,
        ),
        null_def(),
    ];
    unsafe { OptionList::from_core_options(defs.as_ptr()) }
}

#[test]
fn get_and_set_roundtrip() {
    let mut list = sample_list();
    assert_eq!(list.get_option_value("opt_a"), Some("one"));

    list.set_option_value("opt_a", "two");
    assert_eq!(list.get_option_value("opt_a"), Some("two"));
    assert_eq!(list.get_option("opt_a").unwrap().value, 1);
    assert!(list.changed, "set 命中应置位 changed");
}

#[test]
fn unknown_key_returns_none() {
    let list = sample_list();
    assert!(list.get_option("nope").is_none());
    assert!(list.get_option_value("nope").is_none());
}

#[test]
fn unmatched_and_out_of_range_values_are_ignored() {
    let mut list = sample_list();
    // 值字符串不匹配任何 values：C 会静默设为第一项，Rust 忽略
    list.set_option_value("opt_a", "three");
    assert_eq!(list.get_option_value("opt_a"), Some("one"));
    assert!(!list.changed);

    // 索引越界忽略
    list.set_option_raw_value("opt_a", 99);
    assert_eq!(list.get_option("opt_a").unwrap().value, 0);
    assert!(!list.changed);
}

#[test]
fn option_name_override_table() {
    // pcsx dualshock 特例
    assert_eq!(
        get_option_name_from_key("pcsx_rearmed_analog_combo", "Analog Combo"),
        "DualShock Toggle Combo"
    );
    // 其余回退原名
    assert_eq!(
        get_option_name_from_key("gambatte_colorization", "Colorization"),
        "Colorization"
    );
}

// ── 任务 4.x：前端选项表 ────────────────────────────────────────

#[test]
fn frontend_options_without_overscan() {
    let options = frontend_options(false);
    assert_eq!(options.len(), 8);
    let scaling = options
        .iter()
        .find(|o| o.key == "minarch_screen_scaling")
        .unwrap();
    assert_eq!(scaling.count, 3);
    assert_eq!(scaling.values, ["Native", "Aspect", "Fullscreen"]);
    assert!(!scaling.desc.as_deref().unwrap().contains("Cropped"));
    assert_eq!(scaling.default_value, 1);
}

#[test]
fn frontend_options_with_overscan() {
    let options = frontend_options(true);
    let scaling = options
        .iter()
        .find(|o| o.key == "minarch_screen_scaling")
        .unwrap();
    assert_eq!(scaling.count, 4);
    assert_eq!(
        scaling.values,
        ["Native", "Aspect", "Fullscreen", "Cropped"]
    );
    assert!(scaling.desc.as_deref().unwrap().contains("Cropped"));
}

#[test]
fn frontend_option_keys_and_defaults() {
    let options = frontend_options(false);
    let expected: &[(&str, usize)] = &[
        ("minarch_screen_scaling", 1),
        ("minarch_screen_effect", 0),
        ("minarch_screen_sharpness", 2),
        ("minarch_prevent_tearing", 1),
        ("minarch_cpu_speed", 1),
        ("minarch_thread_video", 0),
        ("minarch_debug_hud", 0),
        ("minarch_max_ff_speed", 3),
    ];
    for (key, default_value) in expected {
        let option = options
            .iter()
            .find(|o| &o.key == key)
            .unwrap_or_else(|| panic!("缺键 {key}"));
        assert_eq!(option.default_value, *default_value, "{key} 默认值");
        assert_eq!(option.value, *default_value, "{key} 当前值");
        assert_eq!(option.count, option.values.len());
    }
}

// ── 任务 5.x：cfg 解析与序列化 ──────────────────────────────────

#[test]
fn cfg_lock_prefix_and_strict_separator() {
    let cfg = "-minarch_cpu_speed = 2\nminarch_thread_video=1\n";
    let cpu = get_cfg_value(cfg, "minarch_cpu_speed").expect("lock 行应匹配");
    assert_eq!(cpu.value, "2");
    assert!(cpu.lock);
    // '=' 两侧无空格不匹配
    assert!(get_cfg_value(cfg, "minarch_thread_video").is_none());
}

#[test]
fn cfg_crlf_and_duplicate_keys() {
    let cfg = "minarch_screen_effect = 1\r\nminarch_screen_effect = 2\n";
    let effect = get_cfg_value(cfg, "minarch_screen_effect").expect("应匹配");
    assert_eq!(effect.value, "1"); // 重复 key 取第一个
    assert!(!effect.lock);
}

#[test]
fn cfg_roundtrip() {
    let entries = [
        CfgEntry {
            key: "minarch_cpu_speed".to_string(),
            value: "2".to_string(),
        },
        CfgEntry {
            key: "minarch_debug_hud".to_string(),
            value: "On".to_string(),
        },
        CfgEntry {
            key: "gambatte_colorization".to_string(),
            value: "GBC".to_string(),
        },
    ];
    let serialized = serialize_cfg(&entries);
    for entry in &entries {
        let parsed = get_cfg_value(&serialized, &entry.key).expect("round-trip 应可查回");
        assert_eq!(parsed.value, entry.value);
        assert!(!parsed.lock);
    }
}

#[test]
fn cfg_no_match_returns_none() {
    assert!(get_cfg_value("minarch_cpu_speed = 2\n", "other_key").is_none());
}

// ── cfg 文件 IO 层（implement-minarch-main）────────────────────────

use std::path::PathBuf;

use common::input::{BTN_ID_A, BTN_ID_L1, BTN_ID_START};

/// 在系统临时目录下创建唯一测试目录
fn temp_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("minarch_config_test_{}_{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 用前端选项表构造 OptionList（field 均为 pub）
fn frontend_list() -> OptionList {
    OptionList {
        options: frontend_options(false),
        changed: false,
    }
}

#[test]
fn cfg_load_text_states() {
    let dir = temp_dir("load");
    let existing = dir.join("system.cfg");
    std::fs::write(&existing, "minarch_cpu_speed = 2\n").unwrap();

    // 存在文件返回内容
    assert_eq!(
        load_cfg_text(&existing).as_deref(),
        Some("minarch_cpu_speed = 2\n")
    );
    // 缺失文件返回 None
    assert!(load_cfg_text(&dir.join("missing.cfg")).is_none());
    // 空文件返回 Some("")
    let empty = dir.join("empty.cfg");
    std::fs::write(&empty, "").unwrap();
    assert_eq!(load_cfg_text(&empty).as_deref(), Some(""));
}

#[test]
fn cfg_save_roundtrip_and_error() {
    let dir = temp_dir("save");
    let entries = [
        CfgEntry {
            key: "minarch_screen_scaling".to_string(),
            value: "Aspect".to_string(),
        },
        CfgEntry {
            key: "gambatte_colorization".to_string(),
            value: "GBC".to_string(),
        },
    ];

    let path = dir.join("minarch.cfg");
    save_cfg(&entries, &path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    // 内容与 serialize_cfg 一致
    assert_eq!(text, serialize_cfg(&entries));
    // round-trip：逐 key 可查回
    for entry in &entries {
        let parsed = get_cfg_value(&text, &entry.key).unwrap();
        assert_eq!(parsed.value, entry.value);
    }

    // 目录缺失返回 Err
    let bad = dir.join("no_such_dir").join("minarch.cfg");
    assert!(save_cfg(&entries, &bad).is_err());
}

#[test]
fn cfg_apply_frontend_and_gamepad() {
    let mut frontend = frontend_list();
    let mut core = OptionList {
        options: vec![],
        changed: false,
    };
    let mut mapping = minarch::controls::default_button_mapping();
    let mut shortcuts = minarch::controls::shortcuts_default_mapping();
    let mut gamepad_type = 0u32;

    let cfg = "minarch_screen_scaling = Fullscreen\n\
               minarch_cpu_speed = 0\n\
               minarch_gamepad_type = 2\n\
               bind A Button = MENU+START\n\
               bind Toggle FF = L1\n\
               bind Garbage Name = X\n";

    apply_cfg(
        cfg,
        &mut frontend,
        &mut core,
        &mut mapping,
        &mut shortcuts,
        &mut gamepad_type,
    );

    // frontend 键应用
    assert_eq!(
        frontend.get_option_value("minarch_screen_scaling"),
        Some("Fullscreen")
    );
    // gamepad_type 写出
    assert_eq!(gamepad_type, 2);
    // bind 行应用到 mapping（A Button 是 default 表第 5 项）
    let a_btn = mapping.iter().find(|m| m.name == "A Button").unwrap();
    assert_eq!(a_btn.local, Some(BTN_ID_START));
    assert!(a_btn.mod_);
    // bind 行应用到 shortcut
    let toggle_ff = shortcuts.iter().find(|s| s.name == "Toggle FF").unwrap();
    assert_eq!(toggle_ff.local, Some(BTN_ID_L1));
    assert!(!toggle_ff.mod_);
    // 未知名 bind 行被忽略（不 panic、不中断）
}

#[test]
fn cfg_apply_ignores_invalid_lines() {
    let mut frontend = frontend_list();
    let mut core = OptionList {
        options: vec![],
        changed: false,
    };
    let mut mapping = minarch::controls::default_button_mapping();
    let mut shortcuts = minarch::controls::shortcuts_default_mapping();
    let mut gamepad_type = 0u32;

    let cfg = "minarch_screen_effect = GARBAGE\n\
               bind A Button = GARBAGE\n\
               minarch_thread_video = 1\n";

    apply_cfg(
        cfg,
        &mut frontend,
        &mut core,
        &mut mapping,
        &mut shortcuts,
        &mut gamepad_type,
    );

    // 无效值不匹配任何可选值 → 忽略（set_option_value 语义）
    assert_eq!(
        frontend.get_option_value("minarch_screen_effect"),
        Some("None")
    );
    // 无效 bind 值 → mapping 保持默认
    let a_btn = mapping.iter().find(|m| m.name == "A Button").unwrap();
    assert_eq!(a_btn.local, Some(BTN_ID_A));
    // minarch_thread_video 被解析但值被忽略（不报错，thread_video 边界）
    assert_eq!(
        frontend.get_option_value("minarch_thread_video"),
        Some("Off")
    );
}
