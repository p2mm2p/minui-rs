//! keymon——系统级按键监控独立进程（平台自治 bin，独立 crate）
//!
//! 本二进制对应原版 C `keymon.c`：独立于 minui/minarch 运行，
//! 直接读 evdev 设备（`/dev/input/event0-3`）监控**系统级按键**
//! （MENU / 音量 / 静音 / 耳机），调用 [`platform_tg5040::settings`] 模块调节
//! 亮度/音量——即使 minui/minarch 崩溃，音量/亮度仍可调节。
//!
//! 本 crate 为平台自治 bin：show 迁出平台 lib 后的独立包
//! （`platform-tg5040-keymon`），由平台子 xtask（`tg5040-xtask`）编译并
//! 复制进发布包（`SYSTEM/tg5040/bin/keymon`）。evdev 读取模块随 keymon
//! 迁入本 crate（`src/evdev.rs`，原平台 lib 的 `src/evdev.rs`）——平台 lib
//! 不再暴露 evdev。
//!
//! ## 双通道输入架构
//!
//! 应用级按键（十字键/A/B/...）由 minui/minarch 进程内的 SDL joystick
//! 处理（`Platform::poll_input`）；本进程只处理系统级按键。两者构成
//! 双通道输入架构——详见 tg5040 README「系统设置与 keymon」。
//!
//! ## 与原版 C 的对比
//!
//! | 原版 keymon.c | 本实现 |
//! |--------------|--------|
//! | `sigaction` + `volatile int quit` | `libc::sigaction` + `AtomicBool` |
//! | `pthread` watchMute | `std::thread` mute 线程 |
//! | `usleep(16666)` 60fps | `thread::sleep(16ms)` |
//! | `gettimeofday` 时钟 | `std::time::SystemTime` |
//!

mod evdev;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::evdev::{InputDevices, InputEvent};
use platform_tg5040::settings::SettingsHandle;

// ── 按键码常量 ───────────────────────────────────────────────
// 对应原版 keymon.c:25-29 的 CODE_* 宏。

/// MENU 键事件码（三路冗余，对应原版 CODE_MENU0/1/2）。
const CODE_MENU0: u16 = 314;
/// MENU 键事件码（冗余路由）。
const CODE_MENU1: u16 = 315;
/// MENU 键事件码（冗余路由）。
const CODE_MENU2: u16 = 316;
/// 音量加键事件码。对应原版 `CODE_PLUS 115`（KEY_VOLUMEUP）。
const CODE_PLUS: u16 = 115;
/// 音量减键事件码。对应原版 `CODE_MINUS 114`（KEY_VOLUMEDOWN）。
const CODE_MINUS: u16 = 114;
/// 耳机插拔开关事件码。对应原版 `CODE_JACK 2`。
const CODE_JACK: u16 = 2;
/// 静音开关事件码。对应原版 `CODE_MUTE 1`。
const CODE_MUTE: u16 = 1;

/// 事件类型：EV_KEY。内核 `linux/input.h`。
const EV_KEY: u16 = 1;
/// 事件类型：EV_SW（开关）。内核 `linux/input.h`。
const EV_SW: u16 = 5;

/// 事件值：释放。对应原版 `RELEASED 0`。
const RELEASED: i32 = 0;
/// 事件值：按下。对应原版 `PRESSED 1`。
///
/// 实现用 `value != RELEASED` 判断"按下或重复"（对应原版 `if (val)`——
/// val=2 也成立），PRESSED 常量仅测试中使用（断言具体事件值）。
#[cfg_attr(not(test), allow(dead_code))]
const PRESSED: i32 = 1;
/// 事件值：自动重复。对应原版 `REPEAT 2`。
const REPEAT: i32 = 2;

/// 静音开关 GPIO 路径。对应原版 `MUTE_STATE_PATH`（keymon.c:40）。
const MUTE_STATE_PATH: &str = "/sys/class/gpio/gpio243/value";

