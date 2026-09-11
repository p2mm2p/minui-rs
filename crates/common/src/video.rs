//! 视频相关基础类型
//!
//! 本模块定义了渲染管线中使用的所有数据结构，兼容原版 C 中 `SDL_Surface`
//! 的使用方式，但不依赖 SDL。
//!
//! ## 设计决策
//!
//! `VideoBuffer.pixels` 固定为 `Vec<u16>`（RGB565）。原版 MinUI 所有
//! 目标平台均使用 RGB565（`FIXED_BPP=2`）。如果未来有平台需要 RGB888，
//! 可改为枚举类型，但当前阶段保持简单。
//!
//! ## 与原 C 代码的对比
//!
//! 原版使用 `SDL_Surface` 作为统一的像素容器，Rust 版使用自定义
//! `VideoBuffer`——一个裸 `Vec<u16>` 加上宽高和 pitch 元数据。这消除了
//! 对 SDL 的依赖，渲染函数直接操作像素数组。
//!

/// RGB565 像素缓冲区
///
/// 这是 `Platform` trait 和所有渲染函数之间传递的"画面"类型。
/// `pitch` 表示每行像素数（通常等于 `width`，但可能更大以对齐）。
pub struct VideoBuffer {
    /// 像素数据（RGB565 格式，每像素 2 字节）
    pub pixels: Vec<u16>,
    /// 可视宽度（像素）
    pub width: u32,
    /// 可视高度（像素）
    pub height: u32,
    /// 每行像素数（≥ width），用于步长计算
    pub pitch: u32,
}

impl VideoBuffer {
    /// 创建指定尺寸的黑色缓冲区
    pub fn new(width: u32, height: u32) -> Self {
        let pitch = width;
        let pixel_count = (pitch * height) as usize;
        Self {
            pixels: vec![0u16; pixel_count],
            width,
            height,
            pitch,
        }
    }

    /// 用纯色填充矩形区域
    ///
    /// 所有填充像素被设为同一个 RGB565 颜色值。不做边界裁剪——调用方需确保
    /// `rect` 完全位于 `(width, height)` 范围内。
    ///
    /// 对应原 C `SDL_FillRect()`。
    ///
    /// ## 参数
    ///
    /// * `rect` - 目标矩形区域（x/y/w/h，单位像素）
    /// * `color` - RGB565 格式的颜色值（如 `RGB_WHITE` = `0xFFFF`）
    pub fn fill_rect(&mut self, rect: Rect, color: u16) {
        if rect.w == 0 || rect.h == 0 {
            return;
        }
        let pitch = self.pitch as usize;
        for row in rect.y as usize..(rect.y + rect.h) as usize {
            let start = row * pitch + rect.x as usize;
            let end = start + rect.w as usize;
            self.pixels[start..end].fill(color);
        }
    }
}

/// 矩形区域
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// 坐标点
pub struct Point {
    pub x: u32,
    pub y: u32,
}

// ── 颜色常量（RGB565 格式）─────────────────────

/// 白色
pub const RGB_WHITE: u16 = 0xFFFF;
/// 黑色
pub const RGB_BLACK: u16 = 0x0000;
/// 浅灰
pub const RGB_LIGHT_GRAY: u16 = 0xCE79;
/// 灰色
pub const RGB_GRAY: u16 = 0x9CD3;
/// 深灰
pub const RGB_DARK_GRAY: u16 = 0x4208;
/// 深灰文字色
///
/// 对应原 C `defines.h:41` 的 `TRIAD_DARK_TEXT 0x66,0x66,0x66`（`COLOR_DARK_TEXT`）。
/// RGB565 计算：R=(0x66>>3)=0x0C, G=(0x66>>2)=0x19, B=(0x66>>3)=0x0C
/// → 0x0C<<11 | 0x19<<5 | 0x0C = 0x632C。
///
/// 用途：版本页与按钮提示的**标签文字**。与 `RGB_DARK_GRAY`（素材色
/// 0x26,0x26,0x26）刻意区分——后者用于灰色 pill 等素材，前者用于文字。
pub const RGB_DARK_TEXT: u16 = 0x632C;

// ── Vsync 模式 ──────────────────────────────────

