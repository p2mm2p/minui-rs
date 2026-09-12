//! `clock` — 日期时间设置工具
//!
//! 独立二进制：编辑年/月/日/时/分/秒（及 12 小时制的 AM/PM），
//! SELECT 切换 12/24 小时制并持久化偏好，A 保存到系统 RTC、B 取消。
//!
//! 对应原版 C `clock.c`（约 300 行）。
//! 纯函数 + 薄装配层结构：`validate.rs`/`prefs.rs` 为可测纯逻辑，
//! 本文件（main.rs）负责初始化、主循环、绘制编排与平台接线。
//!
//! ## 主循环结构
//!
//! 每帧：轮询输入 → 按键分发（UP/DOWN/LEFT/RIGHT/SELECT/A/B）→
//! dirty 时重绘（状态栏 + 居中日期时间 + 光标下划线 + 底部按键提示），
//! 否则限幅空转（`GFX_sync` 等价）。对齐原版 clock.c:145-310。
//!

mod prefs;
mod validate;

#[cfg(feature = "platform-tg5040")]
use validate::{adjust, display_hour, field_at, move_cursor, option_count, validate};

#[cfg(feature = "platform-tg5040")]
use common::input::{
    BTN_A, BTN_B, BTN_DPAD_DOWN, BTN_DPAD_LEFT, BTN_DPAD_RIGHT, BTN_DPAD_UP, BTN_SELECT,
};
#[cfg(feature = "platform-tg5040")]
use common::paths::get_res_path;
#[cfg(feature = "platform-tg5040")]
use common::platform::Platform;
#[cfg(feature = "platform-tg5040")]
use common::power::CpuSpeed;
#[cfg(feature = "platform-tg5040")]
use common::video::{
    BUTTON_PADDING, BUTTON_SIZE, FONT_PATH, PILL_SIZE, RGB_BLACK, RGB_WHITE, Rect, VideoBuffer,
};
#[cfg(feature = "platform-tg5040")]
use render::asset::load_atlas;
#[cfg(feature = "platform-tg5040")]
use render::button::blit_button_group;
#[cfg(feature = "platform-tg5040")]
use render::hardware::{HardwareStatus, blit_hardware_group};
#[cfg(feature = "platform-tg5040")]
use render::pill::blit_pill;
#[cfg(feature = "platform-tg5040")]
use render::text::Font;
#[cfg(feature = "platform-tg5040")]
use render::text::{load_font, render_text, size_text};

/// 帧预算（毫秒）——对应 C `FRAME_BUDGET 17`（api.c:204，60fps）
#[cfg(feature = "platform-tg5040")]
const FRAME_BUDGET: u32 = 17;

/// 数字区字号（对应 C `font.large`，defines.h:66 的 `FONT_LARGE 16`）
#[cfg(feature = "platform-tg5040")]
const FONT_LARGE: u32 = 16;

/// 提示文字字号（对应 C `font.tiny`，defines.h:65 的 `FONT_TINY 10`）
#[cfg(feature = "platform-tg5040")]
const FONT_TINY: u32 = 10;

/// 数字区垂直高度（对应 C `DIGIT_HEIGHT 16`，clock.c:39）
#[cfg(feature = "platform-tg5040")]
const DIGIT_HEIGHT: u32 = 16;

/// 光标下划线纵向偏移（对应 C `y += SCALE1(19)`，clock.c:299）
#[cfg(feature = "platform-tg5040")]
const CURSOR_OFFSET_Y: u32 = 19;

/// 年字段光标宽度（对应 C `SCALE1(40)`，clock.c:304）
#[cfg(feature = "platform-tg5040")]
const YEAR_CURSOR_W: u32 = 40;

/// 普通字段光标宽度（对应 C `SCALE1(20)`，clock.c:304）
#[cfg(feature = "platform-tg5040")]
const FIELD_CURSOR_W: u32 = 20;

/// 日期前缀占位宽度（对应 C `SCALE1(50)`——"YYYY/" 前缀，clock.c:302）
#[cfg(feature = "platform-tg5040")]
const DATE_PREFIX_W: u32 = 50;

/// 字段间步进（对应 C `SCALE1(30)`，clock.c:303——两个数字 + 间距）
#[cfg(feature = "platform-tg5040")]
const FIELD_STEP: u32 = 30;

fn main() {
    #[cfg(feature = "platform-tg5040")]
    {
        let mut platform = platform_tg5040::Tg5040::new();
        run(&mut platform);
    }
    // 无平台 feature（如单元测试编译）时保持占位输出
    #[cfg(not(feature = "platform-tg5040"))]
    {
        println!("clock - no platform feature selected");
    }
}