/// 长按首次重复延迟（ms）。对应原版 keymon.c:146 的 `now + 300`。
const REPEAT_DELAY: u32 = 300;
/// 长按连续重复间隔（ms）。对应原版 keymon.c:179 的 `up_repeat_at += 100`。
const REPEAT_INTERVAL: u32 = 100;
/// 睡眠输入忽略阈值（帧间隔超过此时长视为被挂起过）。对应原版 keymon.c:117 的 `>1000`。
const IGNORE_DELAY: u32 = 1000;
/// 主循环帧间隔（ms）。对应原版 `usleep(16666)` 的 60fps。
const FRAME_MS: u64 = 16;

// ── 退出标志 ─────────────────────────────────────────────────

/// 全局退出标志（SIGTERM 处理器写入，主循环每帧读取）
///
/// 对应原版 `static volatile int quit`（keymon.c:46-47）。
/// `AtomicBool` 替代 `volatile`——跨线程（信号处理器 ↔ 主循环）
/// 的同步由原子操作保证。
static QUIT: AtomicBool = AtomicBool::new(false);

// ── 按键状态机 ───────────────────────────────────────────────

/// 跨帧按键状态（对应原版 keymon.c 主循环的局部变量组：keymon.c:95-103）
#[derive(Debug, Default)]
struct KeyState {
    /// MENU 键是否按下（0/1）——用于区分调亮度还是调音量
    menu_pressed: u32,
    /// 音量加键按下状态（0/1）
    up_pressed: u32,
    /// 音量加键本帧新按下（0/1，处理一次后清零）
    up_just_pressed: u32,
    /// 音量加键长按重复时刻（ms）
    up_repeat_at: u32,
    /// 音量减键按下状态（0/1）
    down_pressed: u32,
    /// 音量减键本帧新按下（0/1，处理一次后清零）
    down_just_pressed: u32,
    /// 音量减键长按重复时刻（ms）
    down_repeat_at: u32,
    /// 上一帧时刻（ms）——帧间隔检测（对应原版 `then`）
    last_frame_at: u32,
}

/// 按键状态机产生的调节动作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// 调高亮度（MENU 按下时按 PLUS）
    BrightnessUp,
    /// 调低亮度（MENU 按下时按 MINUS）
    BrightnessDown,
    /// 调高音量（MENU 未按下时按 PLUS）
    VolumeUp,
    /// 调低音量（MENU 未按下时按 MINUS）
    VolumeDown,
    /// 耳机插拔开关事件（EV_SW）
    SetJack(bool),
    /// 静音开关事件（EV_SW）
    SetMute(bool),
}

