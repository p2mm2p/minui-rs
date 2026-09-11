//! 电源与硬件操作（sysfs 读写 + 睡眠/关机流程）
//!
//! 本模块是平台 crate 的电源/硬件层——对应原版 C `platform.c` 的
//! `PLAT_getBatteryStatus`/`PLAT_enableBacklight`/`PLAT_setCPUSpeed`/
//! `PLAT_powerOff`/`PLAT_setRumble` 与 `api.c` 的 `PWR_enterSleep`/
//! `PWR_exitSleep`。
//!
//! 全部硬件操作是 sysfs 节点读写（电量/背光/CPU/振动）+ shell 命令
//! （killall/date/unlink）。**纯函数层**（`read_int`/`write_int`/
//! `battery_charge_level`/`rumble_value`/`is_up`）路径参数化——可单测；
//! `Platform` trait 实现（`lib.rs`）调用本模块的高级函数。
//!

use common::power::{BatteryStatus, CpuSpeed};

// ── sysfs 路径常量（对照原版 platform.c）──────────────────────

/// 充电状态（axp2202 USB 在线）。对应原版 `axp2202-usb/online`（platform.c:477）。
const CHARGE_PATH: &str = "/sys/class/power_supply/axp2202-usb/online";
/// 电量百分比。对应原版 `axp2202-battery/capacity`（platform.c:480）。
const CAPACITY_PATH: &str = "/sys/class/power_supply/axp2202-battery/capacity";
/// CPU 频率控制。对应原版 `GOVERNOR_PATH`（platform.c:545）。
const GOVERNOR_PATH: &str = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_setspeed";
/// 振动电机。对应原版 `RUMBLE_PATH`（platform.c:557）。
const RUMBLE_PATH: &str = "/sys/class/gpio/gpio227/value";
/// 网络状态。对应原版 `wlan0/operstate`（platform.c:488）。
const WIFI_OPERSTATE_PATH: &str = "/sys/class/net/wlan0/operstate";

/// LED 亮度控制（smart）。对应原版 `LED_PATH1`（platform.c:508）。
const LED_PATH1: &str = "/sys/class/led_anim/max_scale";
/// LED 亮度控制（brick 侧面）。对应原版 `LED_PATH2`（platform.c:509）。
#[cfg(feature = "brick")]
const LED_PATH2: &str = "/sys/class/led_anim/max_scale_lr";
/// LED 亮度控制（brick 正面）。对应原版 `LED_PATH3`（platform.c:510）。
#[cfg(feature = "brick")]
const LED_PATH3: &str = "/sys/class/led_anim/max_scale_f1f2";

/// 睡眠/关机时的硬件静音 raw 值。对应原版 `MUTE_VOLUME_RAW 0`。
pub(crate) const MUTE_VOLUME_RAW: i32 = 0;

// ── sysfs 纯函数 ─────────────────────────────────────────────

/// 读取 sysfs 整数节点
///
/// 失败（节点缺失/权限不足/解析失败——开发机常见）返回 `default`。
/// 对应原版 `getInt`（utils.c，失败返回 0）——Rust 版默认值显式传入，
/// 避免隐式 0 的歧义。
pub(crate) fn read_int(path: &str, default: i32) -> i32 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok())
        .unwrap_or(default)
}

/// 写入 sysfs 节点
///
/// 失败静默（无权限/节点缺失——开发机）。对应原版 `putInt`。
pub(crate) fn write_int(path: &str, value: i32) {
    let _ = std::fs::write(path, value.to_string());
}

/// 电量档位映射
///
/// 对应原版 platform.c:481-486 的 if 链——注释 "worry less about battery
/// and more about the game you're playing"：粗粒度档位（10/20/40/60/80/100）
/// 减少 UI 上的电量波动。
pub(crate) fn battery_charge_level(raw: i32) -> u8 {
    if raw > 80 {
        100
    } else if raw > 60 {
        80
    } else if raw > 40 {
        60
    } else if raw > 20 {
        40
    } else if raw > 10 {
        20
    } else {
        10
    }
}

// ── 高级函数（供 lib.rs 的 Platform 实现调用）────────────────

/// 读取电池状态（充电 + 电量档位）
///
/// 对应原版 `PLAT_getBatteryStatus`（platform.c:475-487）。
/// 开发机无 axp2202 节点——读失败返回默认值（不充电、电量 10）。
pub(crate) fn get_battery_status() -> BatteryStatus {
    BatteryStatus {
        charging: read_int(CHARGE_PATH, 0) != 0,
        percentage: battery_charge_level(read_int(CAPACITY_PATH, 50)),
    }
}

/// CPU 频率映射（Hz）
///
/// 对应原版 `PLAT_setCPUSpeed` 的 switch（platform.c:548-553，
/// smart/brick 同表）：Menu=600MHz、Powersave=1.2GHz、Normal=1.608GHz、
/// Performance=2.0GHz。纯函数（可测）——写路径由调用方执行。
pub(crate) fn cpu_frequency(speed: CpuSpeed) -> i32 {
    match speed {
        CpuSpeed::Menu => 600_000,
        CpuSpeed::Powersave => 1_200_000,
        CpuSpeed::Normal => 1_608_000,
        CpuSpeed::Performance => 2_000_000,
    }
}

/// 设置 CPU 速度（写 cpufreq 频率）
///
/// 对应原版 `PLAT_setCPUSpeed`（platform.c:546-555）。
pub(crate) fn set_cpu_speed(speed: CpuSpeed) {
    write_int(GOVERNOR_PATH, cpu_frequency(speed));
}

