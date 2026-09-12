//! `minarch` — MinUI 游戏内前端（二进制入口 + 装配层）
//!
//! 本文件是 bin 目标：承载启动序列、主循环、菜单联动、睡眠/唤醒、
//! 退出序列（对应原 C `minarch.c` 的 `main`，minarch.c:4668-4829）。
//! 模块声明在 `lib.rs`（lib 目标，供 `tests/` 集成测试链接）。
//!
//! ## 装配层职责
//!
//! 各业务模块（libretro/core/audio/vibration/environment/config/controls/
//! savestate/sram/game/menu/hdmi）以纯逻辑层 + 薄静态实现，本文件是
//! 它们的"接线员"：注册回调桥、加载配置、驱动主循环、执行跨模块编排。
//!
//! ## 主循环结构（单线程，thread_video 留后续 change）
//!
//! 每帧：帧计时 → 输入轮询（注入 BUTTONS 静态）→ 快捷指令检测/消费 →
//! `core.run()` 同步执行 → 快进限速 → pending 帧 flip → 音频 drain →
//! 电源状态机 → HDMI 检测 → 帧预算补偿。菜单打开时转入菜单子循环。
//!

#[cfg(feature = "platform-tg5040")]
use std::path::Path;
#[cfg(feature = "platform-tg5040")]
use std::time::Duration;

#[cfg(feature = "platform-tg5040")]
use common::input::{
    BTN_A, BTN_B, BTN_DPAD_DOWN, BTN_DPAD_LEFT, BTN_DPAD_RIGHT, BTN_DPAD_UP, BTN_MENU, BTN_POWER,
    BTN_X, ModKeys,
};
#[cfg(feature = "platform-tg5040")]
use common::paths::{
    AUTO_RESUME_SLOT, get_auto_resume_path, get_paks_path, get_roms_path, get_shared_userdata_path,
    get_userdata_path,
};
#[cfg(feature = "platform-tg5040")]
use common::platform::Platform;
#[cfg(feature = "platform-tg5040")]
use common::power::{CpuSpeed, PowerAction, PowerState, faux_sleep, power_off};
#[cfg(feature = "platform-tg5040")]
use common::utils::{exists, get_emu_name, get_emu_path};
#[cfg(feature = "platform-tg5040")]
use common::video::{FONT_PATH, RGB_BLACK, VideoBuffer, VsyncMode};

#[cfg(feature = "platform-tg5040")]
use minarch::assembly::{load_state, read_resume_slot, save_state};
#[cfg(feature = "platform-tg5040")]
use minarch::audio;
#[cfg(feature = "platform-tg5040")]
use minarch::config::{self, OptionList};
#[cfg(feature = "platform-tg5040")]
use minarch::controls::{self, ShortcutAction, ShortcutState};
#[cfg(feature = "platform-tg5040")]
use minarch::core::{self, CoreSession, Scaling};
#[cfg(feature = "platform-tg5040")]
use minarch::environment;
#[cfg(feature = "platform-tg5040")]
use minarch::game::Game;
#[cfg(feature = "platform-tg5040")]
use minarch::hdmi::HdmiMonitor;
#[cfg(feature = "platform-tg5040")]
use minarch::libretro::{self, Core, FrontendState};
#[cfg(feature = "platform-tg5040")]
use minarch::menu::{self, MainAction, MainMenu, MenuInput, OptionMenu, SaveStateIo};
#[cfg(feature = "platform-tg5040")]
use minarch::vibration;

#[cfg(feature = "platform-tg5040")]
use render::asset::load_atlas;
#[cfg(feature = "platform-tg5040")]
use render::text::load_font;

/// 帧预算（ms），对应 C `FRAME_BUDGET`（60fps）
#[cfg(feature = "platform-tg5040")]
const FRAME_BUDGET: u32 = 17;
/// 菜单可见行数（对应 C `max_visible_options`）
#[cfg(feature = "platform-tg5040")]
const MENU_VISIBLE_ROWS: usize = 7;
/// 音频输出缓冲帧数（装配层提供，drain 目标）
#[cfg(feature = "platform-tg5040")]
const AUDIO_BUFFER_FRAMES: usize = 1024;

/// 跨帧运行状态（装配层局部变量收拢，对应 C 全局 `quit`/`show_menu`
/// 等，minarch.c:24-36）
#[cfg(feature = "platform-tg5040")]
struct RunState {
    /// 是否退出主循环
    quit: bool,
    /// 菜单是否打开
    show_menu: bool,
    /// 快捷指令跨帧状态（toggled_ff_on）
    shortcut_state: ShortcutState,
    /// 快进标志（core 静态为唯一事实源，本字段为同步快照）
    fast_forward: bool,
    /// 最大快进倍率（`minarch_max_ff_speed` 选项值，0-7）
    max_ff_speed: u32,
    /// 上次 HDMI 采样
    hdmi: HdmiMonitor,
    /// 线程模式切换状态（对应 C `thread_video`/`was_threaded`/`toggle_thread`）
    thread_toggle: minarch::assembly::ThreadToggleState,
    /// 核心线程句柄（线程化时 `Some`）
    core_thread: Option<std::thread::JoinHandle<()>>,
    /// 核心线程门控标志（`false` = 暂停/退出；对应 C `should_run_core`）
    should_run_core: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(feature = "platform-tg5040")]