/// 处理一帧：喂入事件，产出调节动作
///
/// 对应原版 keymon.c 主循环的按键状态机部分（keymon.c:114-196）。
/// 纯函数——不接触任何 FFI/设置，便于单元测试。
fn process_frame(state: &mut KeyState, events: &[InputEvent], now_ms: u32) -> Vec<Action> {
    let mut actions = Vec::new();

    // ── 睡眠输入忽略 ──
    // 帧间隔 > 1000ms（被 SIGSTOP 挂起过/系统卡住）→ 挂起期间到达的
    // 按键应被丢弃，并清空状态（对应原版 `if (now-then>1000) ignore = 1`
    // 与 ignore 分支的按键状态清零：keymon.c:117, 158-164）
    let ignore =
        state.last_frame_at != 0 && now_ms.saturating_sub(state.last_frame_at) > IGNORE_DELAY;
    state.last_frame_at = now_ms;

    if ignore {
        state.menu_pressed = 0;
        state.up_pressed = 0;
        state.up_just_pressed = 0;
        state.up_repeat_at = 0;
        state.down_pressed = 0;
        state.down_just_pressed = 0;
        state.down_repeat_at = 0;
    } else {
        // ── 事件处理（对应原版 while(read) 循环体：keymon.c:121-155）──
        for ev in events {
            // 开关事件：耳机插拔 / 静音开关
            // 对应原版 EV_SW 分支（keymon.c:124-134）
            if ev.kind == EV_SW {
                if ev.code == CODE_JACK {
                    actions.push(Action::SetJack(ev.value != 0));
                } else if ev.code == CODE_MUTE {
                    actions.push(Action::SetMute(ev.value != 0));
                }
                continue;
            }

            // 按键事件：跳过非 EV_KEY 与自动重复（val > REPEAT 是内核
            // 未知值，原版同样跳过：`if ((ev.type != EV_KEY) || (val > REPEAT)) continue`）
            if ev.kind != EV_KEY || ev.value > REPEAT {
                continue;
            }

            match ev.code {
                // MENU（三路冗余路由）——只记录按下状态
                CODE_MENU0 | CODE_MENU1 | CODE_MENU2 => {
                    state.menu_pressed = ev.value as u32;
                }
                // 音量加——记录按下 + 新按下，设置首次重复时刻
                CODE_PLUS => {
                    state.up_pressed = ev.value as u32;
                    state.up_just_pressed = ev.value as u32;
                    if ev.value != RELEASED {
                        state.up_repeat_at = now_ms + REPEAT_DELAY;
                    }
                }
                // 音量减——同上
                CODE_MINUS => {
                    state.down_pressed = ev.value as u32;
                    state.down_just_pressed = ev.value as u32;
                    if ev.value != RELEASED {
                        state.down_repeat_at = now_ms + REPEAT_DELAY;
                    }
                }
                _ => {} // 其他按键（应用级按键）不处理——keymon 只关心系统级
            }
        }
    }

    // ── 音量/亮度调节决策 ──
    // 对应原版 up/down 处理块（keymon.c:166-196）：
    // 新按下立即响应；持续按住则按重复节奏（首次 300ms，之后每 100ms）
    if state.up_just_pressed != 0 || (state.up_pressed != 0 && now_ms >= state.up_repeat_at) {
        // MENU 按下 → 调亮度；否则调音量
        actions.push(if state.menu_pressed != 0 {
            Action::BrightnessUp
        } else {
            Action::VolumeUp
        });
        if state.up_just_pressed != 0 {
            state.up_just_pressed = 0;
        } else {
            state.up_repeat_at = state.up_repeat_at.saturating_add(REPEAT_INTERVAL);
        }
    }
    if state.down_just_pressed != 0 || (state.down_pressed != 0 && now_ms >= state.down_repeat_at) {
        actions.push(if state.menu_pressed != 0 {
            Action::BrightnessDown
        } else {
            Action::VolumeDown
        });
        if state.down_just_pressed != 0 {
            state.down_just_pressed = 0;
        } else {
            state.down_repeat_at = state.down_repeat_at.saturating_add(REPEAT_INTERVAL);
        }
    }

    actions
}

// ── SIGTERM 处理（Unix 专属）────────────────────────────────

/// 注册 SIGTERM 处理器：设置退出标志
///
/// 对应原版 `sigaction(SIGTERM, &on_term)`（keymon.c:80-82）。
///
/// # Safety
///
/// 本函数包含 unsafe 代码（FFI `sigaction`）。调用者需满足的前置条件：
/// - 处理器为静态函数指针（无捕获）——信号安全
/// - 处理器只做 `AtomicBool` 的 relaxed store——异步信号安全
///   （无锁、无分配、无系统调用）
///
/// 非 Unix 目标（开发机）为空实现——SIGTERM 概念不存在于 Windows。
#[cfg(unix)]
fn install_sigterm_handler() {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        // # Safety：函数项转为信号处理器指针——静态函数，生命周期为 'static
        sa.sa_sigaction = sigterm_handler as *const () as usize;
        libc::sigemptyset(&mut sa.sa_mask);
        sa.sa_flags = 0;
        let ret = libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut());
        assert!(ret == 0, "sigaction 注册失败");
    }
}

/// 非 Unix 目标（Windows 开发机）：无 SIGTERM——空实现
#[cfg(not(unix))]
fn install_sigterm_handler() {}

/// SIGTERM 处理器：设置退出标志
///
/// 对应原版 `on_term`（keymon.c:47）。
/// 仅做 `AtomicBool` store——信号安全。
#[cfg(unix)]
extern "C" fn sigterm_handler(_sig: libc::c_int) {
    QUIT.store(true, Ordering::Relaxed);
}

