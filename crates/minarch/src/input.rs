//! # Input — 输入映射 + 快捷键
//!
//! 对应原 C 代码中的 `input_poll_callback()`, `input_state_callback()`,
//! `Input_init()` 以及 shortcuts 系统。

use common::types::{Button, PadContext};

/// 按钮映射
pub struct ButtonMapping {
    /// 显示名称
    pub name: String,
    /// libretro 按钮 ID
    pub retro_id: i32,
    /// 本地按钮 ID
    pub local_id: i32,
    /// 是否需要 MENU 修饰键
    pub requires_menu: bool,
}

/// 快捷键类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    SaveState,
    LoadState,
    ResetGame,
    SaveQuit,
    CycleScale,
    CycleEffect,
    ToggleFF,
    HoldFF,
}

/// 快捷键映射
pub struct ShortcutMapping {
    pub action: Shortcut,
    pub local_id: i32,
    pub requires_menu: bool,
}

/// 输入管理器
pub struct InputMapper {
    /// 普通按键映射
    pub controls: Vec<ButtonMapping>,
    /// 快捷键映射
    pub shortcuts: Vec<ShortcutMapping>,
    /// 当前 retropad 状态
    pub retropad_buttons: u32,
    /// 是否忽略 MENU 键（当 MENU+音量键被使用时）
    pub ignore_menu: bool,
    /// 快进状态
    pub fast_forward: bool,
    /// 切换快进标记
    pub ff_toggled: bool,
}

impl InputMapper {
    pub fn new() -> Self {
        Self {
            controls: Vec::new(),
            shortcuts: Vec::new(),
            retropad_buttons: 0,
            ignore_menu: false,
            fast_forward: false,
            ff_toggled: false,
        }
    }

    /// 初始化按键映射（从 default.cfg 解析）
    pub fn init_from_config(&mut self, _config: &crate::config::Config) {
        // TODO: 解析 bind 指令
    }

    /// 每帧轮询输入
    ///
    /// 返回 Some(shortcut) 如果触发了快捷键，否则更新 retropad_buttons。
    pub fn poll(&mut self, _pad: &PadContext) -> Option<Shortcut> {
        // TODO: 实现完整的输入映射逻辑
        None
    }

    /// 获取当前 retropad 状态（供 libretro 核心调用）
    pub fn input_state(&self, _port: u32, _device: u32, _index: u32, _id: u32) -> i16 {
        0
    }
}
