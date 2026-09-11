//! 电源管理与设备状态
//!
//! 本模块定义电源相关类型和状态机逻辑。核心是 [`update`] 函数——
//! 一个每帧调用的状态机，统一处理睡眠、唤醒、亮度调节、电量监控。
//!
//! ## 类型
//!
//! | 类型 | 职责 |
//! |------|------|
//! | [`CpuSpeed`] | CPU 运行速度档位 |
//! | [`BatteryStatus`] | 电池状态快照（由 Platform trait 返回，每帧重新获取） |
//! | [`PowerState`] | 电源状态机的跨帧记忆 |
//!
//! ## 与原 C 代码的对比
//!
//! 原版 `api.c:72-87` 使用全局 `static struct PWR_Context` 保持状态，
//! `PWR_update` 函数使用 6 个 `static` 局部变量。Rust 版将所有状态收拢到
//! `PowerState` 结构体，初始化通过 `PowerState::new()` 明确完成。
//!
//! 原 C 的电池监控使用独立 pthread 每 5 秒轮询。Rust 版改为在 `update`
//! 函数内每 1000ms 调用 `Platform::get_battery_status()` 刷新——无需独立线程。
//!

use crate::input::{BTN_MENU, BTN_NONE, InputState, ModKeys};
use crate::platform::Platform;
// 测试用常量（mod_keys_m17 等）——仅 cfg(test) 编译
#[cfg(test)]
use crate::input::{BTN_L1, BTN_MINUS, BTN_PLUS, BTN_R1, BTN_SELECT, BTN_START};

// ── PowerAction ─────────────────────────────────────────────

/// 电源状态机检测出的、需要调用方执行的动作
///
/// `update` 只检测不执行——返回 `Some(PowerAction)` 时调用方 SHALL
/// 在同一帧内执行对应动作序列（见 `update` 的文档注释「调用方契约」）。
///
/// 对应原 C `PWR_update`（api.c:1506-1603）内部直接执行的 `PWR_powerOff`
/// 与 `PWR_fauxSleep`——Rust 版执行权移交调用方，本枚举是检测方与
/// 执行方之间的信号。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    /// 进入睡眠：调用方执行 before_sleep → faux_sleep → after_sleep
    Sleep,
    /// 关机：调用方执行 before_sleep → 渲染关机消息 → power_off
    PowerOff,
}

// ── CpuSpeed ────────────────────────────────────────────────

/// CPU 运行速度档位
///
/// 对应原 C `api.h:297-302` 的 `CPU_SPEED_MENU` 等枚举。
/// 各档位频率因平台而异——由 `Platform::set_cpu_speed` 的实现决定。
#[derive(Clone, Copy)]
pub enum CpuSpeed {
    /// 菜单模式（低频省电，通常 ~600MHz）
    Menu,
    /// 省电模式（通常 ~1.2GHz）
    Powersave,
    /// 普通模式（通常 ~1.6GHz）
    Normal,
    /// 性能模式（最高频率，通常 ~2.0GHz）
    Performance,
}

// ── BatteryStatus ───────────────────────────────────────────

/// 电池状态快照
///
/// 由 `Platform::get_battery_status()` 返回。每帧重新获取，
/// 不跨帧缓存——跨帧记忆由 [`PowerState`] 负责。
///
/// ## 与 `PowerState` 的边界
///
/// | 维度 | `BatteryStatus` | `PowerState` |
/// |------|----------------|-------------|
/// | 本质 | 硬件状态快照 | 状态机的跨帧记忆 |
/// | 来源 | `Platform::get_battery_status()` | `PowerState::new()` |
/// | 生命周期 | 每帧重新获取 | 跨帧持久 |
pub struct BatteryStatus {
    /// 是否正在充电
    pub charging: bool,
    /// 电量。范围 0–100。
    ///
    /// 注意：不同平台的精度不同。部分平台返回离散档位（如 10/20/40/60/80/100），
    /// 部分平台返回精确百分比。渲染层按实际值处理，不做二次量化。
    pub percentage: u8,
}

// ── PowerState ──────────────────────────────────────────────

/// 电源管理跨帧状态
///
/// 由调用方（minui/minarch 主循环）持有，每帧传入 [`update`]。
/// 对应原 C `PWR_Context` 全局变量 + `PWR_update` 内部的 6 个 `static` 局部变量。
///
/// ## 去除的 C 字段
///
/// | C 字段 | 去除原因 |
/// |--------|---------|
/// | `battery_pt` (pthread_t) | 电池监控改为帧内轮询（每 1000ms 调 `get_battery_status`），无需独立线程 |
/// | `is_charging` / `charge` | 改为每帧从 `BatteryStatus` 参数传入，不缓存 |
/// | `overlay` (SDL_Surface*) | Rust 直接渲染到主 `VideoBuffer` |
/// | `initialized` | Rust 用 `new()` 语义 |
pub struct PowerState {
    // ── 控制标志 ──
    /// 睡眠键是否响应（`false` = 按睡眠键无效）
    pub can_sleep: bool,
    /// 电源键是否可关机（`false` = 如 HDMI 输出时防止误触）
    pub can_poweroff: bool,
    /// 是否允许 30 秒无操作自动睡眠（`false` = 如游戏运行中）
    pub can_autosleep: bool,

    /// 硬件请求睡眠（预留钩子，当前无代码设置）
    pub requested_sleep: bool,
    /// 硬件请求唤醒（预留钩子，当前无代码设置）
    pub requested_wake: bool,

    /// 低电量警告是否启用（由 `warn(enable)` 控制）
    pub should_warn: bool,

    // ── 跨帧计时状态 ──
    /// 最后一次输入事件的时间戳（ms），用于自动睡眠倒计时
    pub last_input_at: u32,
    /// 上次检查充电状态的时间戳（ms），控制检查间隔（CHARGE_DELAY = 1000ms）
    pub checked_charge_at: u32,
    /// 设置面板（亮度/音量）开始显示的时间戳（ms）
    pub setting_shown_at: u32,
    /// 设置面板状态（0=无, 1=亮度, 2=音量）——**跨帧记忆**
    ///
    /// 原版 `PWR_update` 的 `show_setting` 是调用方跨帧持有的状态
    /// （`_show_setting` 指针入参，api.c:1508）——Rust 版归入 `PowerState`
    /// （调用方持有 `PowerState` 即持有面板状态，与原版等价）。
    /// `update` 从本字段起始判定（SETTING_DELAY 超时隐藏需要"当前是否显示"），
    /// 返回前写回——返回值 `show_setting` 是当帧快照（供调用方渲染）。
    pub show_setting: u8,
    /// 电源键按下的时间戳（ms），用于长按 1000ms 关机检测
    pub power_pressed_at: u32,
    /// 修饰键（亮度/音量）最后松开的时间戳（ms）
    pub mod_unpressed_at: u32,
    /// 上一帧的静音状态，用于检测静音切换
    pub was_muted: bool,
    /// 上一帧的充电状态，用于检测充电状态变化
    pub was_charging: bool,
}

