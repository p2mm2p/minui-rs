//! core.rs 集成测试：会话生命周期、布局计算、视频处理器、薄静态层与快进帧控
//!
//! 布局与 limit_ff 为纯函数测试（无 FFI）；会话与视频处理器经
//! `tests/test_common/mod.rs` 编译的 mock 核心（`tests/fixtures/mock_core.c`）
//! 做端到端验证。注意：回调桥 `FRONTEND_STATE` 与 core 薄静态均为
//! 进程级 `OnceLock`——桥联动断言集中在单个测试内串行
//! （与 `tests/core_loading.rs` 同约定）。
//!
//! ## 薄静态的测试纪律
//!
//! `core::init` 是进程级 `OnceLock`，cargo 并行跑测试时谁先注册谁生效。
//! 因此所有依赖薄静态的断言（video_handler / present_frame /
//! set_fast_forward / set_frontend / set_core_aspect / set_pre_render_hook）
//! 全部集中在单个测试 `video_pipeline_and_static_lifecycle` 内串行执行，
//! 与 `core_loading.rs::callback_bridge_lifecycle` 同约定。本文件其余测试
//! （布局、limit_ff、会话）不触碰薄静态，可安全并行。

mod test_common;

/// 构造 160×144 全 `pixel` 帧（pitch 320：每行前 160 像素为 pixel，其余 0）
fn frame_160(pixel: u16) -> Vec<u16> {
    let mut f = vec![0u16; 320 * 144];
    for row in 0..144 {
        f[row * 320..row * 320 + 160].fill(pixel);
    }
    f
}

/// 构造 256×240 全 `pixel` 帧（pitch 256）
fn frame_256(pixel: u16) -> Vec<u16> {
    vec![pixel; 256 * 240]
}

use std::ffi::CString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

use common::video::Rect;
use minarch::core::{
    CoreSession, Scaling, SessionError, Sharpness, compute_layout, ff_frame_budget, limit_ff,
    set_core_aspect, set_fast_forward, set_frontend, set_pre_render_hook, video_handler,
};
use minarch::game::Game;
use minarch::libretro::{Core, RETRO_MEMORY_SAVE_RAM};

/// 构造矩形辅助（x/y/w/h 全 u32）
fn rect(x: u32, y: u32, w: u32, h: u32) -> Rect {
    Rect { x, y, w, h }
}

/// 用 mock 核心编译参数开一个会话（tmp_dir 为临时存档目录；id 必须唯一，
/// 避免并行测试编译同一产物互相覆盖）
fn open_session(id: &str, tmp_dir: &str, defines: &[&str]) -> (CoreSession, Core) {
    let lib = test_common::build_mock_core(id, defines);
    let core = Core::open(
        lib.to_str().unwrap(),
        "TST",
        "/cfg",
        "/states",
        tmp_dir,
        "/bios",
    )
    .expect("mock 核心应加载成功");
    (CoreSession::new(), core)
}

/// 构造一个测试用 Game（name 用于存档文件名）
fn mock_game(tmp_dir: &str) -> Game {
    Game {
        path: format!("{tmp_dir}/game.gb"),
        name: "game.gb".to_string(),
        m3u_path: None,
        tmp_path: None,
        data: Some(vec![0xAA; 4]),
    }
}

