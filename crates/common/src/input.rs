//! 输入状态和按键处理
//!
//! 本模块定义按键位掩码常量、输入状态结构体，以及模仿原版 C 中 `PAD_*`
//! 系列函数的按键查询接口。
//!
//! ## 设计
//!
//! 使用 `u32` 位掩码表示按键状态（每位对应一个物理按键），与原版 C 完全一致。
//! `Platform::poll_input()` 返回原始 `InputState`，本模块的查询方法
//! 在此基础上提供便捷查询。
//!
//! ## 与原 C 代码的对比
//!
//! 原版使用全局变量 `pad: PAD_Context` 存储按键状态，按键检测直接修改全局。
//! Rust 版将 `InputState` 设计为纯数据，由 `Platform::poll_input()` 返回，
//! 后续通过方法查询——消除了全局可变状态。
//!

// ── 按键标识（位索引）───────────────────────────

pub const BTN_ID_NONE: usize = 0;
pub const BTN_ID_DPAD_UP: usize = 0;
pub const BTN_ID_DPAD_DOWN: usize = 1;
pub const BTN_ID_DPAD_LEFT: usize = 2;
pub const BTN_ID_DPAD_RIGHT: usize = 3;
pub const BTN_ID_A: usize = 4;
pub const BTN_ID_B: usize = 5;
pub const BTN_ID_X: usize = 6;
pub const BTN_ID_Y: usize = 7;
pub const BTN_ID_START: usize = 8;
pub const BTN_ID_SELECT: usize = 9;
pub const BTN_ID_L1: usize = 10;
pub const BTN_ID_R1: usize = 11;
pub const BTN_ID_L2: usize = 12;
pub const BTN_ID_R2: usize = 13;
pub const BTN_ID_L3: usize = 14;
pub const BTN_ID_R3: usize = 15;
pub const BTN_ID_MENU: usize = 16;
pub const BTN_ID_PLUS: usize = 17;
pub const BTN_ID_MINUS: usize = 18;
pub const BTN_ID_POWER: usize = 19;
pub const BTN_ID_POWEROFF: usize = 20;
/// 模拟摇杆向上方向（左摇杆推上，超过 deadzone）——对应原版 defines.h
/// `BTN_ID_ANALOG_UP`。由平台 `poll_input` 将摇杆轴翻译为方向键位
/// （原版 `PAD_setAnalog`）；摇杆原始值存平台侧（laxis/raxis）
pub const BTN_ID_ANALOG_UP: usize = 21;
/// 模拟摇杆向下方向
pub const BTN_ID_ANALOG_DOWN: usize = 22;
/// 模拟摇杆向左方向
pub const BTN_ID_ANALOG_LEFT: usize = 23;
/// 模拟摇杆向右方向
pub const BTN_ID_ANALOG_RIGHT: usize = 24;
/// 键位总数（含 4 个模拟摇杆方向键，与原版 defines.h 一致）
pub const BTN_ID_COUNT: usize = 25;

// ── 按键位掩码 ──────────────────────────────────

