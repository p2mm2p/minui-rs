//! environment.rs 环境回调测试
//!
//! 覆盖 spec「environment 30 case 分发」「environment 运行时状态与
//! hook 注册」两个 Requirement 的全部 Scenario。
//!
//! 分层测试策略：
//! - `dispatch` 用本地 `EnvironmentState`/`EnvironmentRuntime` 实例
//!   构造——各 case 互不干扰，可并行执行
//! - 静态薄层（`STATE`/`RUNTIME`/`RUMBLE_HOOK`）是进程级 OnceLock，
//!   注册后无法撤销——生命周期用例各自合并为**单个测试串行执行**
//!   （同 tests/core_loading.rs 的 `callback_bridge_lifecycle` 决策）；
//!   init 相关用例与 rumble hook 相关用例操作不同静态，互不冲突

use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicU8, Ordering};

use minarch::environment::*;
use minarch::libretro::{
    RETRO_AV_ENABLE_AUDIO, RETRO_AV_ENABLE_VIDEO, RETRO_DEVICE_ANALOG, RETRO_DEVICE_ID_JOYPAD_A,
    RETRO_DEVICE_JOYPAD, RETRO_ENVIRONMENT_GET_AUDIO_VIDEO_ENABLE, RETRO_ENVIRONMENT_GET_CAN_DUPE,
    RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION, RETRO_ENVIRONMENT_GET_CURRENT_SOFTWARE_FRAMEBUFFER,
    RETRO_ENVIRONMENT_GET_DISK_CONTROL_INTERFACE_VERSION, RETRO_ENVIRONMENT_GET_INPUT_BITMASKS,
    RETRO_ENVIRONMENT_GET_INPUT_DEVICE_CAPABILITIES, RETRO_ENVIRONMENT_GET_LOG_INTERFACE,
    RETRO_ENVIRONMENT_GET_OVERSCAN, RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE,
    RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY, RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY,
    RETRO_ENVIRONMENT_GET_VARIABLE, RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE,
    RETRO_ENVIRONMENT_SET_AUDIO_CALLBACK, RETRO_ENVIRONMENT_SET_CONTENT_INFO_OVERRIDE,
    RETRO_ENVIRONMENT_SET_CONTROLLER_INFO, RETRO_ENVIRONMENT_SET_CORE_OPTIONS,
    RETRO_ENVIRONMENT_SET_CORE_OPTIONS_DISPLAY, RETRO_ENVIRONMENT_SET_CORE_OPTIONS_INTL,
    RETRO_ENVIRONMENT_SET_DISK_CONTROL_EXT_INTERFACE, RETRO_ENVIRONMENT_SET_DISK_CONTROL_INTERFACE,
    RETRO_ENVIRONMENT_SET_FRAME_TIME_CALLBACK, RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS,
    RETRO_ENVIRONMENT_SET_MESSAGE, RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL,
    RETRO_ENVIRONMENT_SET_PIXEL_FORMAT, RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME,
    RETRO_ENVIRONMENT_SET_VARIABLE, RETRO_ENVIRONMENT_SET_VARIABLES,
    RETRO_NUM_CORE_OPTION_VALUES_MAX, RetroControllerDescription, RetroControllerInfo,
    RetroCoreOptionDefinition, RetroCoreOptionValue, RetroCoreOptionsIntl,
    RetroDiskControlCallback, RetroDiskControlExtCallback, RetroGameInfo, RetroInputDescriptor,
    RetroLogCallback, RetroMessage, RetroPixelFormat, RetroRumbleEffect, RetroRumbleInterface,
    RetroVariable,
};

// ── 夹具辅助 ───────────────────────────────────────────────────

/// 构造测试目录状态（bios/saves 为两个不同路径）
fn test_state() -> EnvironmentState {
    EnvironmentState {
        bios_dir: CString::new("/test/bios").unwrap(),
        saves_dir: CString::new("/test/saves").unwrap(),
    }
}

/// 构造一个空（NULL）的 values 数组——defs 数组的终止项与
/// `info == NULL` 的项都基于它
fn null_values() -> [RetroCoreOptionValue; RETRO_NUM_CORE_OPTION_VALUES_MAX] {
    [RetroCoreOptionValue {
        value: ptr::null(),
        label: ptr::null(),
    }; RETRO_NUM_CORE_OPTION_VALUES_MAX]
}

