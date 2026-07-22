//! # 电源管理
//!
//! 对应原 C 代码 `api.c` 中的 `PWR_*` 函数。
//!
//! 管理设备的休眠、自动关机和电池状态。
//!
//! ## 状态机
//!
//! ```text
//! 活跃 ──(30s无操作)──→ 休眠 ──(2分钟)──→ 自动关机
//!  ↑                      │
//!  └───(按电源键唤醒)──────┘
//! ```
//!
//! ## 亮度/音量调节
//!
//! 按住 MENU + PLUS/MINUS 调节亮度；按住无修饰键的 PLUS/MINUS 调节音量。
//! 调节时在屏幕右上角显示当前值。

use crate::types::*;

// ============================================================================
// 常量
// ============================================================================

/// 自动休眠超时（毫秒）—— 30 秒
pub const AUTOSLEEP_TIMEOUT_MS: u32 = 30_000;

/// 自动关机超时（毫秒）—— 休眠后 2 分钟
pub const AUTOPOWEROFF_TIMEOUT_MS: u32 = 120_000;

/// 亮度范围
pub const BRIGHTNESS_MIN: u8 = 0;
pub const BRIGHTNESS_MAX: u8 = 10;

/// 音量范围
pub const VOLUME_MIN: u8 = 0;
pub const VOLUME_MAX: u8 = 20;

/// 低电量阈值（百分比）
pub const LOW_CHARGE_THRESHOLD: u8 = 10;

/// 设置提示显示时间（毫秒）—— 调整亮度/音量后显示 0.5 秒
/// 对应 C 中的 SETTING_DELAY 500
pub const SETTING_DISPLAY_MS: u32 = 500;

// ============================================================================
// PowerManager
// ============================================================================

/// 电源管理器
///
/// 每帧调用 `update()` 推进计时器并处理状态转换。
pub struct PowerManager {
    /// 是否已初始化
    pub initialized: bool,
    /// 当前是否在休眠状态
    pub is_asleep: bool,
    /// 休眠是否被禁用
    pub sleep_disabled: bool,
    /// 自动休眠是否被禁用
    pub autosleep_disabled: bool,
    /// 距上次用户输入的时间（毫秒）
    pub idle_time_ms: u32,
    /// 休眠持续时间（毫秒）
    pub sleep_time_ms: u32,
    /// 自动休眠超时
    pub autosleep_timeout_ms: u32,
    /// 自动关机超时
    pub autopoweroff_timeout_ms: u32,
    /// 电池电量（0/10/20/40/60/80/100）
    pub battery_charge: u8,
    /// 是否正在充电
    pub battery_charging: bool,
    /// 当前亮度（0-10）
    pub brightness: u8,
    /// 当前音量（0-20）
    pub volume: u8,
    /// 当前显示的系统设置类型（0=无, 1=亮度调整中, 2=音量调整中）
    pub show_setting: u8,
    /// 设置提示的剩余显示时间（毫秒）
    pub setting_display_timer: u32,
    /// 用户是否已收到低电量警告
    pub low_charge_warned: bool,
    /// 是否有关机请求
    pub poweroff_requested: bool,
    /// 关机是否被禁用
    pub poweroff_disabled: bool,
    /// CPU 速度等级
    pub cpu_speed: CpuSpeed,
}

impl PowerManager {
    /// 创建电源管理器，所有字段初始化为默认值
    pub fn new() -> Self {
        Self {
            initialized: false,
            is_asleep: false,
            sleep_disabled: false,
            autosleep_disabled: false,
            idle_time_ms: 0,
            sleep_time_ms: 0,
            autosleep_timeout_ms: AUTOSLEEP_TIMEOUT_MS,
            autopoweroff_timeout_ms: AUTOPOWEROFF_TIMEOUT_MS,
            battery_charge: 80,
            battery_charging: false,
            brightness: BRIGHTNESS_MAX / 2,
            volume: VOLUME_MAX / 2,
            show_setting: 0,
            setting_display_timer: 0,
            low_charge_warned: false,
            poweroff_requested: false,
            poweroff_disabled: false,
            cpu_speed: CpuSpeed::Normal,
        }
    }

    // ================================================================
    // 状态查询
    // ================================================================

    /// 电池是否低电量
    pub fn is_low_charge(&self) -> bool {
        self.battery_charge <= LOW_CHARGE_THRESHOLD
    }

    /// 是否正在关机
    pub fn is_powering_off(&self) -> bool {
        self.poweroff_requested
    }

    /// 是否可以休眠
    pub fn can_sleep(&self) -> bool {
        !self.sleep_disabled && !self.autosleep_disabled
    }

    // ================================================================
    // 每帧更新
    // ================================================================