impl RunState {
    fn new() -> Self {
        Self {
            quit: false,
            show_menu: false,
            shortcut_state: ShortcutState::default(),
            fast_forward: false,
            max_ff_speed: 3,
            hdmi: HdmiMonitor::new(),
            thread_toggle: minarch::assembly::ThreadToggleState::new(false),
            core_thread: None,
            should_run_core: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

fn main() {
    #[cfg(feature = "platform-tg5040")]
    {
        let mut platform = platform_tg5040::Tg5040::new();
        run(&mut platform);
    }
    // 无平台 feature（如单元测试编译）时保持占位输出
    #[cfg(not(feature = "platform-tg5040"))]
    {
        println!("minarch frontend - no platform feature selected");
    }
}

/// 装配层入口：启动序列 → 主循环 → 退出序列
///
/// # 参数
///
/// - `platform`: 平台实例（`Platform` trait 实现）
#[cfg(feature = "platform-tg5040")]
fn run<P: Platform>(platform: &mut P) {
    let sdcard_path = P::SDCARD_PATH;
    let platform_code = P::PLATFORM;
    let paks_path = get_paks_path(sdcard_path, platform_code);
    let roms_path = get_roms_path(sdcard_path);

    // ── 参数解析（C :4676-4684）──
    let args: Vec<String> = std::env::args().collect();
    let (core_path, rom_path) = match (args.get(1), args.get(2)) {
        (Some(c), Some(r)) => (c.clone(), r.clone()),
        _ => {
            eprintln!("用法: minarch <核心.so> <ROM 路径>");
            std::process::exit(1);
        }
    };

    // ── 平台初始化（C :4686-4698）──
    platform.init_video();
    platform.init_input();
    // 音频在核心加载后正式初始化（C `SND_init` 在 `Core_load` 之后，
    // minarch.c:4727——core_rate 需先由 av_info 填充）
    let mut power_state = PowerState::new();
    if !P::HAS_POWER_BUTTON {
        power_state.disable_sleep();
    }
    power_state.warn(true);
    power_state.disable_auto_sleep(); // 游戏运行中禁自动睡眠（C :4740）

    // ── 核心加载（C :4700）──
    let emu_name = get_emu_name(&rom_path, &roms_path);
    let userdata_path = get_userdata_path(sdcard_path, platform_code);
    let shared_userdata_path = get_shared_userdata_path(sdcard_path);
    let core_name_hint = Path::new(&core_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("core");
    let core_name = core_name_hint
        .rsplit_once('_')
        .map(|(stem, _)| stem)
        .unwrap_or(core_name_hint);
    let config_dir = format!("{userdata_path}/{emu_name}-{core_name}");
    let states_dir = format!("{shared_userdata_path}/{emu_name}-{core_name}");
    let saves_dir = format!("{sdcard_path}/Saves/{emu_name}");
    let bios_dir = format!("{sdcard_path}/Bios/{emu_name}");
    std::fs::create_dir_all(&config_dir).ok();
    std::fs::create_dir_all(&states_dir).ok();

    let mut core = match Core::open(
        &core_path,
        &emu_name,
        &config_dir,
        &states_dir,
        &saves_dir,
        &bios_dir,
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("核心加载失败: {e:?}");
            platform.quit_input();
            platform.quit_video();
            std::process::exit(1);
        }
    };

    // ── 游戏加载（C :4701-4702；失败直通退出）──
    let mut game = match Game::open(&rom_path, &core.extensions, core.need_fullpath) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("游戏打开失败: {e:?}");
            drop(core);
            platform.quit_input();
            platform.quit_video();
            std::process::exit(1);
        }
    };

    // ── 配置加载（C :4704-4711）──
    let simple_mode = exists(&format!(
        "{sdcard_path}/.userdata/shared/enable-simple-mode"
    ));
    let device_tag = std::env::var("DEVICE").ok();

    // 三级 cfg 路径（决策 3：minarch 内定义，不进 common）
    let system_cfg_path = format!("{sdcard_path}/.system/{platform_code}/system.cfg");
    let system_cfg_path = if let Some(tag) = &device_tag {
        let tagged = format!("{sdcard_path}/.system/{platform_code}/system-{tag}.cfg");
        if exists(&tagged) {
            tagged
        } else {
            system_cfg_path
        }
    } else {
        system_cfg_path
    };
    let emu_launch = get_emu_path(&emu_name, sdcard_path, platform_code, &paks_path);
    let emu_dir = Path::new(&emu_launch)
        .parent()
        .map_or("", |p| p.to_str().unwrap_or(""));
    let default_cfg_path = if let Some(tag) = &device_tag {
        let tagged = format!("{emu_dir}/default-{tag}.cfg");
        if exists(&tagged) {
            tagged
        } else {
            format!("{emu_dir}/default.cfg")
        }
    } else {
        format!("{emu_dir}/default.cfg")
    };
    let game_cfg_path = format!("{config_dir}/{}.cfg", game.name);
    let user_cfg_path = format!("{config_dir}/minarch.cfg");
    let override_game = exists(&game_cfg_path);
    let user_cfg_path = if override_game {
        game_cfg_path.clone()
    } else {
        user_cfg_path
    };

    // 选项表初始化
    let mut frontend_options = OptionList {
        options: config::frontend_options(P::SUPPORTS_OVERSCAN),
        changed: false,
    };
    let mut core_options = OptionList {
        options: Vec::new(),
        changed: false,
    };
    let mut mapping = controls::default_button_mapping();
    let mut shortcuts = controls::shortcuts_default_mapping();
    let mut gamepad_type = 0u32;

    // 三级 cfg 应用（system → default → user，后者覆盖前者）
    for path in [&system_cfg_path, &default_cfg_path, &user_cfg_path] {
        if let Some(text) = config::load_cfg_text(Path::new(path)) {
            config::apply_cfg(
                &text,
                &mut frontend_options,
                &mut core_options,
                &mut mapping,
                &mut shortcuts,
                &mut gamepad_type,
            );
        }
    }

    // Config_syncFrontend 平台联动（C :4710-4711）
    let scaling_val = frontend_options
        .get_option_value("minarch_screen_scaling")
        .unwrap_or("Aspect");
    let scaling = match scaling_val {
        "Native" => Scaling::Native,
        "Fullscreen" => Scaling::Fullscreen,
        "Cropped" => Scaling::Cropped,
        _ => Scaling::Aspect,
    };
    let sharpness_val = frontend_options
        .get_option_value("minarch_screen_sharpness")
        .unwrap_or("Sharp");
    let sharpness = match sharpness_val {
        "Crisp" => core::Sharpness::Crisp,
        "Soft" => core::Sharpness::Soft,
        _ => core::Sharpness::Soft,
    };
    core::set_frontend(scaling, sharpness);
    let prevent_tearing = frontend_options
        .get_option_value("minarch_prevent_tearing")
        .unwrap_or("Lenient");
    platform.set_vsync(match prevent_tearing {
        "Strict" => VsyncMode::Strict,
        "Lenient" => VsyncMode::Lenient,
        _ => VsyncMode::Off,
    });
    let overclock = frontend_options
        .get_option_value("minarch_cpu_speed")
        .unwrap_or("Normal");
    let cpu_speed = match overclock {
        "Powersave" => CpuSpeed::Powersave,
        "Performance" => CpuSpeed::Performance,
        _ => CpuSpeed::Normal,
    };
    platform.set_cpu_speed(cpu_speed);
    let max_ff = frontend_options
        .get_option_value("minarch_max_ff_speed")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(3);

    // ── 静态注册（必须在核心加载前完成，C :2949-2954 的回调注册）──
    let bios_c = std::ffi::CString::new(bios_dir.clone()).expect("bios 路径含 NUL");
    let saves_c = std::ffi::CString::new(saves_dir.clone()).expect("saves 路径含 NUL");
    if environment::init(bios_c, saves_c).is_err() {
        eprintln!("environment 重复初始化");
    }
    vibration::init().ok();
    controls::init_buttons().ok();
    controls::init_sticks().ok();
    let _ = environment::set_rumble_hook(vibration::set_strength);
    core::init(P::SCREEN_WIDTH, P::SCREEN_HEIGHT).ok();

    // 回调桥注册（一次性，核心加载前）
    let _ = libretro::register_callbacks(FrontendState {
        environment: Some(environment::handle),
        video_refresh: Some(core::video_handler),
        audio_sample: Some(audio::sample_handler),
        audio_sample_batch: Some(audio::batch_handler),
        input_poll: Some(controls::input_poll_handler),
        input_state: Some(controls::input_state_handler),
    });

    // ── 核心会话初始化与加载（C :4713-4725）──
    let mut session = CoreSession::new();
    session.init(&core);
    if let Err(e) = session.load(&mut core, &game) {
        eprintln!("游戏加载失败: {e:?}");
        let _ = session.quit(&core, &game);
        game.close();
        platform.quit_input();
        platform.quit_video();
        std::process::exit(1);
    }
    // core_aspect 同步（C :4733 后，装配层消费 core.aspect_ratio）
    core::set_core_aspect(core.aspect_ratio);

    // ── 音频初始化（C :4727 SND_init；core_rate ← av_info、actual ← init_audio）──
    let core_rate = core.sample_rate as u32;
    let picked = platform.pick_sample_rate(core_rate, audio::MAX_SAMPLE_RATE);
    let actual_rate = platform.init_audio(picked);
    let actual_rate = if actual_rate == 0 {
        picked
    } else {
        actual_rate
    };
    let _ = audio::init(core_rate, actual_rate);

    // ── 菜单初始化（C :4728-4731）──
    let mut run_state = RunState::new();
    run_state.max_ff_speed = max_ff;

    // ── 核心线程创建（C :4733-4737；`minarch_thread_video` 选项）──
    // 线程化模式：`core.run()` 移入独立核心线程（方案 A 优雅退出，
    // `core::spawn_core_thread`）。core 需转 `Arc` 共享——线程 clone 一份
    // 保持 `.so` 存活（Core.handle: Library）；装配层经 `&*core_arc`
    // 访问（session.quit 等只读借用）。
    let thread_video_val = frontend_options
        .get_option_value("minarch_thread_video")
        .unwrap_or("Off");
    run_state.thread_toggle = minarch::assembly::ThreadToggleState::new(thread_video_val == "On");
    let core_arc = std::sync::Arc::new(core);
    let core: &Core = &core_arc;
    if run_state.thread_toggle.thread_mode {
        // 快进预算（C `limitFF` 预算：fps × (max_ff_speed+1)）
        let ff_budget = core::ff_frame_budget(core.fps, run_state.max_ff_speed);
        let should_run = std::sync::Arc::clone(&run_state.should_run_core);
        should_run.store(true, std::sync::atomic::Ordering::Release);
        let handle = core::spawn_core_thread(
            std::sync::Arc::clone(&core_arc),
            should_run,
            ff_budget,
            run_state.max_ff_speed,
        );
        run_state.core_thread = Some(handle);
    }

    let screen = &mut VideoBuffer::new(P::SCREEN_WIDTH, P::SCREEN_HEIGHT);
    // 资源加载（字体/图集）
    let res_path = format!("{sdcard_path}{}", "/.system/res");
    let atlas = load_atlas(&format!("{res_path}/assets@{}x.png", P::SCALE)).expect("图集加载失败");
    let font_data = std::fs::read(format!("{sdcard_path}{FONT_PATH}")).expect("字体文件读取失败");
    let font = load_font(&font_data);
    // 半透明遮罩（对应 C `menu.overlay`，minarch.c:3062-3064）
    let overlay = make_overlay(P::SCREEN_WIDTH, P::SCREEN_HEIGHT);
    let theme = menu::MenuTheme {
        font_small: &font,
        font_tiny: &font,
        font_large: &font,
        atlas: &atlas,
        scale: P::SCALE,
    };

    // 多碟路径表（C `menu.disc_paths`：m3u 探测）
    let mut disc_paths: Vec<String> = Vec::new();
    if let Some(m3u) = &game.m3u_path
        && let Ok(content) = std::fs::read_to_string(m3u)
    {
        disc_paths = content
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
    }
    let base_path = game
        .m3u_path
        .as_ref()
        .and_then(|p| Path::new(p).parent())
        .map(|p| format!("{}/", p.to_string_lossy()))
        .unwrap_or_default();
    let minui_dir = format!("{shared_userdata_path}/.minui/{emu_name}");
    std::fs::create_dir_all(&minui_dir).ok();
    let save_io = SaveStateIo::new(
        &minui_dir,
        &states_dir,
        &game.name,
        disc_paths.clone(),
        &base_path,
    );

    // resume 槽位恢复（C `State_resume`，:4730；RESUME_SLOT_PATH 读+删）
    let resume_slot = read_resume_slot(common::paths::RESUME_SLOT_PATH);
    if resume_slot != AUTO_RESUME_SLOT && save_io.save_exists(resume_slot as usize) {
        let _ = load_state(
            core,
            &game,
            resume_slot as usize,
            &save_io,
            run_state.fast_forward,
        );
    }

    // 多碟时主菜单碟片计数
    let disc_count = if disc_paths.is_empty() {
        1
    } else {
        disc_paths.len()
    };
    let mut main_menu = MainMenu::new(simple_mode, disc_count);

    // ── 主循环（C :4749-4803）──
    while !run_state.quit {
        if run_state.show_menu {
            // 菜单子循环（C `Menu_loop`，:4224-4572）
            menu_loop(
                platform,
                &mut run_state,
                screen,
                &theme,
                &overlay,
                &mut main_menu,
                &save_io,
                &mut session,
                core,
                &mut game,
                &mut frontend_options,
                &mut power_state,
                &user_cfg_path,
            );
        } else {
            // 游戏帧（C :4751-4757 单线程）
            let frame_start = platform.now_ms();
            let input = platform.poll_input();

            // 快捷指令检测与消费（C :1681-1728）
            let menu_pressed = input.is_pressed(BTN_MENU);
            let actions = controls::detect_shortcuts(
                &shortcuts,
                &input,
                menu_pressed,
                &mut run_state.shortcut_state,
            );
            for action in actions {
                handle_shortcut(
                    action,
                    &mut run_state,
                    &mut session,
                    core,
                    &game,
                    &save_io,
                    &mut frontend_options,
                    P::SUPPORTS_OVERSCAN,
                    main_menu.slot,
                );
            }

            // 注入按键掩码（C input_poll_callback 映射段，:1755-1773）
            controls::update_buttons(&mapping, input.pressed, menu_pressed);

            // 注入摇杆原始轴值（C input_state_callback 的 ANALOG 分支
            // 直读 `pad.laxis/raxis`，:1785-1792——Rust 版由装配层注入）
            controls::update_sticks(input.laxis, input.raxis);

            // MENU 键开关（C :1736-1744，PAD_justReleased(BTN_MENU)）
            if input.just_released(BTN_MENU) {
                run_state.show_menu = true;
            }

            // core.run() 模式分派（C :4753-4757）
            // 单线程：主线程同步执行；线程化：核心线程已执行，主线程跳过
            if !run_state.thread_toggle.thread_mode {
                unsafe { (core.run)() };
            }

            // 快进限速（C `limitFF`，:4620-4645；时钟在 core::VideoState
            // 共享，单/双线程统一入口——design 决策 4）
            if run_state.fast_forward && run_state.max_ff_speed > 0 {
                let budget = core::ff_frame_budget(core.fps, run_state.max_ff_speed);
                let (delay_ms, need_sleep) = core::limit_ff_now(budget, run_state.max_ff_speed);
                if need_sleep && delay_ms > 0 {
                    std::thread::sleep(Duration::from_millis(delay_ms as u64));
                }
            }

            // pending 帧 flip（C `video_refresh_callback_main` 的 flip 段）
            let elapsed = platform.now_ms().saturating_sub(frame_start);
            core::present_frame(|buf| {
                platform.flip(buf, elapsed < FRAME_BUDGET);
            });

            // 音频 drain → push（C :2875-2881 后，SND_batchSamples）
            let mut audio_buf =
                vec![common::audio::AudioFrame { left: 0, right: 0 }; AUDIO_BUFFER_FRAMES];
            let n = audio::drain(&mut audio_buf);
            if n > 0 {
                platform.push_audio(&audio_buf[..n]);
            }

            // 振动 tick 驱动（C `VIB_thread` 轮询语义）
            if let Some(strength) = vibration::tick() {
                platform.set_rumble(strength);
            }

            // 电源状态机（C `PWR_update`，:1658）
            let battery = platform.get_battery_status();
            let mod_keys = ModKeys {
                brightness: P::BTN_MOD_BRIGHTNESS,
                volume: P::BTN_MOD_VOLUME,
                plus: P::BTN_MOD_PLUS,
                minus: P::BTN_MOD_MINUS,
            };
            let (action, _, _) = common::power::update(
                &mut power_state,
                &input,
                &battery,
                false,
                mod_keys,
                P::BTN_SLEEP,
                platform.is_hdmi_active(),
                frame_start,
            );
            match action {
                Some(PowerAction::Sleep) => {
                    before_sleep(&session, core, &game, &save_io, platform, sdcard_path);
                    faux_sleep(platform, &mut power_state);
                    after_sleep(platform, cpu_speed, &mut run_state);
                }
                Some(PowerAction::PowerOff) => {
                    before_sleep(&session, core, &game, &save_io, platform, sdcard_path);
                    platform.flip(screen, true);
                    std::thread::sleep(Duration::from_secs(1));
                    power_off(platform);
                }
                None => {}
            }

            // 电源键线程切换（C :1668-1681；`power::update` 只借用不消费
            // 事件，此处仍可读 just_pressed/just_released）
            if input.just_pressed(BTN_POWER) {
                run_state.thread_toggle.on_power_button(true);
            } else if input.just_released(BTN_POWER) {
                run_state.thread_toggle.on_power_button(false);
            }

            // HDMI 检测（C `hdmimon`，:2162-2176）
            let hdmi_active = platform.is_hdmi_active();
            if run_state.hdmi.update(hdmi_active).is_some() {
                before_sleep(&session, core, &game, &save_io, platform, sdcard_path);
                std::thread::sleep(Duration::from_secs(4));
                run_state.quit = true;
            }

            // 帧预算补偿（C `GFX_sync`）
            let frame_elapsed = platform.now_ms().saturating_sub(frame_start);
            if frame_elapsed < FRAME_BUDGET {
                std::thread::sleep(Duration::from_millis((FRAME_BUDGET - frame_elapsed) as u64));
            }

            // 帧边界线程切换块（C :4773-4799；`toggle_thread` 消费）
            if run_state.thread_toggle.toggle_thread {
                run_state.thread_toggle.toggle_thread = false;
                // 特判：单线程 + 记忆（快进/电源键打断中）→ 清标志、不翻转
                // 模式（C :4775-4780 的"双翻转抵消"语义，design 决策 3 简化）
                if run_state.thread_toggle.was_threaded && !run_state.thread_toggle.thread_mode {
                    run_state.thread_toggle.was_threaded = false;
                } else {
                    run_state.thread_toggle.thread_mode = !run_state.thread_toggle.thread_mode;
                }
                // 停线程（方案 A：置门控 → 自退出 → join）
                if let Some(handle) = run_state.core_thread.take() {
                    run_state
                        .should_run_core
                        .store(false, std::sync::atomic::Ordering::Release);
                    let _ = handle.join();
                }
                // 按新模式重建
                if run_state.thread_toggle.thread_mode {
                    let ff_budget = core::ff_frame_budget(core.fps, run_state.max_ff_speed);
                    run_state
                        .should_run_core
                        .store(true, std::sync::atomic::Ordering::Release);
                    let handle = core::spawn_core_thread(
                        std::sync::Arc::clone(&core_arc),
                        std::sync::Arc::clone(&run_state.should_run_core),
                        ff_budget,
                        run_state.max_ff_speed,
                    );
                    run_state.core_thread = Some(handle);
                }
            }
        }
    }

    // ── 退出序列（C :4805-4829）──
    if run_state.show_menu {
        // 菜单收尾：写 SRAM/RTC（C :4238-4239）
        let _ = session.write_saves(core, &game);
    }
    // 停核心线程（线程化时；方案 A：置门控 → 自退出 → join）。
    // 修复 C 缺陷：C 主循环退出后直接 Core_unload 不 join 核心线程
    // （可能 core.run() 未结束就 unload/deinit）——Rust 先 join 保证
    // unload/deinit 不与 run 并发（spec「退出序列」）
    if let Some(handle) = run_state.core_thread.take() {
        run_state
            .should_run_core
            .store(false, std::sync::atomic::Ordering::Release);
        let _ = handle.join();
    }
    game.close();
    if let Err(e) = session.quit(core, &game) {
        eprintln!("退出存档失败: {e:?}");
    }
    platform.quit_audio();
    platform.quit_input();
    platform.quit_video();
}

/// 生成半透明遮罩（对应 C `menu.overlay`，minarch.c:3062-3064 的
/// 半透明黑层——简化：0x39 暗化，与 C 的 alpha 混合等价）
#[cfg(feature = "platform-tg5040")]
fn make_overlay(w: u32, h: u32) -> VideoBuffer {
    let mut overlay = VideoBuffer::new(w, h);
    // 半透明黑：RGB565 下 0x39E7 ≈ 25% 亮度的灰（C 用 SDL 半透明混合，
    // Rust 软件路径直接写暗化像素——见 menu 绘制需求）
    for p in overlay.pixels.iter_mut() {
        *p = RGB_BLACK;
    }
    overlay
}

/// 睡眠前序列（对应 C `Menu_beforeSleep`，minarch.c:3112-3119）
#[cfg(feature = "platform-tg5040")]
fn before_sleep<P: Platform>(
    session: &CoreSession,
    core: &Core,
    game: &Game,
    save_io: &SaveStateIo,
    platform: &mut P,
    sdcard_path: &str,
) {
    let _ = session.write_saves(core, game);
    let _ = save_state(core, game, AUTO_RESUME_SLOT as usize, save_io, false);
    // 写 auto_resume 标记（相对 SD 卡路径）
    let rel = game.path.strip_prefix(sdcard_path).unwrap_or(&game.path);
    let _ = common::utils::put_file(&get_auto_resume_path(sdcard_path), rel);
    platform.set_cpu_speed(CpuSpeed::Menu);
    let _ = vibration::suspend();
}

/// 唤醒后序列（对应 C `Menu_afterSleep`，minarch.c:3120-3124）
#[cfg(feature = "platform-tg5040")]
fn after_sleep<P: Platform>(platform: &mut P, cpu_speed: CpuSpeed, run_state: &mut RunState) {
    let sdcard_path = P::SDCARD_PATH;
    let _ = std::fs::remove_file(get_auto_resume_path(sdcard_path));
    platform.set_cpu_speed(cpu_speed);
    run_state.hdmi = HdmiMonitor::new(); // 唤醒后重新采样
}

/// 快捷指令动作消费（对应 C :1683-1734 的 switch）
#[allow(clippy::too_many_arguments)]
#[cfg(feature = "platform-tg5040")]
fn handle_shortcut(
    action: ShortcutAction,
    run_state: &mut RunState,
    session: &mut CoreSession,
    core: &Core,
    game: &Game,
    save_io: &SaveStateIo,
    frontend: &mut OptionList,
    supports_overscan: bool,
    slot: usize,
) {
    match action {
        ShortcutAction::ToggleFf => {
            let new = !run_state.fast_forward;
            // 线程切换触发（C `setFastForward`，:1637-1650）
            run_state
                .thread_toggle
                .on_fast_forward_change(run_state.fast_forward, new);
            run_state.fast_forward = new;
            core::set_fast_forward(new);
            audio::set_fast_forward(new); // core 为唯一事实源，同步 audio
        }
        ShortcutAction::HoldFf { on } => {
            // 线程切换触发（C `setFastForward`）
            run_state
                .thread_toggle
                .on_fast_forward_change(run_state.fast_forward, on);
            run_state.fast_forward = on;
            core::set_fast_forward(on);
            audio::set_fast_forward(on);
        }
        ShortcutAction::SaveState => {
            // 对应 C `Menu_saveState`（state_slot = menu.slot，minarch.c:4152）
            let _ = save_state(core, game, slot, save_io, run_state.fast_forward);
        }
        ShortcutAction::LoadState => {
            // 对应 C `Menu_loadState`（同用当前槽位）
            let _ = load_state(core, game, slot, save_io, run_state.fast_forward);
        }
        ShortcutAction::ResetGame => {
            session.reset(core);
        }
        ShortcutAction::SaveQuit => {
            let _ = save_state(core, game, slot, save_io, run_state.fast_forward);
            run_state.quit = true;
        }
        ShortcutAction::CycleScaling => {
            let val = frontend
                .get_option_value("minarch_screen_scaling")
                .unwrap_or("Aspect");
            let next = match val {
                "Native" => Scaling::Aspect,
                "Aspect" => Scaling::Fullscreen,
                "Fullscreen" if supports_overscan => Scaling::Cropped,
                _ => Scaling::Native,
            };
            // 同步选项表（C `Config_syncFrontend` 语义，minarch.c:1158-1190）
            frontend.set_option_value("minarch_screen_scaling", scaling_label(next));
            core::set_frontend(next, core::Sharpness::Soft);
        }
        ShortcutAction::CycleEffect => {
            // 效果循环（C `screen_effect` 回绕）：更新前端选项值；
            // 实际效果渲染归后续 render change（本 change 无载体）
            let val = frontend
                .get_option_value("minarch_screen_effect")
                .unwrap_or("None");
            let next = match val {
                "None" => "Line",
                "Line" => "Grid",
                _ => "None",
            };
            frontend.set_option_value("minarch_screen_effect", next);
        }
    }
}

/// Scaling 枚举 → 选项表值字符串（与 `frontend_options` 的 labels 一致）
#[cfg(feature = "platform-tg5040")]
fn scaling_label(scaling: Scaling) -> &'static str {
    match scaling {
        Scaling::Native => "Native",
        Scaling::Aspect => "Aspect",
        Scaling::Fullscreen => "Fullscreen",
        Scaling::Cropped => "Cropped",
    }
}