/// 构造 key 为 NULL 的终止项
fn null_def() -> RetroCoreOptionDefinition {
    RetroCoreOptionDefinition {
        key: ptr::null(),
        desc: ptr::null(),
        info: ptr::null(),
        values: null_values(),
        default_value: ptr::null(),
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
            label: label.map_or(ptr::null(), |l| l.as_ptr()),
        };
    }
    RetroCoreOptionDefinition {
        key: key.as_ptr(),
        desc: desc.as_ptr(),
        info: info.map_or(ptr::null(), |c| c.as_ptr()),
        values: v,
        default_value: default_value.as_ptr(),
    }
}

/// 构造单选项 defs 数组（B 组用例通用夹具）：键 `key`、值
/// `["a","b"]`、默认 `a`
fn make_test_key_defs(
    key: &CString,
    desc: &CString,
    va: &CString,
    vb: &CString,
) -> [RetroCoreOptionDefinition; 2] {
    [
        make_def(key, desc, None, &[(va, Some(va)), (vb, Some(vb))], va),
        null_def(),
    ]
}

/// 读取 C 字符串并复制为 Rust 所有（测试侧断言用，指针须非空）
fn read_cstr(ptr: *const c_char) -> String {
    assert!(!ptr.is_null(), "读回指针不应为 NULL");
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

// ── 换碟回调哑函数（仅作指针标识，永不被调用）───────────────────

unsafe extern "C" fn ext_set_eject(_ejected: bool) -> bool {
    true
}
unsafe extern "C" fn ext_get_eject() -> bool {
    true
}
unsafe extern "C" fn ext_get_image_index() -> u32 {
    0
}
unsafe extern "C" fn ext_set_image_index(_index: u32) -> bool {
    true
}
unsafe extern "C" fn ext_replace_image_index(_index: u32, _info: *const RetroGameInfo) -> bool {
    true
}
unsafe extern "C" fn ext_set_initial_image(_index: u32, _path: *const c_char) -> bool {
    true
}
unsafe extern "C" fn ext_get_image_path(_index: u32, _s: *mut c_char, _len: usize) -> bool {
    true
}
unsafe extern "C" fn v1_set_eject(_ejected: bool) -> bool {
    false
}
unsafe extern "C" fn v1_get_eject() -> bool {
    false
}
unsafe extern "C" fn v1_get_image_index() -> u32 {
    1
}
unsafe extern "C" fn v1_set_image_index(_index: u32) -> bool {
    false
}
unsafe extern "C" fn v1_replace_image_index(_index: u32, _info: *const RetroGameInfo) -> bool {
    false
}

// ── 任务 1.1：静态薄层生命周期（单测试串行）─────────────────────

#[test]
fn static_thin_layer_lifecycle() {
    // 未 init：handle 返回 false（默认语义，与 trampoline 未注册一致）
    assert!(!handle(RETRO_ENVIRONMENT_GET_OVERSCAN, ptr::null_mut()));

    // 首次 init：Ok
    let bios = CString::new("/test/bios").unwrap();
    let saves = CString::new("/test/saves").unwrap();
    assert!(init(bios, saves).is_ok());

    // 重复 init：Err 携带第二次状态
    let bios2 = CString::new("/second/bios").unwrap();
    let saves2 = CString::new("/second/saves").unwrap();
    let err = init(bios2.clone(), saves2.clone()).expect_err("重复 init 应返回 Err");
    assert_eq!(err.bios_dir, bios2);
    assert_eq!(err.saves_dir, saves2);

    // init 后 handle 正常转发（GET_OVERSCAN 出参写 true）
    let mut out = false;
    assert!(handle(
        RETRO_ENVIRONMENT_GET_OVERSCAN,
        &mut out as *mut bool as *mut c_void
    ));
    assert!(out, "init 后 GET_OVERSCAN 出参应写 true");

    // getter 初值：默认映射 16 项 / disc None / 无自定义控制器
    assert_eq!(controls_mapping().len(), 16);
    assert!(disc_control().is_none());
    assert!(!has_custom_controllers());
}

// ── 任务 1.2：rumble hook 注册与 trampoline（单测试串行）─────────

#[test]
fn rumble_hook_registration_and_trampoline() {
    static CAPTURED: AtomicU8 = AtomicU8::new(0);
    fn capture(strength: u8) {
        CAPTURED.store(strength, Ordering::SeqCst);
    }
    fn second_hook(_strength: u8) {}

    // 未注册：静默恒 true（hook 未被调用）
    assert!(unsafe { rumble_trampoline(0, RetroRumbleEffect::Strong, 65535) });
    assert_eq!(CAPTURED.load(Ordering::SeqCst), 0);

    // 首次注册：Ok
    assert!(set_rumble_hook(capture).is_ok());

    // 重复注册：Err 携带第二次 hook
    let err = set_rumble_hook(second_hook).expect_err("重复注册应返回 Err");
    assert!(std::ptr::eq(err as *const (), second_hook as *const ()));

    // 已注册：捕获 strength as u8（65535 → 255 截断），恒 true
    assert!(unsafe { rumble_trampoline(0, RetroRumbleEffect::Strong, 65535) });
    assert_eq!(CAPTURED.load(Ordering::SeqCst), 255);
    assert!(unsafe { rumble_trampoline(0, RetroRumbleEffect::Weak, 7) });
    assert_eq!(CAPTURED.load(Ordering::SeqCst), 7);
}

// ── 任务 2.1：A 组常量与简单查询 ────────────────────────────────

#[test]
fn dispatch_bool_outputs_write_true() {
    let state = test_state();
    let mut runtime = EnvironmentRuntime::default();
    for cmd in [
        RETRO_ENVIRONMENT_GET_OVERSCAN,
        RETRO_ENVIRONMENT_GET_CAN_DUPE,
        RETRO_ENVIRONMENT_GET_INPUT_BITMASKS,
    ] {
        let mut out = false;
        assert!(
            unsafe {
                dispatch(
                    cmd,
                    &mut out as *mut bool as *mut c_void,
                    &state,
                    &mut runtime,
                )
            },
            "cmd {cmd} 应返回 true"
        );
        assert!(out, "cmd {cmd} 出参应写 true");
    }
}

#[test]
fn dispatch_version_and_capability_outputs() {
    let state = test_state();
    let mut runtime = EnvironmentRuntime::default();

    let mut core_options_version = 0u32;
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION,
            &mut core_options_version as *mut u32 as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert_eq!(core_options_version, 1);

    let mut disc_version = 0u32;
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_GET_DISK_CONTROL_INTERFACE_VERSION,
            &mut disc_version as *mut u32 as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert_eq!(disc_version, 1);

    let mut capabilities = 0u32;
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_GET_INPUT_DEVICE_CAPABILITIES,
            &mut capabilities as *mut u32 as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert_eq!(
        capabilities,
        (1 << RETRO_DEVICE_JOYPAD) | (1 << RETRO_DEVICE_ANALOG)
    );

    let mut av_enable = 0i32;
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_GET_AUDIO_VIDEO_ENABLE,
            &mut av_enable as *mut i32 as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert_eq!(
        av_enable as u32,
        RETRO_AV_ENABLE_VIDEO | RETRO_AV_ENABLE_AUDIO
    );
}

#[test]
fn dispatch_set_pixel_format_two_states() {
    let state = test_state();
    let mut runtime = EnvironmentRuntime::default();

    // RGB565：唯一支持格式 → true
    let mut rgb565 = RetroPixelFormat::Rgb565;
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_PIXEL_FORMAT,
            &mut rgb565 as *mut RetroPixelFormat as *mut c_void,
            &state,
            &mut runtime,
        )
    });

    // XRGB8888：不支持 → false（C :1899）
    let mut xrgb = RetroPixelFormat::Xrgb8888;
    assert!(!unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_PIXEL_FORMAT,
            &mut xrgb as *mut RetroPixelFormat as *mut c_void,
            &state,
            &mut runtime,
        )
    });
}

