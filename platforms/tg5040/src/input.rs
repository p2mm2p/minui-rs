//! 应用级按键输入（SDL joystick → InputState）
//!
//! 双通道输入架构的**通道 A**：minui/minarch 进程内的应用级按键
//! （十字键/A/B/X/Y/...），经 SDL joystick 事件进入 `InputState` 供 UI
//! 消费。系统级按键（音量/亮度/静音/耳机）由 keymon 独立进程处理
//! （通道 B，见平台自治 bin `platform-tg5040-keymon` 的 `keymon/src/main.rs`）。
//!
//! 本模块是**纯逻辑层**：事件抽象（`AppInputEvent`）+ 跨帧状态
//! （`PadState`）+ 帧处理（`process_frame`）不依赖 SDL——sdl2 事件到
//! `AppInputEvent` 的转换在 `lib.rs` 的 Platform 实现中完成，便于单测。
//!
//! 对应原版 C：事件源 `platform.c` 的 `PLAT_initInput`/`PLAT_pollInput`，
//! 状态机 `api.c` 的 `PAD_*`（api.c:1180-1365）。
//!

use common::input::{
    BTN_A, BTN_ANALOG_DOWN, BTN_ANALOG_LEFT, BTN_ANALOG_RIGHT, BTN_ANALOG_UP, BTN_B, BTN_DPAD_DOWN,
    BTN_DPAD_LEFT, BTN_DPAD_RIGHT, BTN_DPAD_UP, BTN_ID_A, BTN_ID_ANALOG_DOWN, BTN_ID_ANALOG_LEFT,
    BTN_ID_ANALOG_RIGHT, BTN_ID_ANALOG_UP, BTN_ID_B, BTN_ID_COUNT, BTN_ID_DPAD_DOWN,
    BTN_ID_DPAD_LEFT, BTN_ID_DPAD_RIGHT, BTN_ID_DPAD_UP, BTN_ID_L1, BTN_ID_L2, BTN_ID_MENU,
    BTN_ID_MINUS, BTN_ID_PLUS, BTN_ID_POWER, BTN_ID_R1, BTN_ID_R2, BTN_ID_SELECT, BTN_ID_START,
    BTN_ID_X, BTN_ID_Y, BTN_L1, BTN_L2, BTN_MENU, BTN_MINUS, BTN_PLUS, BTN_POWER, BTN_R1, BTN_R2,
    BTN_SELECT, BTN_START, BTN_X, BTN_Y, InputState,
};
// L3/R3 仅 brick 设备有（原版 `JOY_L3 (is_brick?9:NA)`）——条件导入避免非 brick 构建 unused
#[cfg(feature = "brick")]
use common::input::{BTN_ID_L3, BTN_ID_R3, BTN_L3, BTN_R3};

// ── 映射常量（对照原版 platform.h）────────────────────────────

/// A 键 joystick button 号。对应原版 `JOY_A 1`。
pub(crate) const JOY_A: u8 = 1;
/// B 键 joystick button 号。对应原版 `JOY_B 0`。
pub(crate) const JOY_B: u8 = 0;
/// X 键 joystick button 号。对应原版 `JOY_X 3`。
pub(crate) const JOY_X: u8 = 3;
/// Y 键 joystick button 号。对应原版 `JOY_Y 2`。
pub(crate) const JOY_Y: u8 = 2;
/// L1 键 joystick button 号。对应原版 `JOY_L1 4`。
pub(crate) const JOY_L1: u8 = 4;
/// R1 键 joystick button 号。对应原版 `JOY_R1 5`。
pub(crate) const JOY_R1: u8 = 5;
/// SELECT 键 joystick button 号。对应原版 `JOY_SELECT 6`。
pub(crate) const JOY_SELECT: u8 = 6;
/// START 键 joystick button 号。对应原版 `JOY_START 7`。
pub(crate) const JOY_START: u8 = 7;
/// MENU 键 joystick button 号。对应原版 `JOY_MENU 8`。
///
/// 原版还有冗余路由 `JOY_MENU_ALT`/`JOY_MENU_ALT2`（其他设备），
/// tg5040 上为 NA——此处仅定义实际映射
pub(crate) const JOY_MENU: u8 = 8;
/// POWER 键 joystick button 号。对应原版 `JOY_POWER 102`。
pub(crate) const JOY_POWER: u8 = 102;
/// PLUS 键 joystick button 号（smart）。对应原版 `JOY_PLUS 128`。
///
/// **可达性疑点**：128 超过 SDL2 joystick button 索引上限（32）——
/// 标准 SDL2 驱动可能报不出，原版可能有定制驱动。KEY 分支（CODE_PLUS）
/// 兜底；真机验证项（见 tg5040 README「输入」章节）
#[cfg(feature = "smart")]
pub(crate) const JOY_PLUS: u8 = 128;
/// PLUS 键 joystick button 号（brick）。对应原版 `JOY_PLUS 14`。
#[cfg(feature = "brick")]
pub(crate) const JOY_PLUS: u8 = 14;
/// MINUS 键 joystick button 号（smart）。对应原版 `JOY_MINUS 129`。
#[cfg(feature = "smart")]
pub(crate) const JOY_MINUS: u8 = 129;
/// MINUS 键 joystick button 号（brick）。对应原版 `JOY_MINUS 13`。
#[cfg(feature = "brick")]
pub(crate) const JOY_MINUS: u8 = 13;
/// L3 键 joystick button 号（brick only，摇杆按下）。对应原版 `JOY_L3 (is_brick?9:NA)`。
#[cfg(feature = "brick")]
pub(crate) const JOY_L3: u8 = 9;
/// R3 键 joystick button 号（brick only）。对应原版 `JOY_R3 (is_brick?10:NA)`。
#[cfg(feature = "brick")]
pub(crate) const JOY_R3: u8 = 10;