pub const BTN_NONE: u32 = 0;
pub const BTN_DPAD_UP: u32 = 1 << BTN_ID_DPAD_UP;
pub const BTN_DPAD_DOWN: u32 = 1 << BTN_ID_DPAD_DOWN;
pub const BTN_DPAD_LEFT: u32 = 1 << BTN_ID_DPAD_LEFT;
pub const BTN_DPAD_RIGHT: u32 = 1 << BTN_ID_DPAD_RIGHT;
pub const BTN_A: u32 = 1 << BTN_ID_A;
pub const BTN_B: u32 = 1 << BTN_ID_B;
pub const BTN_X: u32 = 1 << BTN_ID_X;
pub const BTN_Y: u32 = 1 << BTN_ID_Y;
pub const BTN_START: u32 = 1 << BTN_ID_START;
pub const BTN_SELECT: u32 = 1 << BTN_ID_SELECT;
pub const BTN_L1: u32 = 1 << BTN_ID_L1;
pub const BTN_R1: u32 = 1 << BTN_ID_R1;
pub const BTN_L2: u32 = 1 << BTN_ID_L2;
pub const BTN_R2: u32 = 1 << BTN_ID_R2;
pub const BTN_L3: u32 = 1 << BTN_ID_L3;
pub const BTN_R3: u32 = 1 << BTN_ID_R3;
pub const BTN_MENU: u32 = 1 << BTN_ID_MENU;
pub const BTN_PLUS: u32 = 1 << BTN_ID_PLUS;
pub const BTN_MINUS: u32 = 1 << BTN_ID_MINUS;
pub const BTN_POWER: u32 = 1 << BTN_ID_POWER;
pub const BTN_POWEROFF: u32 = 1 << BTN_ID_POWEROFF;
/// 模拟摇杆向上方向掩码
pub const BTN_ANALOG_UP: u32 = 1 << BTN_ID_ANALOG_UP;
/// 模拟摇杆向下方向掩码
pub const BTN_ANALOG_DOWN: u32 = 1 << BTN_ID_ANALOG_DOWN;
/// 模拟摇杆向左方向掩码
pub const BTN_ANALOG_LEFT: u32 = 1 << BTN_ID_ANALOG_LEFT;
/// 模拟摇杆向右方向掩码
pub const BTN_ANALOG_RIGHT: u32 = 1 << BTN_ID_ANALOG_RIGHT;

/// 组合方向键（十字键 + 摇杆）
pub const BTN_UP: u32 = BTN_DPAD_UP;
pub const BTN_DOWN: u32 = BTN_DPAD_DOWN;
pub const BTN_LEFT: u32 = BTN_DPAD_LEFT;
pub const BTN_RIGHT: u32 = BTN_DPAD_RIGHT;

// ── 输入状态 ────────────────────────────────────

/// 单帧输入快照
///
/// 由 `Platform::poll_input()` 返回，包含当前帧的按键状态位掩码
/// 与摇杆原始轴值。通过 `InputState` 的方法查询特定按键的状态。
#[derive(Default)]
pub struct InputState {
    /// 当前按下的键位掩码
    pub pressed: u32,
    /// 本帧新按下的键位掩码
    pub just_pressed: u32,
    /// 本帧释放的键位掩码
    pub just_released: u32,
    /// 本帧触发重复的键位掩码（按住一段时间后自动重复）
    pub just_repeated: u32,
    /// 左摇杆原始轴值 `(x, y)`（对应原版 C `pad.laxis`，api.h:235）
    ///
    /// 原始 SDL 轴值（-32768..32767）直接透传，无缩放或 deadzone 处理
    /// （deadzone 仅用于方向键位判定）。供 minarch 应答 libretro 核心
    /// 的 `RETRO_DEVICE_ANALOG` 查询。
    pub laxis: (i32, i32),
    /// 右摇杆原始轴值 `(x, y)`（对应原版 C `pad.raxis`，api.h:236）
    ///
    /// 语义同 [`InputState::laxis`]。
    pub raxis: (i32, i32),
}

impl InputState {
    /// 创建空状态（无按键按下）
    pub fn new() -> Self {
        Self::default()
    }

    // ── 无参数查询 ──────────────────────────────

    /// 是否有任意键被按下
    ///
    /// 对应原版 C `PAD_anyPressed()`。
    /// 等价于 `(self.pressed != 0)`。
    pub fn any_pressed(&self) -> bool {
        self.pressed != 0
    }

    /// 是否有任意键刚按下
    ///
    /// 对应原版 C `PAD_anyJustPressed()`。
    /// 等价于 `(self.just_pressed != 0)`。
    pub fn any_just_pressed(&self) -> bool {
        self.just_pressed != 0
    }

    /// 是否有任意键刚释放
    ///
    /// 对应原版 C `PAD_anyJustReleased()`。
    /// 等价于 `(self.just_released != 0)`。
    pub fn any_just_released(&self) -> bool {
        self.just_released != 0
    }

    // ── 单按键查询 ──────────────────────────────

