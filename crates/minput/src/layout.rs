//! 设备能力与面板布局纯逻辑
//!
//! 本模块是 minput 的核心逻辑层：把设备按键能力（`Platform` trait 的
//! `HAS_*` 关联常量）翻译成按钮面板的几何排布。全部为纯函数——
//! 不依赖 SDL、不接触硬件，输入能力组合与缩放倍率，输出各按钮组的
//! 绘制矩形与按键状态查询。
//!
//! ## 与原 C 代码的对比
//!
//! 原版 `minput.c` 用平台宏在编译期算死 `has_L2`/`has_R2` 等能力
//! （`BUTTON_*!=BUTTON_NA || ...`），面板几何在 `main` 里直接写死。
//! Rust 版把能力收集（`Capabilities::from_platform`）与几何计算
//! （`layout_buttons`）提取为纯函数模块——能力组合 × 几何的对应
//! 关系可单测。
//!

use common::input::{
    BTN_A, BTN_B, BTN_DPAD_DOWN, BTN_DPAD_LEFT, BTN_DPAD_RIGHT, BTN_DPAD_UP, BTN_L1, BTN_L2,
    BTN_L3, BTN_MENU, BTN_MINUS, BTN_NONE, BTN_PLUS, BTN_POWER, BTN_R1, BTN_R2, BTN_R3, BTN_SELECT,
    BTN_START, BTN_X, BTN_Y, InputState,
};
use common::platform::Platform;
use common::video::{BUTTON_MARGIN, BUTTON_SIZE, PADDING, PILL_SIZE};

/// 设备按键能力集合（9 个能力位）
///
/// 由装配层从 `Platform` trait 的 `HAS_*` 关联常量收集（「调用方收集
/// 数据传入」模式，与 `ModKeys` 同构）。布局函数只读本结构体，
/// 不直接接触 trait——纯数据便于单测。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// 是否有 L2 键（对应 `Platform::HAS_L2`）
    pub has_l2: bool,
    /// 是否有 R2 键（对应 `Platform::HAS_R2`）
    pub has_r2: bool,
    /// 是否有 L3 键（对应 `Platform::HAS_L3`）
    pub has_l3: bool,
    /// 是否有 R3 键（对应 `Platform::HAS_R3`）
    pub has_r3: bool,
    /// 是否有左摇杆（对应 `Platform::HAS_LS`）
    pub has_ls: bool,
    /// 是否有右摇杆（对应 `Platform::HAS_RS`）
    pub has_rs: bool,
    /// 是否有音量键（对应 `Platform::HAS_VOLUME`）
    pub has_volume: bool,
    /// 是否有 MENU 键（对应 `Platform::HAS_MENU`）
    pub has_menu: bool,
    /// 是否有电源键（对应 `Platform::HAS_POWER_BUTTON`）
    pub has_power: bool,
}

impl Capabilities {
    /// 从平台关联常量收集能力（装配层调用）
    ///
    /// # 参数
    ///
    /// - `P`：平台类型（`Platform` trait 实现）
    ///
    /// # 返回
    ///
    /// 能力集合（全部来自编译期关联常量）
    pub fn from_platform<P: Platform>() -> Self {
        Self {
            has_l2: P::HAS_L2,
            has_r2: P::HAS_R2,
            has_l3: P::HAS_L3,
            has_r3: P::HAS_R3,
            has_ls: P::HAS_LS,
            has_rs: P::HAS_RS,
            has_volume: P::HAS_VOLUME,
            has_menu: P::HAS_MENU,
            has_power: P::HAS_POWER_BUTTON,
        }
    }
}

/// 查询按钮是否按下（供绘制编排使用）
///
/// # 参数
///
/// - `input`：当前帧输入快照
/// - `btn`：`BTN_*` 位掩码常量
///
/// # 返回
///
/// 按键当前是否按下
pub fn button_pressed(input: &InputState, btn: u32) -> bool {
    input.is_pressed(btn)
}

/// 面板按钮的布局描述（含按下态）
///
/// `btn` 为 `BTN_*` 位掩码；`BTN_NONE` 表示非按键的纯文本项（如 QUIT），
/// 恒不点亮。坐标与宽度均为缩放后的像素值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonLayout {
    /// 按钮标签（如 `"L1"`、`"VOL. -"`）
    pub label: &'static str,
    /// 对应键位掩码（`BTN_NONE` = 纯文本，不响应按键）
    pub btn: u32,
    /// 水平坐标（像素）
    pub x: u32,
    /// 垂直坐标（像素）
    pub y: u32,
    /// 按钮宽度（像素）
    pub w: u32,
    /// 是否按下（纯文本项恒 false）
    pub pressed: bool,
}