// ── mute 监控线程 ───────────────────────────────────────────

/// 读取 GPIO 值 → 静音状态
///
/// 对应原版 `getInt(MUTE_STATE_PATH)`（keymon.c:49-57）。
/// 读取失败返回 false（不静音）——与原版 `getInt` 失败返回 0 一致。
fn read_gpio(path: &str) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u8>().ok())
        .map(|v| v != 0)
        .unwrap_or(false)
}

/// 一次轮询的静音变化检测（纯函数，可测）
///
/// 对应原版 watchMute 的 `if (was_muted!=is_muted)` 比较（keymon.c:75-76）。
/// 值变化时更新 `was_muted` 并返回 `true`（调用方执行 `set_mute`）；
/// 值未变化返回 `false`——**不重复调用 `set_mute`**。
fn should_set_mute(was_muted: &mut bool, is_muted: bool) -> bool {
    if *was_muted != is_muted {
        *was_muted = is_muted;
        true
    } else {
        false
    }
}

/// mute 监控线程（对应原版 watchMute pthread：keymon.c:60-77）
///
/// 每 200ms 轮询 GPIO——值变化时调用 `set_mute`。
/// 设置访问经 `SettingsHandle` 内部的进程内 Mutex 隔离
/// （与主循环的并发）。
fn watch_mute(settings: Arc<SettingsHandle>) {
    // 启动时同步一次当前状态（对应原版 watchMute 开头）
    let mut was_muted = read_gpio(MUTE_STATE_PATH);
    settings.set_mute(was_muted);

    while !QUIT.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(200));
        let is_muted = read_gpio(MUTE_STATE_PATH);
        if should_set_mute(&mut was_muted, is_muted) {
            settings.set_mute(is_muted);
        }
    }
}

// ── 时钟 ─────────────────────────────────────────────────────

/// 单调时钟（ms）
///
/// 对应原版 `gettimeofday` 合成的毫秒时钟（keymon.c:110-111）。
/// 用 `SystemTime`——单调且不依赖 SDL。
fn now_ms() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0)
}

// ── main ─────────────────────────────────────────────────────

fn main() {
    install_sigterm_handler();

    // 打开输入设备（单设备失败容错）+ 打开系统设置
    // 对应原版：open inputs 循环（keymon.c:88-91）+ InitSettings()（keymon.c:84）
    let devices = InputDevices::open();
    let settings = Arc::new(SettingsHandle::init());

    // mute 监控线程（对应原版 pthread_create(&mute_pt, ...)：keymon.c:85）
    // 线程与进程同生命周期——主循环退出（SIGTERM）时进程结束，线程随之终止
    std::thread::spawn({
        let settings = Arc::clone(&settings);
        move || watch_mute(settings)
    });

    let mut state = KeyState::default();

    // 主循环（对应原版 while(!quit)：keymon.c:114-202）
    loop {
        if QUIT.load(Ordering::Relaxed) {
            break;
        }

        let now = now_ms();
        let events = devices.poll();

        // 状态机 → 执行动作（亮度/音量 clamp 由 settings 内部保证）
        for action in process_frame(&mut state, &events, now) {
            match action {
                Action::BrightnessUp => settings.set_brightness(settings.brightness() + 1),
                Action::BrightnessDown => {
                    settings.set_brightness(settings.brightness().saturating_sub(1))
                }
                Action::VolumeUp => settings.set_volume(settings.volume() + 1),
                Action::VolumeDown => settings.set_volume(settings.volume().saturating_sub(1)),
                Action::SetJack(jack) => settings.set_jack(jack),
                Action::SetMute(mute) => settings.set_mute(mute),
            }
        }

        // 60fps（对应原版 usleep(16666)）
        std::thread::sleep(Duration::from_millis(FRAME_MS));
    }

    // 退出：设备 fd 由 InputDevices::drop 关闭；共享内存由
    // SettingsHandle::drop 清理（host 时 shm_unlink）
}

