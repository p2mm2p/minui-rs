//! # Save — SRAM / RTC / State 管理
//!
//! 对应原 C 代码中的 `SRAM_*`, `RTC_*`, `State_*` 函数。

use std::path::PathBuf;

/// 存档槽位常量
pub const STATE_SLOT_AUTO: u32 = 9;   // 自动恢复槽位
pub const STATE_SLOT_DEFAULT: u32 = 8; // 默认槽位
pub const STATE_SLOT_COUNT: u32 = 8;   // 手动槽位数量

/// 存档管理器
pub struct SaveManager {
    /// 存档目录
    pub states_dir: PathBuf,
    /// 存档目录
    pub saves_dir: PathBuf,
    /// ROM 文件名（不含扩展名和路径）
    pub rom_file: String,
    /// 当前槽位
    pub slot: u32,
}

impl SaveManager {
    pub fn new(states_dir: PathBuf, saves_dir: PathBuf, rom_file: &str) -> Self {
        Self {
            states_dir,
            saves_dir,
            rom_file: rom_file.to_string(),
            slot: 0,
        }
    }

    /// 读取 SRAM
    pub fn read_sram(&self, _data: &mut [u8]) -> Result<(), String> {
        // TODO
        Ok(())
    }

    /// 写入 SRAM
    pub fn write_sram(&self, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }

    /// 读取 RTC
    pub fn read_rtc(&self, _data: &mut [u8]) -> Result<(), String> {
        Ok(())
    }

    /// 写入 RTC
    pub fn write_rtc(&self, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }

    /// 读取即时存档
    pub fn read_state(&self, slot: u32) -> Result<Vec<u8>, String> {
        let _ = slot;
        Err("not implemented".into())
    }

    /// 写入即时存档
    pub fn write_state(&self, slot: u32, _data: &[u8]) -> Result<(), String> {
        let _ = slot;
        Ok(())
    }

    /// 自动存档（休眠/退出前）
    pub fn auto_save(&self, _data: &[u8]) -> Result<(), String> {
        self.write_state(STATE_SLOT_AUTO, _data)
    }

    /// 自动恢复（启动时）
    pub fn auto_resume(&mut self) -> Result<Vec<u8>, String> {
        self.read_state(STATE_SLOT_AUTO)
    }
}