#[test]
fn dispatch_directory_outputs_match_state() {
    let state = test_state();
    let mut runtime = EnvironmentRuntime::default();

    let mut system_dir: *const c_char = ptr::null();
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY,
            &mut system_dir as *mut *const c_char as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert_eq!(system_dir, state.bios_dir.as_ptr());
    assert_eq!(
        unsafe { CStr::from_ptr(system_dir) },
        state.bios_dir.as_c_str()
    );

    let mut save_dir: *const c_char = ptr::null();
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY,
            &mut save_dir as *mut *const c_char as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert_eq!(save_dir, state.saves_dir.as_ptr());
    assert_eq!(
        unsafe { CStr::from_ptr(save_dir) },
        state.saves_dir.as_c_str()
    );
}

#[test]
fn dispatch_set_performance_level_does_not_fallthrough() {
    let state = test_state();
    let mut runtime = EnvironmentRuntime::default();
    // C 的 8→9 fallthrough（minarch.c:1885-1889）会把 data 当 char**
    // 野写 bios_dir——缺陷不继承：哨兵数据必须原样不动
    let mut sentinel: u64 = 0xDEAD_BEEF;
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL,
            &mut sentinel as *mut u64 as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert_eq!(sentinel, 0xDEAD_BEEF, "哨兵数据不应被写入");
}