// ── 测试 ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: u16, code: u16, value: i32) -> InputEvent {
        InputEvent {
            sec: 0,
            usec: 0,
            kind,
            code,
            value,
        }
    }

    // ── 菜单/音量/亮度调节（对应 spec：MENU+PLUS 调亮度、PLUS 调音量）──

    #[test]
    fn menu_plus_adjusts_brightness() {
        let mut state = KeyState::default();
        let now = 1000;
        // 按下 MENU，再按 PLUS
        let actions = process_frame(&mut state, &[ev(EV_KEY, CODE_MENU0, PRESSED)], now);
        assert!(actions.is_empty(), "MENU 按下本身不产生动作");
        let actions = process_frame(&mut state, &[ev(EV_KEY, CODE_PLUS, PRESSED)], now);
        assert_eq!(
            actions,
            vec![Action::BrightnessUp],
            "MENU 按下时按 PLUS 应调亮度"
        );
    }

    #[test]
    fn plus_without_menu_adjusts_volume() {
        let mut state = KeyState::default();
        let now = 1000;
        let actions = process_frame(&mut state, &[ev(EV_KEY, CODE_PLUS, PRESSED)], now);
        assert_eq!(
            actions,
            vec![Action::VolumeUp],
            "MENU 未按下时按 PLUS 应调音量"
        );
    }

    #[test]
    fn minus_without_menu_adjusts_volume_down() {
        let mut state = KeyState::default();
        let now = 1000;
        let actions = process_frame(&mut state, &[ev(EV_KEY, CODE_MINUS, PRESSED)], now);
        assert_eq!(actions, vec![Action::VolumeDown]);
    }

    #[test]
    fn menu_minus_adjusts_brightness_down() {
        let mut state = KeyState::default();
        let now = 1000;
        let _ = process_frame(&mut state, &[ev(EV_KEY, CODE_MENU1, PRESSED)], now);
        let actions = process_frame(&mut state, &[ev(EV_KEY, CODE_MINUS, PRESSED)], now);
        assert_eq!(actions, vec![Action::BrightnessDown]);
    }

    #[test]
    fn menu_release_then_plus_is_volume() {
        let mut state = KeyState::default();
        let now = 1000;
        let _ = process_frame(&mut state, &[ev(EV_KEY, CODE_MENU2, PRESSED)], now);
        let _ = process_frame(&mut state, &[ev(EV_KEY, CODE_MENU2, RELEASED)], now);
        let actions = process_frame(&mut state, &[ev(EV_KEY, CODE_PLUS, PRESSED)], now);
        assert_eq!(actions, vec![Action::VolumeUp], "MENU 释放后 PLUS 应调音量");
    }

    // ── 长按重复（首次 300ms、之后每 100ms）──

    #[test]
    fn hold_plus_repeats_after_300ms_then_every_100ms() {
        let mut state = KeyState::default();
        let now = 1000;
        // 按下（首次立即响应）
        let a = process_frame(&mut state, &[ev(EV_KEY, CODE_PLUS, PRESSED)], now);
        assert_eq!(a, vec![Action::VolumeUp]);

        // 300ms 内按住——不重复
        let a = process_frame(&mut state, &[], now + 299);
        assert!(a.is_empty(), "300ms 内按住不应重复");
        // 300ms 到点——首次重复
        let a = process_frame(&mut state, &[], now + 300);
        assert_eq!(a, vec![Action::VolumeUp]);
        // 再 100ms——第二次重复
        let a = process_frame(&mut state, &[], now + 400);
        assert_eq!(a, vec![Action::VolumeUp]);
        // 间隔内不重复
        let a = process_frame(&mut state, &[], now + 450);
        assert!(a.is_empty());
    }

    #[test]
    fn release_stops_repeat() {
        let mut state = KeyState::default();
        let now = 1000;
        let _ = process_frame(&mut state, &[ev(EV_KEY, CODE_PLUS, PRESSED)], now);
        let _ = process_frame(&mut state, &[], now + 300); // 首次重复
        // 释放
        let a = process_frame(&mut state, &[ev(EV_KEY, CODE_PLUS, RELEASED)], now + 310);
        assert!(a.is_empty());
        // 释放后不再重复
        let a = process_frame(&mut state, &[], now + 500);
        assert!(a.is_empty(), "释放后不应再重复");
    }

    // ── 睡眠输入忽略（帧间隔 > 1000ms）──

    #[test]
    fn input_during_suspend_is_ignored() {
        let mut state = KeyState::default();
        let _ = process_frame(&mut state, &[], 1000); // 正常帧

        // 模拟被 SIGSTOP 挂起 5 秒后恢复——期间到达的按键应被丢弃
        let actions = process_frame(
            &mut state,
            &[
                ev(EV_KEY, CODE_PLUS, PRESSED),
                ev(EV_KEY, CODE_MENU0, PRESSED),
            ],
            6000,
        );
        assert!(actions.is_empty(), "挂起期间到达的输入应被忽略");
        // 状态被清空——恢复后 PLUS 应是"新按下"而不是长按残留
        assert_eq!(state.menu_pressed, 0);
        assert_eq!(state.up_pressed, 0);
    }

    #[test]
    fn normal_frame_gap_not_ignored() {
        let mut state = KeyState::default();
        let _ = process_frame(&mut state, &[], 1000);
        // 正常帧间隔（< 1000ms）——事件正常处理
        let actions = process_frame(&mut state, &[ev(EV_KEY, CODE_PLUS, PRESSED)], 1016);
        assert_eq!(actions, vec![Action::VolumeUp]);
    }

    // ── 开关事件（EV_SW）──

    #[test]
    fn jack_switch_event() {
        let mut state = KeyState::default();
        let actions = process_frame(&mut state, &[ev(EV_SW, CODE_JACK, PRESSED)], 1000);
        assert_eq!(actions, vec![Action::SetJack(true)]);
    }

    #[test]
    fn mute_switch_event() {
        let mut state = KeyState::default();
        let actions = process_frame(&mut state, &[ev(EV_SW, CODE_MUTE, PRESSED)], 1000);
        assert_eq!(actions, vec![Action::SetMute(true)]);
    }

    #[test]
    fn value_greater_than_repeat_is_skipped() {
        // 原版 `val > REPEAT` 检查（keymon.c:122）——只处理 0/1/2，未知值跳过
        let mut state = KeyState::default();
        let actions = process_frame(&mut state, &[ev(EV_KEY, CODE_PLUS, 3)], 1000);
        assert!(actions.is_empty());
    }

    #[test]
    fn kernel_repeat_value_treated_as_press() {
        // 原版：val=2（内核自动重复）不满足 `val > REPEAT`——不跳过，
        // 与按下同样处理（触发一次动作并重置重复计时）。设备上事件流
        // 实际只有 0/1，此测试锁定原版行为
        let mut state = KeyState::default();
        let actions = process_frame(&mut state, &[ev(EV_KEY, CODE_PLUS, REPEAT)], 1000);
        assert_eq!(actions, vec![Action::VolumeUp]);
    }

    // ── GPIO 读取与静音变化检测 ──

    #[test]
    fn gpio_read_failure_is_not_muted() {
        // 读取失败 → false（不静音），与原版 getInt 失败返回 0 一致
        assert!(!read_gpio("/nonexistent/gpio/value"));
    }

    #[test]
    fn mute_switch_change_is_detected() {
        // 对应 spec 场景「静音开关变化被检测」：GPIO 值 0 → 1
        let mut was_muted = false;
        assert!(
            should_set_mute(&mut was_muted, true),
            "值变化应触发 set_mute"
        );
        assert!(was_muted, "was_muted 应更新");
    }

    #[test]
    fn mute_no_change_no_repeat_set() {
        // 对应 spec 场景「值未变化不重复设置」：连续相同值不重复 set_mute
        let mut was_muted = false;
        assert!(
            !should_set_mute(&mut was_muted, false),
            "值未变化不应触发 set_mute"
        );
        assert!(!was_muted, "was_muted 不应变化");
    }
}
