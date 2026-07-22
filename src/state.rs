//! # MinUi 全局状态
//!
//! 对应原 C 代码 `minui.c` 中的所有 `static` 全局变量和核心逻辑的入口。
//!
//! 在 C 版本中，这些是分散的静态变量。在 Rust 中，我们将其封装在 `MinUi` 结构体中，
//! 使所有状态显式化、可追踪、可测试。
//!
//! ## 状态分类
//!
//! 1. **导航状态** — `stack`（目录栈）、`recents`（最近游戏）
//! 2. **UI 状态** — `dirty`（需重绘）、`can_resume`（有存档可恢复）
//! 3. **恢复状态** — `restore_*` 系列（返回上级目录时恢复滚动位置）
//! 4. **模式标志** — `simple_mode`、`should_resume`
//! 5. **持久化路径** — `slot_path`（当前游戏的存档路径）

use crate::types::*;
use crate::platform::Platform;

// ============================================================================
// MinUi 结构体
// ============================================================================

/// MinUI 启动器的全部运行时状态
///
/// 对应原 C 代码中所有的 `static` 全局变量。
///
/// ## 使用方式
///
/// ```ignore
/// // 完整使用示例 — 需要平台实现和主循环
/// let mut platform = MyPlatform::new();
/// let mut minui = MinUi::new();
/// // minui.run(&mut platform).unwrap();
/// ```
pub struct MinUi {
    // ---- 导航 ----
    /// 目录栈 — 最后一个元素是当前显示的目录
    pub stack: Vec<Directory>,
    /// 最近游戏列表
    pub recents: Vec<Recent>,

    // ---- 模式与状态 ----
    /// 主循环退出标志
    pub quit: bool,
    /// 当前选中的游戏是否有存档可恢复（控制 X RESUME 按钮显示）
    pub can_resume: bool,
    /// 用户是否按了 X（从存档恢复而非正常启动）
    pub should_resume: bool,
    /// 简化模式（隐藏 Tools 等高级功能）
    pub simple_mode: bool,

    // ---- 存档恢复 ----
    /// 当前选中游戏的存档路径（用于 can_resume 检测）
    pub slot_path: String,

    // ---- 目录导航恢复 ----
    /// 之前进入子目录时的栈深度
    pub restore_depth: Option<usize>,
    /// 需要恢复到的条目在目录中的相对位置
    pub restore_relative: Option<usize>,
    /// 返回上级时恢复的选中项索引
    pub restore_selected: usize,
    /// 返回上级时恢复的可见窗口起始位置
    pub restore_start: usize,
    /// 返回上级时恢复的可见窗口结束位置
    pub restore_end: usize,

    // ---- 渲染 ----
    /// 界面是否需要重绘
    pub dirty: bool,
    /// 当前是否显示版本信息（Menu 键切换）
    pub show_version: bool,

    // ---- 定时 ----
    /// 自动休眠计时器（毫秒）
    pub autosleep_timer: u32,
    /// 自动关机计时器（毫秒，在休眠状态下）
    pub autopoweroff_timer: u32,

    // ---- 亮度/音量调整 ----
    /// 当前显示的系统设置类型：0=无, 1=亮度, 2=音量
    pub show_setting: u8,

    // ---- 持久化 ----
    /// 最大最近游戏条目数（24，必须是 MAIN_ROW_COUNT 的倍数）
    pub max_recents: usize,
}

impl MinUi {
    /// 创建一个新的 MinUi 状态实例
    ///
    /// 对应 C 中所有 `static` 变量的初始状态。
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            recents: Vec::new(),

            quit: false,
            can_resume: false,
            should_resume: false,
            simple_mode: false,

            slot_path: String::new(),

            restore_depth: None,
            restore_relative: None,
            restore_selected: 0,
            restore_start: 0,
            restore_end: 0,

            dirty: true,
            show_version: false,

            autosleep_timer: 0,
            autopoweroff_timer: 0,

            show_setting: 0,