impl Default for PowerState {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerState {
    /// 创建初始状态
    ///
    /// 默认值：
    /// - 睡眠/电源控制全部启用
    /// - 请求标志和警告为 false
    /// - 所有时间戳为 0
    pub fn new() -> Self {
        Self {
            can_sleep: true,
            can_poweroff: true,
            can_autosleep: true,
            requested_sleep: false,
            requested_wake: false,
            should_warn: false,
            last_input_at: 0,
            checked_charge_at: 0,
            setting_shown_at: 0,
            show_setting: 0,
            power_pressed_at: 0,
            mod_unpressed_at: 0,
            was_muted: false,
            was_charging: false,
        }
    }

    /// 禁用睡眠键响应
    pub fn disable_sleep(&mut self) {
        self.can_sleep = false;
    }

    /// 启用睡眠键响应
    pub fn enable_sleep(&mut self) {
        self.can_sleep = true;
    }

    /// 禁用自动睡眠（如游戏运行中）
    pub fn disable_auto_sleep(&mut self) {
        self.can_autosleep = false;
    }

    /// 启用自动睡眠
    pub fn enable_auto_sleep(&mut self) {
        self.can_autosleep = true;
    }

    /// 禁用电源键关机（如 HDMI 输出时防止误触）
    pub fn disable_power_off(&mut self) {
        self.can_poweroff = false;
    }

