//! `minui` — MinUI 启动器
//!
//! 装配层：启动序列、主循环、帧控制。对应原版 C `minui.c` 的
//! `main()`（:1286-1704）与 `GFX_startFrame`/`GFX_flip`/`GFX_sync`
//! （api.c:204-218）。
//!
//! ## 模块
//!
//! | 模块 | 职责 |
//! |------|------|
//! | `browser` | 文件浏览、`Directory`/`Entry` 管理与索引 |
//! | `disc` | 游戏文件组织判断（m3u/cue/模拟器） |
//! | `launch` | 写入 `/tmp/next`、启动游戏命令 |
//! | `menu` | 菜单导航状态机（栈/滚动/restore/load_last） |
//! | `recents` | 最近游戏列表 + 持久化 |
//! | `ui` | 列表/按钮组/缩略图渲染 |
//! | `version` | 版本信息画面 |
//!
//! ## 平台特化引用
//!
//! 按键语义键（`BTN_SLEEP`/`BTN_MOD_*`）经 `Platform` trait 关联常量
//! 获取；`BTN_RESUME` 为 main 本地常量；系统设置（`SettingsHandle`）
//! 经 `#[cfg(feature = "tg5040")]` 引用平台 crate——对应 C 的
//! `platform.h` 宏 + `libmsettings`（tg5040 README「第二接口面」模式）。
//! 业务模块（menu/ui）经参数接收平台值，保持平台无关。
//!

mod browser;
mod disc;
mod launch;
mod menu;
mod recents;
#[cfg(test)]
mod test_util;
mod ui;
mod version;

#[cfg(feature = "tg5040")]
use common::input::{
    BTN_A, BTN_B, BTN_DPAD_DOWN, BTN_DPAD_LEFT, BTN_DPAD_RIGHT, BTN_DPAD_UP, BTN_L1, BTN_R1, BTN_X,
    MenuTapState, ModKeys, tapped_menu,
};
#[cfg(feature = "tg5040")]
use common::paths::{get_auto_resume_path, get_res_path, get_version_txt_path};
#[cfg(feature = "tg5040")]
use common::platform::Platform;
#[cfg(feature = "tg5040")]
use common::power::{CpuSpeed, PowerAction, PowerState, faux_sleep, power_off};
#[cfg(feature = "tg5040")]
use common::utils::{exists, get_file};
#[cfg(feature = "tg5040")]
use common::video::{FONT_PATH, RGB_BLACK, Rect, VideoBuffer};
#[cfg(feature = "tg5040")]
use menu::{Menu, ScrollDir};
#[cfg(feature = "tg5040")]
use render::asset::load_atlas;
#[cfg(feature = "tg5040")]
use render::hardware::{HardwareStatus, blit_hardware_group};
#[cfg(feature = "tg5040")]
use render::text::{blit_message, load_font};
#[cfg(feature = "tg5040")]
use std::time::Duration;

// ── 平台特化区（spec「平台特化引用」——语义键经 Platform trait 关联
// 常量；平台 crate 引用仅剩 SettingsHandle 与 Tg5040 实例化）──

#[cfg(feature = "tg5040")]
// 续玩键：tg5040 为 X（C platform.h:110 `#define BTN_RESUME BTN_X`）
#[cfg(feature = "tg5040")]
const BTN_RESUME: u32 = BTN_X;
#[cfg(feature = "tg5040")]
use tg5040::settings::SettingsHandle;

/// 帧预算（毫秒）——对应 C `FRAME_BUDGET 17`（api.c:204，60fps）
#[cfg(feature = "tg5040")]
const FRAME_BUDGET: u32 = 17;

/// 版本页字号（对应 C `FONT_LARGE`，defines.h:66）
#[cfg(feature = "tg5040")]
const FONT_LARGE: u32 = 16;