    /// 检查按键是否刚按下（本帧按下，上帧未按下）
    ///
    /// 对应原版 C `PAD_justPressed(btn)`。
    /// `btn` 为 `BTN_*` 位掩码常量，如 `state.just_pressed(BTN_A)`。
    pub fn just_pressed(&self, btn: u32) -> bool {
        self.just_pressed & btn != 0
    }

    /// 检查按键是否正在按住
    ///
    /// 对应原版 C `PAD_isPressed(btn)`。
    /// `btn` 为 `BTN_*` 位掩码常量。
    pub fn is_pressed(&self, btn: u32) -> bool {
        self.pressed & btn != 0
    }

    /// 检查按键是否刚释放（本帧释放，上帧按住）
    ///
    /// 对应原版 C `PAD_justReleased(btn)`。
    /// `btn` 为 `BTN_*` 位掩码常量。
    pub fn just_released(&self, btn: u32) -> bool {
        self.just_released & btn != 0
    }

    /// 检查按键是否触发重复（按住超时后自动触发）
    ///
    /// 对应原版 C `PAD_justRepeated(btn)`。
    /// **注意**：`just_repeated` 字段由 `Platform::poll_input()` 填充，
    /// 本方法仅负责查询。重复判定逻辑（300ms 延迟 + 100ms 间隔）在平台层实现。
    /// `btn` 为 `BTN_*` 位掩码常量。
    pub fn just_repeated(&self, btn: u32) -> bool {
        self.just_repeated & btn != 0
    }

    // ── 状态操作 ────────────────────────────────

    /// 重置输入状态（通常在睡眠唤醒后调用）
    ///
    /// 将四个字段（`pressed`、`just_pressed`、`just_released`、`just_repeated`）全部置零。
    /// 对应原版 C `PAD_reset()`。
    pub fn reset(&mut self) {
        self.pressed = 0;
        self.just_pressed = 0;
        self.just_released = 0;
        self.just_repeated = 0;
    }
}

// ── 轻触检测 ─────────────────────────────────────

/// MENU 键轻触状态
///
/// 用于 `tapped_menu` 函数跟踪跨帧状态。
/// 调用方（minui/minarch 主循环）负责持有此结构体的实例。
///
/// ## 与原 C 代码的对比
///
/// 原版 C 使用 `static` 局部变量 `menu_start` 和 `ignore_menu`。
/// Rust 版将状态提取为独立结构体，由调用方显式管理生命周期。
pub struct MenuTapState {
    /// MENU 键按下的时刻（`SDL_GetTicks()` 毫秒值）
    menu_start: u32,
    /// 是否因修饰键按下而忽略本次轻触
    ignore_menu: bool,
}

impl MenuTapState {
    /// 创建初始状态
    pub fn new() -> Self {
        Self {
            menu_start: 0,
            ignore_menu: false,
        }
    }
}

impl Default for MenuTapState {
    fn default() -> Self {
        Self::new()
    }
}

/// 平台修饰键映射（对应 C `platform.h` 的 `BTN_MOD_*` 宏）
///
/// 修饰键组合（如"MENU 按住 + PLUS = 调亮度"）的平台差异——不同设备
/// 的亮度/音量修饰键不同（tg5040 为 `BTN_MENU`+`BTN_PLUS`；m17/trimuismart
/// 为 `BTN_START`+`BTN_R1`）。原版为编译期宏，Rust 版由调用方（装配层）
/// 从 `Platform` trait 的语义键关联常量（`BTN_MOD_*`，见
/// common-crate「Platform 语义键关联常量」）构造传入——"调用方收集
/// 数据传入"模式，与 `power::update` 的 `mute` 参数同构。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModKeys {
    /// 亮度修饰键（BTN_MOD_BRIGHTNESS）
    pub brightness: u32,
    /// 音量修饰键（BTN_MOD_VOLUME）——tg5040 为 `BTN_NONE`（无音量修饰键）；
    /// m17/trimuismart 类平台为 `BTN_SELECT`。`power::update` 的修饰键
    /// 逻辑（SETTING_DELAY/MOD_DELAY）使用
    pub volume: u32,
    /// 亮度/音量加修饰键（BTN_MOD_PLUS）
    pub plus: u32,
    /// 亮度/音量减修饰键（BTN_MOD_MINUS）
    pub minus: u32,
}