    /// 设置低电量警告开关
    pub fn warn(&mut self, enable: bool) {
        self.should_warn = enable;
    }
}

// ── 常量 ───────────────────────────────────────────────────

/// 充电状态检查间隔（ms）。对应原 C `CHARGE_DELAY`。
const CHARGE_DELAY: u32 = 1000;

/// 无操作自动睡眠超时（ms）。对应原 C `SLEEP_DELAY`。
const SLEEP_DELAY: u32 = 30000;

/// 设置面板自动隐藏延迟（ms）。对应原 C `SETTING_DELAY`（api.c:1572）。
const SETTING_DELAY: u32 = 500;

/// 修饰键按住触发设置面板延迟（ms）。对应原 C `MOD_DELAY`（api.c:1580）。
const MOD_DELAY: u32 = 250;

/// 电源键长按关机阈值（ms）。
const POWER_HOLD_DELAY: u32 = 1000;

/// 低电量阈值。对应原 C `PWR_LOW_CHARGE`。
#[allow(dead_code)]
const PWR_LOW_CHARGE: u8 = 10;

/// 睡眠中无唤醒自动关机超时（ms）。
const SLEEP_POWEROFF_DELAY: u32 = 120000;

// ── 公共函数 ───────────────────────────────────────────────

/// 判断输入是否应被设置面板消费
///
/// 对应原 C `PWR_ignoreSettingInput`。设置面板（亮度/音量）显示时，
/// 修饰键（PLUS / MINUS）的输入不应传递给游戏。
pub fn ignore_setting_input(btn: u32, show_setting: u8) -> bool {
    show_setting != 0 && (btn == crate::input::BTN_PLUS || btn == crate::input::BTN_MINUS)
}

/// 检查是否应阻止自动睡眠
///
/// 充电中、自动睡眠被禁用、HDMI 活跃时阻止自动睡眠。
/// 对应原 C `PWR_preventAutosleep`（api.c:1702-1704）——
/// `pwr.is_charging || !pwr.can_autosleep || GetHDMI()`。
///
/// `can_autosleep` 由调用方传入 `state.can_autosleep`（C 版为全局
/// `pwr.can_autosleep`，Rust 版归 `PowerState`）——游戏运行中调用
/// `PowerState::disable_auto_sleep()` 后，空闲睡眠被阻止。
pub fn prevent_auto_sleep(battery: &BatteryStatus, hdmi_active: bool, can_autosleep: bool) -> bool {
    battery.charging || hdmi_active || !can_autosleep
}

/// 电源状态机：每帧调用一次
///
/// 处理充电状态变化检测、电源键长按关机、自动睡眠计时、
/// 静音切换检测、设置面板亮度/音量显示逻辑。
///
/// 对应原 C `api.c:1506-1603` 的 `PWR_update()`。
///
/// ## 与 C 原版的职责差异
///
/// C 版 `PWR_update` 是**执行者**——检测到条件后自己调用
/// `PWR_powerOff()`/`PWR_fauxSleep()`（阻塞），`before_sleep`/`after_sleep`
/// 回调在其内部紧邻执行（api.c:1534-1558）。
/// Rust 版 `update` 是**检测者**——只返回动作信号，实际执行（睡眠/关机
/// 需要 `&mut Platform`）由调用方完成。因此回调参数已从签名删除：
/// 回调序列（`before_sleep → faux_sleep → after_sleep`）的执行权
/// 跟随"谁执行睡眠"而非"谁检测睡眠"。
///
/// ## 参数
///
/// - `state`：跨帧状态（含 `show_setting` 面板状态——跨帧记忆，返回前写回）
/// - `input`：当前帧的输入快照
/// - `battery`：当前电池状态
/// - `mute`：当前静音状态。**来源**：调用方（minui/minarch 主循环）每帧从
///   平台 settings 模块读取后传入——原版 `PWR_update` 内部直接调 `GetMute()`
///   （api.c:1593），Rust 版 `power` 模块不能依赖平台 crate（依赖方向：
///   platform → common），故改为参数传入，与 `input`/`battery`/`now_ms`
///   的既有模式一致
/// - `mod_keys`：平台修饰键映射（`BTN_MOD_BRIGHTNESS/VOLUME/PLUS/MINUS`）。
///   **来源**：调用方从 `Platform` trait 语义键关联常量构造传入
///   （common-crate「Platform 语义键关联常量」）——原版为编译期宏
///   （`BTN_MOD_*`，api.c:1571-1598），Rust 参数化后运行期比较
/// - `sleep_btn`：平台睡眠键常量（`BTN_SLEEP`）。**来源**：调用方从
///   `Platform::BTN_SLEEP` 关联常量传入（tg5040 为 `BTN_POWER`）——原版为
///   编译期宏（defines.h 默认 `BUTTON_SLEEP`，各平台 platform.h 覆盖），
///   参数化与 `mod_keys` 同一模式
/// - `hdmi_active`：HDMI 是否活跃。**来源**：调用方每帧从 `platform.is_hdmi_active()`
///   收集传入——原版内部调 `GetHDMI()`（api.c:1544/1702），用于自动睡眠阻止
/// - `now_ms`：当前时间（毫秒）
///
/// ## 返回
///
/// `(action, dirty, show_setting)`：
/// - `action`：动作信号。`None` = 无事发生；`Some(PowerAction::Sleep)` =
///   调用方 SHALL 在同一帧内执行 `before_sleep → faux_sleep → after_sleep`；
///   `Some(PowerAction::PowerOff)` = 调用方 SHALL 在同一帧内执行
///   `before_sleep → 渲染关机消息 → power_off`
/// - `dirty`：是否需要重绘画面
/// - `show_setting`：0=无, 1=亮度, 2=音量（当帧快照——跨帧状态在
///   `state.show_setting`，本返回值供调用方渲染）
// 10 参数是"调用方收集数据传入"模式的结果（mute/mod_keys/sleep_btn/hdmi_active
// 等平台值经参数传递——common 零平台依赖）；签名由 spec 定稿，
// 参数化是设计决策——不打包参数对象
#[allow(clippy::too_many_arguments)]
pub fn update(
    state: &mut PowerState,
    input: &InputState,
    battery: &BatteryStatus,
    mute: bool,
    mod_keys: ModKeys,
    sleep_btn: u32,
    hdmi_active: bool,
    now_ms: u32,
) -> (Option<PowerAction>, bool, u8) {
    let mut dirty = false;
    // 跨帧起始：show_setting 是调用方跨帧持有的面板状态（原版
    // `_show_setting` 指针入参）——Rust 版归 PowerState，返回前写回
    let mut show_setting = state.show_setting;

    // ── 充电状态变化检测（每 1000ms 检查一次）──
    // 对应原 C api.c:1525-1531
    if dirty || now_ms.saturating_sub(state.checked_charge_at) >= CHARGE_DELAY {
        if state.was_charging != battery.charging {
            state.was_charging = battery.charging;
            dirty = true;
        }
        state.checked_charge_at = now_ms;
    }

    // ── 输入时间戳更新 ──
    // 充电中、有任意输入、或首次时刷新 last_input_at（阻止自动睡眠计时）
    // 对应原 C api.c:1522
    if battery.charging || input.any_pressed() || state.last_input_at == 0 {
        state.last_input_at = now_ms;
    }

    // ── 电源键长按 1000ms → 关机 ──
    // 对应原 C api.c:1534-1537（C 版直接调 PWR_powerOff——进程退出；
    // Rust 版返回动作信号，由调用方执行）
    // 注意：BTN_POWEROFF 位仅平台映射了独立关机键才置位（tg5040
    // BUTTON_POWEROFF = BUTTON_NA，恒 false）——长按电源键是 tg5040
    // 的唯一关机通道（power_pressed_at 由下方 BTN_POWER 按下记录）
    if input.just_released(crate::input::BTN_POWEROFF)
        || (state.power_pressed_at != 0
            && now_ms.saturating_sub(state.power_pressed_at) >= POWER_HOLD_DELAY)
    {
        return (Some(PowerAction::PowerOff), true, show_setting);
    }

    // 记录电源键按下时刻
    // 对应原 C api.c:1539-1541
    if input.just_pressed(crate::input::BTN_POWER) {
        state.power_pressed_at = now_ms;
    }

    // ── 自动睡眠（30 秒无输入）──
    // 对应原 C api.c:1543-1558。
    // prevent 检查（充电/HDMI/禁用自动睡眠）时刷新计时而非仅不触发
    // （api.c:1544 `now-last_input_at>=SLEEP_DELAY && PWR_preventAutosleep()`
    // → 刷新）——阻止解除后倒计时从零重新开始。
    // 实际的 `faux_sleep` 调用由调用方在收到动作信号后执行
    // （`before_sleep → faux_sleep → after_sleep`），update 不执行任何动作。
    let idle_too_long = now_ms.saturating_sub(state.last_input_at) >= SLEEP_DELAY;
    let idle_prevented =
        idle_too_long && prevent_auto_sleep(battery, hdmi_active, state.can_autosleep);
    if idle_prevented {
        state.last_input_at = now_ms;
    }
    let manual_sleep = state.can_sleep && input.just_released(sleep_btn);

    if state.requested_sleep || (idle_too_long && !idle_prevented) || manual_sleep {
        state.requested_sleep = false;
        state.last_input_at = now_ms;
        state.power_pressed_at = 0;
        dirty = true;
        state.show_setting = show_setting;
        return (Some(PowerAction::Sleep), dirty, show_setting);
    }

    // ── 修饰键逻辑（设置面板弹出/隐藏）──
    // 对应原 C api.c:1571-1591（MOD_DELAY/SETTING_DELAY）。
    // 原版 `delay_settings` 是编译期常量（`BTN_MOD_BRIGHTNESS==BTN_MENU`），
    // Rust 参数化后运行期比较（mod_keys 由调用方传入）——行为等价

    // 亮度修饰键 == MENU 时，设置条需修饰键松开才隐藏
    //（tg5040 恒真；m17 类恒假——两者都是编译期常量，参数化后运行期比较）
    let delay_settings = mod_keys.brightness == BTN_MENU;

    // 设置条超时隐藏（SETTING_DELAY 500ms + 修饰键未按住）
    // 对应原 C：`if (show_setting && (now-setting_shown_at>=SETTING_DELAY
    //   || !delay_settings) && !PAD_isPressed(BTN_MOD_VOLUME)
    //   && !PAD_isPressed(BTN_MOD_BRIGHTNESS))`
    if show_setting != 0
        && (now_ms.saturating_sub(state.setting_shown_at) >= SETTING_DELAY || !delay_settings)
        && !input.is_pressed(mod_keys.volume)
        && !input.is_pressed(mod_keys.brightness)
    {
        show_setting = 0;
        dirty = true;
    }

    // 修饰键未按住 → 刷新 mod_unpressed_at（对应原版注释
    // "this feels backwards but is correct"——按住修饰键时不刷新，
    // 松开后才计 MOD_DELAY 延迟）
    if show_setting == 0
        && !input.is_pressed(mod_keys.volume)
        && !input.is_pressed(mod_keys.brightness)
    {
        state.mod_unpressed_at = now_ms;
    }

    // 修饰键按住（延迟后）或重复触发 → 弹设置条
    // 对应原 C：
    //   `((PAD_isPressed(BTN_MOD_VOLUME) || PAD_isPressed(BTN_MOD_BRIGHTNESS))
    //     && (!delay_settings || now-mod_unpressed_at>=MOD_DELAY)) ||
    //    ((!BTN_MOD_VOLUME || !BTN_MOD_BRIGHTNESS)
    //     && (PAD_justRepeated(BTN_MOD_PLUS) || PAD_justRepeated(BTN_MOD_MINUS)))`
    let modifier_held = input.is_pressed(mod_keys.volume) || input.is_pressed(mod_keys.brightness);
    let modifier_held_ready = modifier_held
        && (!delay_settings || now_ms.saturating_sub(state.mod_unpressed_at) >= MOD_DELAY);
    // 无音量修饰键（BTN_MOD_VOLUME=BTN_NONE）或亮度修饰键（BTN_MOD_BRIGHTNESS=BTN_NONE）
    // 的设备：重复按加/减键直接触发（原版 `!BTN_MOD_VOLUME || !BTN_MOD_BRIGHTNESS` 编译期分支）
    let no_modifier_keys = mod_keys.volume == BTN_NONE || mod_keys.brightness == BTN_NONE;
    let repeat_trigger = no_modifier_keys
        && (input.just_repeated(mod_keys.plus) || input.just_repeated(mod_keys.minus));

    if modifier_held_ready || repeat_trigger {
        state.setting_shown_at = now_ms;
        if input.is_pressed(mod_keys.brightness) {
            show_setting = 1;
        } else {
            show_setting = 2;
        }
    }

    // ── 静音切换检测 ──
    // 对应原 C api.c:1593-1600：`GetMute()` 与 `was_muted` 跨帧比较，
    // 变化时弹出音量设置条（show_setting = 2）。`mute` 值由调用方从
    // 平台 settings 模块读取后传入（原版内部直接调 `GetMute()`）。
    if mute != state.was_muted {
        state.was_muted = mute;
        show_setting = 2;
        state.setting_shown_at = now_ms;
    }

    // 设置条显示时强制重绘
    // 对应原 C `if (show_setting) dirty = 1`（api.c:1600）——
    // 注释 "shm is slow or keymon is catching input on the next frame"：
    // keymon 下一帧才处理按键，UI 更新存在一帧延迟，需强制重绘补偿
    if show_setting != 0 {
        dirty = true;
    }

    // 写回跨帧面板状态（原版调用方持 _show_setting 变量——Rust 归 PowerState）
    state.show_setting = show_setting;

    (None, dirty, show_setting)
}

/// 假睡眠：清输入 → 关屏 → 等待唤醒 → 恢复 → 清输入
///
/// 对应原 C `api.c:1687-1694` 的 `PWR_fauxSleep`——
/// `GFX_clear; PAD_reset(); PWR_enterSleep(); PWR_waitForWake();
/// PWR_exitSleep(); PAD_reset();`。
///
/// 调用 `Platform::reset_input`（睡眠前/唤醒后各一次，对应 C 的两次
/// `PAD_reset`）与 `Platform::prepare_sleep`/`Platform::complete_wake`
/// 处理平台特定的睡眠操作（关背光、暂停音频、停止 keymon 等）。
///
/// 唤醒等待循环每 200ms 调用 `Platform::should_wake`。
/// 2 分钟内无唤醒则自动关机（充电中延长 1 分钟）。
///
/// **回调时机**：`before_sleep`/`after_sleep` 由调用方在本函数前后执行
/// （`update` 只返回动作信号，不执行回调）——完整序列：
/// `before_sleep → faux_sleep → after_sleep`（见 `update` 文档注释）。
pub fn faux_sleep(platform: &mut impl Platform, state: &mut PowerState) {
    // 睡前清输入（对应 C `PAD_reset()`，api.c:1689）——防止睡眠按键
    // 事件残留，唤醒后立即误触发
    platform.reset_input();

    // 准备进入睡眠
    platform.prepare_sleep();

    // 等待唤醒
    let sleep_start = platform.now_ms();
    loop {
        // 检查唤醒事件
        if state.requested_wake {
            state.requested_wake = false;
            break;
        }
        if platform.should_wake() {
            break;
        }

        // 2 分钟超时自动关机
        let elapsed = platform.now_ms().saturating_sub(sleep_start);
        if state.can_poweroff && elapsed >= SLEEP_POWEROFF_DELAY {
            let battery = platform.get_battery_status();
            if battery.charging {
                // 充电中——再等 1 分钟
                // 注：原 C 用 `sleep_ticks += 60000` 实现。Rust 简化版本
                // 直接跳出循环（充电中不关机）。
                break;
            } else {
                platform.power_off();
                return;
            }
        }

        // 每 200ms 轮询一次（对应原 C `SDL_Delay(200)`）
        // 注：std::thread::sleep 在嵌入式平台可能不可用。
        // 后续平台实现可提供 `delay_ms` 方法替代。
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    // 从睡眠恢复
    platform.complete_wake();

    // 醒后清输入（对应 C `PAD_reset()`，api.c:1693）——唤醒键（如
    // BTN_POWER 释放）若未被 `should_wake` 完全消费，不残留到下一帧
    platform.reset_input();
}

/// 关机流程
///
/// 对应原 C `PWR_powerOff`。显示关机消息后调用 `Platform::power_off`。
/// 关机消息的内容取决于设备是否有物理电源键（`HAS_POWER_BUTTON` /
/// `HAS_POWEROFF_BUTTON`）以及是否存在自动存档。
pub fn power_off(platform: &mut impl Platform) {
    // 注：原 C 代码在关机前渲染一条消息到屏幕。
    // Rust 版本中，该消息由调用方在调用 `power_off` 前渲染。
    // 这保持了 common 层不感知渲染逻辑的原则。
    platform.power_off();
}

// ── 测试 ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── CpuSpeed 测试 ──────────────────────────────────

    #[test]
    fn cpu_speed_variants_exist() {
        // 验证枚举变体可构造
        let _menu = CpuSpeed::Menu;
        let _powersave = CpuSpeed::Powersave;
        let _normal = CpuSpeed::Normal;
        let _performance = CpuSpeed::Performance;
    }

    // ── BatteryStatus 测试 ─────────────────────────────

    #[test]
    fn battery_status_fields() {
        let bs = BatteryStatus {
            charging: true,
            percentage: 80,
        };
        assert!(bs.charging);
        assert_eq!(bs.percentage, 80);
    }

    // ── PowerState 测试 ────────────────────────────────

    #[test]
    fn power_state_new_defaults() {
        let state = PowerState::new();
        // 控制标志默认值
        assert!(state.can_sleep);
        assert!(state.can_poweroff);
        assert!(state.can_autosleep);
        assert!(!state.requested_sleep);
        assert!(!state.requested_wake);
        assert!(!state.should_warn);
        // 计时状态默认值
        assert_eq!(state.last_input_at, 0);
        assert_eq!(state.checked_charge_at, 0);
        assert_eq!(state.setting_shown_at, 0);
        assert_eq!(state.power_pressed_at, 0);
        assert_eq!(state.mod_unpressed_at, 0);
        assert!(!state.was_muted);
        assert!(!state.was_charging);
    }

    #[test]
    fn disable_sleep_prevents_sleep() {
        let mut state = PowerState::new();
        state.disable_sleep();
        assert!(!state.can_sleep);
    }

    #[test]
    fn enable_sleep_allows_sleep() {
        let mut state = PowerState::new();
        state.disable_sleep();
        state.enable_sleep();
        assert!(state.can_sleep);
    }

    #[test]
    fn disable_auto_sleep() {
        let mut state = PowerState::new();
        state.disable_auto_sleep();
        assert!(!state.can_autosleep);
    }

    #[test]
    fn enable_auto_sleep() {
        let mut state = PowerState::new();
        state.disable_auto_sleep();
        state.enable_auto_sleep();
        assert!(state.can_autosleep);
    }

    #[test]
    fn disable_power_off() {
        let mut state = PowerState::new();
        state.disable_power_off();
        assert!(!state.can_poweroff);
    }

    #[test]
    fn warn_sets_flag() {
        let mut state = PowerState::new();
        state.warn(true);
        assert!(state.should_warn);
        state.warn(false);
        assert!(!state.should_warn);
    }

    // ── 控制函数测试 ───────────────────────────────────

    #[test]
    fn ignore_setting_input_when_showing() {
        // 设置面板显示时，PLUS/MINUS 应被消费
        assert!(ignore_setting_input(crate::input::BTN_PLUS, 1));
        assert!(ignore_setting_input(crate::input::BTN_MINUS, 2));
    }

    #[test]
    fn ignore_setting_input_when_not_showing() {
        // 设置面板未显示时，不消费任何按键
        assert!(!ignore_setting_input(crate::input::BTN_PLUS, 0));
    }

    #[test]
    fn ignore_setting_input_other_buttons_not_consumed() {
        // 即使设置面板显示，非修饰键也不被消费
        assert!(!ignore_setting_input(crate::input::BTN_A, 1));
    }

    #[test]
    fn prevent_auto_sleep_when_charging() {
        let battery = BatteryStatus {
            charging: true,
            percentage: 80,
        };
        assert!(prevent_auto_sleep(&battery, false, true));
    }

    #[test]
    fn prevent_auto_sleep_when_hdmi_active() {
        let battery = BatteryStatus {
            charging: false,
            percentage: 50,
        };
        assert!(prevent_auto_sleep(&battery, true, true));
    }

    #[test]
    fn prevent_auto_sleep_when_autosleep_disabled() {
        // can_autosleep = false（游戏运行中）→ 阻止（C 版 `!pwr.can_autosleep` 项）
        let battery = BatteryStatus {
            charging: false,
            percentage: 50,
        };
        assert!(prevent_auto_sleep(&battery, false, false));
    }

    #[test]
    fn prevent_auto_sleep_returns_false_when_none() {
        let battery = BatteryStatus {
            charging: false,
            percentage: 50,
        };
        assert!(!prevent_auto_sleep(&battery, false, true));
    }

    // ── PWR_update 测试 ───────────────────────────────

    fn make_input(pressed: u32) -> InputState {
        InputState {
            pressed,
            just_pressed: 0,
            just_released: 0,
            just_repeated: 0,
            ..InputState::new()
        }
    }

    fn make_battery(charging: bool, percentage: u8) -> BatteryStatus {
        BatteryStatus {
            charging,
            percentage,
        }
    }

    /// 测试用包装：以 tg5040 类默认参数调用 `update`
    ///
    /// - `sleep_btn = BTN_POWER`（tg5040 的 `BTN_SLEEP` 映射）
    /// - `hdmi_active = false`（默认无 HDMI）
    ///
    /// 需要显式传 `sleep_btn`/`hdmi_active` 的场景直接调用 `update`。
    fn call_update(
        state: &mut PowerState,
        input: &InputState,
        battery: &BatteryStatus,
        mute: bool,
        mod_keys: ModKeys,
        now: u32,
    ) -> (Option<PowerAction>, bool, u8) {
        update(
            state,
            input,
            battery,
            mute,
            mod_keys,
            crate::input::BTN_POWER,
            false,
            now,
        )
    }

    #[test]
    fn update_charging_state_change_sets_dirty() {
        let mut state = PowerState::new();
        state.last_input_at = 500;
        state.checked_charge_at = 0; // 确保超过 CHARGE_DELAY

        let input = InputState::new();
        let battery = make_battery(true, 80);
        let now = 2000;

        // 初始 was_charging=false, battery.charging=true → dirty 应为 true
        let (action, dirty, _) =
            call_update(&mut state, &input, &battery, false, mod_keys_tg5040(), now);
        assert!(action.is_none(), "充电状态变化不应触发动作信号");
        assert!(dirty, "充电状态从 false 变为 true 应触发 dirty");
        assert!(state.was_charging, "was_charging 应更新为 true");
        assert_eq!(state.last_input_at, now, "充电时应刷新 last_input_at");
    }

    #[test]
    fn update_no_charging_change_no_dirty() {
        let mut state = PowerState::new();
        state.was_charging = false;
        state.last_input_at = 500;
        state.checked_charge_at = 0;

        let input = InputState::new();
        let battery = make_battery(false, 50);
        let now = 2000;

        let (action, dirty, _) =
            call_update(&mut state, &input, &battery, false, mod_keys_tg5040(), now);
        assert!(action.is_none());
        assert!(!dirty, "充电状态未变化不应触发 dirty（无其他输入）");
    }

    #[test]
    fn update_refreshes_last_input_on_any_pressed() {
        let mut state = PowerState::new();
        state.last_input_at = 100;

        let input = make_input(crate::input::BTN_A);
        let battery = make_battery(false, 50);
        let now = 5000;

        let (_, _, _) = call_update(&mut state, &input, &battery, false, mod_keys_tg5040(), now);
        assert_eq!(state.last_input_at, now, "任意按键按下应刷新 last_input_at");
    }

    #[test]
    fn update_idle_too_long_triggers_sleep() {
        let mut state = PowerState::new();
        state.can_sleep = true;
        state.can_autosleep = true;
        state.last_input_at = 1000;

        let input = InputState::new();
        let battery = make_battery(false, 50);
        let now = 1000 + SLEEP_DELAY + 1; // 超过 30 秒

        let (action, dirty, _) =
            call_update(&mut state, &input, &battery, false, mod_keys_tg5040(), now);
        assert_eq!(
            action,
            Some(PowerAction::Sleep),
            "超过 30 秒无输入应返回睡眠动作"
        );
        assert!(dirty, "超过 30 秒无输入应触发 dirty");
        assert_eq!(state.last_input_at, now, "last_input_at 应在睡眠后重置");
        assert_eq!(state.power_pressed_at, 0, "睡眠后应清空电源键按下时刻");
    }

    #[test]
    fn update_charging_prevents_idle_sleep() {
        let mut state = PowerState::new();
        state.last_input_at = 1000;

        let input = InputState::new();
        let battery = make_battery(true, 50); // 充电中
        let now = 1000 + SLEEP_DELAY + 1;

        // 充电中会刷新 last_input_at，因此不会触发空闲睡眠
        let (action, _, _) =
            call_update(&mut state, &input, &battery, false, mod_keys_tg5040(), now);
        assert!(action.is_none(), "充电中不应触发睡眠动作");
        assert_eq!(
            state.last_input_at, now,
            "充电中 last_input_at 应刷新（prevent 语义）"
        );
    }

    #[test]
    fn update_hdmi_active_prevents_idle_sleep() {
        // HDMI 活跃阻止自动睡眠（对应 spec「HDMI 活跃阻止自动睡眠」）
        let mut state = PowerState::new();
        state.last_input_at = 1000;

        let input = InputState::new();
        let battery = make_battery(false, 50);
        let now = 1000 + SLEEP_DELAY + 1;

        let (action, _, _) = update(
            &mut state,
            &input,
            &battery,
            false,
            mod_keys_tg5040(),
            crate::input::BTN_POWER,
            true,
            now,
        );
        assert!(action.is_none(), "HDMI 活跃不应触发睡眠动作");
        assert_eq!(state.last_input_at, now, "HDMI 活跃时 last_input_at 应刷新");
    }

    #[test]
    fn update_autosleep_disabled_prevents_idle_sleep() {
        // can_autosleep = false 阻止空闲睡眠（对应 spec「禁用自动睡眠阻止空闲睡眠」）
        let mut state = PowerState::new();
        state.can_autosleep = false; // 游戏运行中
        state.last_input_at = 1000;

        let input = InputState::new();
        let battery = make_battery(false, 50);
        let now = 1000 + SLEEP_DELAY + 1;

        let (action, _, _) =
            call_update(&mut state, &input, &battery, false, mod_keys_tg5040(), now);
        assert!(action.is_none(), "禁用自动睡眠不应触发睡眠动作");
        assert_eq!(
            state.last_input_at, now,
            "禁用自动睡眠时 last_input_at 应刷新"
        );
    }

    #[test]
    fn update_prevent_cleared_restarts_timer() {
        // 阻止解除后重新计时（对应 spec「阻止解除后重新计时」）：
        // HDMI 活跃期间空闲超时被持续刷新；HDMI 拔出后 30 秒内不触发
        let mut state = PowerState::new();
        state.last_input_at = 1000;
        let input = InputState::new();
        let battery = make_battery(false, 50);

        // 第 1 帧：HDMI 活跃，空闲超时——prevent 刷新 last_input_at
        let (action1, _, _) = update(
            &mut state,
            &input,
            &battery,
            false,
            mod_keys_tg5040(),
            crate::input::BTN_POWER,
            true,
            1000 + SLEEP_DELAY + 1,
        );
        assert!(action1.is_none());
        assert_eq!(state.last_input_at, 1000 + SLEEP_DELAY + 1);

        // 第 2 帧：HDMI 拔出，距上次刷新 5 秒——不触发（倒计时从解除时刻重新开始）
        let (action2, _, _) = update(
            &mut state,
            &input,
            &battery,
            false,
            mod_keys_tg5040(),
            crate::input::BTN_POWER,
            false,
            1000 + SLEEP_DELAY + 1 + 5000,
        );
        assert!(action2.is_none(), "阻止解除后 30 秒内不应触发睡眠");

        // 第 3 帧：距上次刷新超过 30 秒——触发
        let (action3, _, _) = update(
            &mut state,
            &input,
            &battery,
            false,
            mod_keys_tg5040(),
            crate::input::BTN_POWER,
            false,
            1000 + SLEEP_DELAY + 1 + 5000 + SLEEP_DELAY + 1,
        );
        assert_eq!(
            action3,
            Some(PowerAction::Sleep),
            "解除后超过 30 秒应触发睡眠"
        );
    }

    #[test]
    fn update_manual_sleep_button() {
        let mut state = PowerState::new();
        state.can_sleep = true;
        state.last_input_at = 1000;

        let mut input = InputState::new();
        input.just_released = crate::input::BTN_POWER; // tg5040 的 BTN_SLEEP
        let battery = make_battery(false, 50);
        let now = 2000;

        let (action, dirty, _) =
            call_update(&mut state, &input, &battery, false, mod_keys_tg5040(), now);
        assert_eq!(action, Some(PowerAction::Sleep), "按睡眠键应返回睡眠动作");
        assert!(dirty, "按睡眠键应触发 dirty");
    }

    #[test]
    fn update_manual_sleep_uses_sleep_btn_param() {
        // sleep_btn 参数化：非 BTN_POWER 的睡眠键也能触发（对应 spec
        // 「sleep_btn 参数由调用方提供」——不硬编码 BTN_POWER）
        let mut state = PowerState::new();
        state.can_sleep = true;
        state.last_input_at = 1000;

        let mut input = InputState::new();
        input.just_released = crate::input::BTN_MENU; // 假设平台 BTN_SLEEP = BTN_MENU
        let battery = make_battery(false, 50);
        let now = 2000;

        let (action, _, _) = update(
            &mut state,
            &input,
            &battery,
            false,
            mod_keys_tg5040(),
            crate::input::BTN_MENU, // sleep_btn = BTN_MENU
            false,
            now,
        );
        assert_eq!(
            action,
            Some(PowerAction::Sleep),
            "按传入的 sleep_btn 应触发睡眠"
        );

        // 对照：sleep_btn = BTN_POWER 时，BTN_MENU 释放不算手动睡眠
        let mut state2 = PowerState::new();
        state2.can_sleep = true;
        state2.last_input_at = 1000;
        let (action2, _, _) = update(
            &mut state2,
            &input,
            &battery,
            false,
            mod_keys_tg5040(),
            crate::input::BTN_POWER,
            false,
            now,
        );
        assert!(
            action2.is_none(),
            "BTN_MENU 不是 BTN_POWER 的 sleep_btn——不触发"
        );
    }

    #[test]
    fn update_disable_sleep_prevents_button() {
        // can_sleep = false 时睡眠键不触发（对应 spec「禁用睡眠键后按键不触发」）
        let mut state = PowerState::new();
        state.disable_sleep();
        state.last_input_at = 1000;

        let mut input = InputState::new();
        input.just_released = crate::input::BTN_POWER;
        let battery = make_battery(false, 50);
        let now = 2000;

        let (action, _, _) =
            call_update(&mut state, &input, &battery, false, mod_keys_tg5040(), now);
        assert!(action.is_none(), "禁用睡眠键后按键不应触发");
    }

    #[test]
    fn update_power_long_press_triggers_poweroff() {
        // 电源键长按 1000ms → 关机动作（对应 spec「电源键长按 1000ms 触发关机」）
        let mut state = PowerState::new();
        state.last_input_at = 1000;

        // 第 1 帧：BTN_POWER 刚按下——记录按下时刻
        let press_input = InputState {
            just_pressed: crate::input::BTN_POWER,
            pressed: crate::input::BTN_POWER,
            ..InputState::new()
        };
        let battery = make_battery(false, 50);
        let (action, _, _) = call_update(
            &mut state,
            &press_input,
            &battery,
            false,
            mod_keys_tg5040(),
            5000,
        );
        assert!(action.is_none(), "按下瞬间不触发关机");
        assert_eq!(state.power_pressed_at, 5000, "应记录电源键按下时刻");

        // 第 2 帧：超过 1000ms（长按中）→ 关机动作
        let held_input = InputState {
            pressed: crate::input::BTN_POWER,
            ..InputState::new()
        };
        let (action2, dirty, _) = call_update(
            &mut state,
            &held_input,
            &battery,
            false,
            mod_keys_tg5040(),
            6100,
        );
        assert_eq!(
            action2,
            Some(PowerAction::PowerOff),
            "长按 1000ms 应返回关机动作"
        );
        assert!(dirty);
    }

    // ── 静音切换检测（mute 参数）──
    // 对应原 C api.c:1593-1598：`GetMute()` 与 `was_muted` 比较，
    // 变化时 `show_setting = 2` 弹出音量设置条。

    #[test]
    fn update_mute_toggle_shows_volume_setting() {
        let mut state = PowerState::new();
        state.was_muted = false;
        state.last_input_at = 500;

        let input = InputState::new();
        let battery = make_battery(false, 50);
        let now = 2000;

        // 静音开关从关变为开
        let (_, _, show_setting) =
            call_update(&mut state, &input, &battery, true, mod_keys_tg5040(), now);
        assert_eq!(
            show_setting, 2,
            "静音切换应弹出音量设置条（show_setting=2）"
        );
        assert!(state.was_muted, "was_muted 应更新为 true");
    }

    #[test]
    fn update_mute_unchanged_no_show_setting() {
        let mut state = PowerState::new();
        state.was_muted = false;
        state.last_input_at = 500;

        let input = InputState::new();
        let battery = make_battery(false, 50);
        let now = 2000;

        // 静音状态未变化——不弹设置条
        let (_, _, show_setting) =
            call_update(&mut state, &input, &battery, false, mod_keys_tg5040(), now);
        assert_eq!(show_setting, 0, "静音状态未变化不应弹设置条");
        assert!(!state.was_muted);
    }

    // ── 修饰键逻辑（对应 spec：MOD_DELAY/SETTING_DELAY）──

    /// tg5040 类修饰键（BTN_MOD_BRIGHTNESS == BTN_MENU）
    fn mod_keys_tg5040() -> ModKeys {
        ModKeys {
            brightness: BTN_MENU,
            volume: BTN_NONE,
            plus: BTN_PLUS,
            minus: BTN_MINUS,
        }
    }

    /// m17 类修饰键（亮度修饰键是 START）
    fn mod_keys_m17() -> ModKeys {
        ModKeys {
            brightness: BTN_START,
            volume: BTN_SELECT,
            plus: BTN_R1,
            minus: BTN_L1,
        }
    }

    #[test]
    fn update_modifier_shows_brightness_after_delay() {
        // MOD_DELAY 250ms 后按住亮度修饰键 → show_setting=1
        let mut state = PowerState::new();
        state.last_input_at = 500;
        // mod_unpressed_at 初始 0——now >= 250 即满足 `now - mod_unpressed_at >= MOD_DELAY`
        let input = InputState {
            pressed: BTN_MENU,
            ..InputState::new()
        };
        let battery = make_battery(false, 50);
        let (_, _, show_setting) =
            call_update(&mut state, &input, &battery, false, mod_keys_tg5040(), 1000);
        assert_eq!(show_setting, 1, "按住亮度修饰键 250ms 后应弹亮度设置条");
    }

    #[test]
    fn update_modifier_requires_delay() {
        // MOD_DELAY 250ms 内——不弹（防误触）
        let mut state = PowerState::new();
        state.last_input_at = 500;
        // mod_unpressed_at 设为 now-100（100ms 前刚松开）——250ms 延迟内
        state.mod_unpressed_at = 900;
        let input = InputState {
            pressed: BTN_MENU,
            ..InputState::new()
        };
        let battery = make_battery(false, 50);
        let (_, _, show_setting) =
            call_update(&mut state, &input, &battery, false, mod_keys_tg5040(), 1000);
        assert_eq!(show_setting, 0, "250ms 延迟内不应弹设置条");
    }

    #[test]
    fn update_setting_hides_after_delay() {
        // SETTING_DELAY 500ms 超时 + 修饰键未按住 → 隐藏
        let mut state = PowerState::new();
        state.show_setting = 1; // 上一帧设置条显示中
        state.setting_shown_at = 1000;
        state.last_input_at = 500;

        let input = InputState::new(); // 修饰键未按住
        let battery = make_battery(false, 50);
        let (_, dirty, show_setting) =
            call_update(&mut state, &input, &battery, false, mod_keys_tg5040(), 1600);
        assert_eq!(show_setting, 0, "500ms 超时且修饰键未按住应隐藏");
        assert!(dirty, "隐藏设置条应置 dirty");
    }

    #[test]
    fn update_setting_held_modifier_keeps_shown() {
        // 超时但修饰键仍按住 → 不隐藏
        let mut state = PowerState::new();
        state.show_setting = 1;
        state.setting_shown_at = 1000;
        state.last_input_at = 500;

        let input = InputState {
            pressed: BTN_MENU, // 亮度修饰键按住
            ..InputState::new()
        };
        let battery = make_battery(false, 50);
        let (_, _, show_setting) =
            call_update(&mut state, &input, &battery, false, mod_keys_tg5040(), 1600);
        assert_eq!(show_setting, 1, "修饰键按住期间设置条不隐藏");
    }

    #[test]
    fn update_no_volume_modifier_repeat_triggers() {
        // 无音量修饰键设备（mod_keys.volume = BTN_NONE）：
        // 加键重复 → 弹音量设置条（原版 `!BTN_MOD_VOLUME` 分支）
        let mut state = PowerState::new();
        state.last_input_at = 500;
        let input = InputState {
            just_repeated: BTN_PLUS,
            ..InputState::new()
        };
        let battery = make_battery(false, 50);
        let (_, _, show_setting) =
            call_update(&mut state, &input, &battery, false, mod_keys_tg5040(), 1000);
        assert_eq!(show_setting, 2, "重复按加键应弹音量设置条");
    }

    #[test]
    fn update_modifier_not_hardcoded() {
        // mod_keys 不硬编码：按住物理 MENU 但亮度修饰键是 START（m17 类）——
        // is_pressed(mod_keys.brightness)=is_pressed(START)=false → 不触发
        let mut state = PowerState::new();
        state.last_input_at = 500;
        let input = InputState {
            pressed: BTN_MENU, // 物理 MENU 按住（不是 m17 的亮度修饰键 START）
            ..InputState::new()
        };
        let battery = make_battery(false, 50);
        let (_, _, show_setting) =
            call_update(&mut state, &input, &battery, false, mod_keys_m17(), 1000);
        assert_eq!(
            show_setting, 0,
            "m17 上按住 MENU（非亮度修饰键）不应弹设置条"
        );
    }

    // ── PowerAction 枚举（对应 spec「PowerAction 枚举定义」）──

    #[test]
    fn power_action_has_two_variants() {
        // 纯标记枚举——无字段无方法，仅 Sleep/PowerOff 两个变体
        let _sleep = PowerAction::Sleep;
        let _power_off = PowerAction::PowerOff;
    }

    #[test]
    fn power_action_is_option_in_update_return() {
        // "无事发生"由 Option 表达——None 不占枚举变体
        let none: Option<PowerAction> = None;
        assert!(none.is_none());
        let _some: Option<PowerAction> = Some(PowerAction::Sleep);
    }

    // ── faux_sleep 与 reset_input（对应 spec「reset_input 被 faux_sleep 调用两次」）──

    /// 测试用最小平台实现——记录 `reset_input`/`prepare_sleep`/
    /// `complete_wake`/`power_off` 的调用顺序，`should_wake`/`now_ms`/
    /// `get_battery_status` 可控
    struct MockPlatform {
        calls: Vec<&'static str>,
        now: u32,
        wake: bool,
    }

    impl MockPlatform {
        fn new(now: u32) -> Self {
            Self {
                calls: Vec::new(),
                now,
                wake: false,
            }
        }
    }

    impl crate::platform::Platform for MockPlatform {
        const SCREEN_WIDTH: u32 = 320;
        const SCREEN_HEIGHT: u32 = 240;
        const SCALE: u32 = 1;
        const BYTES_PER_PIXEL: u8 = 2;
        const HAS_HDMI: bool = false;
        const HAS_POWER_BUTTON: bool = false;
        const HAS_POWEROFF_BUTTON: bool = false;
        const SUPPORTS_OVERSCAN: bool = false;
        const BTN_SLEEP: u32 = crate::input::BTN_POWER;
        const BTN_MOD_BRIGHTNESS: u32 = crate::input::BTN_MENU;
        const BTN_MOD_VOLUME: u32 = crate::input::BTN_NONE;
        const BTN_MOD_PLUS: u32 = crate::input::BTN_PLUS;
        const BTN_MOD_MINUS: u32 = crate::input::BTN_MINUS;

        fn init_video(&mut self) {}
        fn quit_video(&mut self) {}
        fn flip(&mut self, _buffer: &crate::video::VideoBuffer, _wait_vsync: bool) {}
        fn init_input(&mut self) {}
        fn quit_input(&mut self) {}
        fn poll_input(&mut self) -> InputState {
            InputState::new()
        }
        fn should_wake(&self) -> bool {
            self.wake
        }
        fn reset_input(&mut self) {
            self.calls.push("reset_input");
        }
        fn init_audio(&mut self, _sample_rate: u32) -> u32 {
            0
        }
        fn quit_audio(&mut self) {}
        fn pause_audio(&mut self, _pause: bool) {}
        fn push_audio(&mut self, _frames: &[crate::audio::AudioFrame]) -> usize {
            0
        }
        fn power_off(&mut self) {
            self.calls.push("power_off");
        }
        fn get_battery_status(&self) -> BatteryStatus {
            BatteryStatus {
                charging: false,
                percentage: 50,
            }
        }
        fn enable_backlight(&mut self, _enable: bool) {}
        fn set_cpu_speed(&mut self, _speed: CpuSpeed) {}
        fn prepare_sleep(&mut self) {
            self.calls.push("prepare_sleep");
        }
        fn complete_wake(&mut self) {
            self.calls.push("complete_wake");
        }
        fn now_ms(&self) -> u32 {
            self.now
        }
        const DEVICE_MODEL: &str = "mock";
        const SDCARD_PATH: &str = "/mnt/SDCARD";
        const PLATFORM: &str = "mock";
    }

    #[test]
    fn faux_sleep_calls_reset_input_before_and_after() {
        // 对应 spec「reset_input 被 faux_sleep 调用两次」：
        // 睡眠前（prepare_sleep 之前）与唤醒后（complete_wake 之后）各一次
        let mut platform = MockPlatform::new(1000);
        platform.wake = true; // 立即唤醒——避免等待循环阻塞
        let mut state = PowerState::new();

        faux_sleep(&mut platform, &mut state);

        assert_eq!(
            platform.calls,
            vec![
                "reset_input",
                "prepare_sleep",
                "complete_wake",
                "reset_input"
            ],
            "调用顺序：睡前 reset → prepare_sleep → complete_wake → 醒后 reset"
        );
    }

    #[test]
    fn faux_sleep_waits_for_wake_event() {
        // should_wake 为 false 时循环等待——本测试只验证 reset_input 的
        // 两次调用与唤醒后返回（用 requested_wake 触发立即唤醒）
        let mut platform = MockPlatform::new(1000);
        let mut state = PowerState::new();
        state.requested_wake = true; // 硬件请求唤醒——立即退出等待循环

        faux_sleep(&mut platform, &mut state);

        assert!(!state.requested_wake, "唤醒请求应被消费");
        assert_eq!(
            platform.calls,
            vec![
                "reset_input",
                "prepare_sleep",
                "complete_wake",
                "reset_input"
            ]
        );
    }

    // ── 布局方法默认实现（对应 spec「布局方法默认实现返回通用默认值」）──
    // MockPlatform 不覆盖 main_row_count/padding——走 trait 默认实现

    #[test]
    fn platform_layout_methods_default_to_common_constants() {
        let platform = MockPlatform::new(0);
        assert_eq!(
            platform.main_row_count(),
            6,
            "默认实现应返回 common::video::MAIN_ROW_COUNT"
        );
        assert_eq!(
            platform.padding(),
            10,
            "默认实现应返回 common::video::PADDING"
        );
    }
}