    /// 每帧调用（在主循环中），推进所有计时器
    ///
    /// `dt_ms` 是距上一帧的时间（毫秒），通常为 ~16ms（60fps）。
    ///
    /// 返回 `true` 表示界面需要重绘（dirty）。
    pub fn update(&mut self, dt_ms: u32) -> bool {
        let mut dirty = false;

        // 设置提示计时器
        if self.setting_display_timer > 0 {
            if self.setting_display_timer > dt_ms {
                self.setting_display_timer -= dt_ms;
            } else {
                self.setting_display_timer = 0;
                self.show_setting = 0;
                dirty = true;
            }
        }

        // 休眠状态下的自动关机计时器
        if self.is_asleep && !self.poweroff_disabled {
            self.sleep_time_ms += dt_ms;
            if self.sleep_time_ms >= self.autopoweroff_timeout_ms {
                self.poweroff_requested = true;
            }
        }

        dirty
    }

    /// 通知有用户活动（按键/触摸），重置休眠计时器
    pub fn notify_activity(&mut self) {
        self.idle_time_ms = 0;
        if self.is_asleep {
            self.wake();
        }
    }

    /// 检查是否应进入自动休眠（调用方在每帧 idle 时调用）
    pub fn check_autosleep(&mut self, dt_ms: u32) -> bool {
        if self.is_asleep || self.autosleep_disabled || self.sleep_disabled {
            return false;
        }
        self.idle_time_ms += dt_ms;
        if self.idle_time_ms >= self.autosleep_timeout_ms {
            self.enter_sleep();
            return true;
        }
        false
    }

    // ================================================================
    // 休眠/唤醒
    // ================================================================

    /// 进入休眠
    pub fn enter_sleep(&mut self) {
        self.is_asleep = true;
        self.sleep_time_ms = 0;
        self.cpu_speed = CpuSpeed::Powersave;
    }

    /// 从休眠唤醒
    pub fn wake(&mut self) {
        self.is_asleep = false;
        self.sleep_time_ms = 0;
        self.idle_time_ms = 0;
        self.cpu_speed = CpuSpeed::Normal;
    }

    // ================================================================
    // 亮度/音量调节
    // ================================================================

    /// 调节亮度（正值增加，负值减少）
    pub fn adjust_brightness(&mut self, delta: i8) {
        let new = (self.brightness as i8 + delta)
            .clamp(BRIGHTNESS_MIN as i8, BRIGHTNESS_MAX as i8) as u8;
        if new != self.brightness {
            self.brightness = new;
            self.show_setting = 1;
            self.setting_display_timer = SETTING_DISPLAY_MS;
        }
    }

    /// 调节音量（正值增加，负值减少）
    pub fn adjust_volume(&mut self, delta: i8) {
        let new = (self.volume as i8 + delta)
            .clamp(VOLUME_MIN as i8, VOLUME_MAX as i8) as u8;
        if new != self.volume {
            self.volume = new;
            self.show_setting = 2;
            self.setting_display_timer = SETTING_DISPLAY_MS;
        }
    }

    /// 处理亮度/音量调节的输入
    ///
    /// 返回 `true` 表示有调节操作发生。
    pub fn handle_setting_input(
        &mut self,
        pad: &PadContext,
        mod_brightness: Button,
        mod_volume: Button,
        mod_plus: Button,
        mod_minus: Button,
    ) -> bool {
        // MENU + PLUS/MINUS = 亮度调节
        if pad.just_repeated.contains(mod_plus) && pad.is_pressed.contains(mod_brightness) {
            self.adjust_brightness(1);
            return true;
        }
        if pad.just_repeated.contains(mod_minus) && pad.is_pressed.contains(mod_brightness) {
            self.adjust_brightness(-1);
            return true;
        }

        // 无修饰键的 PLUS/MINUS = 音量调节
        if pad.just_repeated.contains(mod_plus)
            && !pad.is_pressed.contains(mod_brightness)
            && !pad.is_pressed.contains(mod_volume)
        {
            self.adjust_volume(1);
            return true;
        }
        if pad.just_repeated.contains(mod_minus)
            && !pad.is_pressed.contains(mod_brightness)
            && !pad.is_pressed.contains(mod_volume)
        {
            self.adjust_volume(-1);
            return true;
        }

        false
    }

    // ================================================================
    // 电池
    // ================================================================

    /// 更新电池状态（从硬件读取）
    pub fn update_battery(&mut self, charge: u8, charging: bool) {
        self.battery_charge = charge;
        self.battery_charging = charging;
    }

    // ================================================================
    // 控制
    // ================================================================