/// 检查 MENU 键是否被轻触
///
/// 轻触定义为：250ms 内按下并释放，且未同时按亮度/音量修饰键。
///
/// 对应原版 C `PAD_tappedMenu(now)`（api.c:1383-1394）——原版的
/// `BTN_MOD_BRIGHTNESS == BTN_MENU` 条件为平台宏编译期展开，Rust 版
/// 由调用方经 `mod_keys` 传入（见 [`ModKeys`]）。
///
/// ## 参数
///
/// - `state`：当前帧的输入快照
/// - `tap`：跨帧状态（由调用方在主循环外持有）
/// - `now_ms`：当前时间（毫秒），通常来自 `SDL_GetTicks()` 或等价来源
/// - `mod_keys`：平台修饰键映射（从 `Platform` 语义键关联常量构造传入）
pub fn tapped_menu(
    state: &InputState,
    tap: &mut MenuTapState,
    now_ms: u32,
    mod_keys: ModKeys,
) -> bool {
    // 检测 MENU 键刚按下 → 记录时刻，重置忽略标志
    if state.just_pressed(BTN_MENU) {
        tap.ignore_menu = false;
        tap.menu_start = now_ms;
    }
    // 检测修饰键按下 → 设置忽略标志
    // 对应原 C: else if (PAD_isPressed(BTN_MENU) && BTN_MOD_BRIGHTNESS==BTN_MENU
    //       && (PAD_justPressed(BTN_MOD_PLUS) || PAD_justPressed(BTN_MOD_MINUS)))
    // 注意：用 mod_keys 而非硬编码 BTN_MENU/BTN_PLUS/BTN_MINUS——后者的
    // 简化式在 BTN_MOD_BRIGHTNESS != BTN_MENU 的平台（m17/trimuismart）是语义 bug
    // （m17 反例：修饰键必须参数化——见 tapped_menu 文档）
    else if state.is_pressed(BTN_MENU)
        && mod_keys.brightness == BTN_MENU
        && (state.just_pressed(mod_keys.plus) || state.just_pressed(mod_keys.minus))
    {
        tap.ignore_menu = true;
    }

    // MENU 刚释放 + 未被忽略 + 按下时长 < 250ms
    const MENU_DELAY: u32 = 250;
    !tap.ignore_menu && state.just_released(BTN_MENU) && now_ms - tap.menu_start < MENU_DELAY
}

