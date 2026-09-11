//! libretro ABI 常量与布局校验测试
//!
//! 所有期望值派生自 vendored 权威头 `tests/fixtures/libretro.h`
//! （来源 commit b894fa4ed3a7ed6247dd4745cc2cc4f50042b7ee）。
//! 若该头升级导致本测试失败，请按新头同步期望值。
//!
//! 布局断言使用**编译期 const 断言**（`const _: () = assert!(...)`）：
//! 断言错误即编译失败，比运行时 `assert_eq!` 更强，且使 32 位期望值
//! 组可在 arm32 交叉目标上经 `cargo check --tests` 验证（无需真机）。

use core::ffi::{c_char, c_void};
use core::mem::{offset_of, size_of};

use minarch::libretro::*;

// ═══════════════════════════════════════════════════════════════
// 常量断言组
// ═══════════════════════════════════════════════════════════════

#[test]
fn environment_constants_match_vendored_header() {
    assert_eq!(RETRO_ENVIRONMENT_EXPERIMENTAL, 0x10000);
    assert_eq!(RETRO_ENVIRONMENT_SET_ROTATION, 1);
    assert_eq!(RETRO_ENVIRONMENT_GET_OVERSCAN, 2);
    assert_eq!(RETRO_ENVIRONMENT_GET_CAN_DUPE, 3);
    assert_eq!(RETRO_ENVIRONMENT_SET_MESSAGE, 6);
    assert_eq!(RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL, 8);
    assert_eq!(RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY, 9);
    assert_eq!(RETRO_ENVIRONMENT_SET_PIXEL_FORMAT, 10);
    assert_eq!(RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS, 11);
    assert_eq!(RETRO_ENVIRONMENT_SET_DISK_CONTROL_INTERFACE, 13);
    assert_eq!(RETRO_ENVIRONMENT_GET_VARIABLE, 15);
    assert_eq!(RETRO_ENVIRONMENT_SET_VARIABLES, 16);
    assert_eq!(RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE, 17);
    assert_eq!(RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME, 18);
    assert_eq!(RETRO_ENVIRONMENT_SET_FRAME_TIME_CALLBACK, 21);
    assert_eq!(RETRO_ENVIRONMENT_SET_AUDIO_CALLBACK, 22);
    assert_eq!(RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE, 23);
    assert_eq!(RETRO_ENVIRONMENT_GET_INPUT_DEVICE_CAPABILITIES, 24);
    assert_eq!(RETRO_ENVIRONMENT_GET_LOG_INTERFACE, 27);
    assert_eq!(RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY, 31);
    assert_eq!(RETRO_ENVIRONMENT_SET_CONTROLLER_INFO, 35);
    assert_eq!(
        RETRO_ENVIRONMENT_GET_CURRENT_SOFTWARE_FRAMEBUFFER,
        40 | RETRO_ENVIRONMENT_EXPERIMENTAL
    );
    assert_eq!(
        RETRO_ENVIRONMENT_GET_AUDIO_VIDEO_ENABLE,
        47 | RETRO_ENVIRONMENT_EXPERIMENTAL
    );
    assert_eq!(
        RETRO_ENVIRONMENT_GET_INPUT_BITMASKS,
        51 | RETRO_ENVIRONMENT_EXPERIMENTAL
    );
    assert_eq!(RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION, 52);
    assert_eq!(RETRO_ENVIRONMENT_SET_CORE_OPTIONS, 53);
    assert_eq!(RETRO_ENVIRONMENT_SET_CORE_OPTIONS_INTL, 54);
    assert_eq!(RETRO_ENVIRONMENT_SET_CORE_OPTIONS_DISPLAY, 55);
    assert_eq!(RETRO_ENVIRONMENT_GET_DISK_CONTROL_INTERFACE_VERSION, 57);
    assert_eq!(RETRO_ENVIRONMENT_SET_DISK_CONTROL_EXT_INTERFACE, 58);
    assert_eq!(RETRO_ENVIRONMENT_SET_CONTENT_INFO_OVERRIDE, 65);
    assert_eq!(RETRO_ENVIRONMENT_SET_VARIABLE, 70);
}

