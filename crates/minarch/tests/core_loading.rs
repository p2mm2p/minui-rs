//! Core 加载器与静态回调桥集成测试
//!
//! 通过 `tests/common/mod.rs` 在主机端编译 mock 核心（`tests/fixtures/mock_core.c`）
//! 为动态库，对 `minarch::libretro` 做真实的 dlopen/dlsym/回调往返验证。
//!
//! 注意：回调桥 `FRONTEND_STATE` 是进程级 OnceLock，所有桥相关断言集中在
//! 单个测试 `callback_bridge_lifecycle` 内串行覆盖，避免并行测试相互干扰。

mod test_common;

use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::ffi::CStr;

use minarch::libretro::*;

/// 编译期断言：`Core` 可跨线程传递（thread_video 模式 `retro_run` 在独立线程）
fn assert_send_sync<T: Send + Sync>() {}

fn zeroed_system_info() -> RetroSystemInfo {
    unsafe { core::mem::zeroed() }
}

// ═══════════════════════════════════════════════════════════════
// 任务 4.1：加载成功用例
// ═══════════════════════════════════════════════════════════════

#[test]
fn loads_all_symbols_and_fills_info_cache() {
    let path = test_common::build_mock_core("full", &[]);
    let core = Core::open(
        path.to_str().unwrap(),
        "TST",
        "/cfg",
        "/states",
        "/saves",
        "/bios",
    )
    .expect("mock 核心应加载成功");

    // 信息缓存：对应 C struct Core 的字段（minarch.c:68-107）
    assert_eq!(core.tag, "TST");
    assert_eq!(core.name, "mock_core");
    assert_eq!(core.version, "mock_core (0.0.1)");
    assert_eq!(core.extensions, "mc|zip");
    assert!(!core.need_fullpath);
    assert_eq!(core.config_dir, "/cfg");
    assert_eq!(core.states_dir, "/states");
    assert_eq!(core.saves_dir, "/saves");
    assert_eq!(core.bios_dir, "/bios");
    assert_eq!(core.fps, 0.0);
    assert_eq!(core.sample_rate, 0.0);
    assert_eq!(core.aspect_ratio, 0.0);

    // 预留符号：mock 核心导出了它们，应为 Some
    assert!(core.load_game_special.is_some());
    assert!(core.get_region.is_some());

    // 22 个符号逐一调用验证非空（空函数指针调用即崩溃，能成功调用即证明绑定正确）
    unsafe { (core.init)() };
    unsafe { (core.deinit)() };
    unsafe { (core.reset)() };

    let mut info = zeroed_system_info();
    unsafe { (core.get_system_info)(&mut info) };
    let library_name = unsafe { CStr::from_ptr(info.library_name) }
        .to_str()
        .unwrap();
    assert_eq!(library_name, "mock_core");

    let mut av: RetroSystemAvInfo = unsafe { core::mem::zeroed() };
    unsafe { (core.get_system_av_info)(&mut av) };
    assert_eq!(av.geometry.base_width, 160);
    assert_eq!(av.geometry.base_height, 144);
    assert_eq!(av.timing.fps, 60.0);
    assert_eq!(av.timing.sample_rate, 44100.0);

    unsafe { (core.set_controller_port_device)(0, RETRO_DEVICE_JOYPAD) };

    assert_eq!(unsafe { (core.serialize_size)() }, 8);
    let mut buf = [0u8; 8];
    assert!(unsafe { (core.serialize)(buf.as_mut_ptr().cast(), 8) });
    assert_eq!(buf, [0xAB; 8]);
    assert!(unsafe { (core.unserialize)(buf.as_ptr().cast(), 8) });

    assert_eq!(unsafe { (core.get_memory_size)(RETRO_MEMORY_SAVE_RAM) }, 16);
    assert!(!unsafe { (core.get_memory_data)(RETRO_MEMORY_SAVE_RAM) }.is_null());

    let game = RetroGameInfo {
        path: core::ptr::null(),
        data: core::ptr::null(),
        size: 0,
        meta: core::ptr::null(),
    };
    assert!(unsafe { (core.load_game)(&game) });
    unsafe { (core.unload_game)() };
}