/// POWER 键键盘 scancode。对应原版 `CODE_POWER 102`。
/// 注意：SDL 键盘 scancode 的 102 值来自原版定义——键盘事件通道
/// 的到达性同样是真机验证项（KEY 分支保留以兜底）
pub(crate) const CODE_POWER: u16 = 102;
/// PLUS 键键盘 scancode（兜底分支）。对应原版 `CODE_PLUS 128`。
pub(crate) const CODE_PLUS: u16 = 128;
/// MINUS 键键盘 scancode（兜底分支）。对应原版 `CODE_MINUS 129`。
pub(crate) const CODE_MINUS: u16 = 129;

/// L2 轴号（ABSZ）。对应原版 `AXIS_L2 2`。
pub(crate) const AXIS_L2: u8 = 2;
/// R2 轴号（RABSZ）。对应原版 `AXIS_R2 5`。
pub(crate) const AXIS_R2: u8 = 5;
/// 左摇杆 X 轴号（ABS_X）。对应原版 `AXIS_LX 0`。
pub(crate) const AXIS_LX: u8 = 0;
/// 左摇杆 Y 轴号（ABS_Y）。对应原版 `AXIS_LY 1`。
pub(crate) const AXIS_LY: u8 = 1;
/// 右摇杆 X 轴号（ABS_RX）。对应原版 `AXIS_RX 3`。
pub(crate) const AXIS_RX: u8 = 3;
/// 右摇杆 Y 轴号（ABS_RY）。对应原版 `AXIS_RY 4`。
pub(crate) const AXIS_RY: u8 = 4;

// ── 事件抽象（解耦 SDL）───────────────────────────────────────

/// 抽象输入事件——sdl2 事件经 `lib.rs` 适配层转换为本枚举后喂入状态机
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppInputEvent {
    /// joystick 按钮（button 号为 `JOY_*` 常量）
    Button { button: u8, pressed: bool },
    /// 十字键 hat 方向
    Hat(HatDir),
    /// 摇杆/触发轴（value 为 SDL 原始值，-32768..32767）
    Axis { axis: u8, value: i32 },
    /// 键盘 scancode（兜底通道）
    Key { scancode: u16, pressed: bool },
}

/// 十字键 hat 方向（对应 SDL `SDL_HAT_*` 常量与 sdl2 的 `HatState`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HatDir {
    /// 居中（全部释放）
    Centered,
    /// 上
    Up,
    /// 下
    Down,
    /// 左
    Left,
    /// 右
    Right,
    /// 左上（对角线）
    LeftUp,
    /// 左下（对角线）
    LeftDown,
    /// 右上（对角线）
    RightUp,
    /// 右下（对角线）
    RightDown,
}

// ── 跨帧状态与帧处理 ──────────────────────────────────────────

/// 按键重复节奏常量。对应原版 `PAD_REPEAT_DELAY 300`/`PAD_REPEAT_INTERVAL 100`。
pub(crate) const REPEAT_DELAY: u32 = 300;
/// 按键重复间隔（ms）
pub(crate) const REPEAT_INTERVAL: u32 = 100;
/// 摇杆方向 deadzone。对应原版 `AXIS_DEADZONE 0x4000`。
pub(crate) const AXIS_DEADZONE: i32 = 0x4000;

/// 跨帧输入状态（对应原版 `pad: PAD_Context`，api.c:1131）
///
/// `process_frame` 每帧消费事件并产出 `InputState`——本结构体由
/// 平台 `poll_input` 持有（`Tg5040` 结构体的 `input`/`repeat_at` 字段
/// 整合于此），跨帧保留按键/重复计时/摇杆原始值。
#[derive(Debug)]
pub(crate) struct PadState {
    /// 当前按下的键位掩码
    pub pressed: u32,
    /// 本帧新按下的键位掩码（帧内累积）
    pub just_pressed: u32,
    /// 本帧释放的键位掩码（帧内累积）
    pub just_released: u32,
    /// 本帧触发重复的键位掩码（帧内累积）
    pub just_repeated: u32,
    /// 每个键位的下一次重复时刻（ms）
    pub repeat_at: [u32; BTN_ID_COUNT],
    /// 左摇杆原始值（x, y）
    pub laxis: (i32, i32),
    /// 右摇杆原始值（x, y）
    pub raxis: (i32, i32),
}