    /// 禁用自动休眠
    pub fn disable_autosleep(&mut self) { self.autosleep_disabled = true; }
    /// 启用自动休眠
    pub fn enable_autosleep(&mut self) { self.autosleep_disabled = false; }
    /// 禁用休眠（包括手动按钮）
    pub fn disable_sleep(&mut self) { self.sleep_disabled = true; }
    /// 启用休眠（包括手动按钮）
    pub fn enable_sleep(&mut self) { self.sleep_disabled = false; }
    /// 禁用手动关机
    pub fn disable_poweroff(&mut self) { self.poweroff_disabled = true; }

    /// 检查是否应阻止自动休眠 —— 对应 C 中的 `PWR_preventAutosleep()`
    ///
    /// 以下情况阻止自动休眠：
    /// - 设备正在充电
    /// - 自动休眠被禁用
    /// - 正在通过 HDMI 输出
    pub fn prevent_autosleep(&self, has_hdmi: bool) -> bool {
        self.battery_charging || self.autosleep_disabled || has_hdmi
    }

    /// 请求关机
    pub fn request_poweroff(&mut self) { self.poweroff_requested = true; }
}

impl Default for PowerManager {
    fn default() -> Self { Self::new() }
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_power_manager_new() {
        let pm = PowerManager::new();
        assert!(!pm.is_asleep);
        assert!(!pm.sleep_disabled);
        assert!(!pm.poweroff_requested);
        assert_eq!(pm.battery_charge, 80);
    }

    #[test]
    fn test_autosleep_timer() {
        let mut pm = PowerManager::new();
        pm.autosleep_timeout_ms = 100; // 加速用于测试

        // 60ms 不应触发
        assert!(!pm.check_autosleep(60));

        // 再过 50ms（累计 110ms）应触发
        assert!(pm.check_autosleep(50));
        assert!(pm.is_asleep);
    }

    #[test]
    fn test_autosleep_disabled() {
        let mut pm = PowerManager::new();
        pm.autosleep_timeout_ms = 100;
        pm.disable_autosleep();
        assert!(!pm.check_autosleep(200));
        assert!(!pm.is_asleep);
    }

    #[test]
    fn test_autopoweroff() {
        let mut pm = PowerManager::new();
        pm.autosleep_timeout_ms = 100;
        pm.autopoweroff_timeout_ms = 200;

        // 进入休眠
        assert!(pm.check_autosleep(110));
        assert!(pm.is_asleep);

        // 160ms 后仍不应关机
        pm.update(160);
        assert!(!pm.poweroff_requested);

        // 再过 50ms（累计 210ms）应关机
        pm.update(50);
        assert!(pm.poweroff_requested);
    }

    #[test]
    fn test_wake_resets_timers() {
        let mut pm = PowerManager::new();
        pm.autosleep_timeout_ms = 100;
        pm.check_autosleep(110);
        assert!(pm.is_asleep);

        pm.notify_activity();
        assert!(!pm.is_asleep);
        assert_eq!(pm.sleep_time_ms, 0);
    }

    #[test]
    fn test_adjust_brightness() {
        let mut pm = PowerManager::new();
        pm.brightness = 5;
        pm.adjust_brightness(1);
        assert_eq!(pm.brightness, 6);
        assert_eq!(pm.show_setting, 1);
        assert!(pm.setting_display_timer > 0);

        // 不应超过最大值
        pm.brightness = BRIGHTNESS_MAX;
        pm.adjust_brightness(1);
        assert_eq!(pm.brightness, BRIGHTNESS_MAX);
    }

    #[test]
    fn test_adjust_volume() {
        let mut pm = PowerManager::new();
        pm.adjust_volume(-1);
        assert!(pm.volume < VOLUME_MAX);
        assert_eq!(pm.show_setting, 2);

        // 不应低于最小值
        pm.volume = VOLUME_MIN;
        pm.adjust_volume(-1);
        assert_eq!(pm.volume, VOLUME_MIN);
    }

    #[test]
    fn test_setting_display_timer() {
        let mut pm = PowerManager::new();
        pm.adjust_volume(1);
        assert_eq!(pm.show_setting, 2);

        // 模拟时间流逝
        pm.update(SETTING_DISPLAY_MS + 100);
        assert_eq!(pm.show_setting, 0);
    }

    #[test]
    fn test_low_charge_detection() {
        let mut pm = PowerManager::new();
        pm.battery_charge = 80;
        assert!(!pm.is_low_charge());

        pm.battery_charge = 10;
        assert!(pm.is_low_charge());

        pm.battery_charge = 0;
        assert!(pm.is_low_charge());
    }

    #[test]
    fn test_poweroff_disabled() {
        let mut pm = PowerManager::new();
        pm.autosleep_timeout_ms = 100;
        pm.autopoweroff_timeout_ms = 100;
        pm.disable_poweroff();

        pm.check_autosleep(110);
        pm.update(200);
        assert!(!pm.poweroff_requested); // 关机被禁用
    }
}