#[test]
fn dispatch_set_message_returns_true() {
    let state = test_state();
    let mut runtime = EnvironmentRuntime::default();
    let msg = CString::new("hello core").unwrap();
    let message = RetroMessage {
        msg: msg.as_ptr(),
        frames: 60,
    };
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_MESSAGE,
            &message as *const RetroMessage as *mut c_void,
            &state,
            &mut runtime,
        )
    });
}

#[test]
fn dispatch_noop_group_returns_true() {
    let state = test_state();
    let mut runtime = EnvironmentRuntime::default();
    for cmd in [
        RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME,
        RETRO_ENVIRONMENT_SET_FRAME_TIME_CALLBACK,
        RETRO_ENVIRONMENT_SET_AUDIO_CALLBACK,
        RETRO_ENVIRONMENT_GET_CURRENT_SOFTWARE_FRAMEBUFFER,
        RETRO_ENVIRONMENT_SET_CORE_OPTIONS_DISPLAY,
        RETRO_ENVIRONMENT_SET_CONTENT_INFO_OVERRIDE,
    ] {
        assert!(
            unsafe { dispatch(cmd, ptr::null_mut(), &state, &mut runtime) },
            "no-op cmd {cmd} 应返回 true"
        );
    }
}

#[test]
fn dispatch_get_log_interface_returns_false() {
    let state = test_state();
    let mut runtime = EnvironmentRuntime::default();
    // Rust 无法稳定定义 C 可变参函数——不提供日志接口（design 决策 8）。
    // dispatch 不触碰 data：用 MaybeUninit 提供合法内存位置（含 fn 指针
    // 字段的类型不允许 mem::zeroed 零初始化）
    let mut log_cb = core::mem::MaybeUninit::<RetroLogCallback>::uninit();
    assert!(!unsafe {
        dispatch(
            RETRO_ENVIRONMENT_GET_LOG_INTERFACE,
            log_cb.as_mut_ptr() as *mut c_void,
            &state,
            &mut runtime,
        )
    });
}

#[test]
fn dispatch_output_cases_tolerate_null_data() {
    let state = test_state();
    let mut runtime = EnvironmentRuntime::default();
    // 出参 case 在 data 为空时跳过写入、正常返回（C 各 case 均有 if(out) 判空）
    for cmd in [
        RETRO_ENVIRONMENT_GET_OVERSCAN,
        RETRO_ENVIRONMENT_GET_CAN_DUPE,
        RETRO_ENVIRONMENT_GET_INPUT_BITMASKS,
        RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY,
        RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY,
        RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE,
        RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE,
        RETRO_ENVIRONMENT_GET_INPUT_DEVICE_CAPABILITIES,
        RETRO_ENVIRONMENT_GET_AUDIO_VIDEO_ENABLE,
        RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION,
        RETRO_ENVIRONMENT_GET_DISK_CONTROL_INTERFACE_VERSION,
        RETRO_ENVIRONMENT_GET_VARIABLE,
        RETRO_ENVIRONMENT_SET_PIXEL_FORMAT,
    ] {
        assert!(
            unsafe { dispatch(cmd, ptr::null_mut(), &state, &mut runtime) },
            "cmd {cmd} 出参为空应正常返回"
        );
    }
    // GET_LOG_INTERFACE 出参为空同样正常返回（false）
    assert!(!unsafe {
        dispatch(
            RETRO_ENVIRONMENT_GET_LOG_INTERFACE,
            ptr::null_mut(),
            &state,
            &mut runtime,
        )
    });
}