impl PadState {
    /// 创建初始状态
    pub fn new() -> Self {
        Self {
            pressed: 0,
            just_pressed: 0,
            just_released: 0,
            just_repeated: 0,
            repeat_at: [0; BTN_ID_COUNT],
            laxis: (0, 0),
            raxis: (0, 0),
        }
    }

    /// 重置全部跨帧状态（按键掩码 + 重复计时 + 摇杆原始值）
    ///
    /// 对应 C `PAD_reset`（api.c:1180-1186），由 `Platform::reset_input`
    /// 在睡眠前/唤醒后调用（对应 C `PWR_fauxSleep` 的两次 `PAD_reset`，
    /// api.c:1687-1694）——防止睡眠按键事件残留导致唤醒后误触发。
    ///
    /// **与 C 原版的差异**：C 版只清 4 个按键掩码，**保留** `repeat_at`
    /// 重复计时表——唤醒后按键若仍按住会立即重复；Rust 版连 `repeat_at`
    /// 与摇杆原始值一并清零，唤醒后按住键需重新 300ms 才重复
    /// （更干净，属有意改进）。
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

impl Default for PadState {
    fn default() -> Self {
        Self::new()
    }
}

/// 处理一帧：帧首清零瞬态 + repeat 扫描，然后消费全部事件
///
/// 对应原版 `PLAT_pollInput`（api.c:1180-1365）——帧首清零
/// `just_*`、repeat 扫描、`SDL_PollEvent` 事件循环。
///
/// ## 参数
///
/// - `state`：跨帧状态（`pressed` 等跨帧保留）
/// - `events`：本帧从事件泵消费的抽象事件（可为空）
/// - `tick`：当前时刻（毫秒，`SDL_GetTicks()` 或等价）
///
/// ## 返回
///
/// 本帧的 `InputState`（供 `poll_input` 返回给 UI 层）
pub(crate) fn process_frame(
    state: &mut PadState,
    events: &[AppInputEvent],
    tick: u32,
) -> InputState {
    // 帧首：清零瞬态（对应原版 api.c:1183-1185）
    state.just_pressed = 0;
    state.just_released = 0;
    // repeat 扫描（对应原版 api.c:1190-1198）
    state.just_repeated = scan_repeats(state.pressed, tick, &mut state.repeat_at);

    for ev in events {
        match *ev {
            AppInputEvent::Button { button, pressed } => {
                handle_button(state, button, pressed, tick);
            }
            AppInputEvent::Hat(dir) => {
                handle_hat(state, dir, tick);
            }
            AppInputEvent::Axis { axis, value } => {
                handle_axis(state, axis, value, tick);
            }
            AppInputEvent::Key { scancode, pressed } => {
                handle_key(state, scancode, pressed, tick);
            }
        }
    }

    InputState {
        pressed: state.pressed,
        just_pressed: state.just_pressed,
        just_released: state.just_released,
        just_repeated: state.just_repeated,
        // 摇杆原始轴值随帧快照带出（对应原版 `pad.laxis/raxis`——
        // `InputState` 是跨帧 `PadState` 的每帧副本）
        laxis: state.laxis,
        raxis: state.raxis,
    }
}

/// 帧首重复扫描：按住超时的键位置 `just_repeated` 并推进下一次重复时刻
///
/// 对应原版 api.c:1190-1198：`tick >= repeat_at[i]` → 重复 + 间隔推进。
/// 首次重复延迟由按下路径设置（`repeat_at[id] = tick + 300`）。
fn scan_repeats(pressed: u32, tick: u32, repeat_at: &mut [u32; BTN_ID_COUNT]) -> u32 {
    let mut just_repeated = 0;
    for (id, slot) in repeat_at.iter_mut().enumerate() {
        let btn = 1u32 << id;
        if pressed & btn != 0 && tick >= *slot {
            just_repeated |= btn;
            *slot = slot.saturating_add(REPEAT_INTERVAL);
        }
    }
    just_repeated
}

/// 按键按下/释放的共用状态更新
///
/// 对应原版 api.c:1309-1313（`if (!pressed) {...} else if (!(is_pressed & btn)) {...}`）。
/// 释放时同时清 `just_repeated`（hat 分支显式如此，按钮路径由帧首
/// 清零 + 此处位操作共同保证）。
fn apply_button(state: &mut PadState, mask: u32, id: usize, pressed: bool, tick: u32) {
    if !pressed {
        state.pressed &= !mask;
        state.just_repeated &= !mask;
        state.just_released |= mask;
    } else if state.pressed & mask == 0 {
        state.just_pressed |= mask;
        state.just_repeated |= mask;
        state.pressed |= mask;
        state.repeat_at[id] = tick + REPEAT_DELAY;
    }
}

/// joystick 按钮事件 → 键位（对应原版 api.c:1239-1280 的 JOY_* 映射）
///
/// tg5040 的十字键走 hat（JOY_UP/DOWN/LEFT/RIGHT 为 NA）——按钮通道
/// 只处理 JOY_* 中实际映射的按钮号，未映射的按钮忽略
fn handle_button(state: &mut PadState, button: u8, pressed: bool, tick: u32) {
    let Some((mask, id)) = joy_to_btn(button) else {
        return;
    };
    apply_button(state, mask, id, pressed, tick);
}

/// JOY_* 按钮号 → (掩码, 键位 id)
///
/// 对应原版 api.c:1239-1280 的 if-else 链（Rust 用 match）。
/// tg5040 的十字键走 hat（JOY_UP/DOWN/LEFT/RIGHT 为 NA），此处不映射。
fn joy_to_btn(joy: u8) -> Option<(u32, usize)> {
    let (mask, id) = match joy {
        JOY_A => (BTN_A, BTN_ID_A),
        JOY_B => (BTN_B, BTN_ID_B),
        JOY_X => (BTN_X, BTN_ID_X),
        JOY_Y => (BTN_Y, BTN_ID_Y),
        JOY_L1 => (BTN_L1, BTN_ID_L1),
        JOY_R1 => (BTN_R1, BTN_ID_R1),
        JOY_SELECT => (BTN_SELECT, BTN_ID_SELECT),
        JOY_START => (BTN_START, BTN_ID_START),
        JOY_MENU => (BTN_MENU, BTN_ID_MENU),
        JOY_POWER => (BTN_POWER, BTN_ID_POWER),
        JOY_PLUS => (BTN_PLUS, BTN_ID_PLUS),
        JOY_MINUS => (BTN_MINUS, BTN_ID_MINUS),
        #[cfg(feature = "brick")]
        JOY_L3 => (BTN_L3, BTN_ID_L3),
        #[cfg(feature = "brick")]
        JOY_R3 => (BTN_R3, BTN_ID_R3),
        _ => return None,
    };
    Some((mask, id))
}

/// hat 十字键事件 → 4 方向状态机
///
/// 对应原版 api.c:1245-1272——每次 hat 事件是全量描述（4 方向的新状态）。
/// 注意原版语义（逐行等价保留）：
/// - 对角线（LEFTUP 等）同时置两方向
/// - 释放路径**无条件置 `just_released`**（即使该方向本来没按——
///   原版行为，保留）
/// - 释放时同时清 `just_repeated`
/// - 按下时 `just_pressed` + `just_repeated` 同时置位
fn handle_hat(state: &mut PadState, dir: HatDir, tick: u32) {
    // (掩码, id, 本事件后是否按下) × 4 方向
    let dirs = match dir {
        HatDir::Centered => [
            (BTN_DPAD_UP, BTN_ID_DPAD_UP, false),
            (BTN_DPAD_DOWN, BTN_ID_DPAD_DOWN, false),
            (BTN_DPAD_LEFT, BTN_ID_DPAD_LEFT, false),
            (BTN_DPAD_RIGHT, BTN_ID_DPAD_RIGHT, false),
        ],
        HatDir::Up => [
            (BTN_DPAD_UP, BTN_ID_DPAD_UP, true),
            (BTN_DPAD_DOWN, BTN_ID_DPAD_DOWN, false),
            (BTN_DPAD_LEFT, BTN_ID_DPAD_LEFT, false),
            (BTN_DPAD_RIGHT, BTN_ID_DPAD_RIGHT, false),
        ],
        HatDir::Down => [
            (BTN_DPAD_UP, BTN_ID_DPAD_UP, false),
            (BTN_DPAD_DOWN, BTN_ID_DPAD_DOWN, true),
            (BTN_DPAD_LEFT, BTN_ID_DPAD_LEFT, false),
            (BTN_DPAD_RIGHT, BTN_ID_DPAD_RIGHT, false),
        ],
        HatDir::Left => [
            (BTN_DPAD_UP, BTN_ID_DPAD_UP, false),
            (BTN_DPAD_DOWN, BTN_ID_DPAD_DOWN, false),
            (BTN_DPAD_LEFT, BTN_ID_DPAD_LEFT, true),
            (BTN_DPAD_RIGHT, BTN_ID_DPAD_RIGHT, false),
        ],
        HatDir::Right => [
            (BTN_DPAD_UP, BTN_ID_DPAD_UP, false),
            (BTN_DPAD_DOWN, BTN_ID_DPAD_DOWN, false),
            (BTN_DPAD_LEFT, BTN_ID_DPAD_LEFT, false),
            (BTN_DPAD_RIGHT, BTN_ID_DPAD_RIGHT, true),
        ],
        HatDir::LeftUp => [
            (BTN_DPAD_UP, BTN_ID_DPAD_UP, true),
            (BTN_DPAD_DOWN, BTN_ID_DPAD_DOWN, false),
            (BTN_DPAD_LEFT, BTN_ID_DPAD_LEFT, true),
            (BTN_DPAD_RIGHT, BTN_ID_DPAD_RIGHT, false),
        ],
        HatDir::LeftDown => [
            (BTN_DPAD_UP, BTN_ID_DPAD_UP, false),
            (BTN_DPAD_DOWN, BTN_ID_DPAD_DOWN, true),
            (BTN_DPAD_LEFT, BTN_ID_DPAD_LEFT, true),
            (BTN_DPAD_RIGHT, BTN_ID_DPAD_RIGHT, false),
        ],
        HatDir::RightUp => [
            (BTN_DPAD_UP, BTN_ID_DPAD_UP, true),
            (BTN_DPAD_DOWN, BTN_ID_DPAD_DOWN, false),
            (BTN_DPAD_LEFT, BTN_ID_DPAD_LEFT, false),
            (BTN_DPAD_RIGHT, BTN_ID_DPAD_RIGHT, true),
        ],
        HatDir::RightDown => [
            (BTN_DPAD_UP, BTN_ID_DPAD_UP, false),
            (BTN_DPAD_DOWN, BTN_ID_DPAD_DOWN, true),
            (BTN_DPAD_LEFT, BTN_ID_DPAD_LEFT, false),
            (BTN_DPAD_RIGHT, BTN_ID_DPAD_RIGHT, true),
        ],
    };
    for (mask, id, now_pressed) in dirs {
        if now_pressed {
            if state.pressed & mask == 0 {
                // 新按下（对应原版 `state==1 && !(is_pressed & btn)`）
                state.just_pressed |= mask;
                state.just_repeated |= mask;
                state.pressed |= mask;
                state.repeat_at[id] = tick + REPEAT_DELAY;
            }
        } else {
            // 释放（对应原版 state==0 分支——无条件置 just_released）
            state.pressed &= !mask;
            state.just_repeated &= !mask;
            state.just_released |= mask;
        }
    }
}

/// 轴事件 → L2/R2 触发按钮 + 摇杆方向/原始值
///
/// 对应原版 api.c:1290-1319：
/// - L2/R2 为触发按钮（`val > 0` 按下），含"虚假释放"防御——
///   轴在首次按下前会发一个看似释放的事件，`!pressed && 没按过` 时取消
/// - LX/LY 驱动 analog 方向键位（deadzone，对应 `PAD_setAnalog`）
/// - LX/LY/RX/RY 存原始值（`laxis`/`raxis`——预留，模拟器类消费方）
fn handle_axis(state: &mut PadState, axis: u8, value: i32, tick: u32) {
    if axis == AXIS_L2 {
        set_axis_button(state, BTN_L2, BTN_ID_L2, value > 0, tick);
    } else if axis == AXIS_R2 {
        set_axis_button(state, BTN_R2, BTN_ID_R2, value > 0, tick);
    } else if axis == AXIS_LX {
        state.laxis.0 = value;
        set_analog(
            state,
            BTN_ANALOG_LEFT,
            BTN_ID_ANALOG_LEFT,
            BTN_ANALOG_RIGHT,
            BTN_ID_ANALOG_RIGHT,
            value,
            tick,
        );
    } else if axis == AXIS_LY {
        state.laxis.1 = value;
        set_analog(
            state,
            BTN_ANALOG_UP,
            BTN_ID_ANALOG_UP,
            BTN_ANALOG_DOWN,
            BTN_ID_ANALOG_DOWN,
            value,
            tick,
        );
    } else if axis == AXIS_RX {
        state.raxis.0 = value;
    } else if axis == AXIS_RY {
        state.raxis.1 = value;
    }
}

/// 触发轴（L2/R2）的按钮状态更新——含"虚假释放"防御
///
/// 对应原版 api.c:1290-1307：轴事件在首次按下前会发一个看似释放的
/// 事件——`!pressed && 没按过` 时取消（不置 `just_released`）。
fn set_axis_button(state: &mut PadState, mask: u32, id: usize, pressed: bool, tick: u32) {
    if pressed {
        if state.pressed & mask == 0 {
            state.just_pressed |= mask;
            state.just_repeated |= mask;
            state.pressed |= mask;
            state.repeat_at[id] = tick + REPEAT_DELAY;
        }
    } else if state.pressed & mask != 0 {
        // 真实释放（按过才置 just_released——防御"虚假释放"）
        state.pressed &= !mask;
        state.just_repeated &= !mask;
        state.just_released |= mask;
    }
}

/// 摇杆方向键位更新（对应原版 `PAD_setAnalog`，api.c:1134-1178）
///
/// 超过正向 deadzone → 正向按下；低于负向 deadzone → 负向按下；
/// deadzone 内 → 两方向皆释放。释放路径无条件置 `just_released`
/// （原版行为，与 hat 分支一致）。
fn set_analog(
    state: &mut PadState,
    neg_mask: u32,
    neg_id: usize,
    pos_mask: u32,
    pos_id: usize,
    value: i32,
    tick: u32,
) {
    if value > AXIS_DEADZONE {
        if state.pressed & pos_mask == 0 {
            state.just_pressed |= pos_mask;
            state.just_repeated |= pos_mask;
            state.pressed |= pos_mask;
            state.repeat_at[pos_id] = tick + REPEAT_DELAY;
        }
    } else if value < -AXIS_DEADZONE {
        if state.pressed & neg_mask == 0 {
            state.just_pressed |= neg_mask;
            state.just_repeated |= neg_mask;
            state.pressed |= neg_mask;
            state.repeat_at[neg_id] = tick + REPEAT_DELAY;
        }
    } else {
        state.pressed &= !(pos_mask | neg_mask);
        state.just_released |= pos_mask | neg_mask;
    }
}

/// 键盘事件 → 键位（兜底通道，对应原版 api.c:1202-1222 的 CODE_* 映射）
fn handle_key(state: &mut PadState, scancode: u16, pressed: bool, tick: u32) {
    let (mask, id) = match scancode {
        CODE_POWER => (BTN_POWER, BTN_ID_POWER),
        CODE_PLUS => (BTN_PLUS, BTN_ID_PLUS),
        CODE_MINUS => (BTN_MINUS, BTN_ID_MINUS),
        _ => return, // 未映射的 scancode——忽略
    };
    apply_button(state, mask, id, pressed, tick);
}

// ── 测试 ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use common::input::BTN_NONE;

