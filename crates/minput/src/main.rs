//! `minput` — 按键诊断工具
//!
//! 把设备声明的所有按键绘制成面板，用户逐个按键时对应按钮点亮
//! （按下 = 实心 `Button` 素材，未按 = 空心 `Hole` 素材），
//! SELECT+START 同时按下退出。
//! 对应原版 C `minput.c`（`minput.c`）。
//!
//! ## 主循环结构
//!
//! 每帧：轮询输入 → SELECT+START 退出判定 → 按键状态变化（按下/释放）
//! 触发脏帧重绘（背景药丸 + 全部按钮），否则限幅空转（`GFX_sync`
//! 等价）。对齐原版 minput.c:73-266。
//!

mod layout;

#[cfg(feature = "platform-tg5040")]
use common::input::{BTN_SELECT, BTN_START};
#[cfg(feature = "platform-tg5040")]
use common::paths::get_res_path;
#[cfg(feature = "platform-tg5040")]
use common::platform::Platform;
#[cfg(feature = "platform-tg5040")]
use common::power::CpuSpeed;
#[cfg(feature = "platform-tg5040")]
use common::video::{
    ASSET_RGBS, Asset, FONT_PATH, FRAME_BUDGET_MS, RGB_BLACK, RGB_DARK_GRAY, Rect, VideoBuffer,
};
#[cfg(feature = "platform-tg5040")]
use layout::{Capabilities, layout_background, layout_buttons};
#[cfg(feature = "platform-tg5040")]
use render::asset::load_atlas;
#[cfg(feature = "platform-tg5040")]
use render::pill::blit_pill;
#[cfg(feature = "platform-tg5040")]
use render::text::{load_font, render_text, size_text};

/// 按钮标签字号（对应 C `font.medium`/`font.small`/`font.tiny`，
/// defines.h:67-69——单字符 14、短标签 12、多字符 10）
#[cfg(feature = "platform-tg5040")]
const FONT_SINGLE_CHAR: u32 = 14;
#[cfg(feature = "platform-tg5040")]
const FONT_SHORT_LABEL: u32 = 12;
#[cfg(feature = "platform-tg5040")]
const FONT_MULTI_CHAR: u32 = 10;

fn main() {
    #[cfg(feature = "platform-tg5040")]
    {
        let mut platform = platform_tg5040::Tg5040::new();
        run(&mut platform);
    }
    // 无平台 feature（如单元测试编译）时保持占位输出
    #[cfg(not(feature = "platform-tg5040"))]
    {
        println!("minput - no platform feature selected");
    }
}