// ── 任务 3.1：B 组选项体系 ─────────────────────────────────────

#[test]
fn dispatch_option_system_roundtrip() {
    let state = test_state();
    let mut runtime = EnvironmentRuntime::default();
    let key = CString::new("test_key").unwrap();
    let desc = CString::new("Test Key").unwrap();
    let va = CString::new("a").unwrap();
    let vb = CString::new("b").unwrap();
    let defs = make_test_key_defs(&key, &desc, &va, &vb);

    // SET_CORE_OPTIONS 注入 1 项
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_CORE_OPTIONS,
            defs.as_ptr() as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert_eq!(runtime.core_options.options.len(), 1);

    // GET_VARIABLE 读回默认值 "a"（value 写指向列表内 String 数据的指针）
    let mut var = RetroVariable {
        key: key.as_ptr(),
        value: ptr::null(),
    };
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_GET_VARIABLE,
            &mut var as *mut RetroVariable as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert_eq!(read_cstr(var.value), "a");

    // 未知 key：不写 value 字段（哨兵保持原值）
    let unknown = CString::new("unknown_key").unwrap();
    let sentinel = va.as_ptr();
    let mut var2 = RetroVariable {
        key: unknown.as_ptr(),
        value: sentinel,
    };
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_GET_VARIABLE,
            &mut var2 as *mut RetroVariable as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert_eq!(var2.value, sentinel, "未知 key 不应写 value 字段");

    // 修改后读回 "b"
    runtime.core_options.set_option_value("test_key", "b");
    let mut var3 = RetroVariable {
        key: key.as_ptr(),
        value: ptr::null(),
    };
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_GET_VARIABLE,
            &mut var3 as *mut RetroVariable as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert_eq!(read_cstr(var3.value), "b");
}

#[test]
fn dispatch_get_variable_update_consumes_and_clears() {
    let state = test_state();
    let mut runtime = EnvironmentRuntime::default();
    let key = CString::new("test_key").unwrap();
    let desc = CString::new("Test Key").unwrap();
    let va = CString::new("a").unwrap();
    let vb = CString::new("b").unwrap();
    let defs = make_test_key_defs(&key, &desc, &va, &vb);
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_CORE_OPTIONS,
            defs.as_ptr() as *mut c_void,
            &state,
            &mut runtime,
        )
    });

    // 无修改：写 false
    let mut changed = true;
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE,
            &mut changed as *mut bool as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert!(!changed);

    // 修改后：写 true 并消费清零（再次查询写 false）
    runtime.core_options.set_option_value("test_key", "b");
    let mut changed = false;
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE,
            &mut changed as *mut bool as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert!(changed);
    let mut changed = false;
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE,
            &mut changed as *mut bool as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert!(!changed);
}

#[test]
fn dispatch_set_core_options_intl_us_branches() {
    let state = test_state();
    let mut runtime = EnvironmentRuntime::default();
    let key = CString::new("test_key").unwrap();
    let desc = CString::new("Test Key").unwrap();
    let va = CString::new("a").unwrap();
    let vb = CString::new("b").unwrap();
    let defs = make_test_key_defs(&key, &desc, &va, &vb);

    // us 非空 → 替换
    let intl = RetroCoreOptionsIntl {
        us: defs.as_ptr(),
        local: ptr::null(),
    };
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_CORE_OPTIONS_INTL,
            &intl as *const RetroCoreOptionsIntl as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert_eq!(runtime.core_options.options.len(), 1);
    assert_eq!(runtime.core_options.options[0].key, "test_key");

    // 换一张表后再以 us 空调用 → 表不变（C :2044-2048 仅在 us 非空时替换）
    let key2 = CString::new("other_key").unwrap();
    let defs2 = make_test_key_defs(&key2, &desc, &va, &vb);
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_CORE_OPTIONS,
            defs2.as_ptr() as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert_eq!(runtime.core_options.options[0].key, "other_key");

    let intl_null = RetroCoreOptionsIntl {
        us: ptr::null(),
        local: ptr::null(),
    };
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_CORE_OPTIONS_INTL,
            &intl_null as *const RetroCoreOptionsIntl as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert_eq!(runtime.core_options.options.len(), 1);
    assert_eq!(runtime.core_options.options[0].key, "other_key");
}