    // ── 映射常量（对照 platform.h）──

    #[test]
    fn joy_constants_match_original() {
        assert_eq!(JOY_A, 1);
        assert_eq!(JOY_B, 0);
        assert_eq!(JOY_X, 3);
        assert_eq!(JOY_Y, 2);
        assert_eq!(JOY_L1, 4);
        assert_eq!(JOY_R1, 5);
        assert_eq!(JOY_SELECT, 6);
        assert_eq!(JOY_START, 7);
        assert_eq!(JOY_MENU, 8);
        assert_eq!(JOY_POWER, 102);
    }

    #[test]
    #[cfg(feature = "smart")]
    fn joy_plus_minus_smart_values() {
        // smart：128/129（原版 JOY_PLUS/MINUS）
        assert_eq!(JOY_PLUS, 128);
        assert_eq!(JOY_MINUS, 129);
    }

    #[test]
    #[cfg(feature = "brick")]
    fn joy_plus_minus_l3_r3_brick_values() {
        // brick：14/13 + L3/R3 = 9/10（原版 is_brick 分支）
        assert_eq!(JOY_PLUS, 14);
        assert_eq!(JOY_MINUS, 13);
        assert_eq!(JOY_L3, 9);
        assert_eq!(JOY_R3, 10);
    }

    #[test]
    fn code_and_axis_constants_match_original() {
        assert_eq!(CODE_POWER, 102);
        assert_eq!(CODE_PLUS, 128);
        assert_eq!(CODE_MINUS, 129);
        assert_eq!(AXIS_L2, 2);
        assert_eq!(AXIS_R2, 5);
        assert_eq!(AXIS_LX, 0);
        assert_eq!(AXIS_LY, 1);
        assert_eq!(AXIS_RX, 3);
        assert_eq!(AXIS_RY, 4);
    }

