//! # Config — 三级配置系统
//!
//! 对应原 C 代码中的 `Config_*` 和 `OptionList_*` 函数。
//!
//! ## 三级优先级
//!
//! ```text
//! system.cfg     (最高优先级，系统强制限制，可锁定选项)
//!     ↓ 覆盖
//! default.cfg    (PAK 默认值，开发者设的最佳值)
//!     ↓ 覆盖
//! 用户配置        (minarch.cfg 全局 或 <游戏名>.cfg 单独)
//! ```
//!
//! `-` 前缀表示锁定：`-minarch_screen_scaling = Native`

use std::collections::HashMap;

/// 选项定义
pub struct OptionDef {
    /// 配置键名
    pub key: String,
    /// 显示名称
    pub name: String,
    /// 简短描述
    pub desc: Option<String>,
    /// 完整描述
    pub full: Option<String>,
    /// 当前值索引
    pub value: usize,
    /// 默认值索引
    pub default_value: usize,
    /// 选项总数
    pub count: usize,
    /// 可选值列表
    pub values: Vec<String>,
    /// 显示标签列表
    pub labels: Vec<String>,
    /// 是否被锁定
    pub locked: bool,
}

/// 选项列表
pub struct OptionList {
    pub count: usize,
    pub changed: bool,
    pub options: Vec<OptionDef>,
    /// 启用的选项数量（未被锁定的）
    pub enabled_count: usize,
}

/// 前端选项索引（对应原 C 的 FE_OPT_* 枚举）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontendOption {
    Scaling = 0,
    Effect,
    Sharpness,
    Tearing,
    Overclock,
    ThreadVideo,
    DebugHud,
    MaxFFSpeed,
}

/// 三级配置
pub struct Config {
    /// 系统强制配置（最高优先级，含锁定标记）
    pub system: Option<HashMap<String, String>>,
    /// PAK 默认配置
    pub defaults: Option<HashMap<String, String>>,
    /// 用户全局配置 (minarch.cfg)
    pub user_global: Option<HashMap<String, String>>,
    /// 用户游戏单独配置 (<游戏名>.cfg)
    pub user_game: Option<HashMap<String, String>>,
    /// 被锁定的选项集合
    pub locks: std::collections::HashSet<String>,
    /// 当前加载的配置类型
    pub loaded: ConfigLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigLevel {
    None,
    Console,
    Game,
}

impl Config {
    pub fn new() -> Self {
        Self {
            system: None,
            defaults: None,
            user_global: None,
            user_game: None,
            locks: std::collections::HashSet::new(),
            loaded: ConfigLevel::None,
        }
    }

    /// 按优先级查询配置值
    pub fn get(&self, key: &str) -> Option<&str> {
        // system > defaults > user_global > user_game
        if let Some(ref m) = self.system { if let Some(v) = m.get(key) { return Some(v); } }
        if let Some(ref m) = self.defaults { if let Some(v) = m.get(key) { return Some(v); } }
        if let Some(ref m) = self.user_global { if let Some(v) = m.get(key) { return Some(v); } }
        if let Some(ref m) = self.user_game { if let Some(v) = m.get(key) { return Some(v); } }
        None
    }

    /// 选项是否被锁定
    pub fn is_locked(&self, key: &str) -> bool {
        self.locks.contains(key)
    }

    /// 加载配置（从 SD 卡路径）
    pub fn load(&mut self, _core_tag: &str, _core_name: &str, _game_name: &str) -> Result<(), String> {
        // TODO: 读取 system.cfg, default.cfg, minarch.cfg, <game>.cfg
        Ok(())
    }
}