/// 垂直同步模式
///
/// 对应原 C `api.h:167-171` 的 `VSYNC_OFF` / `VSYNC_LENIENT` / `VSYNC_STRICT` 枚举。
/// 由 `Platform::set_vsync` 和帧计时逻辑（`GFX_flip`/`GFX_sync` Rust 等价实现）使用。
pub enum VsyncMode {
    /// 不等待垂直同步
    Off = 0,
    /// 默认。帧预算内等待 vsync，超预算则跳过
    Lenient,
    /// 始终等待垂直同步
    Strict,
}

/// 每帧时间预算（毫秒）
///
/// 60fps = 1000ms / 60 ≈ 17ms。对应原 C `api.c:202` 的 `#define FRAME_BUDGET 17`。
pub const FRAME_BUDGET_MS: u32 = 17;

// ── 字体 ─────────────────────────────────────────

/// 默认字体文件路径
///
/// 对应原 C `defines.h` 中 `FONT_PATH` 宏：
/// `#define FONT_PATH RES_PATH "/BPreplayBold-unhinted.otf"`
/// 完整路径由调用方拼接 SD 卡根目录前缀后构成。
pub const FONT_PATH: &str = "/.system/res/BPreplayBold-unhinted.otf";

// ── 布局常量（未缩放像素）─────────────────────

/// 药丸 UI 元素的基准尺寸（未缩放像素）
///
/// 所有与 `PILL_SIZE` 存在数学关系的常量均以此为基准：
/// - `BUTTON_MARGIN = (PILL_SIZE - BUTTON_SIZE) / 2`
/// - `PADDING = PILL_SIZE / 3`
///
/// 对应原 C `defines.h:51`：`#define PILL_SIZE 30`。
///
/// **项目约定**：所有 UI 布局常量定义在本模块。禁止在业务 crate 中本地重复定义。
/// 如需新常量，请在本模块的「布局常量」区块中新增。
pub const PILL_SIZE: u32 = 30;

/// 按钮 sprite 尺寸（未缩放像素）
///
/// 精灵图集中 `ASSET_BUTTON` 的宽度和高度。
/// 对应原 C `defines.h:52`：`#define BUTTON_SIZE 20`。
///
/// **项目约定**：所有 UI 布局常量定义在 `common::video` 模块。
/// 禁止在业务 crate 中本地重复定义。如需新常量，请在本模块中新增。
pub const BUTTON_SIZE: u32 = 20;

/// 按钮与文字之间的间距（未缩放像素）
///
/// `(PILL_SIZE - BUTTON_SIZE) / 2`。
/// 对应原 C `defines.h:53`：`#define BUTTON_MARGIN 5`。
///
/// **项目约定**：所有 UI 布局常量定义在 `common::video` 模块。
/// 禁止在业务 crate 中本地重复定义。如需新常量，请在本模块中新增。
pub const BUTTON_MARGIN: u32 = 5;

/// 按钮组内部填充（未缩放像素）
///
/// 对应原 C `defines.h:54`：`#define BUTTON_PADDING 12`。
///
/// **项目约定**：所有 UI 布局常量定义在 `common::video` 模块。
/// 禁止在业务 crate 中本地重复定义。如需新常量，请在本模块中新增。
pub const BUTTON_PADDING: u32 = 12;

/// 页面边缘留白（未缩放像素）
///
/// `PILL_SIZE / 3`。用于页面顶部、底部、左右边距。
/// 对应原 C `defines.h:63`：`#define PADDING 10`。
///
/// **项目约定**：所有 UI 布局常量定义在 `common::video` 模块。
/// 禁止在业务 crate 中本地重复定义。如需新常量，请在本模块中新增。
pub const PADDING: u32 = 10;

/// 列表每页可显示行数（通用默认，未缩放像素）
///
/// 对应原 C `defines.h:58-59` 的 `#ifndef MAIN_ROW_COUNT` 通用默认值
/// （`FIXED_HEIGHT / (PILL_SIZE * FIXED_SCALE) - 2` 的取整结果）。
/// 被 minui 列表渲染用于滚动窗口与翻页步长计算。
///
/// **平台差异化**：本常量是通用默认——平台自定义值（如 tg5040
/// smart=8、brick=7；my355 类平台按运行时 HDMI 状态分支）通过覆盖
/// `Platform::main_row_count()` 方法表达，不在平台 crate 中重复定义。
///
/// **项目约定**：所有 UI 布局常量定义在 `common::video` 模块。
/// 禁止在业务 crate 中本地重复定义。如需新常量，请在本模块中新增。
pub const MAIN_ROW_COUNT: u32 = 6;