    #[test]
    fn semantic_keys_match_original() {
        // 对照 platform.h:111-114——语义键已归 Platform trait 关联常量
        use crate::{Platform, Tg5040};
        assert_eq!(<Tg5040 as Platform>::BTN_MOD_BRIGHTNESS, BTN_MENU);
        assert_eq!(<Tg5040 as Platform>::BTN_MOD_VOLUME, BTN_NONE);
        assert_eq!(<Tg5040 as Platform>::BTN_MOD_PLUS, BTN_PLUS);
        assert_eq!(<Tg5040 as Platform>::BTN_MOD_MINUS, BTN_MINUS);
    }

    #[test]
    fn btn_sleep_matches_original() {
        // 对照 platform.h:111 `#define BTN_SLEEP BTN_POWER`——供调用方
        // 组装 `power::update` 的 `sleep_btn` 参数（spec「BTN_SLEEP 可组装
        // sleep_btn」场景）
        use crate::{Platform, Tg5040};
        assert_eq!(<Tg5040 as Platform>::BTN_SLEEP, BTN_POWER);
    }

    // ── button 事件 ──

    #[test]
    fn button_press_sets_just_pressed() {
        let mut state = PadState::new();
        let out = process_frame(
            &mut state,
            &[AppInputEvent::Button {
                button: JOY_A,
                pressed: true,
            }],
            1000,
        );
        assert!(out.just_pressed(BTN_A));
        assert!(out.is_pressed(BTN_A));
    }