/// 组容器药丸矩形（DarkGrayPill 背景）
///
/// `w == 0` 表示用默认宽度（`PILL_SIZE * scale`，对应 C 传 0 的语义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PillRect {
    /// 水平坐标（像素）
    pub x: u32,
    /// 垂直坐标（像素）
    pub y: u32,
    /// 宽度（像素，0 = 默认 PILL_SIZE）
    pub w: u32,
}

/// 纯色连接条矩形（DPAD 组 U-D / L-R 之间的填充条，RGB_DARK_GRAY）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StripRect {
    /// 水平坐标（像素）
    pub x: u32,
    /// 垂直坐标（像素）
    pub y: u32,
    /// 宽度（像素）
    pub w: u32,
    /// 高度（像素）
    pub h: u32,
}

/// 面板背景元素（组容器药丸 + DPAD 连接条）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PanelBackground {
    /// 组容器药丸（blit `DarkGrayPill`）
    pub pills: Vec<PillRect>,
    /// 纯色连接条（`fill_rect` `RGB_DARK_GRAY`）
    pub strips: Vec<StripRect>,
}

/// 计算面板背景（对应原 C `main` 的组药丸与连接条绘制段，minput.c:87-260）
///
/// 各组背景：L/R 组（包住 L1/L2、R1/R2 的药丸）、DPAD 组（4 个独立
/// 药丸 + U-D 竖条 + L-R 横条）、ABXY 组（4 个独立药丸）、音量组、
/// 系统组、META 组、L3/R3 组。垂直起点与 [`layout_buttons`] 一致。
///
/// # 参数
///
/// - `caps`：设备按键能力
/// - `scale`：平台缩放倍率
/// - `screen_w`：屏幕物理宽度（像素）
/// - `text_width`：文字宽度测量闭包
///
/// # 返回
///
/// 背景元素（药丸 + 连接条，按绘制顺序）
pub fn layout_background(
    caps: &Capabilities,
    scale: u32,
    screen_w: u32,
    text_width: &impl Fn(&str) -> u32,
) -> PanelBackground {
    let mut bg = PanelBackground::default();

    // 垂直起点（C :65-66）——与 layout_buttons 一致
    let mut oy = PADDING * scale;
    if !caps.has_l3 && !caps.has_r3 {
        oy += PILL_SIZE * scale;
    }

    // ── L 组背景（C :99-104）────────────────────
    {
        let mut x = (BUTTON_MARGIN + PADDING) * scale;
        let y = oy;
        let mut w = button_width("L1", scale, text_width) + BUTTON_MARGIN * scale * 2;
        if caps.has_l2 {
            w += button_width("L2", scale, text_width) + BUTTON_MARGIN * scale;
        }
        if !caps.has_l2 {
            x += PILL_SIZE * scale;
        }
        bg.pills.push(PillRect { x, y, w });
    }

    // ── R 组背景（C :117-125）────────────────────
    {
        let mut w = button_width("R1", scale, text_width) + BUTTON_MARGIN * scale * 2;
        if caps.has_r2 {
            w += button_width("R2", scale, text_width) + BUTTON_MARGIN * scale;
        }
        let mut x = screen_w - w - (BUTTON_MARGIN + PADDING) * scale;
        if !caps.has_r2 {
            x -= PILL_SIZE * scale;
        }
        bg.pills.push(PillRect { x, y: oy, w });
    }

    // ── DPAD 组背景（C :133-156）────────────────
    {
        let mut x = (PADDING + PILL_SIZE) * scale;
        let mut y = oy + PILL_SIZE * 2 * scale;
        let s = PILL_SIZE * scale;

        // U 药丸
        bg.pills.push(PillRect { x, y, w: 0 });
        // U-D 竖条（C :138——y+15s 起，宽 30s 高 60s）
        bg.strips.push(StripRect {
            x,
            y: y + s / 2,
            w: s,
            h: s * 2,
        });
        // D 药丸
        y += s * 2;
        bg.pills.push(PillRect { x, y, w: 0 });
        // L 药丸（左移一个 PILL_SIZE）
        x -= s;
        y -= s;
        bg.pills.push(PillRect { x, y, w: 0 });
        // L-R 横条（C :149——x+15s 起，宽 60s 高 30s）
        bg.strips.push(StripRect {
            x: x + s / 2,
            y,
            w: s * 2,
            h: s,
        });
        // R 药丸（右移两个 PILL_SIZE）
        x += s * 2;
        bg.pills.push(PillRect { x, y, w: 0 });
    }

    // ── ABXY 组背景（C :162-183，无连接条）──────
    {
        let mut x = screen_w - (PADDING + PILL_SIZE * 3) * scale + PILL_SIZE * scale;
        let mut y = oy + PILL_SIZE * 2 * scale;
        let s = PILL_SIZE * scale;

        bg.pills.push(PillRect { x, y, w: 0 }); // X
        y += s * 2;
        bg.pills.push(PillRect { x, y, w: 0 }); // B
        x -= s;
        y -= s;
        bg.pills.push(PillRect { x, y, w: 0 }); // Y
        x += s * 2;
        bg.pills.push(PillRect { x, y, w: 0 }); // A
    }

    // ── 音量组背景（C :192）─────────────────────
    if caps.has_volume {
        let x = (screen_w - 99 * scale) / 2;
        bg.pills.push(PillRect {
            x,
            y: oy + PILL_SIZE * scale,
            w: 98 * scale,
        });
    }

    // ── 系统组背景（C :210）─────────────────────
    if caps.has_menu || caps.has_power {
        let bw = 42;
        let pw = if caps.has_menu && caps.has_power {
            bw * 2 + BUTTON_MARGIN * 3
        } else {
            bw + BUTTON_MARGIN * 2
        };
        let x = (screen_w - pw * scale) / 2;
        bg.pills.push(PillRect {
            x,
            y: oy + PILL_SIZE * 3 * scale,
            w: pw * scale,
        });
    }

    // ── META 组背景（C :229）────────────────────
    {
        let x = (screen_w - 99 * scale) / 2;
        bg.pills.push(PillRect {
            x,
            y: oy + PILL_SIZE * 5 * scale,
            w: 130 * scale,
        });
    }

    // ── L3/R3 背景（C :248/255）─────────────────
    if caps.has_l3 {
        bg.pills.push(PillRect {
            x: (PADDING + PILL_SIZE) * scale,
            y: oy + PILL_SIZE * 6 * scale,
            w: 0,
        });
    }
    if caps.has_r3 {
        bg.pills.push(PillRect {
            x: screen_w - (PADDING + PILL_SIZE * 3) * scale + PILL_SIZE * scale,
            y: oy + PILL_SIZE * 6 * scale,
            w: 0,
        });
    }

    bg
}