// ═══════════════════════════════════════════════════════════════
// 任务 4.2：错误路径用例
// ═══════════════════════════════════════════════════════════════

#[test]
fn missing_so_returns_library_open_failed() {
    let result = Core::open("/nonexistent/mock_core.so", "TST", "/c", "/s", "/v", "/b");
    match result {
        Err(CoreError::LibraryOpenFailed { path, .. }) => {
            assert_eq!(path, "/nonexistent/mock_core.so");
        }
        other => panic!("期望 LibraryOpenFailed，得到 {:?}", other.err()),
    }
}

#[test]
fn missing_required_symbol_returns_symbol_missing() {
    let path = test_common::build_mock_core("no_run", &["MOCK_NO_RETRO_RUN"]);
    let result = Core::open(path.to_str().unwrap(), "TST", "/c", "/s", "/v", "/b");
    match result {
        Err(CoreError::SymbolMissing { name }) => assert_eq!(name, "retro_run"),
        other => panic!("期望 SymbolMissing，得到 {:?}", other.err()),
    }
}

#[test]
fn missing_reserved_symbols_load_with_none() {
    let path = test_common::build_mock_core(
        "no_reserved",
        &["MOCK_NO_LOAD_GAME_SPECIAL", "MOCK_NO_GET_REGION"],
    );
    let core = Core::open(path.to_str().unwrap(), "TST", "/c", "/s", "/v", "/b")
        .expect("预留符号缺失不应报错");
    assert!(core.load_game_special.is_none());
    assert!(core.get_region.is_none());
}

#[test]
fn core_is_send_and_sync() {
    assert_send_sync::<Core>();
}

// ═══════════════════════════════════════════════════════════════
// 任务 5.1 + 5.2：回调桥生命周期（单测试串行覆盖：默认值 → 注册转发
// → 重复注册报错 → 首次处理器保持有效）
// ═══════════════════════════════════════════════════════════════