#[test]
fn device_and_misc_constants_match_vendored_header() {
    assert_eq!(RETRO_DEVICE_JOYPAD, 1);
    assert_eq!(RETRO_DEVICE_ANALOG, 5);

    assert_eq!(RETRO_DEVICE_ID_JOYPAD_B, 0);
    assert_eq!(RETRO_DEVICE_ID_JOYPAD_Y, 1);
    assert_eq!(RETRO_DEVICE_ID_JOYPAD_SELECT, 2);
    assert_eq!(RETRO_DEVICE_ID_JOYPAD_START, 3);
    assert_eq!(RETRO_DEVICE_ID_JOYPAD_UP, 4);
    assert_eq!(RETRO_DEVICE_ID_JOYPAD_DOWN, 5);
    assert_eq!(RETRO_DEVICE_ID_JOYPAD_LEFT, 6);
    assert_eq!(RETRO_DEVICE_ID_JOYPAD_RIGHT, 7);
    assert_eq!(RETRO_DEVICE_ID_JOYPAD_A, 8);
    assert_eq!(RETRO_DEVICE_ID_JOYPAD_X, 9);
    assert_eq!(RETRO_DEVICE_ID_JOYPAD_L, 10);
    assert_eq!(RETRO_DEVICE_ID_JOYPAD_R, 11);
    assert_eq!(RETRO_DEVICE_ID_JOYPAD_L2, 12);
    assert_eq!(RETRO_DEVICE_ID_JOYPAD_R2, 13);
    assert_eq!(RETRO_DEVICE_ID_JOYPAD_L3, 14);
    assert_eq!(RETRO_DEVICE_ID_JOYPAD_R3, 15);
    assert_eq!(RETRO_DEVICE_ID_JOYPAD_MASK, 256);

    assert_eq!(RETRO_DEVICE_ID_ANALOG_X, 0);
    assert_eq!(RETRO_DEVICE_ID_ANALOG_Y, 1);
    assert_eq!(RETRO_DEVICE_INDEX_ANALOG_LEFT, 0);
    assert_eq!(RETRO_DEVICE_INDEX_ANALOG_RIGHT, 1);

    assert_eq!(RETRO_MEMORY_SAVE_RAM, 0);
    assert_eq!(RETRO_MEMORY_RTC, 1);

    assert_eq!(RETRO_PIXEL_FORMAT_RGB565, 2);

    assert_eq!(RETRO_AV_ENABLE_VIDEO, 1 << 0);
    assert_eq!(RETRO_AV_ENABLE_AUDIO, 1 << 1);

    assert_eq!(RETRO_API_VERSION, 1);
    assert_eq!(RETRO_NUM_CORE_OPTION_VALUES_MAX, 128);
}

/// 常量使用精确宽度类型（spec「常量类型精确宽度」Scenario 的编译期证据）：
/// 断言失败即编译失败，覆盖 C `int`/`unsigned` 的平台宽度歧义风险。
#[test]
fn constants_use_exact_width_types() {
    let _: u32 = RETRO_ENVIRONMENT_SET_PIXEL_FORMAT;
    let _: u32 = RETRO_ENVIRONMENT_EXPERIMENTAL;
    let _: u32 = RETRO_DEVICE_JOYPAD;
    let _: u32 = RETRO_DEVICE_ANALOG;
    let _: u32 = RETRO_DEVICE_ID_JOYPAD_B;
    let _: u32 = RETRO_DEVICE_ID_JOYPAD_MASK;
    let _: u32 = RETRO_DEVICE_ID_ANALOG_X;
    let _: u32 = RETRO_DEVICE_INDEX_ANALOG_LEFT;
    let _: u32 = RETRO_MEMORY_SAVE_RAM;
    let _: u32 = RETRO_PIXEL_FORMAT_RGB565;
    let _: u32 = RETRO_AV_ENABLE_VIDEO;
    let _: u32 = RETRO_API_VERSION;
    let _: usize = RETRO_NUM_CORE_OPTION_VALUES_MAX;
}

/// 回调/符号签名使用 `u32` 对应 C 的 `unsigned`、`usize` 对应 `size_t`、
/// `i16` 对应 `int16_t`（spec「常量类型精确宽度」Scenario 的编译期证据）
#[test]
fn callback_signatures_match_c_abi_widths() {
    let _: unsafe extern "C" fn(u32, *mut c_void) -> bool = environment_trampoline;
    let _: unsafe extern "C" fn(*const c_void, u32, u32, usize) = video_refresh_trampoline;
    let _: unsafe extern "C" fn(i16, i16) = audio_sample_trampoline;
    let _: unsafe extern "C" fn(*const i16, usize) -> usize = audio_sample_batch_trampoline;
    let _: unsafe extern "C" fn() = input_poll_trampoline;
    let _: unsafe extern "C" fn(u32, u32, u32, u32) -> i16 = input_state_trampoline;
}

// ═══════════════════════════════════════════════════════════════
// 布局断言组（编译期 const 断言，期望值派生自 vendored libretro.h）
// ═══════════════════════════════════════════════════════════════