/// 亮度/音量条滑块尺寸（未缩放像素）
///
/// 对应原 C `defines.h:55`：`#define SETTINGS_SIZE 4`。
///
/// **项目约定**：所有 UI 布局常量定义在 `common::video` 模块。
/// 禁止在业务 crate 中本地重复定义。如需新常量，请在本模块中新增。
pub const SETTINGS_SIZE: u32 = 4;

/// 亮度/音量条宽度（未缩放像素）
///
/// 对应原 C `defines.h:56`：`#define SETTINGS_WIDTH 80`。
///
/// **项目约定**：所有 UI 布局常量定义在 `common::video` 模块。
/// 禁止在业务 crate 中本地重复定义。如需新常量，请在本模块中新增。
pub const SETTINGS_WIDTH: u32 = 80;

/// 亮度最小值
///
/// 亮度调节范围的下界。
/// 对应原 C `defines.h:8`：`#define BRIGHTNESS_MIN 0`。
///
/// **项目约定**：所有 UI 布局常量定义在 `common::video` 模块。
/// 禁止在业务 crate 中本地重复定义。如需新常量，请在本模块中新增。
pub const BRIGHTNESS_MIN: u32 = 0;

/// 亮度最大值
///
/// 亮度调节范围的上界。
/// 对应原 C `defines.h:9`：`#define BRIGHTNESS_MAX 10`。
///
/// **项目约定**：所有 UI 布局常量定义在 `common::video` 模块。
/// 禁止在业务 crate 中本地重复定义。如需新常量，请在本模块中新增。
pub const BRIGHTNESS_MAX: u32 = 10;

/// 音量最小值
///
/// 音量调节范围的下界。
/// 对应原 C `defines.h:6`：`#define VOLUME_MIN 0`。
///
/// **项目约定**：所有 UI 布局常量定义在 `common::video` 模块。
/// 禁止在业务 crate 中本地重复定义。如需新常量，请在本模块中新增。
pub const VOLUME_MIN: u32 = 0;

/// 音量最大值
///
/// 音量调节范围的上界。
/// 对应原 C `defines.h:7`：`#define VOLUME_MAX 20`。
///
/// **项目约定**：所有 UI 布局常量定义在 `common::video` 模块。
/// 禁止在业务 crate 中本地重复定义。如需新常量，请在本模块中新增。
pub const VOLUME_MAX: u32 = 20;

// ── Asset 枚举 ──────────────────────────────────

/// UI 素材标识
///
/// 精灵图集中每个素材的唯一标识符。枚举成员可用作 `ASSET_RECTS` 数组的索引，
/// 获取该素材在精灵图集中的源矩形坐标。
///
/// 成员分为三组：
/// - **单色素材**（索引 0–13）：用颜色常量（如 `RGB_WHITE`）单色填充的几何形状
/// - **`Colors`**（索引 14）：分隔符，标记单色和彩色素材的边界。不可渲染
/// - **彩色素材**（索引 15–24）：用精灵图集原图颜色渲染
///
/// ## 与原 C 代码的对比
///
/// 对应原版 C `api.h` 的 `ASSET_*` 枚举（`ASSET_WHITE_PILL` 等）。
/// 枚举顺序与原 C 完全一致，保证与 `ASSET_RECTS` 坐标表索引兼容。
/// Rust 版去掉了 `ASSET_` 前缀——`Asset` 类型名已提供命名空间。
#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asset {
    /// 白色药丸圆角矩形（选中项高亮）
    WhitePill = 0,
    /// 黑色药丸圆角矩形（菜单标题背景）
    BlackPill = 1,
    /// 深灰药丸圆角矩形（状态栏背景）
    DarkGrayPill = 2,
    /// 选项卡片（菜单选项行选中背景）
    Option = 3,
    /// 按钮圆角矩形（单个按键标签背景）
    Button = 4,
    /// 页面背景（存档槽窗口背景）
    PageBg = 5,
    /// 存档状态背景（存档槽预览窗口）
    StateBg = 6,
    /// 页面指示点（当前存档槽高亮）
    Page = 7,
    /// 进度条填充
    Bar = 8,
    /// 进度条背景（亮度/音量滑条底层）
    BarBg = 9,
    /// 菜单中进度条背景（菜单模式下）
    BarBgMenu = 10,
    /// 下划线（时钟光标指示器）
    Underline = 11,
    /// 圆点（存档槽分页指示器——未选中状态）
    Dot = 12,
    /// 挖空圆角矩形（按钮未按下状态）
    Hole = 13,
    /// 分隔符：单色素材与彩色素材的边界
    ///
    /// 不是实际可渲染的 asset，仅用于保持与原 C `ASSET_COLORS` 的索引兼容性。
    Colors = 14,
    /// 亮度图标
    Brightness = 15,
    /// 静音图标
    VolumeMute = 16,
    /// 音量图标
    Volume = 17,
    /// 电池外壳图标（正常电量）
    BatteryIcon = 18,
    /// 电池外壳图标（低电量）
    BatteryLow = 19,
    /// 电池填充条（正常电量）
    BatteryFill = 20,
    /// 电池填充条（低电量）
    BatteryFillLow = 21,
    /// 充电闪电图标
    BatteryBolt = 22,
    /// 向上滚动箭头
    ScrollUp = 23,
    /// 向下滚动箭头
    ScrollDown = 24,
    /// WiFi 信号图标
    WiFi = 25,
    /// 元数据：Asset 枚举成员总数
    ///
    /// 对应原版 C `ASSET_COUNT`。用于定义 `ASSET_RECTS` 数组长度。
    Count = 26,
}