/// 启动序列 + 主循环 + 退出序列（对应 C `main`，clock.c:22-321）
///
/// # 参数
///
/// - `platform`: 平台实例（`Platform` trait 实现）
#[cfg(feature = "platform-tg5040")]
fn run<P: Platform>(platform: &mut P) {
    // ── 启动序列（对应 C :23-60）──

    // 1. CPU 降频（C :23）
    platform.set_cpu_speed(CpuSpeed::Menu);

    // 2. 视频 + 输入（C :25-26）
    platform.init_video();
    platform.init_input();

    // 3. 资源加载（图集 + 字体，对应 C `font.large` 加载）
    let sdcard_path = P::SDCARD_PATH;
    let scale = P::SCALE;
    let atlas = load_atlas(&format!(
        "{}/assets@{scale}x.png",
        get_res_path(sdcard_path)
    ))
    .expect("图集加载失败");
    let font_data = std::fs::read(format!("{sdcard_path}{FONT_PATH}")).expect("字体文件读取失败");
    let font = load_font(&font_data);

    // 4. 偏好读取（C :57——show_24hour 标记文件）
    let show_24hour_path = format!("{}/.userdata/{}/show_24hour", sdcard_path, P::PLATFORM);
    let mut show_24hour = prefs::read_show_24hour(&show_24hour_path);

    // 5. 当前系统时间初始化字段（C :59-68——localtime）
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let local = localtime(now);
    let (mut y, mut m, mut d, mut h, mut min, mut s) = (
        local.year,
        local.month,
        local.day,
        local.hour,
        local.minute,
        local.second,
    );

    // 6. 主循环状态（C :54-56, 140-144）
    let mut quit = false;
    let mut save_changes = false;
    let mut select_cursor: u32 = 0;
    let mut dirty = true;
    let mut was_online = platform.is_online();
    let mut screen = VideoBuffer::new(P::SCREEN_WIDTH, P::SCREEN_HEIGHT);

    // ── 主循环（对应 C :145-310）──
    while !quit {
        let frame_start = platform.now_ms();
        let input = platform.poll_input();

        // ── 按键分发（C :150-235）──
        if input.just_repeated(BTN_DPAD_UP) {
            dirty = true;
            let field = field_at(select_cursor);
            (y, m, d, h, min, s) = adjust(field, 1, (y, m, d, h, min, s));
        } else if input.just_repeated(BTN_DPAD_DOWN) {
            dirty = true;
            let field = field_at(select_cursor);
            (y, m, d, h, min, s) = adjust(field, -1, (y, m, d, h, min, s));
        } else if input.just_repeated(BTN_DPAD_LEFT) {
            dirty = true;
            select_cursor = move_cursor(select_cursor, -1, show_24hour);
        } else if input.just_repeated(BTN_DPAD_RIGHT) {
            dirty = true;
            select_cursor = move_cursor(select_cursor, 1, show_24hour);
        } else if input.just_pressed(BTN_A) {
            save_changes = true;
            quit = true;
        } else if input.just_pressed(BTN_B) {
            quit = true;
        } else if input.just_pressed(BTN_SELECT) {
            dirty = true;
            show_24hour = !show_24hour;
            // 字段数变化时光标钳制（C :226-227）
            if select_cursor >= option_count(show_24hour) {
                select_cursor -= option_count(show_24hour);
            }
            // 持久化偏好（C :229-235）
            prefs::write_show_24hour(&show_24hour_path, show_24hour);
        }

        // ── 在线状态变化（C :239-241）──
        let is_online = platform.is_online();
        if was_online != is_online {
            dirty = true;
        }
        was_online = is_online;

        // 电量状态（C :237——PWR_update 刷新，仅用于状态栏显示）
        let battery = platform.get_battery_status();

        // ── 脏帧渲染（C :243-308）──
        if dirty {
            // 回卷校验（C :244）
            (y, m, d, h, min, s) = validate((y, m, d, h, min, s));

            // 清屏（C :246）
            screen.fill_rect(
                Rect {
                    x: 0,
                    y: 0,
                    w: screen.width,
                    h: screen.height,
                },
                RGB_BLACK,
            );

            // 顶部状态栏（C :248——blitHardwareGroup）
            let hw_status = HardwareStatus {
                show_setting: 0,
                setting_value: 0,
                battery_percentage: battery.percentage,
                battery_charging: battery.charging,
                wifi_online: is_online,
                mode_main: true,
                has_hdmi: false,
            };
            blit_hardware_group(&atlas, &hw_status, scale, &mut screen);

            // 底部按键提示（C :251-253——两行）
            let screen_w = screen.width;
            let screen_h = screen.height;
            let btn_h = (BUTTON_SIZE + BUTTON_PADDING) * scale;
            // 第一行：SELECT 12/24 HOUR
            blit_button_group(
                &[("SELECT", if show_24hour { "12 HOUR" } else { "24 HOUR" })],
                &atlas,
                &font,
                &mut screen,
                Rect {
                    x: 0,
                    y: screen_h - 2 * btn_h,
                    w: screen_w,
                    h: btn_h,
                },
                false,
                FONT_TINY * scale,
                scale,
            );
            // 第二行：B CANCEL + A SET
            blit_button_group(
                &[("CANCEL", "B"), ("SET", "A")],
                &atlas,
                &font,
                &mut screen,
                Rect {
                    x: 0,
                    y: screen_h - btn_h,
                    w: screen_w,
                    h: btn_h,
                },
                false,
                FONT_TINY * scale,
                scale,
            );

            // 居中日期时间（C :255-295）
            // 内容：YYYY/MM/DD HH:MM:SS（或 H:MM:SS AM/PM）
            let am_selected = h < 12;
            let date_str = format!("{y:04}/{m:02}/{d:02}");
            let time_str = if show_24hour {
                format!("{h:02}:{min:02}:{s:02}")
            } else {
                format!(
                    "{}:{min:02}:{s:02} {}",
                    display_hour(h),
                    if am_selected { "AM" } else { "PM" }
                )
            };
            let full_str = format!("{date_str} {time_str}");
            let (content_w, _) = size_text(&font, &full_str, FONT_LARGE * scale, 0);
            let ox = (screen.width - content_w) / 2;
            // 垂直居中：((FIXED_HEIGHT/FIXED_SCALE) - PILL_SIZE - DIGIT_HEIGHT) / 2（C :261）
            let oy = ((screen.height / scale - PILL_SIZE - DIGIT_HEIGHT) / 2) * scale;
            render_text(
                &mut screen,
                &font,
                &full_str,
                FONT_LARGE * scale,
                RGB_WHITE,
                (ox, oy),
            );

            // 光标下划线（C :297-304）
            let cursor_y = oy + CURSOR_OFFSET_Y * scale;
            let (cursor_x, cursor_w) = cursor_geometry(
                select_cursor,
                show_24hour,
                ox,
                &font,
                FONT_LARGE * scale,
                scale,
            );
            blit_pill(
                &atlas,
                common::video::Asset::Underline,
                scale,
                &mut screen,
                Rect {
                    x: cursor_x,
                    y: cursor_y,
                    w: cursor_w,
                    h: 0,
                },
            );

            // 帧提交（C :306——GFX_flip）
            let elapsed = platform.now_ms().saturating_sub(frame_start);
            platform.flip(&screen, elapsed < FRAME_BUDGET);
            dirty = false;
        } else {
            // 不脏帧时限幅（C :309——GFX_sync）
            let elapsed = platform.now_ms().saturating_sub(frame_start);
            if elapsed < FRAME_BUDGET {
                std::thread::sleep(std::time::Duration::from_millis(
                    (FRAME_BUDGET - elapsed) as u64,
                ));
            }
        }
    }

    // ── 退出序列（C :312-321）──
    platform.quit_input();
    platform.quit_video();

    // 保存（C :319——仅 A 键保存）
    if save_changes {
        platform.set_date_time(y, m, d, h, min, s);
    }
}

