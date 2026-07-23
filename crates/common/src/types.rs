//! # 核心数据类型
//!
//! 这些类型直接对应原 minui.c 中的 C 结构体。
//! 与 C 版本的关键区别：
//! - 不再用 `void*` 和手动 malloc/free —— 全部由 Rust 所有权系统管理
//! - `Vec<T>` 替换了手写的动态数组 `Array`
//! - `Option<T>` 替换了 NULL 指针
//! - 不再有 `IntArray`（定长 27 的 int 数组）—— 直接用 `Vec<usize>`

// ============================================================================
// EntryType — 文件系统条目的类型
// ============================================================================

/// 对应 C 中的 `enum EntryType { ENTRY_DIR, ENTRY_PAK, ENTRY_ROM }`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    /// 目录 —— 进入会显示子目录内容
    Dir,
    /// .pak 目录 —— 模拟器或工具包
    Pak,
    /// ROM 文件 —— 选择一个会启动游戏
    Rom,
}

// ============================================================================
// Entry — 文件系统中的一项
// ============================================================================

/// 对应 C 中的 `typedef struct Entry`
///
/// Entry 代表文件系统中的一个可导航项：可能是一个游戏主机目录、
/// 一个 ROM 文件、或一个 Pak 包。
///
/// ## C 版本中的字段对应关系
///
/// | C 字段      | Rust 字段       | 说明 |
/// |-------------|-----------------|------|
/// | `char* path`   | `path: String` | 完整文件系统路径 |
/// | `char* name`   | `name: String` | 去除扩展名/区域标记后的显示名 |
/// | `char* unique` | `unique: Option<String>` | 同名条目的区分名，NULL → None |
/// | `int type`     | `entry_type: EntryType` | 条目类型 |
/// | `int alpha`    | `alpha: usize` | 在父目录 alphas 数组中的索引 |
#[derive(Debug, Clone)]
pub struct Entry {
    /// 完整路径，如 `/mnt/sdcard/Roms/Game Boy (GB)/Zelda.gb`
    pub path: String,
    /// 显示名称（已去除扩展名和区域/版本标记）
    pub name: String,
    /// 唯一标识名。当两个条目显示名相同时，此字段包含用于区分的额外信息。
    /// 例如，两个 "Super Mario World" ROM 的不同版本会通过文件名区分。
    pub unique: Option<String>,
    /// 条目类型
    pub entry_type: EntryType,
    /// 首字母索引 —— 指向父 Directory 的 `alphas` Vec 中的位置。
    /// `alphas[alpha]` 返回的是该字母第一个 Entry 在 entries 中的索引。
    pub alpha: usize,
}

// ============================================================================
// Directory — 一个屏幕的浏览内容
// ============================================================================

/// 对应 C 中的 `typedef struct Directory`
///
/// 代表当前浏览的一个目录层级。包含了该目录下所有条目的列表、
/// 字母快速跳转索引、以及当前的滚动/选中状态。
///
/// ## 滚动窗口机制
///
/// 屏幕只能显示 `MAIN_ROW_COUNT` 行（通常为 6）。
/// `selected` 是当前光标位置，`start..end` 定义可见窗口：
///
/// ```text
/// 条目总数 = 20, MAIN_ROW_COUNT = 6
///
/// entries: [0] [1] [2] [3] [4] [5] [6] [7] ... [19]
///                  ↑start=2     ↑selected=4   ↑end=8
///                               (可见窗口: 索引 2~7)
/// ```
///
/// **不变量：** `start <= selected < end`，`end - start <= MAIN_ROW_COUNT`
#[derive(Debug, Clone)]
pub struct Directory {
    /// 此目录的路径
    pub path: String,
    /// 显示名称
    pub name: String,
    /// 条目列表（已排序）
    pub entries: Vec<Entry>,
    /// 字母索引 —— `alphas[i]` 是第 i 个字母分组在 entries 中的起始索引
    pub alphas: Vec<usize>,
    /// 当前选中项的索引
    pub selected: usize,
    /// 可见窗口的起始索引（含）
    pub start: usize,
    /// 可见窗口的结束索引（不含）
    pub end: usize,
}