#[test]
fn dispatch_set_variables_replaces() {
    let state = test_state();
    let mut runtime = EnvironmentRuntime::default();
    let key = CString::new("test_key").unwrap();
    let desc = CString::new("Test Key").unwrap();
    let va = CString::new("a").unwrap();
    let vb = CString::new("b").unwrap();
    let defs = make_test_key_defs(&key, &desc, &va, &vb);
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_CORE_OPTIONS,
            defs.as_ptr() as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert_eq!(runtime.core_options.options.len(), 1);

    // SET_VARIABLES 整体替换为 vars 表（test_key 消失）
    let var_key = CString::new("snes9x_region").unwrap();
    let var_value = CString::new("Region; auto|ntsc|pal").unwrap();
    let vars = [
        RetroVariable {
            key: var_key.as_ptr(),
            value: var_value.as_ptr(),
        },
        RetroVariable {
            key: ptr::null(),
            value: ptr::null(),
        },
    ];
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_VARIABLES,
            vars.as_ptr() as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert_eq!(runtime.core_options.options.len(), 1);
    assert_eq!(runtime.core_options.options[0].key, "snes9x_region");

    // 旧键现在未知：GET_VARIABLE 不写 value
    let mut var = RetroVariable {
        key: key.as_ptr(),
        value: ptr::null(),
    };
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_GET_VARIABLE,
            &mut var as *mut RetroVariable as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert!(var.value.is_null(), "替换后旧键不应写 value");
}

#[test]
fn dispatch_set_variable_dual_mode() {
    let state = test_state();
    let mut runtime = EnvironmentRuntime::default();
    let key = CString::new("test_key").unwrap();
    let desc = CString::new("Test Key").unwrap();
    let va = CString::new("a").unwrap();
    let vb = CString::new("b").unwrap();
    let defs = make_test_key_defs(&key, &desc, &va, &vb);
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_CORE_OPTIONS,
            defs.as_ptr() as *mut c_void,
            &state,
            &mut runtime,
        )
    });

    // 变量模式：key 非空 → 设置选项值
    let value = CString::new("b").unwrap();
    let var = RetroVariable {
        key: key.as_ptr(),
        value: value.as_ptr(),
    };
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_VARIABLE,
            &var as *const RetroVariable as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert_eq!(runtime.core_options.get_option_value("test_key"), Some("b"));
    assert!(runtime.core_options.changed);

    // i32 模式：key 位置为空 → 出参写 1（表示支持）
    // 缓冲区取 RetroVariable 大小，保证实现侧读取 key 字段（指针宽度）
    // 不发生越界（见 dispatch 的 SET_VARIABLE 契约注释）
    let mut buf = [0u8; core::mem::size_of::<RetroVariable>()];
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_VARIABLE,
            buf.as_mut_ptr() as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    assert_eq!(u32::from_ne_bytes(buf[..4].try_into().unwrap()), 1);
    assert!(buf[4..].iter().all(|&b| b == 0), "仅应写入前 4 字节");
}

// ── 任务 4.1：C/D/E/F 组 ───────────────────────────────────────

#[test]
fn dispatch_set_input_descriptors_replaces_mapping() {
    let state = test_state();
    let mut runtime = EnvironmentRuntime::default();
    let desc_a = CString::new("A (Core)").unwrap();
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
            device: 0,
            index: 0,
            id: 0,
            description: ptr::null(), // 终止项
        },
    ];

    let ok = unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS,
            descriptors.as_ptr() as *mut c_void,
            &state,
            &mut runtime,
        )
    };
    assert!(!ok, "SET_INPUT_DESCRIPTORS 恒返回 false（C :1909）");

    // controls_mapping 反映 init_core_mapping 结果：A 覆盖名、其余 ignore
    assert_eq!(runtime.controls_mapping.len(), 16);
    assert_eq!(runtime.controls_mapping[4].name, "A (Core)");
    assert!(!runtime.controls_mapping[4].ignore);
    assert!(
        runtime.controls_mapping[0].ignore,
        "核心未上报的按键应标记 ignore"
    );
}