    #[test]
    fn button_release_clears_and_sets_just_released() {
        let mut state = PadState::new();
        let _ = process_frame(
            &mut state,
            &[AppInputEvent::Button {
                button: JOY_A,
                pressed: true,
            }],
            1000,
        );
        let out = process_frame(
            &mut state,
            &[AppInputEvent::Button {
                button: JOY_A,
                pressed: false,
            }],
            1016,
        );
        assert!(out.just_released(BTN_A));
        assert!(!out.is_pressed(BTN_A));
        assert!(!out.just_repeated(BTN_A), "释放应清 just_repeated");
    }

    #[test]
    fn unknown_button_ignored() {
        let mut state = PadState::new();
        let out = process_frame(
            &mut state,
            &[AppInputEvent::Button {
                button: 200,
                pressed: true,
            }],
            1000,
        );
        assert!(!out.any_pressed());
    }

    // ── hat 十字键 ──

    #[test]
    fn hat_up_presses_dpad_up() {
        let mut state = PadState::new();
        let out = process_frame(&mut state, &[AppInputEvent::Hat(HatDir::Up)], 1000);
        assert!(out.just_pressed(BTN_DPAD_UP));
        assert!(out.is_pressed(BTN_DPAD_UP));
        assert!(
            out.just_repeated(BTN_DPAD_UP),
            "hat 按下同时置 just_repeated（原版行为）"
        );
    }