/// 单色素材的颜色映射表（RGB565）
///
/// 索引为 `Asset` 枚举值 0–13（`Asset::Colors` 之前的 14 个单色素材），
/// 每个元素为对应的 RGB565 颜色值。彩色素材（索引 15–25）不使用此表——
/// 它们直接使用精灵图集中的像素颜色。
///
/// 对应原 C `api.c:108-121` 的 `asset_rgbs[]` 数组。
/// 原 C 中 `asset_rgbs` 存储 `SDL_MapRGB` 的运行期返回值，
/// Rust 版直接使用编译期 RGB565 常量。
pub const ASSET_RGBS: [u16; Asset::Colors as usize] = [
    RGB_WHITE,      // WhitePill
    RGB_BLACK,      // BlackPill
    RGB_DARK_GRAY,  // DarkGrayPill
    RGB_DARK_GRAY,  // Option
    RGB_WHITE,      // Button
    RGB_WHITE,      // PageBg
    RGB_WHITE,      // StateBg
    RGB_BLACK,      // Page
    RGB_WHITE,      // Bar
    RGB_BLACK,      // BarBg
    RGB_DARK_GRAY,  // BarBgMenu
    RGB_GRAY,       // Underline
    RGB_LIGHT_GRAY, // Dot
    RGB_BLACK,      // Hole
];