/// 按键诊断工具装配（对应 C `main`，minput.c:44-274）
///
/// 启动序列：CPU 降频 → 视频初始化 → 输入初始化 → 能力收集 →
/// 图集/字体加载；随后进入主循环（轮询 + 脏帧重绘 + 退出判定）
/// 直至 SELECT+START 退出，最后清理资源。
///
/// 仅在有平台 feature 时编译（平台特化常量随 feature 引入）。
///
/// # 参数
///
/// - `platform`:平台实例（`Platform` trait 实现，如 `Tg5040`）
#[cfg(feature = "platform-tg5040")]
fn run<P: Platform>(platform: &mut P) {
    let sdcard_path = P::SDCARD_PATH;
    let scale = P::SCALE;

    // ── 启动序列（对应 C :45-50）──

    // 1. CPU 降频（C :45）
    platform.set_cpu_speed(CpuSpeed::Menu);

    // 2. 视频 + 输入（C :47-48）——不初始化 settings（C `InitSettings`
    //    调用后从未使用，Rust 版省略）
    platform.init_video();
    platform.init_input();

    // 3. 资源加载（图集 + 字体，与 minui/clock 同模式）
    let atlas = load_atlas(&format!(
        "{}/assets@{scale}x.png",
        get_res_path(sdcard_path)
    ))
    .expect("图集加载失败");
    let font_data = std::fs::read(format!("{sdcard_path}{FONT_PATH}")).expect("字体文件读取失败");
    let font = load_font(&font_data);

    // 4. 能力收集（C :53-63——编译期能力常量 → Capabilities）
    let caps = Capabilities::from_platform::<P>();

    // ── 主循环状态（C :65-71）──

    let mut dirty = true;
    let mut screen = VideoBuffer::new(P::SCREEN_WIDTH, P::SCREEN_HEIGHT);

    // ── 主循环（对应 C :73-266）──
    loop {
        let frame_start = platform.now_ms();
        let input = platform.poll_input();

        // 退出判定（C :79——SELECT+START 同时按下）
        if input.is_pressed(BTN_SELECT) && input.is_pressed(BTN_START) {
            break;
        }

        // 脏帧判定（C :78——anyPressed || anyJustReleased）
        if input.any_pressed() || input.any_just_released() {
            dirty = true;
        }

        // ── 脏帧渲染（C :87-264）──
        if dirty {
            // 清屏（C :88——GFX_clear）
            screen.fill_rect(
                Rect {
                    x: 0,
                    y: 0,
                    w: screen.width,
                    h: screen.height,
                },
                RGB_BLACK,
            );

            // 背景：组药丸 + DPAD 连接条（C :92-260 各组 pill/fill）
            let text_width = |label: &str| {
                let px = label_font_px(label, scale);
                let (w, _) = size_text(&font, label, px, 0);
                w
            };
            let bg = layout_background(&caps, scale, screen.width, &text_width);
            for pill in &bg.pills {
                blit_pill(
                    &atlas,
                    Asset::DarkGrayPill,
                    scale,
                    &mut screen,
                    Rect {
                        x: pill.x,
                        y: pill.y,
                        w: pill.w,
                        h: 0,
                    },
                );
            }
            for strip in &bg.strips {
                screen.fill_rect(
                    Rect {
                        x: strip.x,
                        y: strip.y,
                        w: strip.w,
                        h: strip.h,
                    },
                    RGB_DARK_GRAY,
                );
            }

            // 按钮：按下 = Button 素材，未按 = Hole 素材 + 标签文字
            let layout = layout_buttons(&caps, &input, scale, screen.width, &text_width);
            for btn in &layout {
                if btn.btn == common::input::BTN_NONE {
                    // QUIT 纯文本（垂直居中于按钮行）
                    render_text(
                        &mut screen,
                        &font,
                        btn.label,
                        FONT_MULTI_CHAR * scale,
                        ASSET_RGBS[Asset::Button as usize],
                        (
                            btn.x,
                            btn.y + (BUTTON_SIZE_S * scale - FONT_MULTI_CHAR * scale) / 2,
                        ),
                    );
                    continue;
                }
                // 背景素材（按下实心 / 未按空心）
                let asset = if btn.pressed {
                    Asset::Button
                } else {
                    Asset::Hole
                };
                blit_pill(
                    &atlas,
                    asset,
                    scale,
                    &mut screen,
                    Rect {
                        x: btn.x,
                        y: btn.y,
                        w: btn.w,
                        h: 0,
                    },
                );
                // 标签文字（单字符用大字号，多字符用小字号）
                let px = label_font_px(btn.label, scale);
                let (label_w, _) = size_text(&font, btn.label, px, 0);
                let label_x = btn.x + (btn.w.saturating_sub(label_w)) / 2;
                let label_y = btn.y + (BUTTON_SIZE_S * scale - px) / 2;
                render_text(
                    &mut screen,
                    &font,
                    btn.label,
                    px,
                    ASSET_RGBS[Asset::Button as usize],
                    (label_x, label_y),
                );
            }

            // 帧提交（C :262——GFX_flip；耗时 < 帧预算才等 vsync）
            let elapsed = platform.now_ms().saturating_sub(frame_start);
            platform.flip(&screen, elapsed < FRAME_BUDGET_MS);
            dirty = false;
        } else {
            // 不脏帧时限幅（C :265——GFX_sync）
            let elapsed = platform.now_ms().saturating_sub(frame_start);
            if elapsed < FRAME_BUDGET_MS {
                std::thread::sleep(std::time::Duration::from_millis(
                    (FRAME_BUDGET_MS - elapsed) as u64,
                ));
            }
        }
    }

    // ── 退出序列（对应 C :268-271）──
    platform.quit_input();
    platform.quit_video();
}

/// 按钮标签字号（未缩放 px，调用方乘 scale）
///
/// 对应原 C `font.medium`（单字符 14）/`font.small`（短标签 12）/
/// `font.tiny`（多字符 10）——defines.h:67-69。单字符按钮（U/D/L/R/
/// A/B/X/Y）用 medium，2 字符（L1/L2/R1/R2/L3/R3）用 small，
/// 多字符（VOL. -/MENU/POWER/SELECT/START）用 tiny。
///
/// # 参数
///
/// - `label`：按钮标签
/// - `scale`：平台缩放倍率
#[cfg(feature = "platform-tg5040")]
fn label_font_px(label: &str, scale: u32) -> u32 {
    let n = label.chars().count();
    let px = if n == 1 {
        FONT_SINGLE_CHAR
    } else if n == 2 {
        FONT_SHORT_LABEL
    } else {
        FONT_MULTI_CHAR
    };
    px * scale
}

/// 按钮素材的未缩放边长（对应 `common::video::BUTTON_SIZE`，局部别名
/// 避免在绘制段重复书写 `common::video::BUTTON_SIZE`）
#[cfg(feature = "platform-tg5040")]
const BUTTON_SIZE_S: u32 = common::video::BUTTON_SIZE;