/// 临时目录辅助（每次调用唯一，避免并行测试冲突）
fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("minarch_core_{tag}_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

// ═══════════════════════════════════════════════════════════════
// 任务 3.1：compute_layout 布局组（纯函数，表驱动）
// ═══════════════════════════════════════════════════════════════

#[test]
fn layout_native_integer_centered() {
    let l = compute_layout(160, 144, 10.0 / 7.0, 1280, 720, Scaling::Native);

    assert_eq!(l.scale, 5, "MIN(1280/160, 720/144)");
    assert_eq!(l.aspect, 0.0);
    assert_eq!(l.src, rect(0, 0, 160, 144));
    assert_eq!(l.dst, rect(240, 0, 800, 720), "整倍居中");
}

#[test]
fn layout_cropped_symmetric() {
    let l = compute_layout(256, 240, 4.0 / 3.0, 1024, 768, Scaling::Cropped);

    assert_eq!(l.scale, 4, "MIN(CEIL_DIV(1024,256), CEIL_DIV(768,240))");
    assert_eq!(l.src, rect(0, 24, 256, 192), "垂直双侧对称裁剪");
    assert_eq!(l.dst, rect(0, 0, 1024, 768), "整屏");
}

#[test]
fn layout_forced_crop_source_bigger_than_screen() {
    let l = compute_layout(1920, 1080, 16.0 / 9.0, 1280, 720, Scaling::Native);

    assert_eq!(l.scale, 0, "MIN(1280/1920, 720/1080) == 0");
    assert_eq!(l.dst, rect(0, 0, 1280, 720));
    assert_eq!(l.src, rect(320, 180, 1280, 720), "负偏移转为正 src 起点");
}

#[test]
fn layout_fullscreen_stretch() {
    let l = compute_layout(320, 240, 4.0 / 3.0, 1280, 720, Scaling::Fullscreen);

    assert_eq!(l.scale, -1);
    assert_eq!(l.aspect, -1.0);
    assert_eq!(l.src, rect(0, 0, 320, 240), "整帧");
    assert_eq!(l.dst, rect(0, 0, 1280, 720), "整屏拉伸");
}

#[test]
fn layout_aspect_letterbox_and_pillarbox() {
    // 4:3 内容在 16:9 屏 → 信箱
    let l = compute_layout(256, 224, 4.0 / 3.0, 1280, 720, Scaling::Aspect);
    assert_eq!(l.aspect, 4.0f32 / 3.0);
    assert_eq!(l.scale, -1);
    assert_eq!(l.dst, rect(160, 0, 960, 720), "dst_h=720, dst_w=960 居中");

    // 16:9 内容在 16:9 屏 → 整屏（柱箱退化）
    let l = compute_layout(256, 224, 16.0 / 9.0, 1280, 720, Scaling::Aspect);
    assert_eq!(l.dst, rect(0, 0, 1280, 720));
}

#[test]
fn layout_odd_resolution_no_snap_to_8() {
    // PS1 奇高度 239：朴素 CEIL_DIV，不移植 C 的 %8 对齐
    let l = compute_layout(320, 239, 4.0 / 3.0, 1280, 720, Scaling::Cropped);

    assert_eq!(l.scale, 4);
    // src_y = 118/4 = 29（朴素整数除）、src_h = 239 - 58 = 181
    assert_eq!(l.src, rect(0, 29, 320, 181));
    // dst 与屏幕求交：朴素 724 高被收窄到 720
    assert_eq!(l.dst, rect(0, 0, 1280, 720));
    assert!(l.dst.w <= 1280 && l.dst.h <= 720, "dst 不得超出屏幕");
}

#[test]
fn layout_forced_crop_on_brick_screen() {
    // 屏 1024×768 的 forced crop 边界
    let l = compute_layout(2048, 1024, 2.0, 1024, 768, Scaling::Native);

    assert_eq!(l.scale, 0);
    assert_eq!(l.src, rect(512, 128, 1024, 768));
    assert_eq!(l.dst, rect(0, 0, 1024, 768));
}

// ═══════════════════════════════════════════════════════════════
// 任务 4.1：会话生命周期组（经 mock 核心端到端）
// ═══════════════════════════════════════════════════════════════

#[test]
fn session_load_fills_cache_and_reads_sram() {
    let tmp = tmp_dir("sess_ok");
    let tmp_str = tmp.to_str().unwrap().to_string();

    // 预置 .sav（16 字节 = mock SAVE_RAM 长度）
    let seed: Vec<u8> = (0u8..16).collect();
    std::fs::write(tmp.join("game.gb.sav"), &seed).unwrap();

    let (mut session, mut core) = open_session("sess_ok", &tmp_str, &[]);
    session.init(&core);
    let game = mock_game(&tmp_str);
    let result = session.load(&mut core, &game);
    assert!(result.is_ok(), "load 应成功: {result:?}");

    assert_eq!(core.fps, 60.0);
    assert_eq!(core.sample_rate, 44100.0);
    assert!(
        (core.aspect_ratio - 160.0 / 144.0).abs() < 1e-6,
        "aspect 应取核心上报值（f32→f64，允许 ±1e-6）"
    );

    // 端到端：核心 SAVE_RAM 内存 == 预置 .sav 内容（read_into 接线）
    let size = unsafe { (core.get_memory_size)(RETRO_MEMORY_SAVE_RAM) };
    let ptr = unsafe { (core.get_memory_data)(RETRO_MEMORY_SAVE_RAM) };
    let mem = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), size) };
    assert_eq!(mem, seed.as_slice(), "读档应灌入核心 SAVE_RAM");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn session_load_game_fail_propagates() {
    let tmp = tmp_dir("sess_fail");
    let tmp_str = tmp.to_str().unwrap().to_string();

    let (mut session, mut core) = open_session("sess_fail", &tmp_str, &["MOCK_LOAD_GAME_FAIL"]);
    session.init(&core);
    let game = mock_game(&tmp_str);
    let result = session.load(&mut core, &game);
    assert!(matches!(result, Err(SessionError::GameLoadFailed)));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn session_zero_aspect_falls_back_to_base_ratio() {
    let tmp = tmp_dir("sess_aspect");
    let tmp_str = tmp.to_str().unwrap().to_string();

    let (mut session, mut core) = open_session("sess_aspect", &tmp_str, &["MOCK_ZERO_ASPECT"]);
    session.init(&core);
    let game = mock_game(&tmp_str);
    let result = session.load(&mut core, &game);
    assert!(result.is_ok());
    assert!(
        (core.aspect_ratio - 160.0 / 144.0).abs() < 1e-6,
        "0 aspect 应回落 base_w/base_h"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn session_quit_writes_sram_and_rtc_and_unloads() {
    let tmp = tmp_dir("sess_quit");
    let tmp_str = tmp.to_str().unwrap().to_string();

    let (mut session, mut core) = open_session("sess_quit", &tmp_str, &["MOCK_TRACK_CALLS"]);
    session.init(&core);
    let game = mock_game(&tmp_str);
    session.load(&mut core, &game).unwrap();

    // 改核心 SAVE_RAM 内存，quit 后应原样落盘
    let size = unsafe { (core.get_memory_size)(RETRO_MEMORY_SAVE_RAM) };
    let ptr = unsafe { (core.get_memory_data)(RETRO_MEMORY_SAVE_RAM) };
    let mem = unsafe { std::slice::from_raw_parts_mut(ptr.cast::<u8>(), size) };
    mem.fill(0x5A);

    session.quit(&core, &game).unwrap();

    let written = std::fs::read(tmp.join("game.gb.sav")).unwrap();
    assert_eq!(written, vec![0x5A; 16], "SRAM 写档应含核心内存内容");
    assert!(tmp.join("game.gb.rtc").exists(), "RTC 文件应生成");

    // MOCK_TRACK_CALLS：从**同一个** dlopen 实例查询计数（mock 的计数
    // 静态在库的数据段，重新 dlopen 会得到一份新的）
    let count: libloading::Symbol<unsafe extern "C" fn(*const std::ffi::c_char) -> u32> =
        unsafe { core.handle.get(b"mock_call_count") }.unwrap();
    let name = CString::new("retro_unload_game").unwrap();
    assert_eq!(
        unsafe { count(name.as_ptr()) },
        1,
        "unload_game 应被调用一次"
    );
    let name = CString::new("retro_deinit").unwrap();
    assert_eq!(unsafe { count(name.as_ptr()) }, 1, "deinit 应被调用一次");
    let name = CString::new("retro_set_controller_port_device").unwrap();
    assert_eq!(
        unsafe { count(name.as_ptr()) },
        1,
        "load 时 controller 应设一次"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn session_quit_uninitialized_is_noop() {
    let tmp = tmp_dir("sess_noop");
    let tmp_str = tmp.to_str().unwrap().to_string();

    let (mut session, core) = open_session("sess_noop", &tmp_str, &["MOCK_TRACK_CALLS"]);
    let game = mock_game(&tmp_str);
    let result = session.quit(&core, &game);
    assert!(result.is_ok(), "未 init quit 应为空操作 Ok");

    // 无文件写入
    assert!(!tmp.join("game.gb.sav").exists());
    assert!(!tmp.join("game.gb.rtc").exists());

    let _ = std::fs::remove_dir_all(&tmp);
}

// ═══════════════════════════════════════════════════════════════
// 任务 5.1：limit_ff 快进帧预算组（纯函数，无 FFI）
// ═══════════════════════════════════════════════════════════════

#[test]
fn ff_budget_and_limit() {
    // 预算计算
    assert_eq!(ff_frame_budget(60.0, 3), 4166, "1000000/240");

    // 快进中延迟推进：delay=(4166-1000)/1000=3, next=last+budget
    let (delay, next) = limit_ff(1_001_000, 1_000_000, 4166, true, 3);
    assert_eq!(delay, 3);
    assert_eq!(next, 1_004_166);

    // 16ms 封顶：budget=20000、elapsed=1000 → delay 应封顶 16
    let (delay, _) = limit_ff(1_001_000, 1_000_000, 20_000, true, 3);
    assert_eq!(delay, 16, "delay 不得超过一帧");

    // 直通条件：非快进 / max_speed==0
    assert_eq!(limit_ff(5, 1, 4166, false, 3), (0, 5));
    assert_eq!(limit_ff(5, 1, 4166, true, 0), (0, 5));

    // 时钟异常（elapsed 超大）→ 重置
    let (delay, next) = limit_ff(0x100000, 1, 4166, true, 3);
    assert_eq!(delay, 0);
    assert_eq!(next, 0x100000);
}

// ═══════════════════════════════════════════════════════════════
// 任务 5.1 + 6.1：薄静态层 + 视频处理器 + Special 钩子（单个串行测试）
// ═══════════════════════════════════════════════════════════════
//
// `core::init` 是进程级 OnceLock——本测试是本文件唯一调用它的测试，
// 保证首次注册成功；其余断言按「注册 → 消费 → 节流 → 变化 → 钩子」
// 顺序串行走完，与 callback_bridge_lifecycle 同约定。
//
// 屏幕取 320×288：mock 帧 160×144 恰好整数 2 倍 → dst 整屏 {0,0,320,288}，
// 断言锚点简单（源左上角像素出现在屏幕左上角 2×2 区域）。

#[test]
fn video_pipeline_and_static_lifecycle() {
    // ── 注册：首次 init 成功，重复 init 报错并携带第二次传入的原值 ──
    let first = minarch::core::init(320, 288);
    assert!(first.is_ok(), "本测试是本文件唯一 init 调用方，首次应成功");
    let second = minarch::core::init(640, 480);
    match second {
        Err(state) => {
            assert_eq!(
                state.buffer.width, 640,
                "错误应携带第二次传入的 VideoState 原值"
            );
            assert_eq!(state.buffer.height, 480);
        }
        Ok(()) => panic!("重复 init 应返回错误"),
    }

    // ── 快进标志读写一致 ──
    set_fast_forward(true);
    assert!(minarch::core::fast_forward(), "置位后应读回 true");
    set_fast_forward(false);
    assert!(!minarch::core::fast_forward(), "复位后应读回 false");

    // ── 帧缩放进 pending + present_frame 一次性消费 ──
    let frame = frame_160(0x1234);
    video_handler(frame.as_ptr().cast(), 160, 144, 320);

    let mut seen = 0usize;
    let first_present = minarch::core::present_frame(|buf| {
        seen = buf.pixels.len();
        // 源左上角 0x1234 应出现在 dst 左上角 2×2 区域。中心对齐反查
        // (0+0.5)×160/320−0.5 = −0.25 → 钳制到源原点、权重 100%——
        // 软锐度双线性下该区域为纯 0x1234
        assert_eq!(buf.pixels[0], 0x1234, "dst 左上角应为源左上角像素");
        assert_eq!(buf.pixels[1], 0x1234);
        assert_eq!(buf.pixels[320], 0x1234);
        assert_eq!(buf.pixels[321], 0x1234);
    });
    assert!(first_present, "处理后应有 pending 帧");
    assert_eq!(seen, 320 * 288, "闭包应收到 pending 帧引用");

    let mut called = false;
    let second_present = minarch::core::present_frame(|_| called = true);
    assert!(!second_present, "pending 清空后第二次应返回 false");
    assert!(!called, "第二次闭包不应执行");

    // ── 快进 10ms 节流：上一帧刚写 pending（<10ms）→ 直接跳过 ──
    set_fast_forward(true);
    let f_throttle = frame_160(0xFFFF);
    video_handler(f_throttle.as_ptr().cast(), 160, 144, 320);
    let mut throttled = false;
    let present = minarch::core::present_frame(|_| throttled = true);
    assert!(present, "节流窗口外的第一帧应正常写 pending");
    assert!(throttled);
    // 立即第二帧：距上次写 pending <10ms → 无新 pending
    video_handler(f_throttle.as_ptr().cast(), 160, 144, 320);
    let mut skipped = false;
    let present = minarch::core::present_frame(|_| skipped = true);
    assert!(!present, "节流应跳过处理：无新 pending");
    assert!(!skipped, "节流帧的闭包不应执行");
    set_fast_forward(false);

    // ── 尺寸变化：重算布局并清黑（旧位置残留不可见）──
    let f_small = frame_160(0x1234);
    video_handler(f_small.as_ptr().cast(), 160, 144, 320);
    minarch::core::present_frame(|_| {});

    let f_big = frame_256(0xFFFF);
    video_handler(f_big.as_ptr().cast(), 256, 240, 256);
    let mut changed = None;
    let present = minarch::core::present_frame(|buf| {
        // 新布局 Native scale=1、dst {32,24,256,240}：旧 2×2 区域清黑，
        // 新 dst 左上角有像素
        assert_eq!(buf.pixels[0], 0x0000, "旧位置应清黑");
        changed = Some((buf.pixels[32 + 24 * 320], buf.pixels[32 + 24 * 320 + 1]));
    });
    assert!(present, "尺寸变化后应产出新帧");
    assert_eq!(changed, Some((0xFFFF, 0xFFFF)), "新 dst 左上角应有像素");

    // ── 空帧安全返回：pending 与节流时钟不变 ──
    video_handler(std::ptr::null(), 160, 144, 320);
    let mut null_called = false;
    let present = minarch::core::present_frame(|_| null_called = true);
    assert!(!present, "空帧不应置 pending");
    assert!(!null_called);

    // ── 前端选项与宽高比同步 ──
    set_frontend(Scaling::Cropped, Sharpness::Crisp);
    set_core_aspect(4.0 / 3.0);
    let f_sync = frame_160(0xFFFF);
    video_handler(f_sync.as_ptr().cast(), 160, 144, 320);
    let mut synced = false;
    let present = minarch::core::present_frame(|buf| {
        // Cropped 160×144 → 320×288：scale=2、dst 整屏；crisp → IntegerScaler
        assert_eq!(buf.pixels[0], 0xFFFF, "crisp+整数模式应走最近邻");
        synced = true;
    });
    assert!(present, "同步后 video_handler 应产出 pending 帧");
    assert!(synced);

    // ── Special 预渲染钩子：先于节流执行、重复注册报错 ──
    static HOOK_CALLS: AtomicUsize = AtomicUsize::new(0);
    fn hook() {
        HOOK_CALLS.fetch_add(1, AtomicOrdering::SeqCst);
    }

    assert!(set_pre_render_hook(hook).is_ok(), "首次注册应成功");
    assert!(
        set_pre_render_hook(hook).is_err(),
        "重复注册应返回错误并携带原值"
    );

    // 节流生效条件成立（快进 + 上一帧 <10ms）：钩子仍执行（C :2776 先于节流）
    set_fast_forward(true);
    let f_hook = frame_160(0x1234);
    video_handler(f_hook.as_ptr().cast(), 160, 144, 320);
    let n = HOOK_CALLS.load(AtomicOrdering::SeqCst);
    assert!(n >= 1, "钩子应执行（先于节流 return）");
    let mut hook_skipped = false;
    let present = minarch::core::present_frame(|_| hook_skipped = true);
    assert!(!present, "节流仍生效：pending 不应变化");
    assert!(!hook_skipped);
    set_fast_forward(false);

    // ── limit_ff_now：快进时钟共享存储（design 决策 4）──
    // 首次调用：ff_last_us 初始 0 → limit_ff 重置为 now、delay 0
    let (delay, need_sleep) = minarch::core::limit_ff_now(4166, 3);
    assert_eq!(delay, 0, "时钟未初始化时首帧直通");
    assert!(!need_sleep);

    // 置位快进后连续两帧：第二次调用共享同一时钟累计推进
    set_fast_forward(true);
    let (delay1, need1) = minarch::core::limit_ff_now(4166, 3);
    assert_eq!(delay1, 0, "时钟刚重置（last==now）不延迟");
    assert!(!need1);
    // 紧接调用（elapsed≈0）→ 直通条件，不产生延迟
    let (delay2, need2) = minarch::core::limit_ff_now(4166, 3);
    assert_eq!(delay2, 0);
    assert!(!need2);
    set_fast_forward(false);

    // 直通条件：非快进 → (0,false)
    let (delay3, need3) = minarch::core::limit_ff_now(4166, 3);
    assert_eq!(delay3, 0);
    assert!(!need3);
    // max_speed == 0 → (0,false)
    set_fast_forward(true);
    let (delay4, need4) = minarch::core::limit_ff_now(4166, 0);
    assert_eq!(delay4, 0);
    assert!(!need4);
    set_fast_forward(false);
}