/// 精灵图集中每个 Asset 的源矩形坐标
///
/// 坐标来自原版 C `api.c` 的 `asset_rects[]` 数组（第 123–147 行），
/// 已移除 `SCALE4()` 宏的 `FIXED_SCALE` 缩放因子。所有值均为**未缩放**的
/// 原始像素坐标，精灵图集画布为 100×64 像素。
///
/// render crate 的 `blit_asset` 函数负责在运行时按平台 `SCALE` 进行缩放。
///
/// ## 数组索引
///
/// 使用 `Asset` 枚举的 `as usize` 值作为索引：
/// ```ignore
/// let rect = &ASSET_RECTS[Asset::WhitePill as usize];
/// ```
pub const ASSET_RECTS: [Rect; Asset::Count as usize] = [
    // WhitePill     — SCALE4( 1,  1, 30, 30)
    Rect {
        x: 1,
        y: 1,
        w: 30,
        h: 30,
    },
    // BlackPill     — SCALE4(33,  1, 30, 30)
    Rect {
        x: 33,
        y: 1,
        w: 30,
        h: 30,
    },
    // DarkGrayPill  — SCALE4(65,  1, 30, 30)
    Rect {
        x: 65,
        y: 1,
        w: 30,
        h: 30,
    },
    // Option        — SCALE4(97,  1, 20, 20)
    Rect {
        x: 97,
        y: 1,
        w: 20,
        h: 20,
    },
    // Button        — SCALE4( 1, 33, 20, 20)
    Rect {
        x: 1,
        y: 33,
        w: 20,
        h: 20,
    },
    // PageBg        — SCALE4(64, 33, 15, 15)
    Rect {
        x: 64,
        y: 33,
        w: 15,
        h: 15,
    },
    // StateBg       — SCALE4(23, 54,  8,  8)
    Rect {
        x: 23,
        y: 54,
        w: 8,
        h: 8,
    },
    // Page          — SCALE4(39, 54,  6,  6)
    Rect {
        x: 39,
        y: 54,
        w: 6,
        h: 6,
    },
    // Bar           — SCALE4(33, 58,  4,  4)
    Rect {
        x: 33,
        y: 58,
        w: 4,
        h: 4,
    },
    // BarBg         — SCALE4(15, 55,  4,  4)
    Rect {
        x: 15,
        y: 55,
        w: 4,
        h: 4,
    },
    // BarBgMenu     — SCALE4(85, 56,  4,  4)
    Rect {
        x: 85,
        y: 56,
        w: 4,
        h: 4,
    },
    // Underline     — SCALE4(85, 51,  3,  3)
    Rect {
        x: 85,
        y: 51,
        w: 3,
        h: 3,
    },
    // Dot           — SCALE4(33, 54,  2,  2)
    Rect {
        x: 33,
        y: 54,
        w: 2,
        h: 2,
    },
    // Hole          — SCALE4( 1, 63, 20, 20)
    Rect {
        x: 1,
        y: 63,
        w: 20,
        h: 20,
    },
    // Colors        — 分隔符，无对应 sprite
    Rect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    },
    // Brightness    — SCALE4(23, 33, 19, 19)
    Rect {
        x: 23,
        y: 33,
        w: 19,
        h: 19,
    },
    // VolumeMute    — SCALE4(44, 33, 10, 16)
    Rect {
        x: 44,
        y: 33,
        w: 10,
        h: 16,
    },
    // Volume        — SCALE4(44, 33, 18, 16)
    Rect {
        x: 44,
        y: 33,
        w: 18,
        h: 16,
    },
    // BatteryIcon   — SCALE4(47, 51, 17, 10)
    Rect {
        x: 47,
        y: 51,
        w: 17,
        h: 10,
    },
    // BatteryLow    — SCALE4(66, 51, 17, 10)
    Rect {
        x: 66,
        y: 51,
        w: 17,
        h: 10,
    },
    // BatteryFill   — SCALE4(81, 33, 12,  6)
    Rect {
        x: 81,
        y: 33,
        w: 12,
        h: 6,
    },
    // BatteryFillLow— SCALE4( 1, 55, 12,  6)
    Rect {
        x: 1,
        y: 55,
        w: 12,
        h: 6,
    },
    // BatteryBolt   — SCALE4(81, 41, 12,  6)
    Rect {
        x: 81,
        y: 41,
        w: 12,
        h: 6,
    },
    // ScrollUp      — SCALE4(97, 23, 24,  6)
    Rect {
        x: 97,
        y: 23,
        w: 24,
        h: 6,
    },
    // ScrollDown    — SCALE4(97, 31, 24,  6)
    Rect {
        x: 97,
        y: 31,
        w: 24,
        h: 6,
    },
    // WiFi          — SCALE4(95, 39, 14, 10)
    Rect {
        x: 95,
        y: 39,
        w: 14,
        h: 10,
    },
];