fn main() {
    #[cfg(feature = "tg5040")]
    {
        let mut platform = tg5040::Tg5040::new();
        run(&mut platform);
    }
    // 无平台 feature（如单元测试编译）时保持占位输出
    #[cfg(not(feature = "tg5040"))]
    {
        println!("minui launcher - no platform feature selected");
    }
}

/// 启动器装配（对应 C `main`，minui.c:1305-1704）
///
/// 启动序列：auto_resume → simple_mode → settings → 视频/资源 → 输入 →
/// 菜单初始化 → CPU 降频；随后进入主循环（输入处理 + 脏帧渲染 +
/// 帧控制）直至启动退出。
///
/// 仅在有平台 feature 时编译（平台特化常量随 feature 引入）。
///
/// # 参数
///
/// - `platform`:平台实例（`Platform` trait 实现，如 `Tg5040`）
#[cfg(feature = "tg5040")]
fn run<P: Platform>(platform: &mut P) {
    let sdcard_path = P::SDCARD_PATH;
    let platform_code = P::PLATFORM;
    let paks_path = common::paths::get_paks_path(sdcard_path, platform_code);
    let scale = P::SCALE;

    // ── 启动序列（对应 C :1305-1335）──

    // 1. 自动续玩（C :1305）——已排队启动直接退出
    if launch::auto_resume(sdcard_path, platform_code, &paks_path) {
        return;
    }

    // 2. 简洁模式（C :1307）
    let simple_mode = exists(&common::paths::get_simple_mode_path(sdcard_path));

    // 3. 系统设置（C `InitSettings`——libmsettings）
    let settings = SettingsHandle::init();

    // 4. 视频 + 资源（C `GFX_init(MODE_MAIN)`，:1312）
    platform.init_video();
    let mut screen = VideoBuffer::new(P::SCREEN_WIDTH, P::SCREEN_HEIGHT);
    let atlas = load_atlas(&format!(
        "{}/assets@{scale}x.png",
        get_res_path(sdcard_path)
    ))
    .expect("图集加载失败");
    let font_data = std::fs::read(format!("{sdcard_path}{FONT_PATH}")).expect("字体文件读取失败");
    let font = load_font(&font_data);

    // 5. 输入 + 电源状态（C `PAD_init`/`PWR_init`，:1315-1319）
    platform.init_input();
    let mut power_state = PowerState::new();
    if !P::HAS_POWER_BUTTON && !simple_mode {
        power_state.disable_sleep(); // C :1319
    }

    // 6. 菜单初始化（C `Menu_init`，:1324——openDirectory(SDCARD) + loadLast）
    let rows = platform.main_row_count() as usize;
    let padding = platform.padding();
    let mut menu = Menu::new(sdcard_path, platform_code, &paks_path, rows);

    // 7. CPU 降频（C :1328）
    platform.set_cpu_speed(CpuSpeed::Menu);

    // ── 主循环状态（C :1331-1336）──

    let mod_keys = ModKeys {
        brightness: P::BTN_MOD_BRIGHTNESS,
        volume: P::BTN_MOD_VOLUME,
        plus: P::BTN_MOD_PLUS,
        minus: P::BTN_MOD_MINUS,
    };
    let mut menu_tap = MenuTapState::new();
    let mut dirty = true;
    let mut was_online = platform.is_online();
    // 版本信息懒加载（进入版本页时读取一次，对应 C 的 lazy surface 构建）
    let mut version_info: Option<(String, String)> = None;

    // ── 主循环（对应 C :1338-1696）──
    loop {
        let frame_start = platform.now_ms();
        let input = platform.poll_input();
        let total = menu.top().entries.len();

        // ── 电源/设置状态更新（对应 C `PWR_update`，:1347）──
        let battery = platform.get_battery_status();
        let mute = settings.mute();
        let (action, power_dirty, show_setting) = common::power::update(
            &mut power_state,
            &input,
            &battery,
            mute,
            mod_keys,
            P::BTN_SLEEP,
            platform.is_hdmi_active(),
            frame_start,
        );
        dirty |= power_dirty;
        match action {
            Some(PowerAction::Sleep) => {
                // C :1687-1694——faux_sleep（update 返回信号，调用方执行序列）
                faux_sleep(platform, &mut power_state);
                dirty = true;
            }
            Some(PowerAction::PowerOff) => {
                // C :1616-1635——关机消息 + 关机
                let msg = if exists(&get_auto_resume_path(sdcard_path)) {
                    "Quicksave created,\npowering off"
                } else {
                    "Powering off"
                };
                let rect = Rect {
                    x: 0,
                    y: 0,
                    w: screen.width,
                    h: screen.height,
                };
                screen.fill_rect(rect, RGB_BLACK);
                blit_message(&mut screen, &font, msg, FONT_LARGE * scale, rect);
                platform.flip(&screen, true);
                std::thread::sleep(Duration::from_secs(1));
                power_off(platform);
            }
            None => {}
        }

        // ── 在线状态变化（C :1349-1351）──
        let is_online = platform.is_online();
        if was_online != is_online {
            dirty = true;
        }
        was_online = is_online;

        // ── 输入处理（C :1353-1483）──
        let selected = menu.top().selected;
        if menu.show_version() {
            // 版本页：MENU 轻触或 B 退出（C :1353-1359）
            if tapped_menu(&input, &mut menu_tap, frame_start, mod_keys)
                || input.just_released(BTN_B)
            {
                menu.set_show_version(false);
                dirty = true;
                if !P::HAS_POWER_BUTTON && !simple_mode {
                    power_state.disable_sleep();
                }
            }
        } else {
            // 进入版本页（C :1361-1365）
            if tapped_menu(&input, &mut menu_tap, frame_start, mod_keys) {
                menu.set_show_version(true);
                dirty = true;
                if !P::HAS_POWER_BUTTON && !simple_mode {
                    power_state.enable_sleep();
                }
            } else if total > 0 {
                // 滚动（C :1367-1428）
                let start = menu.top().start;
                let end = menu.top().end;
                let mut moved = false;
                for (btn, dir) in [
                    (BTN_DPAD_UP, ScrollDir::Up),
                    (BTN_DPAD_DOWN, ScrollDir::Down),
                    (BTN_DPAD_LEFT, ScrollDir::Left),
                    (BTN_DPAD_RIGHT, ScrollDir::Right),
                ] {
                    if input.just_repeated(btn) {
                        let from_press = input.just_pressed(btn);
                        let (sel, st, en) =
                            menu::scroll(selected, start, end, total, rows, dir, from_press);
                        menu.move_to(sel, st, en);
                        moved = true;
                        break;
                    }
                }
                // 字母组跳转（C :1431-1456）
                if !moved && (input.just_repeated(BTN_L1) || input.just_repeated(BTN_R1)) {
                    let current_alpha = menu.top().entries[selected].alpha;
                    if let Some((sel, st, en)) = menu::alpha_jump(
                        &menu.top().alphas,
                        current_alpha,
                        total,
                        rows,
                        input.just_repeated(BTN_R1),
                    ) {
                        menu.move_to(sel, st, en);
                        moved = true;
                    }
                }
                if moved && menu.top().selected != selected {
                    dirty = true;
                }

                // X 键续玩（C :1465-1469）
                if menu.can_resume() && input.just_released(BTN_RESUME) {
                    menu.request_resume();
                    if menu.entry_open(menu.top().selected, sdcard_path, platform_code, &paks_path)
                    {
                        break;
                    }
                    dirty = true;
                }
                // A 键打开（C :1470-1476）
                if input.just_pressed(BTN_A) {
                    if menu.entry_open(menu.top().selected, sdcard_path, platform_code, &paks_path)
                    {
                        break;
                    }
                    dirty = true;
                    if !menu.top().entries.is_empty() {
                        menu.refresh_resume(sdcard_path);
                    }
                }
                // B 键返回（C :1477-1483）
                if input.just_pressed(BTN_B) && menu.stack_depth() > 1 {
                    menu.close_directory();
                    dirty = true;
                    menu.refresh_resume(sdcard_path);
                }
            }
        }

        // ── 脏帧渲染（C :1486-1675）──
        if dirty {
            screen.fill_rect(
                Rect {
                    x: 0,
                    y: 0,
                    w: screen.width,
                    h: screen.height,
                },
                RGB_BLACK,
            );

            // 缩略图（C :1494-1519）
            let thumb = if !menu.show_version() && total > 0 {
                let entry = &menu.top().entries[menu.top().selected];
                ui::render_thumbnail(&mut screen, entry, scale)
            } else {
                None
            };

            // 硬件状态组（C :1521）
            let setting_value = if show_setting == 1 {
                settings.brightness()
            } else {
                settings.volume()
            };
            let hw_status = HardwareStatus {
                show_setting,
                setting_value,
                battery_percentage: battery.percentage,
                battery_charging: battery.charging,
                wifi_online: is_online,
                mode_main: true,
                has_hdmi: platform.is_hdmi_active(),
            };
            let ow = blit_hardware_group(&atlas, &hw_status, scale, &mut screen);

            if menu.show_version() {
                // 版本页（C :1523-1586）
                let (release, commit) = version_info.get_or_insert_with(|| {
                    let content = get_file(&get_version_txt_path(sdcard_path)).unwrap_or_default();
                    version::parse_version(&content)
                });
                version::render_version(
                    &mut screen,
                    &font,
                    release,
                    commit,
                    P::DEVICE_MODEL,
                    scale,
                );
                ui::render_buttons(
                    &mut screen,
                    &atlas,
                    &font,
                    true,
                    false,
                    menu.stack_depth(),
                    simple_mode,
                    total,
                    scale,
                );
            } else if total > 0 {
                // 列表（C :1588-1644）
                let top = menu.top();
                ui::render_list(
                    &mut screen,
                    &atlas,
                    &font,
                    &top.entries,
                    top.selected,
                    top.start,
                    top.end,
                    thumb,
                    ow,
                    padding,
                    scale,
                );
                ui::render_buttons(
                    &mut screen,
                    &atlas,
                    &font,
                    false,
                    menu.can_resume(),
                    menu.stack_depth(),
                    simple_mode,
                    total,
                    scale,
                );
            } else {
                // 空目录（C :1645-1648）
                ui::render_empty(&mut screen, &font, scale);
                ui::render_buttons(
                    &mut screen,
                    &atlas,
                    &font,
                    false,
                    false,
                    menu.stack_depth(),
                    simple_mode,
                    0,
                    scale,
                );
            }

            // 帧提交（C `GFX_flip`，api.c:208——耗时 < 帧预算才等 vsync）
            let elapsed = platform.now_ms().saturating_sub(frame_start);
            platform.flip(&screen, elapsed < FRAME_BUDGET);
            dirty = false;
        } else {
            // 不脏帧时限幅（C `GFX_sync`，api.c:212-218）
            let elapsed = platform.now_ms().saturating_sub(frame_start);
            if elapsed < FRAME_BUDGET {
                std::thread::sleep(Duration::from_millis((FRAME_BUDGET - elapsed) as u64));
            }
        }

        // ── HDMI 变化检测（C :1684-1695——重启语义保留，tg5040 恒 false）──
        let had_hdmi = if platform.is_hdmi_active() { 1 } else { 0 };
        if had_hdmi != 0 {
            // 仅作结构保留：tg5040 的 is_hdmi_active 恒 false，本分支不触发
            launch::save_last(
                menu.top().path == common::paths::get_faux_recent_path(sdcard_path),
                &menu.top().entries[menu.top().selected].path,
                sdcard_path,
            );
            std::thread::sleep(Duration::from_secs(4));
            break;
        }
    }

    // ── 退出序列（对应 C :1700-1704）──
    platform.quit_input();
    platform.quit_video();
}