/// 菜单子循环（对应 C `Menu_loop`，minarch.c:4224-4572）
#[allow(clippy::too_many_arguments)]
#[cfg(feature = "platform-tg5040")]
fn menu_loop<P: Platform>(
    platform: &mut P,
    run_state: &mut RunState,
    screen: &mut VideoBuffer,
    theme: &menu::MenuTheme,
    overlay: &VideoBuffer,
    main_menu: &mut MainMenu,
    save_io: &SaveStateIo,
    session: &mut CoreSession,
    core: &Core,
    game: &mut Game,
    frontend: &mut OptionList,
    power_state: &mut PowerState,
    user_cfg_path: &str,
) {
    // 进入菜单准备（C :4238-4249）
    let _ = session.write_saves(core, game);
    let suspended_strength = vibration::suspend();
    platform.set_cpu_speed(CpuSpeed::Menu);
    let was_ff = run_state.fast_forward;
    if was_ff {
        run_state.fast_forward = false;
        core::set_fast_forward(false);
        audio::set_fast_forward(false);
    }
    // 线程化时暂停核心线程（对应 C :1739-1743 `should_run_core=0`；
    // 线程空转不退出，菜单关闭后恢复）
    if run_state.thread_toggle.thread_mode {
        run_state
            .should_run_core
            .store(false, std::sync::atomic::Ordering::Release);
    }

    let mut option_menu: Option<OptionMenu> = None;

    // 菜单帧循环（C :4283-4537）
    'menu: loop {
        let frame_start = platform.now_ms();
        let input = platform.poll_input();
        let menu_input = MenuInput {
            up: input.just_repeated(BTN_DPAD_UP),
            down: input.just_repeated(BTN_DPAD_DOWN),
            left: input.just_repeated(BTN_DPAD_LEFT),
            right: input.just_repeated(BTN_DPAD_RIGHT),
            a: input.just_pressed(BTN_A),
            b: input.just_pressed(BTN_B),
            x: input.just_pressed(BTN_X),
            menu: input.just_pressed(BTN_MENU),
        };

        // 电源状态机（C :4393）
        let battery = platform.get_battery_status();
        let mod_keys = ModKeys {
            brightness: P::BTN_MOD_BRIGHTNESS,
            volume: P::BTN_MOD_VOLUME,
            plus: P::BTN_MOD_PLUS,
            minus: P::BTN_MOD_MINUS,
        };
        let (action, _, _) = common::power::update(
            power_state,
            &input,
            &battery,
            false,
            mod_keys,
            P::BTN_SLEEP,
            platform.is_hdmi_active(),
            frame_start,
        );
        match action {
            Some(PowerAction::Sleep) => {
                faux_sleep(platform, power_state);
            }
            Some(PowerAction::PowerOff) => {
                power_off(platform);
            }
            None => {}
        }

        if let Some(opts) = &mut option_menu {
            // 选项子菜单（C `Menu_options`）
            match opts.update(&menu_input) {
                menu::OptionMenuEvent::Close => {
                    option_menu = None;
                }
                menu::OptionMenuEvent::Change(idx) => {
                    if let Some(item) = opts.items.get(idx)
                        && let Some(key) = &item.key
                        && let Some(val) = item.values.get(item.value)
                    {
                        frontend.set_option_value(key, val);
                        sync_frontend_from_option(platform, run_state, frontend, key);
                    }
                }
                menu::OptionMenuEvent::Confirm(_) | menu::OptionMenuEvent::Advance => {}
                menu::OptionMenuEvent::None => {}
            }
            if let Some(opts) = &option_menu {
                let draw_state = menu::OptionDrawState {
                    menu: opts,
                    show_settings: false,
                };
                menu::draw_option_menu(theme, screen, &draw_state);
            }
            platform.flip(screen, true);
        } else {
            // 主菜单
            match main_menu.update(&menu_input) {
                Some(MainAction::Continue) => {
                    // A（Continue 项）与 B 都返回游戏（C :4330-4333）
                    run_state.show_menu = false;
                    break 'menu;
                }
                Some(MainAction::Quit) => {
                    run_state.quit = true;
                    run_state.show_menu = false;
                    break 'menu;
                }
                Some(MainAction::Save) => {
                    let _ = save_state(core, game, main_menu.slot, save_io, was_ff);
                }
                Some(MainAction::Load) => {
                    // 换碟联动（C :4164-4176）：记忆碟片 ≠ 当前 → 先换碟再读档
                    if let Some(req) = save_io.disc_change_request(main_menu.slot, main_menu.disc) {
                        let _ = game.change_disc(&req.path, &core.extensions, core.need_fullpath);
                    }
                    let _ = load_state(core, game, main_menu.slot, save_io, was_ff);
                }
                Some(MainAction::Options) => {
                    let items = OptionMenu::from_options(&frontend.options, menu::ItemKind::Var);
                    option_menu = Some(OptionMenu::new(items, MENU_VISIBLE_ROWS, None, None));
                }
                Some(MainAction::Reset) => {
                    session.reset(core);
                }
                Some(MainAction::DiscChange) => {
                    if let Some(path) = save_io.disc_paths.get(main_menu.disc) {
                        let _ = game.change_disc(path, &core.extensions, core.need_fullpath);
                    }
                }
                None => {}
            }
            // 绘制主菜单
            let slot_state = if main_menu.selected == menu::item::SAVE
                || main_menu.selected == menu::item::LOAD
            {
                Some(menu::SlotState {
                    save_exists: save_io.save_exists(main_menu.slot),
                    preview: None,
                })
            } else {
                None
            };
            let disc_name = if main_menu.disc_count > 1 {
                Some(format!("Disc {}", main_menu.disc + 1))
            } else {
                None
            };
            menu::draw_main_menu(
                theme,
                screen,
                overlay,
                &game.name,
                main_menu,
                disc_name.as_deref(),
                slot_state.as_ref(),
            );
            platform.flip(screen, true);
        }

        // HDMI 检测（C :4536）
        let hdmi_active = platform.is_hdmi_active();
        if let Some(_change) = run_state.hdmi.update(hdmi_active) {
            run_state.quit = true;
            run_state.show_menu = false;
            break 'menu;
        }
    }

    // 退出菜单恢复（C :4539-4571）
    platform.set_cpu_speed(CpuSpeed::Normal);
    if suspended_strength != 0 {
        vibration::resume(suspended_strength);
    }
    if was_ff {
        run_state.fast_forward = true;
        core::set_fast_forward(true);
        audio::set_fast_forward(true);
    }
    // 恢复核心线程（对应 C :4560-4564 `should_run_core=1`）
    if run_state.thread_toggle.thread_mode {
        run_state
            .should_run_core
            .store(true, std::sync::atomic::Ordering::Release);
    }
    // 写回 user cfg（对应 C `Config_write`，minarch.c:1284-1313；
    // implement-minarch-main 遗留缺口，本 change 闭环）。收集 frontend
    // 8 键当前值 → CfgEntry → save_cfg（截断创建 + sync_all）；写回失败
    // 静默（与 sram/savestate 偏离记录一致）
    let entries: Vec<minarch::config::CfgEntry> = frontend
        .options
        .iter()
        .map(|opt| minarch::config::CfgEntry {
            key: opt.key.clone(),
            value: opt.values.get(opt.value).cloned().unwrap_or_default(),
        })
        .collect();
    let _ = minarch::config::save_cfg(&entries, std::path::Path::new(user_cfg_path));
    run_state.hdmi = HdmiMonitor::new();
}