impl Directory {
    /// 可见条目数
    pub fn visible_count(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// 条目总数
    pub fn total(&self) -> usize {
        self.entries.len()
    }

    /// 是否是根目录（栈底）
    /// 注意：这个信息不在 Directory 中存储，由调用者通过 stack 长度判断。
    /// 此处仅提供一个便利方法。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ============================================================================
// Recent — 最近玩过的游戏
// ============================================================================

/// 对应 C 中的 `typedef struct Recent`
///
/// ## 关键设计
///
/// `path` 存储的是**去除 SDCARD_PATH 前缀后的相对路径**。
/// 这使得最近游戏列表在不同设备之间可以共享 ——
/// 同一张 SD 卡插入不同设备，只要模拟器可用即可。
///
/// `available` 标记该游戏的模拟器在当前设备上是否可用。
/// 如果用户换了一张没有对应模拟器的 SD 卡，条目仍在但灰掉。
#[derive(Debug, Clone)]
pub struct Recent {
    /// 相对路径（**不包含** SDCARD_PATH 前缀）
    pub path: String,
    /// 可选的自定义别名（来自 map.txt 或用户设置）
    pub alias: Option<String>,
    /// 模拟器在当前设备上是否可用
    pub available: bool,
}

// ============================================================================
// Button — 按钮枚举和位掩码
// ============================================================================

/// 按钮 ID 枚举 —— 对应 C 中的 `enum { BTN_ID_* }`
///
/// 每个物理按钮有一个唯一 ID，用于索引按钮状态数组。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ButtonId {
    DpadUp = 0,
    DpadDown,
    DpadLeft,
    DpadRight,
    A,
    B,
    X,
    Y,
    Start,
    Select,
    L1,
    R1,
    L2,
    R2,
    L3,
    R3,
    Menu,
    Plus,
    Minus,
    Power,
    PowerOff,

    AnalogUp,
    AnalogDown,
    AnalogLeft,
    AnalogRight,

    /// 按钮总数
    Count,
}

impl ButtonId {
    pub const COUNT: usize = ButtonId::Count as usize;
}

/// 按钮状态位掩码 —— 对应 C 中的 `enum { BTN_* = 1 << BTN_ID_* }`
///
/// 每个变体是一个独立的 bit，可以组合（例如 `Button::Up` 同时包含 DpadUp 和 AnalogUp）。
/// 使用 `bitflags` 风格，但为了零依赖手动实现。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Button(pub u32);

impl Button {
    pub const NONE: Self = Self(0);
    pub const DPAD_UP: Self = Self(1 << ButtonId::DpadUp as u32);
    pub const DPAD_DOWN: Self = Self(1 << ButtonId::DpadDown as u32);
    pub const DPAD_LEFT: Self = Self(1 << ButtonId::DpadLeft as u32);
    pub const DPAD_RIGHT: Self = Self(1 << ButtonId::DpadRight as u32);
    pub const A: Self = Self(1 << ButtonId::A as u32);
    pub const B: Self = Self(1 << ButtonId::B as u32);
    pub const X: Self = Self(1 << ButtonId::X as u32);
    pub const Y: Self = Self(1 << ButtonId::Y as u32);
    pub const START: Self = Self(1 << ButtonId::Start as u32);
    pub const SELECT: Self = Self(1 << ButtonId::Select as u32);
    pub const L1: Self = Self(1 << ButtonId::L1 as u32);
    pub const R1: Self = Self(1 << ButtonId::R1 as u32);
    pub const L2: Self = Self(1 << ButtonId::L2 as u32);
    pub const R2: Self = Self(1 << ButtonId::R2 as u32);
    pub const L3: Self = Self(1 << ButtonId::L3 as u32);
    pub const R3: Self = Self(1 << ButtonId::R3 as u32);
    pub const MENU: Self = Self(1 << ButtonId::Menu as u32);
    pub const PLUS: Self = Self(1 << ButtonId::Plus as u32);
    pub const MINUS: Self = Self(1 << ButtonId::Minus as u32);
    pub const POWER: Self = Self(1 << ButtonId::Power as u32);
    pub const POWER_OFF: Self = Self(1 << ButtonId::PowerOff as u32);

    pub const ANALOG_UP: Self = Self(1 << ButtonId::AnalogUp as u32);
    pub const ANALOG_DOWN: Self = Self(1 << ButtonId::AnalogDown as u32);
    pub const ANALOG_LEFT: Self = Self(1 << ButtonId::AnalogLeft as u32);
    pub const ANALOG_RIGHT: Self = Self(1 << ButtonId::AnalogRight as u32);