#[cfg(target_pointer_width = "64")]
mod layout64 {
    use super::*;

    // 64 位 pointer width（macOS arm64 主机 / aarch64 设备）
    const _: () = {
        assert!(size_of::<RetroGameGeometry>() == 20);
        assert!(size_of::<RetroSystemTiming>() == 16);
        assert!(size_of::<RetroSystemAvInfo>() == 40);
        assert!(offset_of!(RetroSystemAvInfo, geometry) == 0);
        assert!(offset_of!(RetroSystemAvInfo, timing) == 24);

        assert!(size_of::<RetroSystemInfo>() == 32);
        assert!(size_of::<RetroGameInfo>() == 32);
        assert!(size_of::<RetroVariable>() == 16);
        assert!(size_of::<RetroMessage>() == 16);
        assert!(size_of::<RetroInputDescriptor>() == 24);

        assert!(size_of::<RetroRumbleInterface>() == 8);
        assert!(size_of::<RetroLogCallback>() == 8);
        assert!(size_of::<RetroControllerInfo>() == 16);
        assert!(size_of::<RetroControllerDescription>() == 16);
        assert!(size_of::<RetroDiskControlCallback>() == 56);
        assert!(size_of::<RetroDiskControlExtCallback>() == 80);

        assert!(size_of::<RetroCoreOptionValue>() == 16);
        assert!(size_of::<RetroCoreOptionDefinition>() == 2080);
        assert!(offset_of!(RetroCoreOptionDefinition, key) == 0);
        assert!(offset_of!(RetroCoreOptionDefinition, desc) == 8);
        assert!(offset_of!(RetroCoreOptionDefinition, info) == 16);
        assert!(offset_of!(RetroCoreOptionDefinition, values) == 24);
        assert!(offset_of!(RetroCoreOptionDefinition, default_value) == 2072);
        assert!(size_of::<RetroCoreOptionsIntl>() == 16);

        assert!(size_of::<RetroPixelFormat>() == 4);
        assert!(size_of::<RetroRumbleEffect>() == 4);
        assert!(size_of::<RetroLogLevel>() == 4);

        assert!(size_of::<*const c_char>() == 8);
        assert!(size_of::<*const c_void>() == 8);
    };
}

#[cfg(target_pointer_width = "32")]
mod layout32 {
    use super::*;

    // 32 位 pointer width（arm32 设备，如 rg35xx）——
    // 经 `cargo check --tests --target <arm32>` 交叉编译时校验
    const _: () = {
        assert!(size_of::<RetroGameGeometry>() == 20);
        assert!(size_of::<RetroSystemTiming>() == 16);
        assert!(size_of::<RetroSystemAvInfo>() == 40);
        assert!(offset_of!(RetroSystemAvInfo, geometry) == 0);
        assert!(offset_of!(RetroSystemAvInfo, timing) == 24);

        assert!(size_of::<RetroSystemInfo>() == 16);
        assert!(size_of::<RetroGameInfo>() == 16);
        assert!(size_of::<RetroVariable>() == 8);
        assert!(size_of::<RetroMessage>() == 8);
        assert!(size_of::<RetroInputDescriptor>() == 20);

        assert!(size_of::<RetroRumbleInterface>() == 4);
        assert!(size_of::<RetroLogCallback>() == 4);
        assert!(size_of::<RetroControllerInfo>() == 8);
        assert!(size_of::<RetroControllerDescription>() == 8);
        assert!(size_of::<RetroDiskControlCallback>() == 28);
        assert!(size_of::<RetroDiskControlExtCallback>() == 40);

        assert!(size_of::<RetroCoreOptionValue>() == 8);
        assert!(size_of::<RetroCoreOptionDefinition>() == 1040);
        assert!(offset_of!(RetroCoreOptionDefinition, key) == 0);
        assert!(offset_of!(RetroCoreOptionDefinition, desc) == 4);
        assert!(offset_of!(RetroCoreOptionDefinition, info) == 8);
        assert!(offset_of!(RetroCoreOptionDefinition, values) == 12);
        assert!(offset_of!(RetroCoreOptionDefinition, default_value) == 1036);
        assert!(size_of::<RetroCoreOptionsIntl>() == 8);

        assert!(size_of::<RetroPixelFormat>() == 4);
        assert!(size_of::<RetroRumbleEffect>() == 4);
        assert!(size_of::<RetroLogLevel>() == 4);

        assert!(size_of::<*const c_char>() == 4);
        assert!(size_of::<*const c_void>() == 4);
    };
}