#[test]
fn callback_bridge_lifecycle() {
    // ── 5.2a：未注册时 6 个 trampoline 安全返回默认值，不 panic ──
    assert!(!unsafe {
        environment_trampoline(RETRO_ENVIRONMENT_GET_OVERSCAN, core::ptr::null_mut())
    });
    unsafe { video_refresh_trampoline(core::ptr::null(), 160, 144, 320) };
    unsafe { audio_sample_trampoline(0, 0) };
    let frames = [0i16; 8];
    assert_eq!(
        unsafe { audio_sample_batch_trampoline(frames.as_ptr(), 4) },
        4
    );
    unsafe { input_poll_trampoline() };
    assert_eq!(
        unsafe { input_state_trampoline(0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_B) },
        0
    );

    // ── 5.1：注册处理器 → 挂接 trampoline → retro_run 触发回调转发 ──
    // 处理器内只捕获参数（写入静态量），断言放回测试主体——
    // 避免断言 panic 穿过 extern "C" 边界导致进程 abort。
    static VIDEO_CALLS: AtomicUsize = AtomicUsize::new(0);
    static VIDEO_WIDTH: AtomicUsize = AtomicUsize::new(0);
    static VIDEO_HEIGHT: AtomicUsize = AtomicUsize::new(0);
    static VIDEO_PITCH: AtomicUsize = AtomicUsize::new(0);
    static VIDEO_FIRST_PIXEL: AtomicUsize = AtomicUsize::new(0);
    static AUDIO_CALLS: AtomicUsize = AtomicUsize::new(0);
    static AUDIO_FRAMES: AtomicUsize = AtomicUsize::new(0);
    static ENV_CMD: AtomicUsize = AtomicUsize::new(0);
    static ENV_FORMAT: AtomicUsize = AtomicUsize::new(0);

    fn video_handler(data: *const c_void, width: u32, height: u32, pitch: usize) {
        VIDEO_CALLS.fetch_add(1, Ordering::SeqCst);
        VIDEO_WIDTH.store(width as usize, Ordering::SeqCst);
        VIDEO_HEIGHT.store(height as usize, Ordering::SeqCst);
        VIDEO_PITCH.store(pitch, Ordering::SeqCst);
        let first_pixel = unsafe { *(data.cast::<u16>()) };
        VIDEO_FIRST_PIXEL.store(first_pixel as usize, Ordering::SeqCst);
    }
    fn audio_handler(_data: *const i16, frames: usize) -> usize {
        AUDIO_CALLS.fetch_add(1, Ordering::SeqCst);
        AUDIO_FRAMES.store(frames, Ordering::SeqCst);
        frames
    }
    fn environment_handler(cmd: u32, data: *mut c_void) -> bool {
        ENV_CMD.store(cmd as usize, Ordering::SeqCst);
        let format = unsafe { *(data.cast::<RetroPixelFormat>()) };
        ENV_FORMAT.store(format as u32 as usize, Ordering::SeqCst);
        true
    }

    if register_callbacks(FrontendState {
        environment: Some(environment_handler),
        video_refresh: Some(video_handler),
        audio_sample_batch: Some(audio_handler),
        ..FrontendState::default()
    })
    .is_err()
    {
        panic!("首次注册应成功");
    }

    let path = test_common::build_mock_core("bridge", &[]);
    let core = Core::open(path.to_str().unwrap(), "TST", "/c", "/s", "/v", "/b")
        .expect("mock 核心应加载成功");

    unsafe { (core.set_video_refresh)(video_refresh_trampoline) };
    unsafe { (core.set_audio_sample)(audio_sample_trampoline) };
    unsafe { (core.set_audio_sample_batch)(audio_sample_batch_trampoline) };
    unsafe { (core.set_input_poll)(input_poll_trampoline) };
    unsafe { (core.set_input_state)(input_state_trampoline) };
    unsafe { (core.set_environment)(environment_trampoline) };

    unsafe { (core.run)() };
    assert_eq!(VIDEO_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(VIDEO_WIDTH.load(Ordering::SeqCst), 160);
    assert_eq!(VIDEO_HEIGHT.load(Ordering::SeqCst), 144);
    assert_eq!(VIDEO_PITCH.load(Ordering::SeqCst), 320);
    assert_eq!(VIDEO_FIRST_PIXEL.load(Ordering::SeqCst), 0x1234);
    assert_eq!(AUDIO_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(AUDIO_FRAMES.load(Ordering::SeqCst), 735);

    // 环境回调转发路径（直接调用 trampoline，参数断言在测试主体）
    let mut format = RetroPixelFormat::Rgb565;
    assert!(unsafe {
        environment_trampoline(
            RETRO_ENVIRONMENT_SET_PIXEL_FORMAT,
            (&mut format as *mut RetroPixelFormat).cast(),
        )
    });
    assert_eq!(
        ENV_CMD.load(Ordering::SeqCst),
        RETRO_ENVIRONMENT_SET_PIXEL_FORMAT as usize
    );
    assert_eq!(
        ENV_FORMAT.load(Ordering::SeqCst),
        RetroPixelFormat::Rgb565 as u32 as usize
    );

    // ── 5.2b：重复注册报错且携带第二次状态；首次处理器保持有效 ──
    fn second_handler(_data: *const c_void, _w: u32, _h: u32, _p: usize) {}
    let second = FrontendState {
        video_refresh: Some(second_handler),
        ..FrontendState::default()
    };
    let result = register_callbacks(second);
    assert!(result.is_err(), "重复注册应返回错误");
    // 错误携带第二次传入的 FrontendState（OnceLock::set 语义）
    match result {
        Err(state) => assert!(state.video_refresh.is_some(), "错误应携带第二次状态原值"),
        Ok(()) => panic!("重复注册不应成功"),
    }

    // 首次处理器仍生效：trampoline 仍转发到 video_handler
    static FRAME: [u16; 2] = [0x1234, 0];
    unsafe {
        video_refresh_trampoline(FRAME.as_ptr().cast(), 160, 144, 320);
    }
    assert_eq!(VIDEO_CALLS.load(Ordering::SeqCst), 2);
    assert_eq!(VIDEO_FIRST_PIXEL.load(Ordering::SeqCst), 0x1234);
}