    // 组合按钮（D-pad 和摇杆的并集，对应 C 中的 BTN_UP = BTN_DPAD_UP | BTN_ANALOG_UP）
    pub const UP: Self = Self(Self::DPAD_UP.0 | Self::ANALOG_UP.0);
    pub const DOWN: Self = Self(Self::DPAD_DOWN.0 | Self::ANALOG_DOWN.0);
    pub const LEFT: Self = Self(Self::DPAD_LEFT.0 | Self::ANALOG_LEFT.0);
    pub const RIGHT: Self = Self(Self::DPAD_RIGHT.0 | Self::ANALOG_RIGHT.0);

    // 位运算方法
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// 添加按钮到位掩码（按位或）
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// 从位掩码中移除按钮（按位与非）
    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }

    /// 是否没有按钮被按下
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// 任何一个方向键被按下
    pub fn has_any_direction(self) -> bool {
        self.contains(Self::UP) || self.contains(Self::DOWN)
            || self.contains(Self::LEFT) || self.contains(Self::RIGHT)
    }
}

// ============================================================================
// PadContext — 手柄/按键上下文
// ============================================================================

/// 摇杆轴状态 —— 对应 C 中的 `typedef struct PAD_Axis`
#[derive(Debug, Clone, Copy, Default)]
pub struct Axis {
    /// 水平轴值（-32768 到 32767，0 为居中）
    pub x: i32,
    /// 垂直轴值（-32768 到 32767，0 为居中）
    pub y: i32,
}

/// 手柄输入上下文 —— 对应 C 中的 `typedef struct PAD_Context`
///
/// 维护当前帧和上一帧之间的按键状态转换。
/// 这是实现 "just pressed" / "just released" 检测的关键。
#[derive(Debug, Clone)]
pub struct PadContext {
    /// 当前帧被按下的所有按钮
    pub is_pressed: Button,
    /// 当前帧刚被按下的按钮（上升沿 — 之前未按下，现在按下）
    pub just_pressed: Button,
    /// 当前帧刚被释放的按钮（下降沿）
    pub just_released: Button,
    /// 当前帧触发了 repeat 的按钮（长按自动重复）
    pub just_repeated: Button,
    /// 每个按钮上次触发 repeat 的时间戳（毫秒）
    pub repeat_at: [u32; ButtonId::COUNT],
    /// 左摇杆
    pub laxis: Axis,
    /// 右摇杆
    pub raxis: Axis,
}

impl Default for PadContext {
    fn default() -> Self {
        Self {
            is_pressed: Button::NONE,
            just_pressed: Button::NONE,
            just_released: Button::NONE,
            just_repeated: Button::NONE,
            repeat_at: [0; ButtonId::COUNT],
            laxis: Axis::default(),
            raxis: Axis::default(),
        }
    }
}

impl PadContext {
    /// 重置所有按钮状态（通常在初始化后调用，清除杂讯）
    pub fn reset(&mut self) {
        self.is_pressed = Button::NONE;
        self.just_pressed = Button::NONE;
        self.just_released = Button::NONE;
        self.just_repeated = Button::NONE;
    }

    /// 是否有任何按钮刚被按下
    pub fn any_just_pressed(&self) -> bool {
        !self.just_pressed.is_empty()
    }

    /// 是否有任何按钮正在被按住
    pub fn any_pressed(&self) -> bool {
        !self.is_pressed.is_empty()
    }

    /// 是否有任何按钮刚被释放
    pub fn any_just_released(&self) -> bool {
        !self.just_released.is_empty()
    }
}

// ============================================================================
// 颜色和渲染相关类型
// ============================================================================

/// RGB 颜色 —— 对应 C 中的 `SDL_Color`
///
/// MinUI 使用一个有限的调色板（Triad 色彩系统），只有 5 种颜色：
/// - White:   (255, 255, 255) — 普通文字
/// - Black:   (0, 0, 0)     — 选中项文字
/// - LightGray: (204, 204, 204) — 亮色 UI 元素
/// - Gray:    (153, 153, 153) — 按钮文字
/// - DarkGray: (38, 38, 38)    — 暗色 UI 元素 / 副文字
///
/// # 示例
///
/// ```
/// use minui::Color;
/// let white = Color::WHITE;
/// assert_eq!(white.r, 255);
/// assert_eq!(white.g, 255);
/// assert_eq!(white.b, 255);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    /// 红色分量 (0-255)
    pub r: u8,
    /// 绿色分量 (0-255)
    pub g: u8,
    /// 蓝色分量 (0-255)
    pub b: u8,
}