    #[test]
    fn hat_centered_releases_and_clears_repeated() {
        let mut state = PadState::new();
        let _ = process_frame(&mut state, &[AppInputEvent::Hat(HatDir::Up)], 1000);
        let out = process_frame(&mut state, &[AppInputEvent::Hat(HatDir::Centered)], 1016);
        assert!(out.just_released(BTN_DPAD_UP));
        assert!(!out.is_pressed(BTN_DPAD_UP));
        assert!(!out.just_repeated(BTN_DPAD_UP));
    }

    #[test]
    fn hat_diagonal_presses_both_directions() {
        let mut state = PadState::new();
        let out = process_frame(&mut state, &[AppInputEvent::Hat(HatDir::LeftUp)], 1000);
        assert!(out.is_pressed(BTN_DPAD_LEFT) && out.is_pressed(BTN_DPAD_UP));
    }

    // ── axis ──

    #[test]
    fn l2_axis_trigger_button() {
        let mut state = PadState::new();
        let out = process_frame(
            &mut state,
            &[AppInputEvent::Axis {
                axis: AXIS_L2,
                value: 1000,
            }],
            1000,
        );
        assert!(out.is_pressed(BTN_L2));
        // 释放（真实按过）
        let out = process_frame(
            &mut state,
            &[AppInputEvent::Axis {
                axis: AXIS_L2,
                value: 0,
            }],
            1016,
        );
        assert!(out.just_released(BTN_L2));
        assert!(!out.is_pressed(BTN_L2));
    }

    #[test]
    fn l2_axis_fake_release_is_cancelled() {
        // 轴在首次按下前发"看似释放"的事件——没按过时不置 just_released
        let mut state = PadState::new();
        let out = process_frame(
            &mut state,
            &[AppInputEvent::Axis {
                axis: AXIS_L2,
                value: 0,
            }],
            1000,
        );
        assert!(!out.just_released(BTN_L2), "虚假释放应被取消（原版防御）");
    }

    #[test]
    fn analog_axis_direction_keys() {
        let mut state = PadState::new();
        // 超过正向 deadzone → 右方向
        let out = process_frame(
            &mut state,
            &[AppInputEvent::Axis {
                axis: AXIS_LX,
                value: 0x5000,
            }],
            1000,
        );
        assert!(out.just_pressed(BTN_ANALOG_RIGHT));
        // 回到 deadzone → 释放（两方向）
        let out = process_frame(
            &mut state,
            &[AppInputEvent::Axis {
                axis: AXIS_LX,
                value: 0,
            }],
            1016,
        );
        assert!(out.just_released(BTN_ANALOG_LEFT) || out.just_released(BTN_ANALOG_RIGHT));
        assert!(!out.is_pressed(BTN_ANALOG_LEFT) && !out.is_pressed(BTN_ANALOG_RIGHT));
    }

    #[test]
    fn analog_negative_direction() {
        let mut state = PadState::new();
        let out = process_frame(
            &mut state,
            &[AppInputEvent::Axis {
                axis: AXIS_LX,
                value: -0x5000,
            }],
            1000,
        );
        assert!(out.just_pressed(BTN_ANALOG_LEFT));
    }

    #[test]
    fn analog_raw_values_stored() {
        let mut state = PadState::new();
        let _ = process_frame(
            &mut state,
            &[AppInputEvent::Axis {
                axis: AXIS_LX,
                value: 12345,
            }],
            1000,
        );
        let _ = process_frame(
            &mut state,
            &[AppInputEvent::Axis {
                axis: AXIS_RY,
                value: -6789,
            }],
            1016,
        );
        assert_eq!(state.laxis.0, 12345);
        assert_eq!(state.raxis.1, -6789);
    }