#[test]
fn dispatch_disc_control_last_wins_and_zeroes() {
    let state = test_state();
    let mut runtime = EnvironmentRuntime::default();

    // ext 表：10 字段全非空 → 全量拷贝（last-wins）
    let ext = RetroDiskControlExtCallback {
        set_eject_state: ext_set_eject,
        get_eject_state: ext_get_eject,
        get_image_index: ext_get_image_index,
        set_image_index: ext_set_image_index,
        get_num_images: ext_get_image_index,
        replace_image_index: ext_replace_image_index,
        add_image_index: ext_get_eject,
        set_initial_image: ext_set_initial_image,
        get_image_path: ext_get_image_path,
        get_image_label: ext_get_image_path,
    };
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_DISK_CONTROL_EXT_INTERFACE,
            &ext as *const RetroDiskControlExtCallback as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    let disc = runtime.disc.expect("ext 注入后应为 Some");
    assert!(std::ptr::eq(
        disc.set_eject_state as *const (),
        ext_set_eject as *const ()
    ));
    assert!(std::ptr::eq(
        disc.get_eject_state as *const (),
        ext_get_eject as *const ()
    ));
    assert!(std::ptr::eq(
        disc.get_image_index as *const (),
        ext_get_image_index as *const ()
    ));
    assert!(std::ptr::eq(
        disc.set_image_index as *const (),
        ext_set_image_index as *const ()
    ));
    assert!(std::ptr::eq(
        disc.get_num_images as *const (),
        ext_get_image_index as *const ()
    ));
    assert!(std::ptr::eq(
        disc.replace_image_index as *const (),
        ext_replace_image_index as *const ()
    ));
    assert!(std::ptr::eq(
        disc.add_image_index as *const (),
        ext_get_eject as *const ()
    ));
    assert!(std::ptr::eq(
        disc.set_initial_image as *const (),
        ext_set_initial_image as *const ()
    ));
    assert!(std::ptr::eq(
        disc.get_image_path as *const (),
        ext_get_image_path as *const ()
    ));
    assert!(std::ptr::eq(
        disc.get_image_label as *const (),
        ext_get_image_path as *const ()
    ));

    // v1 表注入：last-wins 替换前 7 字段；后 3 字段先清零（C memset
    // 同构——libretro.h 规定 ext-only 字段 Optional 不被调用）
    let v1 = RetroDiskControlCallback {
        set_eject_state: v1_set_eject,
        get_eject_state: v1_get_eject,
        get_image_index: v1_get_image_index,
        set_image_index: v1_set_image_index,
        get_num_images: v1_get_image_index,
        replace_image_index: v1_replace_image_index,
        add_image_index: v1_get_eject,
    };
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_DISK_CONTROL_INTERFACE,
            &v1 as *const RetroDiskControlCallback as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    let disc = runtime.disc.expect("v1 注入后仍应为 Some");
    assert!(std::ptr::eq(
        disc.set_eject_state as *const (),
        v1_set_eject as *const ()
    ));
    assert!(std::ptr::eq(
        disc.get_eject_state as *const (),
        v1_get_eject as *const ()
    ));
    assert!(std::ptr::eq(
        disc.get_image_index as *const (),
        v1_get_image_index as *const ()
    ));
    assert!(std::ptr::eq(
        disc.set_image_index as *const (),
        v1_set_image_index as *const ()
    ));
    assert!(std::ptr::eq(
        disc.get_num_images as *const (),
        v1_get_image_index as *const ()
    ));
    assert!(std::ptr::eq(
        disc.replace_image_index as *const (),
        v1_replace_image_index as *const ()
    ));
    assert!(std::ptr::eq(
        disc.add_image_index as *const (),
        v1_get_eject as *const ()
    ));
    assert_eq!(
        disc.set_initial_image as *const () as usize, 0,
        "后 3 字段应为 NULL"
    );
    assert_eq!(
        disc.get_image_path as *const () as usize, 0,
        "后 3 字段应为 NULL"
    );
    assert_eq!(
        disc.get_image_label as *const () as usize, 0,
        "后 3 字段应为 NULL"
    );
}