            max_recents: 24,
        }
    }

    // ================================================================
    // 便利访问方法
    // ================================================================

    /// 获取当前目录（栈顶）的不可变引用
    ///
    /// 对应 C 中的全局 `top` 指针 —— 但 Rust 中我们通过栈访问。
    ///
    /// # Panics
    /// 如果栈为空（不应该在正常运行中发生）
    pub fn current_dir(&self) -> &Directory {
        self.stack.last().expect("MinUi: stack should never be empty during navigation")
    }

    /// 获取当前目录的可变引用
    pub fn current_dir_mut(&mut self) -> &mut Directory {
        self.stack.last_mut().expect("MinUi: stack should never be empty during navigation")
    }

    /// 判断当前是否是根目录（栈底）
    pub fn is_at_root(&self) -> bool {
        self.stack.len() <= 1
    }

    /// 栈深度
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// 当前目录的总条目数
    pub fn total_entries(&self) -> usize {
        self.current_dir().total()
    }

    /// 当前选中的条目
    pub fn selected_entry(&self) -> Option<&Entry> {
        let dir = self.current_dir();
        if dir.entries.is_empty() {
            None
        } else {
            Some(&dir.entries[dir.selected])
        }
    }

    // ================================================================
    // 状态修改方法
    // ================================================================

    /// 标记界面需要重绘
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// 请求退出主循环
    pub fn request_quit(&mut self) {
        self.quit = true;
    }

    /// 重置输入相关状态（切换界面时调用）
    pub fn reset_after_navigation(&mut self) {
        self.can_resume = false;
        self.should_resume = false;
    }

    /// 设置当前选中的存档路径
    pub fn set_slot_path(&mut self, path: String) {
        self.slot_path = path;
    }

    /// 记录当前导航位置以用于后续恢复
    pub fn save_restore_state(&mut self) {
        // 先在一个独立作用域中取出值，避免同时借用 self.stack 和 self.restore_*
        let (selected, start, end) = {
            let dir = self.current_dir();
            (dir.selected, dir.start, dir.end)
        };
        self.restore_selected = selected;
        self.restore_start = start;
        self.restore_end = end;
    }

    /// 标记恢复深度
    pub fn mark_restore_depth(&mut self) {
        self.restore_depth = Some(self.depth());
    }

    /// 标记相对恢复位置
    pub fn mark_restore_relative(&mut self) {
        self.restore_relative = Some(self.current_dir().selected);
    }

    // ================================================================
    // 最近游戏管理方法
    // ================================================================

    /// 添加一个最近游戏条目
    ///
    /// 对应 C 中的 `addRecent()`
    ///
    /// - 如果已存在 → 移到列表最前面
    /// - 如果不存在 → 插入到头部
    /// - 超出 `max_recents` → 删除最旧的
    ///
    /// `path` 是完整的 SD 卡路径（会去除 SDCARD_PATH 前缀后存储）
    pub fn add_recent<P: Platform>(&mut self, full_path: &str, alias: Option<&str>) {
        let relative = full_path
            .strip_prefix(P::SDCARD_PATH)
            .unwrap_or(full_path)
            .to_string();

        // 查找是否已存在
        if let Some(pos) = self.recents.iter().position(|r| r.path == relative) {
            if pos > 0 {
                // 移到最前面
                let recent = self.recents.remove(pos);
                self.recents.insert(0, recent);
            }
            // pos == 0: 已是最前，无需操作
            return;
        }

        // 新条目
        let recent = Recent {
            path: relative,
            alias: alias.map(String::from),
            available: true, // 由调用者负责验证
        };

        // 保持列表在限制内
        while self.recents.len() >= self.max_recents {
            self.recents.pop();
        }

        self.recents.insert(0, recent);
    }

    /// 获取最近游戏条目（用于 UI 显示）
    ///
    /// 对应 C 中的 `getRecents()`
    pub fn get_available_recents<P: Platform>(&self) -> Vec<Entry> {
        self.recents
            .iter()
            .filter(|r| r.available)
            .map(|r| {
                let sd_path = format!("{}{}", P::SDCARD_PATH, r.path);
                let entry_type = if sd_path.ends_with(".pak") {
                    EntryType::Pak
                } else {
                    EntryType::Rom
                };
                Entry {
                    path: sd_path,
                    name: r.alias.clone().unwrap_or_else(|| {
                        // 从路径提取显示名
                        std::path::Path::new(&r.path)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default()
                    }),
                    unique: None,
                    entry_type,
                    alpha: 0,
                }
            })
            .collect()
    }
}

