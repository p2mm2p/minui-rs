//! # Menu — 游戏内菜单
//!
//! 对应原 C 代码中的 `Menu_loop()`, `Menu_options()` 等。
//!
//! ## 菜单结构
//!
//! ```text
//! ┌──────────────────────────┐
//! │  Continue                │
//! │  Save                    │
//! │  Load                    │
//! │  Options  ──→  Frontend  │
//! │  Quit         Emulator   │
//! │               Controls   │
//! │               Shortcuts  │
//! │               Save Changes│
//! └──────────────────────────┘
//! ```

/// 菜单项
pub struct MenuItem {
    pub name: String,
    pub desc: Option<String>,
    pub values: Option<Vec<String>>,
    pub value: usize,
    pub key: Option<String>,
    pub id: Option<usize>,
}

/// 菜单列表类型
pub enum MenuType {
    List,
    Var,
    Fixed,
    Input,
}

/// 菜单列表
pub struct MenuList {
    pub list_type: MenuType,
    pub desc: Option<String>,
    pub items: Vec<MenuItem>,
    pub max_width: Option<u32>,
}

/// 主菜单选中项
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainMenuSelection {
    Continue,
    Save,
    Load,
    Options,
    Quit,
}

/// 游戏内菜单状态
pub struct GameMenu {
    /// 当前选中项
    pub selected: MainMenuSelection,
    /// 当前存档槽位 (0-7)
    pub slot: usize,
    /// 存档预览是否存在
    pub preview_exists: bool,
    /// 存档是否存在
    pub save_exists: bool,
    /// 多碟游戏的碟片列表
    pub disc_paths: Vec<String>,
    /// 当前碟片索引
    pub disc: usize,
    /// 总碟片数
    pub total_discs: usize,
}

impl GameMenu {
    pub fn new() -> Self {
        Self {
            selected: MainMenuSelection::Continue,
            slot: 0,
            preview_exists: false,
            save_exists: false,
            disc_paths: Vec::new(),
            disc: 0,
            total_discs: 0,
        }
    }

    /// 运行菜单主循环
    ///
    /// 返回时游戏继续或退出。
    pub fn run(&mut self) -> bool {
        // TODO: 实现完整的菜单循环
        // - 上下导航
        // - A 确认 / B 返回
        // - Save/Load 时左右切换槽位
        // - Continue 时左右切换碟片
        // - Options 进入子菜单
        true // 返回 true = 继续游戏
    }
}