/// 前端选项 → 平台/核心联动（对应 C `Config_syncFrontend`，minarch.c:1158-1190）
#[cfg(feature = "platform-tg5040")]
fn sync_frontend_from_option<P: Platform>(
    platform: &mut P,
    run_state: &mut RunState,
    frontend: &OptionList,
    key: &str,
) {
    match key {
        "minarch_screen_scaling" => {
            let v = frontend.get_option_value(key).unwrap_or("Aspect");
            let scaling = match v {
                "Native" => Scaling::Native,
                "Fullscreen" => Scaling::Fullscreen,
                "Cropped" => Scaling::Cropped,
                _ => Scaling::Aspect,
            };
            core::set_frontend(scaling, core::Sharpness::Soft);
        }
        "minarch_prevent_tearing" => {
            let v = frontend.get_option_value(key).unwrap_or("Lenient");
            platform.set_vsync(match v {
                "Strict" => VsyncMode::Strict,
                "Lenient" => VsyncMode::Lenient,
                _ => VsyncMode::Off,
            });
        }
        // 线程模式运行时切换（对应 C :1002-1006 FE_OPT_THREAD：
        // `old = thread_video || was_threaded; toggle = old != value`）
        "minarch_thread_video" => {
            let v = frontend.get_option_value(key).unwrap_or("Off");
            run_state.thread_toggle.on_option_change(v == "On");
        }
        _ => {}
    }
}
