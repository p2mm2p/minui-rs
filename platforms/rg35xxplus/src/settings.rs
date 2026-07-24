//! # 文件通信设置（替代 libmsettings.so）
//!
//! 通过 `/tmp/settings/` 目录下的文件实现进程间状态共享。
//! keymon 是 writer（写亮度/音量/HDMI/耳机），minui/minarch 是 reader。
//!
//! 和原版共享内存方案的区别：
//! - 不需要 shm_open / mmap（纯文件 I/O，无 unsafe）
//! - 崩溃安全（文件由 OS 管理，不会泄露）
//! - 可调试（`cat /tmp/settings/brightness`）

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const SETTINGS_DIR: &str = "/tmp/settings";

#[derive(Clone)]
pub struct Settings {
    dir: PathBuf,
}

impl Settings {
    pub fn new() -> Self {
        let dir = PathBuf::from(SETTINGS_DIR);
        let _ = fs::create_dir_all(&dir);
        Self { dir }
    }

    fn read_u8(&self, name: &str, default: u8) -> u8 {
        fs::read_to_string(self.dir.join(name))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(default)
    }

    fn read_i32(&self, name: &str, default: i32) -> i32 {
        fs::read_to_string(self.dir.join(name))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(default)
    }

    fn write_u8(&self, name: &str, val: u8) {
        let _ = fs::write(self.dir.join(name), val.to_string());
    }

    // ── 亮度 ──

    pub fn brightness(&self) -> u8 {
        self.read_u8("brightness", 5) // 默认值 5/10
    }

    pub fn set_brightness(&self, val: u8) {
        self.write_u8("brightness", val.min(10));
    }

    /// 将 0-10 逻辑值转换为 0-255 硬件值
    pub fn brightness_raw(&self) -> u8 {
        match self.brightness() {
             0 => 4,   1 => 6,   2 => 10,  3 => 16,
             4 => 32,  5 => 48,  6 => 64,  7 => 96,
             8 => 128, 9 => 192, 10 => 255,
             _ => 255,
        }
    }

    // ── 音量 ──

    pub fn volume(&self) -> u8 {
        self.read_u8("volume", 10) // 默认 10/20
    }

    pub fn set_volume(&self, val: u8) {
        self.write_u8("volume", val.min(20));
    }

    /// 将 0-20 映射到 amixer 百分比
    pub fn volume_percent(&self) -> u8 {
        self.volume() * 5
    }

    /// 应用到硬件
    pub fn apply_volume(&self) {
        let jack = self.jack();
        let raw = if jack {
            self.read_u8("headphones_vol", 4)
        } else {
            self.read_u8("speaker_vol", 8)
        };
        if self.hdmi() { return; }

        let vol = raw * 5;
        let _ = Command::new("amixer")
            .args(["sset", "lineout volume", &format!("{}%", vol)])
            .output();
    }

    // ── 耳机 ──

    pub fn jack(&self) -> bool {
        self.read_u8("jack", 0) != 0
    }

    pub fn set_jack(&self, plugged: bool) {
        self.write_u8("jack", if plugged { 1 } else { 0 });
    }

    // ── HDMI ──

    pub fn hdmi(&self) -> bool {
        self.read_u8("hdmi", 0) != 0
    }

    pub fn set_hdmi(&self, active: bool) {
        self.write_u8("hdmi", if active { 1 } else { 0 });
    }

    // ── 静音 ──

    pub fn mute(&self) -> bool {
        self.read_u8("mute", 0) != 0
    }

    pub fn set_mute(&self, muted: bool) {
        self.write_u8("mute", if muted { 1 } else { 0 });
    }

    // ── 摇杆轴（临时状态，不持久化）──

    pub fn read_axis(&self, name: &str) -> Option<i32> {
        let path = self.dir.join(format!("axis_{}", name));
        fs::read_to_string(&path).ok()
            .and_then(|s| s.trim().parse().ok())
    }
}