// ============================================================================
// Default 实现
// ============================================================================

impl Default for MinUi {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Debug 实现（跳过敏感数据）
// ============================================================================

impl std::fmt::Debug for MinUi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MinUi")
            .field("stack_depth", &self.stack.len())
            .field("recents_count", &self.recents.len())
            .field("quit", &self.quit)
            .field("can_resume", &self.can_resume)
            .field("simple_mode", &self.simple_mode)
            .field("dirty", &self.dirty)
            .field("show_version", &self.show_version)
            .field("show_setting", &self.show_setting)
            .finish()
    }
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_minui() {
        let m = MinUi::new();
        assert!(m.stack.is_empty());
        assert!(m.recents.is_empty());
        assert!(m.dirty);
        assert!(!m.quit);
        assert!(!m.can_resume);
        assert!(!m.simple_mode);
    }

    #[test]
    fn test_stack_operations() {
        let mut m = MinUi::new();

        // 模拟打开根目录
        m.stack.push(Directory {
            path: "/".into(),
            name: "Root".into(),
            entries: vec![
                Entry {
                    path: "/game.gb".into(),
                    name: "Zelda".into(),
                    unique: None,
                    entry_type: EntryType::Rom,
                    alpha: 0,
                },
            ],
            alphas: vec![0],
            selected: 0,
            start: 0,
            end: 1,
        });

        assert!(m.is_at_root()); // stack depth 1 仍是根（只有一个目录时就是根）
        assert_eq!(m.depth(), 1);
        assert_eq!(m.total_entries(), 1);
        assert!(m.selected_entry().is_some());
    }

    #[test]
    fn test_add_recent() {
        use crate::platform::test_platform::TestPlatform;
        let mut m = MinUi::new();
        m.max_recents = 3;

        m.add_recent::<TestPlatform>("/tmp/test_sdcard/Roms/GB/Zelda.gb", Some("Zelda"));
        assert_eq!(m.recents.len(), 1);
        assert_eq!(m.recents[0].path, "/Roms/GB/Zelda.gb");
        assert_eq!(m.recents[0].alias.as_deref(), Some("Zelda"));

        // 添加第二个
        m.add_recent::<TestPlatform>("/tmp/test_sdcard/Roms/GB/Mario.gb", None);
        assert_eq!(m.recents.len(), 2);
        assert_eq!(m.recents[0].path, "/Roms/GB/Mario.gb"); // 最新的在前面

        // 重复添加第一个 → 应该移到最前面
        m.add_recent::<TestPlatform>("/tmp/test_sdcard/Roms/GB/Zelda.gb", None);
        assert_eq!(m.recents.len(), 2);
        assert_eq!(m.recents[0].path, "/Roms/GB/Zelda.gb"); // 被 bump 到前面
    }

    #[test]
    fn test_save_restore_state() {
        let mut m = MinUi::new();
        m.stack.push(Directory {
            path: "/test".into(),
            name: "Test".into(),
            entries: vec![
                Entry { path: "/a.gb".into(), name: "A".into(), unique: None, entry_type: EntryType::Rom, alpha: 0 },
                Entry { path: "/b.gb".into(), name: "B".into(), unique: None, entry_type: EntryType::Rom, alpha: 0 },
                Entry { path: "/c.gb".into(), name: "C".into(), unique: None, entry_type: EntryType::Rom, alpha: 0 },
            ],
            alphas: vec![0],
            selected: 1,
            start: 0,
            end: 3,
        });

        m.save_restore_state();
        assert_eq!(m.restore_selected, 1);
        assert_eq!(m.restore_start, 0);
        assert_eq!(m.restore_end, 3);
    }
}