// ── 测试 ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── 无参数查询测试 ──────────────────────────

    #[test]
    fn any_pressed_detects_active_buttons() {
        let state = InputState {
            pressed: BTN_A,
            ..InputState::new()
        };
        assert!(state.any_pressed());
    }

    #[test]
    fn any_pressed_returns_false_when_none() {
        let state = InputState::new();
        assert!(!state.any_pressed());
    }

    #[test]
    fn any_just_pressed_detects_new_presses() {
        let state = InputState {
            just_pressed: BTN_B,
            ..InputState::new()
        };
        assert!(state.any_just_pressed());
    }

    #[test]
    fn any_just_pressed_returns_false_when_none() {
        let state = InputState::new();
        assert!(!state.any_just_pressed());
    }

    #[test]
    fn any_just_released_detects_releases() {
        let state = InputState {
            just_released: BTN_START,
            ..InputState::new()
        };
        assert!(state.any_just_released());
    }

    #[test]
    fn any_just_released_returns_false_when_none() {
        let state = InputState::new();
        assert!(!state.any_just_released());
    }

    // ── 摇杆原始轴值 ────────────────────────────

    #[test]
    fn sticks_default_to_neutral() {
        // 空构造（new/Default）的轴值应为中立位 (0,0)
        let state = InputState::new();
        assert_eq!(state.laxis, (0, 0));
        assert_eq!(state.raxis, (0, 0));
    }

    // ── 单按键查询测试 ──────────────────────────

    #[test]
    fn just_pressed_returns_true_for_active_button() {
        let state = InputState {
            just_pressed: BTN_A,
            ..InputState::new()
        };
        assert!(state.just_pressed(BTN_A));
    }

    #[test]
    fn just_pressed_returns_false_for_inactive_button() {
        let state = InputState {
            just_pressed: BTN_B,
            ..InputState::new()
        };
        assert!(!state.just_pressed(BTN_A));
    }

    #[test]
    fn just_pressed_returns_false_when_all_zero() {
        let state = InputState::new();
        assert!(!state.just_pressed(BTN_A));
    }

    #[test]
    fn is_pressed_returns_true_for_active_button() {
        let state = InputState {
            pressed: BTN_A,
            ..InputState::new()
        };
        assert!(state.is_pressed(BTN_A));
    }

    #[test]
    fn is_pressed_returns_false_for_inactive_button() {
        let state = InputState::new();
        assert!(!state.is_pressed(BTN_A));
    }

    #[test]
    fn just_released_returns_true_for_released_button() {
        let state = InputState {
            just_released: BTN_A,
            ..InputState::new()
        };
        assert!(state.just_released(BTN_A));
    }

    #[test]
    fn just_released_returns_false_when_all_zero() {
        let state = InputState::new();
        assert!(!state.just_released(BTN_A));
    }

    #[test]
    fn just_repeated_returns_true_for_repeating_button() {
        let state = InputState {
            just_repeated: BTN_DOWN,
            ..InputState::new()
        };
        assert!(state.just_repeated(BTN_DOWN));
    }

    #[test]
    fn just_repeated_returns_false_when_all_zero() {
        let state = InputState::new();
        assert!(!state.just_repeated(BTN_DOWN));
    }

    #[test]
    fn combo_query_two_buttons() {
        let state = InputState {
            just_pressed: BTN_UP | BTN_A,
            ..InputState::new()
        };
        assert!(state.just_pressed(BTN_UP) && state.just_pressed(BTN_A));
        assert!(!state.just_pressed(BTN_B));
    }

    // ── reset 测试 ──────────────────────────────

    #[test]
    fn reset_clears_all_fields() {
        let mut state = InputState {
            pressed: BTN_A | BTN_B,
            just_pressed: BTN_A,
            just_released: BTN_B,
            just_repeated: BTN_DOWN,
            ..InputState::new()
        };
        state.reset();
        assert_eq!(state.pressed, 0);
        assert_eq!(state.just_pressed, 0);
        assert_eq!(state.just_released, 0);
        assert_eq!(state.just_repeated, 0);
    }

    // ── tapped_menu 测试 ────────────────────────

    /// tg5040 类平台的修饰键映射（BTN_MOD_BRIGHTNESS == BTN_MENU）
    const TG5040_KEYS: ModKeys = ModKeys {
        brightness: BTN_MENU,
        volume: BTN_NONE,
        plus: BTN_PLUS,
        minus: BTN_MINUS,
    };

    /// m17/trimuismart 类平台（亮度修饰键是 START、加键是 R1）
    const M17_KEYS: ModKeys = ModKeys {
        brightness: BTN_START,
        volume: BTN_SELECT,
        plus: BTN_R1,
        minus: BTN_L1,
    };

    #[test]
    fn tapped_menu_normal_tap_returns_true() {
        // 第 1 帧：MENU 刚按下
        let press_state = InputState {
            just_pressed: BTN_MENU,
            pressed: BTN_MENU,
            ..InputState::new()
        };
        let mut tap = MenuTapState::new();
        assert!(!tapped_menu(&press_state, &mut tap, 1000, TG5040_KEYS));
        // 内部应记录 menu_start = 1000

        // 第 2 帧：MENU 刚释放（1100 - 1000 = 100ms < 250ms）
        let release_state = InputState {
            just_released: BTN_MENU,
            ..InputState::new()
        };
        assert!(tapped_menu(&release_state, &mut tap, 1100, TG5040_KEYS));
    }

    #[test]
    fn tapped_menu_long_press_returns_false() {
        // MENU 按下
        let press_state = InputState {
            just_pressed: BTN_MENU,
            pressed: BTN_MENU,
            ..InputState::new()
        };
        let mut tap = MenuTapState::new();
        tapped_menu(&press_state, &mut tap, 1000, TG5040_KEYS);

        // 300ms 后释放（超过 250ms 阈值）
        let release_state = InputState {
            just_released: BTN_MENU,
            ..InputState::new()
        };
        assert!(!tapped_menu(&release_state, &mut tap, 1300, TG5040_KEYS));
    }

    #[test]
    fn tapped_menu_modifier_ignores_tap() {
        // MENU 刚按下
        let press_state = InputState {
            just_pressed: BTN_MENU,
            pressed: BTN_MENU,
            ..InputState::new()
        };
        let mut tap = MenuTapState::new();
        tapped_menu(&press_state, &mut tap, 1000, TG5040_KEYS);

        // 修饰键（PLUS）被按下
        let mod_state = InputState {
            just_pressed: BTN_PLUS,
            pressed: BTN_MENU | BTN_PLUS,
            ..InputState::new()
        };
        tapped_menu(&mod_state, &mut tap, 1050, TG5040_KEYS);
        assert!(tap.ignore_menu);

        // MENU 释放，但已被忽略
        let release_state = InputState {
            just_released: BTN_MENU,
            ..InputState::new()
        };
        assert!(!tapped_menu(&release_state, &mut tap, 1100, TG5040_KEYS));
    }

    #[test]
    fn tapped_menu_no_menu_press_returns_false() {
        let state = InputState::new();
        let mut tap = MenuTapState::new();
        assert!(!tapped_menu(&state, &mut tap, 1000, TG5040_KEYS));
    }

    // ── ANALOG 键位（对应原版 defines.h，编号 21-24）──

    #[test]
    fn analog_button_ids_match_original() {
        assert_eq!(BTN_ID_ANALOG_UP, 21);
        assert_eq!(BTN_ID_ANALOG_DOWN, 22);
        assert_eq!(BTN_ID_ANALOG_LEFT, 23);
        assert_eq!(BTN_ID_ANALOG_RIGHT, 24);
        assert_eq!(BTN_ID_COUNT, 25);
    }

    #[test]
    fn analog_masks_do_not_overlap_existing() {
        let analog = BTN_ANALOG_UP | BTN_ANALOG_DOWN | BTN_ANALOG_LEFT | BTN_ANALOG_RIGHT;
        let existing = BTN_DPAD_UP
            | BTN_DPAD_DOWN
            | BTN_DPAD_LEFT
            | BTN_DPAD_RIGHT
            | BTN_A
            | BTN_B
            | BTN_X
            | BTN_Y
            | BTN_START
            | BTN_SELECT
            | BTN_L1
            | BTN_R1
            | BTN_L2
            | BTN_R2
            | BTN_L3
            | BTN_R3
            | BTN_MENU
            | BTN_PLUS
            | BTN_MINUS
            | BTN_POWER
            | BTN_POWEROFF;
        assert_eq!(analog & existing, 0, "ANALOG 掩码不得与既有键位重叠");
        assert_eq!(analog, (1 << 21) | (1 << 22) | (1 << 23) | (1 << 24));
    }

    #[test]
    fn analog_masks_queryable_via_input_state() {
        let state = InputState {
            pressed: BTN_ANALOG_RIGHT,
            just_pressed: BTN_ANALOG_LEFT,
            ..InputState::new()
        };
        assert!(state.is_pressed(BTN_ANALOG_RIGHT));
        assert!(state.just_pressed(BTN_ANALOG_LEFT));
        assert!(!state.is_pressed(BTN_ANALOG_LEFT));
    }
    // ── ModKeys 参数化（对应 spec 场景）──

    #[test]
    fn mod_keys_has_volume_field() {
        // volume 字段（BTN_MOD_VOLUME）——tg5040=BTN_NONE、m17=BTN_SELECT
        assert_eq!(TG5040_KEYS.volume, BTN_NONE);
        assert_eq!(M17_KEYS.volume, BTN_SELECT);
    }

    #[test]
    fn tapped_menu_brightness_is_menu_ignores_tap() {
        // 亮度修饰键是 MENU（tg5040 类）：MENU 按住 + PLUS → ignore_menu
        let mut tap = MenuTapState::new();
        let press_state = InputState {
            just_pressed: BTN_MENU,
            pressed: BTN_MENU,
            ..InputState::new()
        };
        tapped_menu(&press_state, &mut tap, 1000, TG5040_KEYS);
        let mod_state = InputState {
            just_pressed: BTN_PLUS,
            pressed: BTN_MENU | BTN_PLUS,
            ..InputState::new()
        };
        tapped_menu(&mod_state, &mut tap, 1050, TG5040_KEYS);
        let release_state = InputState {
            just_released: BTN_MENU,
            ..InputState::new()
        };
        assert!(
            !tapped_menu(&release_state, &mut tap, 1100, TG5040_KEYS),
            "调亮度后 MENU 释放不应算轻触"
        );
    }

    #[test]
    fn tapped_menu_brightness_not_menu_still_taps() {
        // 亮度修饰键不是 MENU（m17/trimuismart 类）：MENU+PLUS 不算调亮度——
        // 原版条件 BTN_MOD_BRIGHTNESS==BTN_MENU 不成立，不 ignore
        let mut tap = MenuTapState::new();
        let press_state = InputState {
            just_pressed: BTN_MENU,
            pressed: BTN_MENU,
            ..InputState::new()
        };
        tapped_menu(&press_state, &mut tap, 1000, M17_KEYS);
        // m17 上按物理 PLUS 键（不是修饰键 R1）
        let mod_state = InputState {
            just_pressed: BTN_PLUS,
            pressed: BTN_MENU | BTN_PLUS,
            ..InputState::new()
        };
        tapped_menu(&mod_state, &mut tap, 1050, M17_KEYS);
        let release_state = InputState {
            just_released: BTN_MENU,
            ..InputState::new()
        };
        assert!(
            tapped_menu(&release_state, &mut tap, 1100, M17_KEYS),
            "m17 类平台 MENU+PLUS 后 MENU 释放仍算轻触（原版行为）"
        );
    }

    #[test]
    fn tapped_menu_m17_mod_plus_key_still_taps() {
        // m17 类平台：即使按下 mod_keys.plus 对应的键（R1），也不 ignore——
        // 原版 ignore 分支的前提是 `BTN_MOD_BRIGHTNESS == BTN_MENU`
        // （api.c:1391），m17 上恒假，MENU 释放始终按轻触判定。
        // 本场景同时锁定"实现必须用 mod_keys 而非硬编码"——若实现
        // 硬编码 BTN_PLUS，则按 R1 时行为相同（都不 ignore），本测试
        // 无法区分；真正区分由 tg5040 场景（brightness==MENU）承担
        let mut tap = MenuTapState::new();
        let press_state = InputState {
            just_pressed: BTN_MENU,
            pressed: BTN_MENU,
            ..InputState::new()
        };
        tapped_menu(&press_state, &mut tap, 1000, M17_KEYS);
        let mod_state = InputState {
            just_pressed: BTN_R1,
            pressed: BTN_MENU | BTN_R1,
            ..InputState::new()
        };
        tapped_menu(&mod_state, &mut tap, 1050, M17_KEYS);
        let release_state = InputState {
            just_released: BTN_MENU,
            ..InputState::new()
        };
        assert!(
            tapped_menu(&release_state, &mut tap, 1100, M17_KEYS),
            "m17 上 MENU+mod_keys.plus(R1) 后 MENU 释放仍算轻触（brightness!=MENU，原版行为）"
        );
    }
}