/// 控制背光（settings 亮度 + LED）
///
/// 对应原版 `PLAT_enableBacklight`（platform.c:514-525）：
/// - 关闭：`SetRawBrightness(0)`（硬件关）+ LED 开（常亮指示睡眠）
/// - 开启：`SetBrightness(GetBrightness())` 恢复（brick 先写 raw 8）
///
/// 原版注释掉的 fb0 blank 路径不使用（亮度控制已覆盖关屏语义）。
/// settings 经 `SettingsHandle::init()` 访问（client 路径——共享内存
/// 已存在时快速连接；与 keymon 的交互见 README「系统设置与 keymon」）。
pub(crate) fn enable_backlight(enable: bool) {
    let settings = crate::settings::SettingsHandle::init();
    if enable {
        #[cfg(feature = "brick")]
        crate::settings::set_raw_brightness(8);
        settings.set_brightness(settings.brightness());
    } else {
        crate::settings::set_raw_brightness(0);
    }
    set_led(!enable);
}

/// LED 控制（睡眠时亮灯指示）
///
/// 对应原版 `PLAT_enableLED`（platform.c:508-512）——
/// smart 一路（max_scale）、brick 三路（侧面 + 正面）。
fn set_led(enable: bool) {
    let value = if enable { 60 } else { 0 };
    write_int(LED_PATH1, value);
    #[cfg(feature = "brick")]
    {
        write_int(LED_PATH2, value);
        write_int(LED_PATH3, value);
    }
}

/// 振动输出值（静音抑制）
///
/// 对应原版 `PLAT_setRumble`（platform.c:558-560）：
/// `strength && !GetMute()`——静音时不振动。
/// 纯函数（可测）——写路径由调用方执行。
pub(crate) fn rumble_value(strength: u8, muted: bool) -> i32 {
    if strength != 0 && !muted { 1 } else { 0 }
}

/// 写入振动电机（gpio227）——静音时抑制（settings mute 状态）
pub(crate) fn set_rumble(strength: u8) {
    let settings = crate::settings::SettingsHandle::init();
    write_int(RUMBLE_PATH, rumble_value(strength, settings.mute()));
}

/// 网络状态判断（operstate 前缀匹配 "up"）
///
/// 对应原版 `prefixMatch("up", status)`（platform.c:489）。
/// 纯函数（可测）。
pub(crate) fn is_up(state: &str) -> bool {
    state.starts_with("up")
}

/// 是否在线（wifi 已连接）
///
/// 对应原版 getBatteryStatus 顺带的 wifi 检查（platform.c:488-489）——
/// Rust 版独立实现（每次读 sysfs，简单直接）。
pub(crate) fn is_online() -> bool {
    std::fs::read_to_string(WIFI_OPERSTATE_PATH)
        .map(|s| is_up(s.trim()))
        .unwrap_or(false)
}

/// 设置系统时间（date + hwclock）
///
/// 对应原版 `PLAT_setDateTime`（api.c:1717-1722 的 system 命令拼接）——
/// `std::process::Command` 免 shell 转义（改进：无注入风险）。
pub(crate) fn set_date_time(y: i32, m: i32, d: i32, h: i32, min: i32, s: i32) {
    let date_str = format!("{y:04}-{m:02}-{d:02} {h:02}:{min:02}:{s:02}");
    let _ = std::process::Command::new("date")
        .args(["-s", &date_str])
        .status();
    let _ = std::process::Command::new("hwclock")
        .args(["--utc", "-w"])
        .status();
}

// ── 测试 ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── sysfs 纯函数（临时文件——路径参数化可测）──

    #[test]
    fn read_int_success() {
        let dir = std::env::temp_dir().join("minui_power_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("read_test");
        std::fs::write(&path, "42\n").unwrap();
        assert_eq!(read_int(path.to_str().unwrap(), 0), 42);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_int_failure_returns_default() {
        assert_eq!(read_int("/nonexistent/power/node", 7), 7);
    }

    #[test]
    fn write_int_writes_value() {
        let dir = std::env::temp_dir().join("minui_power_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("write_test");
        write_int(path.to_str().unwrap(), 1234);
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "1234");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_int_failure_silent() {
        write_int("/nonexistent/power/node", 1); // 不 panic
    }

    // ── CPU 频率映射（对应原版 platform.c:548-553）──

    #[test]
    fn cpu_frequency_mapping() {
        assert_eq!(cpu_frequency(CpuSpeed::Menu), 600_000);
        assert_eq!(cpu_frequency(CpuSpeed::Powersave), 1_200_000);
        assert_eq!(cpu_frequency(CpuSpeed::Normal), 1_608_000);
        assert_eq!(cpu_frequency(CpuSpeed::Performance), 2_000_000);
    }

    // ── 电量档位映射（对应原版 if 链）──

    #[test]
    fn battery_charge_level_mapping() {
        assert_eq!(battery_charge_level(85), 100);
        assert_eq!(battery_charge_level(81), 100, ">80 边界");
        assert_eq!(battery_charge_level(80), 80, "=80 落入 60-80 档");
        assert_eq!(battery_charge_level(55), 60);
        assert_eq!(battery_charge_level(25), 40);
        assert_eq!(battery_charge_level(5), 10);
        assert_eq!(battery_charge_level(0), 10);
    }

    // ── 振动静音抑制 ──

    #[test]
    fn rumble_value_mutes_when_silenced() {
        assert_eq!(rumble_value(1, false), 1);
        assert_eq!(rumble_value(1, true), 0, "静音时不振动");
        assert_eq!(rumble_value(0, false), 0);
    }

    // ── 网络状态判断 ──

    #[test]
    fn is_up_prefix_match() {
        assert!(is_up("up"));
        assert!(is_up("up\n"), "operstate 读出的值可能带换行");
        assert!(!is_up("down"));
        assert!(!is_up("unknown"));
    }
}