/// 计算按钮宽度
///
/// 对应原 C `getButtonWidth()`（minput.c:11-22）：短标签（≤2 字符）
/// 用 `BUTTON_SIZE`；长标签用 `BUTTON_SIZE + 文字宽度`。
/// 音量/系统/META 组的长标签按钮由调用方传固定宽度，不走本函数。
///
/// # 参数
///
/// - `label`：按钮标签
/// - `scale`：平台缩放倍率
/// - `text_width`：文字宽度测量闭包（装配层用 `render::text::size_text`）
fn button_width(label: &str, scale: u32, text_width: &impl Fn(&str) -> u32) -> u32 {
    if label.chars().count() <= 2 {
        BUTTON_SIZE * scale
    } else {
        BUTTON_SIZE * scale + text_width(label)
    }
}

/// 计算按钮面板的完整布局（对应原 C `main` 的绘制段，minput.c:87-260）
///
/// 按 7 组排列：L 组（L1/L2）、R 组（R1/R2）、DPAD（U/D/L/R）、
/// ABXY（X/B/Y/A）、音量组（VOL -/VOL +）、系统组（MENU/POWER）、
/// META 组（SELECT/START + QUIT 文本）、L3/R3。
///
/// 垂直起点 `oy`：无 L3/R3 时下移一个 `PILL_SIZE`（该行被压缩，
/// 对应 C `minput.c:65-66`）。
///
/// # 参数
///
/// - `caps`：设备按键能力
/// - `input`：当前帧输入快照（决定按钮按下态）
/// - `scale`：平台缩放倍率
/// - `screen_w`：屏幕物理宽度（像素）
/// - `text_width`：文字宽度测量闭包（`render::text::size_text` 的宽度）
///
/// # 返回
///
/// 全部按钮的布局描述（按绘制顺序）
#[allow(clippy::too_many_arguments)]
pub fn layout_buttons(
    caps: &Capabilities,
    input: &InputState,
    scale: u32,
    screen_w: u32,
    text_width: &impl Fn(&str) -> u32,
) -> Vec<ButtonLayout> {
    let mut buttons = Vec::new();

    // 垂直起点（C :65-66）——无 L3/R3 时压缩一行
    let mut oy = PADDING * scale;
    if !caps.has_l3 && !caps.has_r3 {
        oy += PILL_SIZE * scale;
    }
    let o = BUTTON_MARGIN * scale; // 按钮在药丸内的边距

    // ── L 组（C :93-108）────────────────────────

    {
        let mut x = (BUTTON_MARGIN + PADDING) * scale;
        let y = oy;
        let mut w = button_width("L1", scale, text_width) + BUTTON_MARGIN * scale * 2;
        let ox = w;
        if caps.has_l2 {
            w += button_width("L2", scale, text_width) + BUTTON_MARGIN * scale;
        }
        if !caps.has_l2 {
            x += PILL_SIZE * scale; // 无 L2 时左移补齐药丸宽度
        }

        buttons.push(ButtonLayout {
            label: "L1",
            btn: BTN_L1,
            x: x + o,
            y: y + o,
            w: button_width("L1", scale, text_width),
            pressed: button_pressed(input, BTN_L1),
        });
        if caps.has_l2 {
            buttons.push(ButtonLayout {
                label: "L2",
                btn: BTN_L2,
                x: x + ox + o,
                y: y + o,
                w: button_width("L2", scale, text_width),
                pressed: button_pressed(input, BTN_L2),
            });
        }
        let _ = w;
    }

    // ── R 组（C :110-129）────────────────────────

    {
        let mut w = button_width("R1", scale, text_width) + BUTTON_MARGIN * scale * 2;
        let ox = w;
        if caps.has_r2 {
            w += button_width("R2", scale, text_width) + BUTTON_MARGIN * scale;
        }
        let mut x = screen_w - w - (BUTTON_MARGIN + PADDING) * scale;
        if !caps.has_r2 {
            x -= PILL_SIZE * scale; // 无 R2 时右移补齐药丸宽度
        }
        let y = oy;

        // 有 R2 时先画 R2（在左），再画 R1（在右）；无 R2 时 R1 顶位
        if caps.has_r2 {
            buttons.push(ButtonLayout {
                label: "R2",
                btn: BTN_R2,
                x: x + o,
                y: y + o,
                w: button_width("R2", scale, text_width),
                pressed: button_pressed(input, BTN_R2),
            });
        }
        buttons.push(ButtonLayout {
            label: "R1",
            btn: BTN_R1,
            x: x + ox + o,
            y: y + o,
            w: button_width("R1", scale, text_width),
            pressed: button_pressed(input, BTN_R1),
        });
    }

    // ── DPAD 组（C :131-157）─────────────────────

    {
        let mut x = (PADDING + PILL_SIZE) * scale;
        let mut y = oy + PILL_SIZE * 2 * scale;

        // U（上）
        buttons.push(ButtonLayout {
            label: "U",
            btn: BTN_DPAD_UP,
            x: x + o,
            y: y + o,
            w: BUTTON_SIZE * scale,
            pressed: button_pressed(input, BTN_DPAD_UP),
        });
        // D（下）
        y += PILL_SIZE * 2 * scale;
        buttons.push(ButtonLayout {
            label: "D",
            btn: BTN_DPAD_DOWN,
            x: x + o,
            y: y + o,
            w: BUTTON_SIZE * scale,
            pressed: button_pressed(input, BTN_DPAD_DOWN),
        });
        // L（左）
        x -= PILL_SIZE * scale;
        y -= PILL_SIZE * scale;
        buttons.push(ButtonLayout {
            label: "L",
            btn: BTN_DPAD_LEFT,
            x: x + o,
            y: y + o,
            w: BUTTON_SIZE * scale,
            pressed: button_pressed(input, BTN_DPAD_LEFT),
        });
        // R（右）
        x += PILL_SIZE * 2 * scale;
        buttons.push(ButtonLayout {
            label: "R",
            btn: BTN_DPAD_RIGHT,
            x: x + o,
            y: y + o,
            w: BUTTON_SIZE * scale,
            pressed: button_pressed(input, BTN_DPAD_RIGHT),
        });
    }

    // ── ABXY 组（C :159-184）─────────────────────

    {
        let mut x = screen_w - (PADDING + PILL_SIZE * 3) * scale + PILL_SIZE * scale;
        let mut y = oy + PILL_SIZE * 2 * scale;

        // X（上）
        buttons.push(ButtonLayout {
            label: "X",
            btn: BTN_X,
            x: x + o,
            y: y + o,
            w: BUTTON_SIZE * scale,
            pressed: button_pressed(input, BTN_X),
        });
        // B（下）
        y += PILL_SIZE * 2 * scale;
        buttons.push(ButtonLayout {
            label: "B",
            btn: BTN_B,
            x: x + o,
            y: y + o,
            w: BUTTON_SIZE * scale,
            pressed: button_pressed(input, BTN_B),
        });
        // Y（左）
        x -= PILL_SIZE * scale;
        y -= PILL_SIZE * scale;
        buttons.push(ButtonLayout {
            label: "Y",
            btn: BTN_Y,
            x: x + o,
            y: y + o,
            w: BUTTON_SIZE * scale,
            pressed: button_pressed(input, BTN_Y),
        });
        // A（右）
        x += PILL_SIZE * 2 * scale;
        buttons.push(ButtonLayout {
            label: "A",
            btn: BTN_A,
            x: x + o,
            y: y + o,
            w: BUTTON_SIZE * scale,
            pressed: button_pressed(input, BTN_A),
        });
    }

    // ── 音量组（C :186-199）──────────────────────

    if caps.has_volume {
        let x = (screen_w - 99 * scale) / 2;
        let y = oy + PILL_SIZE * scale;
        let w = 42 * scale;

        buttons.push(ButtonLayout {
            label: "VOL. -",
            btn: BTN_MINUS,
            x: x + o,
            y: y + o,
            w,
            pressed: button_pressed(input, BTN_MINUS),
        });
        buttons.push(ButtonLayout {
            label: "VOL. +",
            btn: BTN_PLUS,
            x: x + o + w + BUTTON_MARGIN * scale,
            y: y + o,
            w,
            pressed: button_pressed(input, BTN_PLUS),
        });
    }

    // ── 系统组（C :201-221）──────────────────────

    if caps.has_menu || caps.has_power {
        let bw = 42;
        let pw = if caps.has_menu && caps.has_power {
            bw * 2 + BUTTON_MARGIN * 3
        } else {
            bw + BUTTON_MARGIN * 2
        };
        let x = (screen_w - pw * scale) / 2;
        let y = oy + PILL_SIZE * 3 * scale;
        let w = bw * scale;

        if caps.has_menu {
            buttons.push(ButtonLayout {
                label: "MENU",
                btn: BTN_MENU,
                x: x + o,
                y: y + o,
                w,
                pressed: button_pressed(input, BTN_MENU),
            });
        }
        if caps.has_power {
            buttons.push(ButtonLayout {
                label: "POWER",
                btn: BTN_POWER,
                x: x + o + w + BUTTON_MARGIN * scale,
                y: y + o,
                w,
                pressed: button_pressed(input, BTN_POWER),
            });
        }
    }

    // ── META 组（C :223-240）─────────────────────

    {
        let x = (screen_w - 99 * scale) / 2;
        let y = oy + PILL_SIZE * 5 * scale;
        let w = 42 * scale;

        buttons.push(ButtonLayout {
            label: "SELECT",
            btn: BTN_SELECT,
            x: x + o,
            y: y + o,
            w,
            pressed: button_pressed(input, BTN_SELECT),
        });
        buttons.push(ButtonLayout {
            label: "START",
            btn: BTN_START,
            x: x + o + w + BUTTON_MARGIN * scale,
            y: y + o,
            w,
            pressed: button_pressed(input, BTN_START),
        });
        // QUIT 纯文本（不响应按键），垂直居中于按钮行
        buttons.push(ButtonLayout {
            label: "QUIT",
            btn: BTN_NONE,
            x: x + o + w * 2 + BUTTON_MARGIN * scale * 2,
            y,
            w: 0,
            pressed: false,
        });
    }

    // ── L3/R3（C :242-260）───────────────────────

    if caps.has_l3 {
        let x = (PADDING + PILL_SIZE) * scale;
        let y = oy + PILL_SIZE * 6 * scale;
        buttons.push(ButtonLayout {
            label: "L3",
            btn: BTN_L3,
            x: x + o,
            y: y + o,
            w: BUTTON_SIZE * scale,
            pressed: button_pressed(input, BTN_L3),
        });
    }
    if caps.has_r3 {
        let x = screen_w - (PADDING + PILL_SIZE * 3) * scale + PILL_SIZE * scale;
        let y = oy + PILL_SIZE * 6 * scale;
        buttons.push(ButtonLayout {
            label: "R3",
            btn: BTN_R3,
            x: x + o,
            y: y + o,
            w: BUTTON_SIZE * scale,
            pressed: button_pressed(input, BTN_R3),
        });
    }

    buttons
}