/// 计算光标下划线的 x 位置与宽度
///
/// 对应原版 `clock.c:297-304` 的定位逻辑：
/// - 年字段：x = ox（无前缀偏移），宽 40
/// - 其他字段：x = ox + 50 + (cursor-1)*30（跳过 "YYYY/" 前缀），
///   宽 20（AMPM 字段用 AM/PM 文字宽度）
///
/// # 参数
///
/// - `cursor`：当前光标索引
/// - `show_24hour`：是否 24 小时制
/// - `ox`：内容区起始 x
/// - `font`：字体
/// - `px`：字号（已缩放）
/// - `scale`：平台缩放倍率
///
/// # 返回值
///
/// `(x, 宽度)`——下划线定位
#[cfg(feature = "platform-tg5040")]
fn cursor_geometry(
    cursor: u32,
    show_24hour: bool,
    ox: u32,
    font: &Font,
    px: u32,
    scale: u32,
) -> (u32, u32) {
    if cursor == 0 {
        // 年字段：x = ox，宽 40
        (ox, YEAR_CURSOR_W * scale)
    } else {
        // 其他字段：跳过 "YYYY/" 前缀 + 字段步进
        let x = ox + DATE_PREFIX_W * scale + (cursor - 1) * FIELD_STEP * scale;
        if cursor == 6 && !show_24hour {
            // AMPM 字段：宽度 = "AM"/"PM" 文字宽度
            let (w, _) = size_text(font, "AM", px, 0);
            (x, w + 2 * scale)
        } else {
            (x, FIELD_CURSOR_W * scale)
        }
    }
}

/// 本地时间分解（对应原版 C `localtime()`，clock.c:60）
///
/// 用 libc `localtime_r` 读取系统本地时间（含时区）。clock 依赖 libc
/// 与此处 FFI 的原因：Rust 标准库无本地时间函数。
///
/// # 参数
///
/// - `secs`：Unix 时间戳（秒）
///
/// # 返回值
///
/// 本地时间的年/月/日/时/分/秒
#[cfg(feature = "platform-tg5040")]
fn localtime(secs: i64) -> LocalTime {
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        let t = secs as libc::time_t;
        libc::localtime_r(&t, &mut tm);
        LocalTime {
            year: tm.tm_year + 1900,
            month: tm.tm_mon + 1,
            day: tm.tm_mday,
            hour: tm.tm_hour,
            minute: tm.tm_min,
            second: tm.tm_sec,
        }
    }
}

/// 本地时间分解结果
#[cfg(feature = "platform-tg5040")]
struct LocalTime {
    year: i32,
    month: i32,
    day: i32,
    hour: i32,
    minute: i32,
    second: i32,
}