#[test]
fn dispatch_get_rumble_interface_fills_trampoline() {
    let state = test_state();
    let mut runtime = EnvironmentRuntime::default();
    // 含 fn 指针字段的类型不允许 mem::zeroed 零初始化——用 MaybeUninit
    // 占位，dispatch 写入后方可 assume_init
    let mut iface = core::mem::MaybeUninit::<RetroRumbleInterface>::uninit();
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE,
            iface.as_mut_ptr() as *mut c_void,
            &state,
            &mut runtime,
        )
    });
    let iface = unsafe { iface.assume_init() };
    // 填入 trampoline 函数指针（非空断言 + 与 rumble_trampoline 同一函数）
    assert!(std::ptr::eq(
        iface.set_rumble_state as *const (),
        rumble_trampoline as *const ()
    ));
}

#[test]
fn dispatch_set_controller_info_dualshock_scan() {
    let state = test_state();
    let gamepad = CString::new("gamepad").unwrap();

    // 不含 dualshock：保持 false；恒返回 false
    let mut runtime = EnvironmentRuntime::default();
    let types = [RetroControllerDescription {
        desc: gamepad.as_ptr(),
        id: 1,
    }];
    let info = RetroControllerInfo {
        types: types.as_ptr(),
        num_types: 1,
    };
    let ok = unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_CONTROLLER_INFO,
            &info as *const RetroControllerInfo as *mut c_void,
            &state,
            &mut runtime,
        )
    };
    assert!(!ok, "SET_CONTROLLER_INFO 恒返回 false（C :2007 TODO）");
    assert!(!runtime.has_custom_controllers);

    // 含 DualShock（大小写混合）：置 true
    let mut runtime = EnvironmentRuntime::default();
    let dualshock = CString::new("DualShock").unwrap();
    let types = [
        RetroControllerDescription {
            desc: dualshock.as_ptr(),
            id: 1,
        },
        RetroControllerDescription {
            desc: gamepad.as_ptr(),
            id: 1,
        },
    ];
    let info = RetroControllerInfo {
        types: types.as_ptr(),
        num_types: 2,
    };
    let ok = unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_CONTROLLER_INFO,
            &info as *const RetroControllerInfo as *mut c_void,
            &state,
            &mut runtime,
        )
    };
    assert!(!ok);
    assert!(runtime.has_custom_controllers);

    // 小写 dualshock 同样命中（大小写不敏感——C strncmp 大小写敏感的偏离点）
    let mut runtime = EnvironmentRuntime::default();
    let lower = CString::new("dualshock").unwrap();
    let types = [RetroControllerDescription {
        desc: lower.as_ptr(),
        id: 1,
    }];
    let info = RetroControllerInfo {
        types: types.as_ptr(),
        num_types: 1,
    };
    let ok = unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_CONTROLLER_INFO,
            &info as *const RetroControllerInfo as *mut c_void,
            &state,
            &mut runtime,
        )
    };
    assert!(!ok);
    assert!(runtime.has_custom_controllers);
}

// ── 任务 5.1：默认值与边界 ─────────────────────────────────────

#[test]
fn dispatch_unrecognized_command_returns_false() {
    let state = test_state();
    let mut runtime = EnvironmentRuntime::default();
    assert!(!unsafe { dispatch(9999, ptr::null_mut(), &state, &mut runtime) });
}

#[test]
fn dispatch_get_variable_null_data_skips_write() {
    let state = test_state();
    let mut runtime = EnvironmentRuntime::default();
    let key = CString::new("test_key").unwrap();
    let desc = CString::new("Test Key").unwrap();
    let va = CString::new("a").unwrap();
    let vb = CString::new("b").unwrap();
    let defs = make_test_key_defs(&key, &desc, &va, &vb);
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_SET_CORE_OPTIONS,
            defs.as_ptr() as *mut c_void,
            &state,
            &mut runtime,
        )
    });

    // data == null → 跳过写入、正常返回 true（不 panic 即可观测契约）
    assert!(unsafe {
        dispatch(
            RETRO_ENVIRONMENT_GET_VARIABLE,
            ptr::null_mut(),
            &state,
            &mut runtime,
        )
    });
}