// ── 测试 ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use common::power::BatteryStatus;
    use common::video::VideoBuffer;

    /// 测试用 mock 平台——不覆盖能力常量（默认值 false）
    struct MockPlatform;

    impl Platform for MockPlatform {
        const SCREEN_WIDTH: u32 = 640;
        const SCREEN_HEIGHT: u32 = 480;
        const SCALE: u32 = 1;
        const BYTES_PER_PIXEL: u8 = 2;
        const HAS_HDMI: bool = false;
        const HAS_POWER_BUTTON: bool = false;
        const HAS_POWEROFF_BUTTON: bool = false;
        const SUPPORTS_OVERSCAN: bool = false;
        const DEVICE_MODEL: &'static str = "Mock";
        const SDCARD_PATH: &'static str = "/mnt/SDCARD";
        const PLATFORM: &'static str = "mock";
        const BTN_SLEEP: u32 = 0;
        const BTN_MOD_BRIGHTNESS: u32 = 0;
        const BTN_MOD_VOLUME: u32 = 0;
        const BTN_MOD_PLUS: u32 = 0;
        const BTN_MOD_MINUS: u32 = 0;

        fn init_video(&mut self) {}
        fn quit_video(&mut self) {}
        fn flip(&mut self, _buffer: &VideoBuffer, _wait_vsync: bool) {}
        fn init_input(&mut self) {}
        fn quit_input(&mut self) {}
        fn poll_input(&mut self) -> InputState {
            InputState::new()
        }
        fn init_audio(&mut self, _sample_rate: u32) -> u32 {
            0
        }
        fn quit_audio(&mut self) {}
        fn pause_audio(&mut self, _pause: bool) {}
        fn push_audio(&mut self, _frames: &[common::audio::AudioFrame]) -> usize {
            0
        }
        fn power_off(&mut self) {}
        fn get_battery_status(&self) -> BatteryStatus {
            BatteryStatus {
                percentage: 100,
                charging: false,
            }
        }
        fn enable_backlight(&mut self, _enable: bool) {}
        fn set_cpu_speed(&mut self, _speed: common::power::CpuSpeed) {}
        fn prepare_sleep(&mut self) {}
        fn complete_wake(&mut self) {}
        fn now_ms(&self) -> u32 {
            0
        }
    }

    /// 全能力（含 L3/R3/音量/系统键）
    fn full_caps() -> Capabilities {
        Capabilities {
            has_l2: true,
            has_r2: true,
            has_l3: true,
            has_r3: true,
            has_ls: true,
            has_rs: true,
            has_volume: true,
            has_menu: true,
            has_power: true,
        }
    }

    /// 最小能力（无 L2/R2/L3/R3/音量/系统键——仅 L1/R1/DPAD/ABXY/META）
    fn minimal_caps() -> Capabilities {
        Capabilities {
            has_l2: false,
            has_r2: false,
            has_l3: false,
            has_r3: false,
            has_ls: false,
            has_rs: false,
            has_volume: false,
            has_menu: false,
            has_power: false,
        }
    }

    /// 固定宽度测量（测试不依赖字体）
    fn fixed_width(label: &str) -> u32 {
        label.len() as u32 * 8
    }

    /// 提取布局中的标签集合
    fn labels(buttons: &[ButtonLayout]) -> Vec<&'static str> {
        buttons.iter().map(|b| b.label).collect()
    }

    // ── Capabilities::from_platform ──────────────

    /// 能力收集自 trait 常量
    #[test]
    fn from_platform_collects_all_capabilities() {
        let caps = Capabilities::from_platform::<MockPlatform>();
        assert!(!caps.has_l2);
        assert!(!caps.has_r2);
        assert!(!caps.has_l3);
        assert!(!caps.has_r3);
        assert!(!caps.has_ls);
        assert!(!caps.has_rs);
        assert!(!caps.has_volume);
        assert!(!caps.has_menu);
        assert!(!caps.has_power);
    }

    // ── button_pressed ───────────────────────────

    #[test]
    fn button_pressed_true_when_active() {
        let input = InputState {
            pressed: BTN_A,
            ..InputState::new()
        };
        assert!(button_pressed(&input, BTN_A));
        assert!(!button_pressed(&input, BTN_B));
    }

    #[test]
    fn button_pressed_combined_buttons() {
        let input = InputState {
            pressed: BTN_DPAD_UP | BTN_L1,
            ..InputState::new()
        };
        assert!(button_pressed(&input, BTN_DPAD_UP));
        assert!(button_pressed(&input, BTN_L1));
    }

    #[test]
    fn button_pressed_after_release() {
        let input = InputState {
            just_released: BTN_A,
            ..InputState::new()
        };
        assert!(!button_pressed(&input, BTN_A));
    }

    #[test]
    fn button_pressed_select_start_combo() {
        let input = InputState {
            pressed: BTN_SELECT | BTN_START,
            ..InputState::new()
        };
        assert!(button_pressed(&input, BTN_SELECT));
        assert!(button_pressed(&input, BTN_START));
    }

    // ── layout_buttons：按钮集合 ─────────────────

    /// 全能力设备：全部按钮出现（L1/L2/R1/R2/U/D/L/R/X/B/Y/A/
    /// VOL. -/VOL. +/MENU/POWER/SELECT/START/QUIT/L3/R3）
    #[test]
    fn full_capability_buttons_all_present() {
        let caps = full_caps();
        let input = InputState::new();
        let layout = layout_buttons(&caps, &input, 2, 1280, &fixed_width);
        let got = labels(&layout);
        for expect in [
            "L1", "L2", "R1", "R2", "U", "D", "L", "R", "X", "B", "Y", "A", "VOL. -", "VOL. +",
            "MENU", "POWER", "SELECT", "START", "QUIT", "L3", "R3",
        ] {
            assert!(got.contains(&expect), "缺少按钮 {expect}: {got:?}");
        }
    }

    /// 最小能力设备：仅基础按钮（L1/R1/DPAD/ABXY/META），无 L2/R2/L3/R3/音量/系统
    #[test]
    fn minimal_capability_buttons_only_basics() {
        let caps = minimal_caps();
        let input = InputState::new();
        let layout = layout_buttons(&caps, &input, 2, 1280, &fixed_width);
        let got = labels(&layout);
        assert!(got.contains(&"L1"));
        assert!(got.contains(&"R1"));
        assert!(!got.contains(&"L2"));
        assert!(!got.contains(&"R2"));
        assert!(!got.contains(&"L3"));
        assert!(!got.contains(&"R3"));
        assert!(!got.contains(&"VOL. -"));
        assert!(!got.contains(&"VOL. +"));
        assert!(!got.contains(&"MENU"));
        assert!(!got.contains(&"POWER"));
        // DPAD/ABXY/META 恒有
        for expect in [
            "U", "D", "L", "R", "X", "B", "Y", "A", "SELECT", "START", "QUIT",
        ] {
            assert!(got.contains(&expect), "缺少按钮 {expect}: {got:?}");
        }
    }

    /// 仅音量键（无系统键）→ 音量组出现、系统组不出现
    #[test]
    fn volume_without_system_keys() {
        let caps = Capabilities {
            has_volume: true,
            ..minimal_caps()
        };
        let input = InputState::new();
        let layout = layout_buttons(&caps, &input, 2, 1280, &fixed_width);
        let got = labels(&layout);
        assert!(got.contains(&"VOL. -"));
        assert!(got.contains(&"VOL. +"));
        assert!(!got.contains(&"MENU"));
        assert!(!got.contains(&"POWER"));
    }

    /// 仅 MENU（无电源键）→ 系统组只画 MENU
    #[test]
    fn system_group_menu_only() {
        let caps = Capabilities {
            has_menu: true,
            ..minimal_caps()
        };
        let input = InputState::new();
        let layout = layout_buttons(&caps, &input, 2, 1280, &fixed_width);
        let got = labels(&layout);
        assert!(got.contains(&"MENU"));
        assert!(!got.contains(&"POWER"));
    }

    /// 仅 POWER（无 MENU）→ 系统组只画 POWER
    #[test]
    fn system_group_power_only() {
        let caps = Capabilities {
            has_power: true,
            ..minimal_caps()
        };
        let input = InputState::new();
        let layout = layout_buttons(&caps, &input, 2, 1280, &fixed_width);
        let got = labels(&layout);
        assert!(got.contains(&"POWER"));
        assert!(!got.contains(&"MENU"));
    }

    // ── layout_buttons：垂直布局 ─────────────────

    /// 无 L3/R3 时垂直起点下移一个 PILL_SIZE（oy 压缩，C :65-66）
    #[test]
    fn vertical_offset_compressed_without_l3_r3() {
        let input = InputState::new();
        let scale = 2;

        // 无 L3/R3：oy = PADDING*scale + PILL_SIZE*scale = 40 + 60 = 100
        let minimal = layout_buttons(&minimal_caps(), &input, scale, 1280, &fixed_width);
        let l1_min = minimal.iter().find(|b| b.label == "L1").unwrap();
        assert_eq!(
            l1_min.y,
            (PADDING + PILL_SIZE) * scale + BUTTON_MARGIN * scale
        );

        // 有 L3/R3：oy = PADDING*scale = 20
        let full = layout_buttons(&full_caps(), &input, scale, 1280, &fixed_width);
        let l1_full = full.iter().find(|b| b.label == "L1").unwrap();
        assert_eq!(l1_full.y, PADDING * scale + BUTTON_MARGIN * scale);
    }

    /// L3 行位置：有 L3 时位于 oy + PILL_SIZE*6
    #[test]
    fn l3_position_at_bottom_row() {
        let caps = full_caps();
        let input = InputState::new();
        let scale = 2;
        let layout = layout_buttons(&caps, &input, scale, 1280, &fixed_width);
        let l3 = layout.iter().find(|b| b.label == "L3").unwrap();
        // oy = PADDING*scale = 20；y = 20 + 30*6*2 = 380（+ o = 10）
        assert_eq!(
            l3.y,
            PADDING * scale + PILL_SIZE * 6 * scale + BUTTON_MARGIN * scale
        );
    }

    // ── layout_buttons：按下态 ───────────────────

    /// 按下态跟随 InputState
    #[test]
    fn pressed_state_follows_input() {
        let caps = full_caps();
        let input = InputState {
            pressed: BTN_A | BTN_DPAD_UP,
            ..InputState::new()
        };
        let layout = layout_buttons(&caps, &input, 2, 1280, &fixed_width);
        let a = layout.iter().find(|b| b.label == "A").unwrap();
        let up = layout.iter().find(|b| b.label == "U").unwrap();
        let b = layout.iter().find(|b| b.label == "B").unwrap();
        assert!(a.pressed);
        assert!(up.pressed);
        assert!(!b.pressed);
    }

    /// QUIT 是纯文本（BTN_NONE），恒不点亮
    #[test]
    fn quit_is_text_only() {
        let caps = minimal_caps();
        let input = InputState {
            pressed: BTN_SELECT | BTN_START,
            ..InputState::new()
        };
        let layout = layout_buttons(&caps, &input, 2, 1280, &fixed_width);
        let quit = layout.iter().find(|b| b.label == "QUIT").unwrap();
        assert_eq!(quit.btn, BTN_NONE);
        assert!(!quit.pressed);
    }

    // ── layout_background：背景元素 ──────────────

    /// 全能力设备：L/R/DPAD(4+2条)/ABXY(4)/音量/系统/META/L3/R3 药丸齐全
    #[test]
    fn background_full_capabilities() {
        let caps = full_caps();
        let bg = layout_background(&caps, 2, 1280, &fixed_width);
        // 药丸：L、R、DPAD×4、ABXY×4、音量、系统、META、L3、R3 = 15
        assert_eq!(bg.pills.len(), 15, "药丸数: {:?}", bg.pills);
        // 连接条：DPAD U-D 竖条 + L-R 横条 = 2
        assert_eq!(bg.strips.len(), 2, "连接条数: {:?}", bg.strips);
    }

    /// 最小能力设备：L/R/DPAD(4+2条)/ABXY(4)/META 药丸，无音量/系统/L3/R3
    #[test]
    fn background_minimal_capabilities() {
        let caps = minimal_caps();
        let bg = layout_background(&caps, 2, 1280, &fixed_width);
        // 药丸：L、R、DPAD×4、ABXY×4、META = 11
        assert_eq!(bg.pills.len(), 11, "药丸数: {:?}", bg.pills);
        assert_eq!(bg.strips.len(), 2);
    }

    /// 背景垂直偏移与按钮一致（无 L3/R3 时 oy 压缩）
    #[test]
    fn background_vertical_offset_matches_buttons() {
        let scale = 2;
        let minimal = layout_background(&minimal_caps(), scale, 1280, &fixed_width);
        let full = layout_background(&full_caps(), scale, 1280, &fixed_width);
        // 最小能力：L 组药丸 y = (PADDING+PILL_SIZE)*scale = 100
        let l_min = minimal.pills[0];
        assert_eq!(l_min.y, (PADDING + PILL_SIZE) * scale);
        // 全能力：L 组药丸 y = PADDING*scale = 20
        let l_full = full.pills[0];
        assert_eq!(l_full.y, PADDING * scale);
    }
}