    #[test]
    fn analog_raw_values_carried_in_frame_snapshot() {
        // 轴值应随 `process_frame` 返回的 `InputState` 带出（跨帧状态 → 帧快照副本）
        let mut state = PadState::new();
        let out = process_frame(
            &mut state,
            &[
                AppInputEvent::Axis {
                    axis: AXIS_LX,
                    value: 12345,
                },
                AppInputEvent::Axis {
                    axis: AXIS_RY,
                    value: -6789,
                },
            ],
            1000,
        );
        assert_eq!(out.laxis.0, 12345);
        assert_eq!(out.raxis.1, -6789);
        // 未动过的分量保持中立位
        assert_eq!(out.laxis.1, 0);
        assert_eq!(out.raxis.0, 0);
    }

    // ── key 兜底通道 ──

    #[test]
    fn key_power_event() {
        let mut state = PadState::new();
        let out = process_frame(
            &mut state,
            &[AppInputEvent::Key {
                scancode: CODE_POWER,
                pressed: true,
            }],
            1000,
        );
        assert!(out.just_pressed(BTN_POWER));
    }

    // ── repeat 扫描 ──

    #[test]
    fn repeat_rhythm_300_then_100() {
        let mut state = PadState::new();
        // tick=1000 按下
        let _ = process_frame(
            &mut state,
            &[AppInputEvent::Button {
                button: JOY_A,
                pressed: true,
            }],
            1000,
        );
        // 300ms 内：无重复
        let out = process_frame(&mut state, &[], 1300);
        assert!(out.just_repeated(BTN_A), "300ms 到点应首次重复");
        // 100ms 内：无重复
        let out = process_frame(&mut state, &[], 1350);
        assert!(!out.just_repeated(BTN_A));
        // 再 100ms：再次重复
        let out = process_frame(&mut state, &[], 1400);
        assert!(out.just_repeated(BTN_A), "之后每 100ms 重复");
        let out = process_frame(&mut state, &[], 1500);
        assert!(out.just_repeated(BTN_A));
    }

    #[test]
    fn repeat_stops_after_release() {
        let mut state = PadState::new();
        let _ = process_frame(
            &mut state,
            &[AppInputEvent::Button {
                button: JOY_A,
                pressed: true,
            }],
            1000,
        );
        let _ = process_frame(&mut state, &[], 1300); // 首次重复
        // 释放
        let out = process_frame(
            &mut state,
            &[AppInputEvent::Button {
                button: JOY_A,
                pressed: false,
            }],
            1310,
        );
        assert!(out.just_released(BTN_A));
        // 释放后不重复
        let out = process_frame(&mut state, &[], 1500);
        assert!(!out.just_repeated(BTN_A));
    }

    // ── PadState::reset（对应 spec「PadState 重置能力」）──

    #[test]
    fn reset_clears_all_state() {
        // 按键掩码 + 重复计时 + 摇杆原始值全部清零
        let mut state = PadState::new();
        let _ = process_frame(
            &mut state,
            &[
                AppInputEvent::Button {
                    button: JOY_A,
                    pressed: true,
                },
                AppInputEvent::Hat(HatDir::Up),
                AppInputEvent::Axis {
                    axis: AXIS_LX,
                    value: 12345,
                },
                AppInputEvent::Axis {
                    axis: AXIS_RY,
                    value: -6789,
                },
            ],
            1000,
        );
        assert!(state.pressed != 0);
        assert!(state.repeat_at.iter().any(|&t| t != 0));

        state.reset();

        assert_eq!(state.pressed, 0);
        assert_eq!(state.just_pressed, 0);
        assert_eq!(state.just_released, 0);
        assert_eq!(state.just_repeated, 0);
        assert!(state.repeat_at.iter().all(|&t| t == 0), "repeat_at 应清零");
        assert_eq!(state.laxis, (0, 0));
        assert_eq!(state.raxis, (0, 0));
    }

    #[test]
    fn reset_restarts_repeat_timing() {
        // 对应 spec 场景「reset 后重复计时重新开始」：
        // 重置后重复计时从新的按下时刻起算（旧的 1300 已作废）
        let mut state = PadState::new();
        let _ = process_frame(
            &mut state,
            &[AppInputEvent::Button {
                button: JOY_A,
                pressed: true,
            }],
            1000,
        ); // repeat_at[A] = 1000+300 = 1300

        state.reset(); // 模拟睡眠唤醒后重置

        // 唤醒后重新按下（tick 已远超旧重复时刻 1300）
        let _ = process_frame(
            &mut state,
            &[AppInputEvent::Button {
                button: JOY_A,
                pressed: true,
            }],
            5000,
        ); // repeat_at[A] = 5000+300 = 5300

        // 5300 之前（无事件帧）：不重复
        let out = process_frame(&mut state, &[], 5299);
        assert!(!out.just_repeated(BTN_A), "重置后需重新按住 300ms 才重复");
        // 5300 到点：首次重复
        let out = process_frame(&mut state, &[], 5300);
        assert!(out.just_repeated(BTN_A), "300ms 到点应首次重复");
    }
}