// ── 测试 ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Asset 枚举测试 ──────────────────────────

    #[test]
    fn asset_count_is_26() {
        // Asset::Count 的值应为 26（与 C 的 ASSET_COUNT 一致）
        assert_eq!(Asset::Count as usize, 26);
    }

    #[test]
    fn asset_count_matches_rects_array_length() {
        assert_eq!(ASSET_RECTS.len(), Asset::Count as usize);
    }

    #[test]
    fn asset_colors_is_separator_at_index_14() {
        assert_eq!(Asset::Colors as usize, 14);
    }

    #[test]
    fn asset_first_is_white_pill() {
        assert_eq!(Asset::WhitePill as usize, 0);
    }

    #[test]
    fn asset_last_is_wifi() {
        assert_eq!(Asset::WiFi as usize, 25);
    }

    // ── ASSET_RECTS 坐标测试 ────────────────────

    #[test]
    fn white_pill_rect() {
        assert_eq!(
            ASSET_RECTS[Asset::WhitePill as usize],
            Rect {
                x: 1,
                y: 1,
                w: 30,
                h: 30
            }
        );
    }

    #[test]
    fn wifi_rect() {
        assert_eq!(
            ASSET_RECTS[Asset::WiFi as usize],
            Rect {
                x: 95,
                y: 39,
                w: 14,
                h: 10
            }
        );
    }

    #[test]
    fn hole_rect() {
        assert_eq!(
            ASSET_RECTS[Asset::Hole as usize],
            Rect {
                x: 1,
                y: 63,
                w: 20,
                h: 20
            }
        );
    }

    #[test]
    fn button_rect() {
        assert_eq!(
            ASSET_RECTS[Asset::Button as usize],
            Rect {
                x: 1,
                y: 33,
                w: 20,
                h: 20
            }
        );
    }

    #[test]
    fn battery_icon_rect() {
        assert_eq!(
            ASSET_RECTS[Asset::BatteryIcon as usize],
            Rect {
                x: 47,
                y: 51,
                w: 17,
                h: 10
            }
        );
    }

    #[test]
    fn scroll_up_rect() {
        assert_eq!(
            ASSET_RECTS[Asset::ScrollUp as usize],
            Rect {
                x: 97,
                y: 23,
                w: 24,
                h: 6
            }
        );
    }

    #[test]
    fn colors_separator_has_zero_rect() {
        assert_eq!(
            ASSET_RECTS[Asset::Colors as usize],
            Rect {
                x: 0,
                y: 0,
                w: 0,
                h: 0
            }
        );
    }

    // ── fill_rect 测试 ──────────────────────────

    #[test]
    fn fill_rect_fills_sub_region() {
        let mut dst = VideoBuffer::new(64, 64);
        let rect = Rect {
            x: 10,
            y: 10,
            w: 20,
            h: 30,
        };
        dst.fill_rect(rect, RGB_WHITE);

        // 填充区域内应为白色
        for row in 10..40 {
            for col in 10..30 {
                let idx = row as usize * 64 + col as usize;
                assert_eq!(dst.pixels[idx], RGB_WHITE, "像素 ({col}, {row}) 应为白色");
            }
        }
        // 填充区域外应保持黑色
        assert_eq!(dst.pixels[0], 0x0000); // 左上角
        assert_eq!(dst.pixels[63 * 64 + 63], 0x0000); // 右下角
    }

    #[test]
    fn fill_rect_fills_entire_buffer() {
        let mut dst = VideoBuffer::new(8, 8);
        let rect = Rect {
            x: 0,
            y: 0,
            w: 8,
            h: 8,
        };
        dst.fill_rect(rect, RGB_DARK_GRAY);
        assert!(dst.pixels.iter().all(|&p| p == RGB_DARK_GRAY));
    }

    #[test]
    fn fill_rect_empty_noop() {
        let mut dst = VideoBuffer::new(16, 16);

        // w=0
        dst.fill_rect(
            Rect {
                x: 0,
                y: 0,
                w: 0,
                h: 10,
            },
            RGB_WHITE,
        );
        // h=0
        dst.fill_rect(
            Rect {
                x: 0,
                y: 0,
                w: 10,
                h: 0,
            },
            RGB_WHITE,
        );

        // 所有像素仍为黑色
        assert!(dst.pixels.iter().all(|&p| p == 0x0000));
    }

    #[test]
    fn fill_rect_respects_pitch() {
        // 创建一个 pitch > width 的缓冲区（宽度 16，行距 32）
        let mut dst = VideoBuffer {
            pixels: vec![0u16; 32 * 16],
            width: 16,
            height: 16,
            pitch: 32,
        };
        let rect = Rect {
            x: 0,
            y: 0,
            w: 16,
            h: 2,
        };
        dst.fill_rect(rect, RGB_WHITE);

        // 前两行（行索引 0 和 1）应为白色
        for row in 0..2 {
            for col in 0..16 {
                let idx = row as usize * 32 + col as usize;
                assert_eq!(dst.pixels[idx], RGB_WHITE);
            }
        }
        // 第 3 行应保持黑色
        assert_eq!(dst.pixels[2 * 32], 0x0000);
    }

    // ── ASSET_RGBS 测试 ────────────────────────

    #[test]
    fn asset_rgbs_white_pill_is_white() {
        assert_eq!(ASSET_RGBS[Asset::WhitePill as usize], RGB_WHITE);
    }

    #[test]
    fn asset_rgbs_black_pill_is_black() {
        assert_eq!(ASSET_RGBS[Asset::BlackPill as usize], RGB_BLACK);
    }

    #[test]
    fn asset_rgbs_dark_gray_pill_is_dark_gray() {
        assert_eq!(ASSET_RGBS[Asset::DarkGrayPill as usize], RGB_DARK_GRAY);
    }

    #[test]
    fn asset_rgbs_length_equals_colors() {
        assert_eq!(ASSET_RGBS.len(), Asset::Colors as usize);
        assert_eq!(ASSET_RGBS.len(), 14);
    }

    #[test]
    fn asset_rgbs_all_entries_valid() {
        // 索引 0–13 均存储有效的 RGB565 颜色值
        let valid_colors = [
            RGB_WHITE,
            RGB_BLACK,
            RGB_LIGHT_GRAY,
            RGB_GRAY,
            RGB_DARK_GRAY,
        ];
        for (i, rgb) in ASSET_RGBS.iter().enumerate() {
            assert!(
                valid_colors.contains(rgb),
                "ASSET_RGBS[{i}] = {rgb} 不是有效颜色",
            );
        }
    }

    // ── 布局常量测试 ────────────────────────────

    #[test]
    fn pill_size_is_30() {
        assert_eq!(PILL_SIZE, 30);
    }

    #[test]
    fn button_size_is_20() {
        assert_eq!(BUTTON_SIZE, 20);
    }

    #[test]
    fn button_margin_is_5() {
        assert_eq!(BUTTON_MARGIN, 5);
    }

    #[test]
    fn button_padding_is_12() {
        assert_eq!(BUTTON_PADDING, 12);
    }

    #[test]
    fn padding_is_10() {
        assert_eq!(PADDING, 10);
    }

    #[test]
    fn settings_size_is_4() {
        assert_eq!(SETTINGS_SIZE, 4);
    }

    #[test]
    fn settings_width_is_80() {
        assert_eq!(SETTINGS_WIDTH, 80);
    }

    #[test]
    fn main_row_count_is_6() {
        // 对应 defines.h:58 的通用默认（各平台 platform.h 可覆盖——
        // 平台差异化值由 Platform::main_row_count() 覆盖表达）
        assert_eq!(MAIN_ROW_COUNT, 6);
    }

    #[test]
    fn button_margin_equals_half_pill_minus_button() {
        assert_eq!(BUTTON_MARGIN, (PILL_SIZE - BUTTON_SIZE) / 2);
    }

    #[test]
    fn padding_equals_pill_size_div_3() {
        assert_eq!(PADDING, PILL_SIZE / 3);
    }

    #[test]
    fn all_layout_constants_are_u32() {
        let _: u32 = PILL_SIZE;
        let _: u32 = BUTTON_SIZE;
        let _: u32 = BUTTON_MARGIN;
        let _: u32 = BUTTON_PADDING;
        let _: u32 = PADDING;
        let _: u32 = MAIN_ROW_COUNT;
        let _: u32 = SETTINGS_SIZE;
        let _: u32 = SETTINGS_WIDTH;
        let _: u32 = BRIGHTNESS_MIN;
        let _: u32 = BRIGHTNESS_MAX;
        let _: u32 = VOLUME_MIN;
        let _: u32 = VOLUME_MAX;
    }

    #[test]
    fn brightness_min_is_0() {
        assert_eq!(BRIGHTNESS_MIN, 0);
    }

    #[test]
    fn brightness_max_is_10() {
        assert_eq!(BRIGHTNESS_MAX, 10);
    }

    #[test]
    fn volume_min_is_0() {
        assert_eq!(VOLUME_MIN, 0);
    }

    #[test]
    fn volume_max_is_20() {
        assert_eq!(VOLUME_MAX, 20);
    }

    // ── VsyncMode 测试 ──────────────────────────

    #[test]
    fn vsync_mode_off_is_zero() {
        assert_eq!(VsyncMode::Off as i32, 0);
    }

    #[test]
    fn vsync_mode_lenient_is_one() {
        assert_eq!(VsyncMode::Lenient as i32, 1);
    }

    #[test]
    fn vsync_mode_strict_is_two() {
        assert_eq!(VsyncMode::Strict as i32, 2);
    }

    #[test]
    fn frame_budget_ms_is_17() {
        assert_eq!(FRAME_BUDGET_MS, 17);
    }
}