impl Color {
    pub const WHITE: Self = Self { r: 0xff, g: 0xff, b: 0xff };
    pub const BLACK: Self = Self { r: 0x00, g: 0x00, b: 0x00 };
    pub const LIGHT_GRAY: Self = Self { r: 0x7f, g: 0x7f, b: 0x7f };
    pub const GRAY: Self = Self { r: 0x99, g: 0x99, b: 0x99 };
    pub const DARK_GRAY: Self = Self { r: 0x26, g: 0x26, b: 0x26 };

    /// 对应 C 中的 TRIAD_LIGHT_TEXT
    pub const LIGHT_TEXT: Self = Self { r: 0xcc, g: 0xcc, b: 0xcc };
    /// 对应 C 中的 TRIAD_DARK_TEXT
    pub const DARK_TEXT: Self = Self { r: 0x66, g: 0x66, b: 0x66 };
}

/// UI 渲染模式 —— 对应 C 中的 `enum { MODE_MAIN, MODE_MENU }`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    /// 主启动器模式（列表浏览）
    Main,
    /// 菜单模式（minarch 的游戏内菜单）
    Menu,
}

/// CPU 速度等级 —— 对应 C 中的 `enum { CPU_SPEED_MENU, ... }`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuSpeed {
    /// 菜单浏览时用最低频率省电（504 MHz）
    Menu,
    /// 省电模式（1.1 GHz）
    Powersave,
    /// 正常模式（1.3 GHz）
    Normal,
    /// 性能模式（1.5 GHz）
    Performance,
}

/// 画面缩放模式 —— 对应 C 中的 `enum { SCALE_NATIVE, ... }`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleMode {
    /// 原生分辨率
    Native = 0,
    /// 保持宽高比
    Aspect,
    /// 拉伸到全屏
    Fullscreen,
    /// 裁剪到全屏
    Cropped,
}

/// 画面锐度 —— 对应 C 中的 `enum { SHARPNESS_SHARP, ... }`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sharpness {
    Sharp = 0,
    Crisp,
    Soft,
}

/// 画面效果 —— 对应 C 中的 `enum { EFFECT_NONE, EFFECT_LINE, EFFECT_GRID }`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenEffect {
    None = 0,
    /// 扫描线效果
    Line,
    /// 网格效果（CRT 模拟）
    Grid,
}

/// VSync 模式 —— 对应 C 中的 `enum { VSYNC_OFF, ... }`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VsyncMode {
    Off = 0,
    /// 宽松同步（默认）
    Lenient,
    /// 严格同步
    Strict,
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_button_basics() {
        let mut b = Button::NONE;
        assert!(b.is_empty());

        b.insert(Button::A);
        assert!(b.contains(Button::A));
        assert!(!b.contains(Button::B));

        let dir = Button::UP;
        assert!(dir.contains(Button::DPAD_UP));
        assert!(dir.contains(Button::ANALOG_UP));
    }

    #[test]
    fn test_button_combined() {
        let dpad_and_btn = Button::DPAD_UP.union(Button::A);
        assert!(dpad_and_btn.contains(Button::DPAD_UP));
        assert!(dpad_and_btn.contains(Button::A));
        assert!(!dpad_and_btn.contains(Button::B));
    }

    #[test]
    fn test_directory_empty() {
        let dir = Directory {
            path: String::from("/test"),
            name: String::from("Test"),
            entries: vec![],
            alphas: vec![],
            selected: 0,
            start: 0,
            end: 0,
        };
        assert!(dir.is_empty());
        assert_eq!(dir.total(), 0);
    }

    #[test]
    fn test_directory_total() {
        let dir = Directory {
            path: String::from("/test"),
            name: String::from("Test"),
            entries: vec![
                Entry {
                    path: "/test/a.gb".into(),
                    name: "a".into(),
                    unique: None,
                    entry_type: EntryType::Rom,
                    alpha: 0,
                },
                Entry {
                    path: "/test/b.gb".into(),
                    name: "b".into(),
                    unique: None,
                    entry_type: EntryType::Rom,
                    alpha: 0,
                },
            ],
            alphas: vec![0],
            selected: 0,
            start: 0,
            end: 2,
        };
        assert_eq!(dir.total(), 2);
        assert!(!dir.is_empty());
    }

    #[test]
    fn test_pad_context_default() {
        let pad = PadContext::default();
        assert!(pad.is_pressed.is_empty());
        assert!(pad.just_pressed.is_empty());
    }
}
