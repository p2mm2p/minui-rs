//! # MinUi 全局状态
//!
//! 对应原 C 代码 `minui.c` 中的所有 `static` 全局变量和核心逻辑。
//!
//! ## 状态分类
//!
//! | 类别 | 字段 | 对应 C 变量 |
//! |------|------|-----------|
//! | 导航 | `stack`, `recents` | `stack`, `recents`, `top` |
//! | 模式 | `can_resume`, `should_resume`, `simple_mode` | 同名 static |
//! | 恢复 | `restore_*` | 同名 static |
//! | 渲染 | `dirty`, `show_version` | `dirty` flag |
//! | 电源 | `autosleep_timer`, `autopoweroff_timer`, `show_setting` | 分散在 PWR 逻辑中 |

use std::fs;
use std::time::Instant;

use common::types::*;
use common::utils::*;
use common::paths;
use minui_render::{UiRenderer, ListRenderInput, HardwareStatus, ButtonHint, Rgb565};
use minui_power::PowerManager;
use minui_platform::{Platform, Framebuffer};

use crate::scan;
use crate::launch;

// ============================================================================
// MinUi 结构体
// ============================================================================

/// MinUI 启动器的全部运行时状态
///
/// 对应原 C 代码 `minui.c` 中所有的 `static` 全局变量。
/// 在 Rust 中封装为单一结构体以便于测试和所有权管理。
///
/// # 字段分组
///
/// | 分组 | 字段 | 说明 |
/// |------|------|------|
/// | 导航 | `stack`, `recents` | 目录栈 + 最近游戏列表 |
/// | 存档 | `can_resume`, `should_resume`, `slot_path` | 存档恢复状态 |
/// | 恢复 | `restore_*` | 返回上级目录时恢复滚动位置 |
/// | 渲染 | `dirty`, `show_version` | 重绘标记 + 版本界面切换 |
/// | 电源 | `autosleep_timer`, `autopoweroff_timer`, `show_setting` | 休眠/关机计时器 |
pub struct MinUi {
    // ==== 导航 ====
    /// 目录栈 —— 最后一个元素是当前显示的目录。
    /// 进入子目录时 push，返回上级时 pop。栈底（第一个）是根目录。
    /// 对应 C 中的全局 `stack` + `top`（top 永远指向栈顶）。
    pub stack: Vec<Directory>,
    /// 最近游戏列表 —— 按时间倒序排列，最新的在最前面。
    /// 条目中的 `path` 是去除 SDCARD_PATH 前缀的相对路径，
    /// 以实现跨设备兼容。
    /// 对应 C 中的全局 `recents`。
    pub recents: Vec<Recent>,

    // ==== 模式与状态 ====
    /// 主事件循环退出标志。设置为 true 后，下一帧退出主循环。
    /// 对应 C 中的 `static int quit`。
    pub quit: bool,
    /// 当前选中的游戏是否有存档可恢复。控制界面是否显示 "X RESUME" 按钮。
    /// 对应 C 中的 `static int can_resume`。
    pub can_resume: bool,
    /// 用户是否按了 X 键（从存档恢复而非正常启动）。
    /// 仅在 `can_resume == true` 时可被设置为 true。
    /// 对应 C 中的 `static int should_resume`。
    pub should_resume: bool,
    /// 简化模式 —— 隐藏 Tools 等高级功能。
    /// 通过 `/.userdata/shared/enable-simple-mode` 文件启用。
    /// 对应 C 中的 `static int simple_mode`。
    pub simple_mode: bool,
    /// 当前选中游戏的存档槽位文件路径。
    /// 格式：`/.userdata/shared/.minui/<EMU>/<ROM>.txt`
    /// 对应 C 中的 `static char slot_path[256]`。
    pub slot_path: String,

    // ==== 目录导航恢复 ====
    /// 进入子目录前的栈深度，用于判断是否需要恢复导航位置。
    /// `None` 表示不需要恢复。
    /// 对应 C 中的 `static int restore_depth`（-1 表示不恢复）。
    pub restore_depth: Option<usize>,
    /// 需要恢复到的条目在父目录中的相对位置（selected 值）。
    /// 对应 C 中的 `static int restore_relative`。
    pub restore_relative: Option<usize>,
    /// 返回上级时恢复的选中项索引。
    /// 对应 C 中的 `static int restore_selected`。
    pub restore_selected: usize,
    /// 返回上级时恢复的可见窗口起始位置。
    /// 对应 C 中的 `static int restore_start`。
    pub restore_start: usize,
    /// 返回上级时恢复的可见窗口结束位置（不含）。
    /// 对应 C 中的 `static int restore_end`。
    pub restore_end: usize,

    // ==== 渲染 ====
    /// 界面是否需要重绘。任何改变显示状态的操作都应设置此标志。
    /// 对应 C 中 main() 的 `dirty` 局部变量。
    pub dirty: bool,
    /// 是否显示版本信息界面（MENU 键切换）。
    /// 对应 C 中 main() 的 `show_version` 局部变量。
    pub show_version: bool,

    // ==== 定时 ====
    /// 自动休眠计时器（距上次用户输入的毫秒数）。
    /// 达到 30 秒后触发休眠。
    pub autosleep_timer: u32,
    /// 自动关机计时器（进入休眠后的毫秒数）。
    /// 达到 2 分钟后触发自动关机。
    pub autopoweroff_timer: u32,
    /// 当前显示的系统设置类型：0=无, 1=亮度调整中, 2=音量调整中。
    pub show_setting: u8,

    // ==== 持久化 ====
    /// 最大最近游戏条目数（24，必须是 MAIN_ROW_COUNT 的倍数）。
    /// 对应 C 中的 `MAX_RECENTS`。
    pub max_recents: usize,
}

impl MinUi {
    /// 创建新的 MinUi 状态，所有字段初始化为默认值
    ///
    /// 对应 C 中所有 `static` 变量的初始值。
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
    // 便利访问
    // ================================================================

    /// 获取栈顶目录（当前浏览的目录）的不可变引用
    ///
    /// # Panics
    /// 栈为空时 panic（正常运行中不应发生）
    pub fn current_dir(&self) -> &Directory {
        self.stack.last().expect("MinUi: stack should never be empty")
    }

    /// 获取栈顶目录的可变引用
    pub fn current_dir_mut(&mut self) -> &mut Directory {
        self.stack.last_mut().expect("MinUi: stack should never be empty")
    }

    /// 当前是否在根目录（栈深度 ≤ 1）
    pub fn is_at_root(&self) -> bool { self.stack.len() <= 1 }

    /// 当前目录是否在 Collections 中
    ///
    /// 用于确定"最后位置"（last path）的保存方式。
    /// 对应 C 中 `Entry_open()` 的 `prefixMatch(COLLECTIONS_PATH, top->path)` 检查。
    pub fn is_in_collection(&self, sdcard: &str) -> bool {
        if self.stack.is_empty() { return false; }
        let col = scan::collections_path(sdcard);
        prefix_match(&col, &self.current_dir().path)
            && self.current_dir().path != col
    }

    /// 栈深度（当前目录层级数）
    pub fn depth(&self) -> usize { self.stack.len() }

    /// 当前目录的条目总数
    pub fn total_entries(&self) -> usize { self.current_dir().total() }

    /// 获取当前选中条目的克隆
    ///
    /// 返回 `Entry` 的克隆而非引用，以避免在需要可变借用 `self` 时
    /// 与不可变借用冲突。
    pub fn selected_entry_cloned(&self) -> Option<Entry> {
        let dir = self.current_dir();
        if dir.entries.is_empty() { None } else { Some(dir.entries[dir.selected].clone()) }
    }

    // ================================================================
    // 状态修改
    // ================================================================

    /// 标记界面需要重绘（设置 dirty 标志）
    pub fn mark_dirty(&mut self) { self.dirty = true; }

    /// 请求退出主事件循环（设置 quit 标志）
    pub fn request_quit(&mut self) { self.quit = true; }

    /// 导航后重置存档恢复相关状态
    pub fn reset_after_navigation(&mut self) {
        self.can_resume = false;
        self.should_resume = false;
    }

    pub fn save_restore_state(&mut self) {
        let (selected, start, end) = {
            let dir = self.current_dir();
            (dir.selected, dir.start, dir.end)
        };
        self.restore_selected = selected;
        self.restore_start = start;
        self.restore_end = end;
    }

    pub fn mark_restore_depth(&mut self) {
        self.restore_depth = Some(self.depth());
    }

    pub fn mark_restore_relative(&mut self) {
        self.restore_relative = Some(self.current_dir().selected);
    }

    // ================================================================
    // 最近游戏管理
    // ================================================================

    pub fn add_recent_direct(&mut self, full_path: &str, alias: Option<&str>, sdcard: &str) {
        let relative = full_path
            .strip_prefix(sdcard)
            .map(|s| s.to_string())
            .unwrap_or_else(|| full_path.to_string());

        if let Some(pos) = self.recents.iter().position(|r| r.path == relative) {
            if pos > 0 {
                let recent = self.recents.remove(pos);
                self.recents.insert(0, recent);
            }
            return;
        }

        while self.recents.len() >= self.max_recents {
            self.recents.pop();
        }
        self.recents.insert(0, Recent {
            path: relative,
            alias: alias.map(String::from),
            available: true,
        });
    }

    pub fn save_recents(&self, sdcard: &str) {
        let path = scan::recent_file_path(sdcard);
        let mut content = String::new();
        for r in &self.recents {
            content.push_str(&r.path);
            if let Some(ref alias) = r.alias {
                content.push('\t');
                content.push_str(alias);
            }
            content.push('\n');
        }
        let _ = put_file(&path, &content);
    }

    // ================================================================
    // 最后位置持久化
    // ================================================================

    pub fn save_last(&self, path: &str, sdcard: &str) {
        let faux = scan::faux_recent_path(sdcard);
        let save_path = if self.current_dir().path == faux {
            faux.as_str()
        } else {
            path
        };
        let _ = put_file(paths::LAST_PATH, save_path);
    }

    /// 恢复上次浏览位置 —— 对应 C 中的 `loadLast()`
    ///
    /// 从 `/tmp/last.txt` 读取上次保存的路径，逐级重现导航，
    /// 将光标定位到最后的条目上。
    pub fn load_last(&mut self, sdcard: &str, platform_tag: &str, paks: &str) {
        if !path_exists(paths::LAST_PATH) { return; }

        let last_path = match fs::read_to_string(paths::LAST_PATH) {
            Ok(s) => s.trim().to_string(),
            Err(_) => return,
        };
        if last_path.is_empty() { return; }

        let full_path = last_path.clone();
        let col = scan::collections_path(sdcard);

        // 逐级拆分路径（从底向上）
        let mut path_parts: Vec<String> = Vec::new();
        let mut current = last_path;
        while current != sdcard && !current.is_empty() {
            path_parts.push(current.clone());
            if let Some(slash_pos) = current.rfind('/') {
                current.truncate(slash_pos);
            } else { break; }
        }

        // 从根向下逐级重现导航
        let roms = scan::roms_path(sdcard);
        while let Some(path) = path_parts.pop() {
            if path == roms { continue; }

            let collated = if suffix_match(")", &path)
                && scan::is_console_dir(&path, sdcard)
            {
                scan::extract_collate_prefix(&path)
            } else {
                String::new()
            };

            let Some((found_idx, found_type, found_path)) = ({
                let dir = self.current_dir();
                let mut result = None;
                for (i, entry) in dir.entries.iter().enumerate() {
                    let matches = entry.path == path
                        || (!collated.is_empty() && prefix_match(&collated, &entry.path))
                        || (prefix_match(&col, &full_path)
                            && suffix_match(
                                &format!("/{}", file_name(&path).unwrap_or("")),
                                &entry.path,
                            ));
                    if matches {
                        result = Some((i, entry.entry_type, entry.path.clone()));
                        break;
                    }
                }
                result
            }) else {
                break;
            };

            // found_idx 已被解包（let Some((...)) = result），直接使用
            {
                let dir = self.current_dir_mut();
                dir.selected = found_idx;
                let total = dir.entries.len();
                let main_rows = 6;
                if found_idx >= dir.end {
                    dir.start = found_idx;
                    dir.end = (dir.start + main_rows).min(total);
                }
            }

            let is_last = path_parts.is_empty();
            if found_type == EntryType::Dir && !is_last {
                self.open_directory(
                    &found_path, false, sdcard, platform_tag, paks,
                );
            }
        }
    }

    // ================================================================
    // 目录导航
    // ================================================================

    /// 打开一个目录 —— 对应 C 中的 `openDirectory()`
    ///
    /// 当 `auto_launch=true` 且目录下有 `.cue` 或 `.m3u` 时，
    /// 直接启动游戏而非进入目录浏览。
    pub fn open_directory(
        &mut self,
        path: &str,
        auto_launch: bool,
        sdcard: &str,
        platform_tag: &str,
        paks: &str,
    ) {
        // CUE/M3U 自动启动检查
        if auto_launch {
            if let Some(cue) = scan::find_cue(path) {
                self.save_last(path, sdcard);
                launch::open_rom(self, &cue, Some(path), None, sdcard, platform_tag, paks);
                return;
            }
            let dir_name = file_name(path).unwrap_or("");
            let parent = parent_dir(path).unwrap_or("");
            let m3u_path = format!("{}/{}.m3u", parent, dir_name);
            if path_exists(&m3u_path) {
                if let Some(first_disc) = scan::get_first_disc(&m3u_path) {
                    self.save_last(path, sdcard);
                    launch::open_rom(self, &first_disc, Some(path), None, sdcard, platform_tag, paks);
                    return;
                }
            }
        }

        // 确定恢复位置
        let mut selected = 0;
        let mut start = 0;
        let mut end = 0;

        if !self.stack.is_empty() {
            let dir = self.current_dir();
            if !dir.entries.is_empty()
                && self.restore_depth == Some(self.stack.len())
                    && dir.selected == self.restore_relative.unwrap_or(0)
                {
                    selected = self.restore_selected;
                    start = self.restore_start;
                    end = self.restore_end;
                }
        }

        // 获取条目并创建目录
        let simple_mode = self.simple_mode;
        let entries = scan::get_entries_for_path(
            path, sdcard, platform_tag, paks, &self.recents, simple_mode,
        );
        let total = entries.len();
        let main_rows = 6;

        let final_end = if end > 0 && end <= total { end }
            else if total < main_rows { total }
            else { main_rows };

        let mut dir = scan::make_directory(path, entries, selected, sdcard, platform_tag);
        dir.start = start;
        dir.end = final_end;

        self.stack.push(dir);
    }

    /// 关闭当前目录（返回上级）—— 对应 C 中的 `closeDirectory()`
    pub fn close_directory(&mut self) {
        self.save_restore_state();
        self.stack.pop();
        self.mark_restore_depth();
        self.mark_restore_relative();
    }

    // ================================================================
    // Menu 生命周期
    // ================================================================

    /// 初始化菜单系统 —— 加载最近游戏、打开根目录、恢复上次位置
    ///
    /// 对应 C 中的 `Menu_init()`。
    pub fn init_menu(&mut self, sdcard: &str, platform_tag: &str, paks: &str) {
        let (recents, _) = scan::load_recents(sdcard, platform_tag, paks);
        self.recents = recents;

        self.open_directory(sdcard, false, sdcard, platform_tag, paks);
        self.load_last(sdcard, platform_tag, paks);
    }

    /// 清理菜单系统，释放所有资源
    ///
    /// 对应 C 中的 `Menu_quit()`。
    pub fn quit_menu(&mut self) {
        self.recents.clear();
        self.stack.clear();
    }

    // ================================================================
    // 输入处理
    // ================================================================

    /// 在主启动器模式下处理导航输入
    ///
    /// 对应 C 代码 `main()` 中 `show_version==0` 段的按键处理。
    /// 注意：此方法通过索引而非持有引用来避免借用冲突。
    pub fn handle_launcher_input(
        &mut self,
        pad: &PadContext,
        _now: u32,
        main_rows: usize,
        sdcard: &str,
        platform_tag: &str,
        paks: &str,
    ) {
        let total = self.total_entries();
        if total == 0 { return; }

        // 复制索引值以避免在修改 self 时持有引用
        let mut selected = self.current_dir().selected;
        let mut start = self.current_dir().start;
        let mut end = self.current_dir().end;
        let alphas_count = self.current_dir().alphas.len();

        // ==== 上下导航 ====
        if pad.just_repeated.contains(Button::UP) {
            if selected == 0 && !pad.just_pressed.contains(Button::UP) {
                // 已在顶部
            } else if selected == 0 {
                selected = total - 1;
                let s = total.saturating_sub(main_rows);
                start = s;
                end = total;
            } else {
                selected -= 1;
                if selected < start { start -= 1; end -= 1; }
            }
            self.dirty = true;
        } else if pad.just_repeated.contains(Button::DOWN) {
            if selected >= total - 1 && !pad.just_pressed.contains(Button::DOWN) {
                // 已在底部
            } else if selected >= total - 1 {
                selected = 0;
                start = 0;
                end = total.min(main_rows);
            } else {
                selected += 1;
                if selected >= end { start += 1; end += 1; }
            }
            self.dirty = true;
        }

        // ==== 左右翻页 ====
        if pad.just_repeated.contains(Button::LEFT) {
            selected = selected.saturating_sub(main_rows);
            if selected < start {
                start = start.saturating_sub(main_rows);
                end = start + main_rows;
            }
            self.dirty = true;
        } else if pad.just_repeated.contains(Button::RIGHT) {
            selected = (selected + main_rows).min(total.saturating_sub(1));
            if selected >= end {
                end = (end + main_rows).min(total);
                start = end.saturating_sub(main_rows);
            }
            self.dirty = true;
        }

        // ==== L1/R1 字母跳转 ====
        // 取当前条目 alpha（需要临时读取，不能同时持有 dir 和 self 的可变引用）
        let entry_alpha = {
            let dir = self.current_dir();
            dir.entries.get(selected).map(|e| e.alpha)
        };

        if pad.just_repeated.contains(Button::L1)
            && !pad.is_pressed.contains(Button::R1)
        {
            if let Some(alpha) = entry_alpha {
                let i = alpha.saturating_sub(1);
                if i < alphas_count {
                    selected = self.current_dir().alphas[i];
                    if total > main_rows {
                        start = selected;
                        end = (start + main_rows).min(total);
                        start = end.saturating_sub(main_rows);
                    }
                    self.dirty = true;
                }
            }
        } else if pad.just_repeated.contains(Button::R1)
            && !pad.is_pressed.contains(Button::L1)
        {
            if let Some(alpha) = entry_alpha {
                let i = alpha + 1;
                if i < alphas_count {
                    selected = self.current_dir().alphas[i];
                    if total > main_rows {
                        start = selected;
                        end = (start + main_rows).min(total);
                        start = end.saturating_sub(main_rows);
                    }
                    self.dirty = true;
                }
            }
        }

        // 写回索引值
        {
            let dir = self.current_dir_mut();
            dir.selected = selected;
            dir.start = start;
            dir.end = end;
        }

        // ==== 存档恢复检测 ====
        if self.dirty && total > 0 {
            if let Some(entry) = self.selected_entry_cloned() {
                launch::ready_resume(self, &entry, sdcard, platform_tag);
            }
        }

        // ==== X 键：从存档恢复 ====
        if total > 0 && self.can_resume && pad.just_released.contains(Button::X) {
            self.should_resume = true;
            if let Some(entry) = self.selected_entry_cloned() {
                self.save_last(&entry.path, sdcard);
                launch::entry_open(self, &entry, sdcard, platform_tag, paks);
            }
            self.dirty = true;
        }

        // ==== A 键：打开条目 ====
        if total > 0 && pad.just_pressed.contains(Button::A) {
            if let Some(entry) = self.selected_entry_cloned() {
                self.save_last(&entry.path, sdcard);
                launch::entry_open(self, &entry, sdcard, platform_tag, paks);
            }
            self.dirty = true;
        }

        // ==== B 键：返回上级 ====
        if pad.just_pressed.contains(Button::B) && !self.is_at_root() {
            self.close_directory();
            self.dirty = true;
            self.reset_after_navigation();
            if self.total_entries() > 0 {
                if let Some(entry) = self.selected_entry_cloned() {
                    launch::ready_resume(self, &entry, sdcard, platform_tag);
                }
            }
        }
    }
    // ================================================================
    // 主事件循环
    // ================================================================

    /// 启动 MinUI 主事件循环 —— 对应 C 中的 `main()` 函数
    ///
    /// 这是整个 minui 启动器的入口。自动恢复检查、初始化、主循环、清理。
    ///
    /// ## 参数
    ///
    /// - `platform`: 平台实现（视频、输入、电源等硬件抽象）
    /// - `renderer`: UI 渲染器
    /// - `power`: 电源管理器
    /// - `sdcard`: SD 卡根路径
    /// - `platform_tag`: 平台标识字符串
    /// - `paks`: Pak 目录路径
    /// - `font_data`: 字体文件的字节数据（用于渲染文字）
    ///
    /// ## 返回值
    ///
    /// `Ok(true)` 表示正常退出（例如启动了游戏），调用方应读取 `/tmp/next`。
    /// `Ok(false)` 表示自动恢复已处理，无需进一步操作。
    pub fn run(
        &mut self,
        platform: &mut impl Platform,
        renderer: &UiRenderer,
        power: &mut PowerManager,
        sdcard: &str,
        platform_tag: &str,
        paks: &str,
    ) -> Result<bool, String> {
        // 0. 自动恢复检查
        if launch::auto_resume(sdcard, platform_tag, paks) {
            return Ok(false); // 已自动启动游戏，不需要显示 UI
        }

        // 1. 检查简化模式
        let simple_path = paths::simple_mode_path_direct(sdcard);
        self.simple_mode = path_exists(&simple_path);

        log::info!("MinUI starting...");

        // 2. 初始化视频
        let mut fb = platform.init_video()?;
        log::info!("- video initialized");

        // 3. 初始化输入
        platform.init_input()?;
        log::info!("- input initialized");

        // 4. 初始化电源管理
        power.initialized = true;
        if !platform.has_power_button() && !self.simple_mode {
            power.disable_sleep();
        }
        log::info!("- power initialized");

        // 5. 初始化菜单
        self.init_menu(sdcard, platform_tag, paks);
        log::info!("- menu initialized");

        // 6. 降低 CPU 到菜单模式省电，开启严格 VSync
        platform.set_cpu_speed(CpuSpeed::Menu);
        platform.set_vsync(VsyncMode::Strict);

        // 7. 初始化状态
        self.dirty = true;
        self.show_version = false;
        let mut pad = PadContext::default();
        let main_rows = 6;

        // 帧率控制（目标 60fps）
        let frame_time_ms = 16u32;
        let mut last_frame = Instant::now();
        let mut was_online = platform.is_online();

        log::info!("- entering main loop");

        // ================================================================
        // 主循环
        // ================================================================
        while !self.quit && !power.poweroff_requested {
            let frame_start = Instant::now();

            // a. 轮询输入
            platform.poll_input(&mut pad);

            // b. 更新电源状态
            let dt_ms = last_frame.elapsed().as_millis().min(100) as u32;
            let pwr_dirty = power.update(dt_ms);
            if pwr_dirty { self.dirty = true; }

            // 检查自动休眠（未休眠期间）
            // 对应 C: if (now-last_input_at>=SLEEP_DELAY && PWR_preventAutosleep()) last_input_at = now;
            // 即：如果阻止自动休眠（充电/禁用/HDMI），重置计时器
            if !power.is_asleep && !pad.any_pressed() {
                if power.prevent_autosleep(platform.is_hdmi()) {
                    power.notify_activity(); // 重置空闲计时器
                } else if power.check_autosleep(dt_ms) {
                    self.dirty = true;
                }
            }

            // 有操作时唤醒
            if pad.any_just_pressed() {
                power.notify_activity();
                self.dirty = true;
            }

            // c. 网络状态检测
            let is_online = platform.is_online();
            if was_online != is_online {
                self.dirty = true;
                was_online = is_online;
            }

            // d. 亮度/音量调节（在任何界面都可用）
            let mod_brightness = Button::MENU;
            let mod_volume = Button::NONE; // 无修饰键调节音量
            let setting_changed = power.handle_setting_input(
                &pad, mod_brightness, mod_volume, Button::PLUS, Button::MINUS,
            );
            if setting_changed {
                self.dirty = true;
            }

            // e. 版本界面切换（MENU 键）
            // 对应 C 中 PAD_tappedMenu(now) 逻辑：MENU 短按 (<250ms) 切换版本界面
            if pad.just_released.contains(Button::MENU)
                && !pad.is_pressed.contains(Button::PLUS)
                && !pad.is_pressed.contains(Button::MINUS)
            {
                self.show_version = !self.show_version;
                self.dirty = true;
                // C: 进入版本界面时启用休眠（MENU 键不再兼任休眠键）
                // C: 退出版本界面时禁用休眠（MENU 键恢复为休眠键）
                if !platform.has_power_button() && !self.simple_mode {
                    if self.show_version {
                        power.enable_sleep();
                    } else {
                        power.disable_sleep();
                    }
                }
            }

            // f. 处理输入
            if self.show_version {
                // 版本界面：B 或 MENU 短按退出（对应 C: PAD_justPressed(BTN_B) || PAD_tappedMenu(now)）
                if pad.just_pressed.contains(Button::B)
                    || (pad.just_released.contains(Button::MENU)
                        && !pad.is_pressed.contains(Button::PLUS)
                        && !pad.is_pressed.contains(Button::MINUS))
                {
                    self.show_version = false;
                    self.dirty = true;
                    if !platform.has_power_button() && !self.simple_mode {
                        power.disable_sleep();
                    }
                }
            } else {
                self.handle_launcher_input(
                    &pad, dt_ms, main_rows, sdcard, platform_tag, paks,
                );
            }

            // g. 渲染
            if self.dirty && !power.is_asleep {
                self.render_frame(
                    platform, renderer, power, &mut fb,
                    sdcard, main_rows,
                );
                platform.flip(&fb, true);
                self.dirty = false;
            } else if !power.is_asleep {
                // 无变化时也等待 VSync（维持 60fps）
                platform.vsync_wait(0);
            }

            // h. 帧率控制
            let elapsed = frame_start.elapsed();
            let elapsed_ms = elapsed.as_millis() as u32;
            if elapsed_ms < frame_time_ms {
                std::thread::sleep(std::time::Duration::from_millis(
                    (frame_time_ms - elapsed_ms) as u64,
                ));
            }
            last_frame = frame_start;

            // i. HDMI 状态检测
            if platform.hdmi_changed() {
                log::info!("HDMI state changed, saving and restarting...");
                if let Some(entry) = self.selected_entry_cloned() {
                    self.save_last(&entry.path, sdcard);
                }
                std::thread::sleep(std::time::Duration::from_secs(4));
                self.request_quit();
            }
        }

        // ================================================================
        // 清理
        // ================================================================
        log::info!("- exiting main loop");

        if power.poweroff_requested {
            platform.power_off();
            // power_off 不应返回
        }

        self.quit_menu();
        platform.quit_input();
        platform.quit_video();

        Ok(true)
    }

    /// 渲染一帧 —— 组装列表数据、硬件状态、按钮提示，调用渲染器
    fn render_frame(
        &self,
        platform: &impl Platform,
        renderer: &UiRenderer,
        power: &PowerManager,
        fb: &mut Framebuffer,
        sdcard: &str,
        _main_rows: usize,
    ) {
        // ==== 硬件状态 ====
        let status = HardwareStatus {
            charge: power.battery_charge,
            is_charging: power.battery_charging,
            is_low: power.is_low_charge(),
            show_setting: power.show_setting,
            brightness: power.brightness,
            volume: power.volume,
            has_wifi: platform.is_online(),
            wifi_connected: platform.is_online(),
            has_hdmi: platform.is_hdmi(),
        };

        // ==== 版本界面数据 ====
        let release_str;
        let model_str;
        let version_info = if self.show_version {
            release_str = fs::read_to_string(
                format!("{}/.system/{}/version.txt", sdcard, platform.get_model())
            ).unwrap_or_default();
            model_str = platform.get_model().to_string();
            Some((release_str.as_str(), "", "Model", model_str.as_str()))
        } else {
            None
        };

        // ==== 列表数据 ====
        let list_input = if !self.show_version {
            let dir = self.current_dir();
            if dir.entries.is_empty() {
                None
            } else {
                let scale = renderer.scale;
                let pill_h = 30 * scale;
                let padding = 10 * scale;
                let row_h = pill_h;
                let selected_row = dir.selected.saturating_sub(dir.start);

                Some(ListRenderInput {
                    entries: &dir.entries[dir.start..dir.end.min(dir.entries.len())],
                    selected_row,
                    start: dir.start,
                    end: dir.end.min(dir.entries.len()),
                    row_height: row_h,
                    padding,
                    text_color: Rgb565::WHITE,
                    selected_text_color: Rgb565::BLACK,
                    selected_bg: Rgb565::WHITE,
                    has_thumb: false,
                    thumb_width: 0,
                })
            }
        } else {
            None
        };

        // ==== 按钮提示 ====
        let (left_buttons, right_buttons) = if self.show_version {
            let left = vec![
                ButtonHint {
                    button: if platform.has_power_button() { "POWER" } else { "MENU" },
                    hint: "SLEEP",
                },
            ];
            let right = vec![
                ButtonHint { button: "B", hint: "BACK" },
            ];
            (left, right)
        } else if self.total_entries() == 0 {
            let left = if self.is_at_root() {
                vec![]
            } else {
                vec![ButtonHint { button: "B", hint: "BACK" }]
            };
            (vec![], left)
        } else {
            let left = vec![
                if self.can_resume {
                    ButtonHint { button: "X", hint: "RESUME" }
                } else {
                    ButtonHint {
                        button: if power.can_sleep() && platform.has_power_button() {
                            "POWER"
                        } else {
                            "MENU"
                        },
                        hint: if power.can_sleep() || self.simple_mode {
                            "SLEEP"
                        } else {
                            "INFO"
                        },
                    }
                },
            ];
            let right = if self.is_at_root() {
                vec![ButtonHint { button: "A", hint: "OPEN" }]
            } else {
                vec![
                    ButtonHint { button: "B", hint: "BACK" },
                    ButtonHint { button: "A", hint: "OPEN" },
                ]
            };
            (left, right)
        };

        // ==== 调用渲染器 ====
        renderer.render_frame(
            fb,
            list_input,
            &status,
            &left_buttons,
            &right_buttons,
            self.show_version,
            version_info,
        );
    }
}

// ============================================================================
// Default / Debug
// ============================================================================

impl Default for MinUi {
    fn default() -> Self { Self::new() }
}

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
    use std::fs;

    fn setup_nav_test(name: &str) -> String {
        let base = format!("/tmp/minui_nav_test_{}", name);
        let _ = fs::remove_dir_all(&base);

        fs::create_dir_all(format!("{}/Roms/Game Boy (GB)", base)).unwrap();
        fs::create_dir_all(format!("{}/.system/test/paks/Emus/GB.pak", base)).unwrap();
        fs::create_dir_all(format!("{}/.userdata/shared/.minui", base)).unwrap();

        fs::write(format!("{}/Roms/Game Boy (GB)/Zelda.gb", base), "dummy").unwrap();
        fs::write(format!("{}/Roms/Game Boy (GB)/Mario.gb", base), "dummy").unwrap();
        fs::write(
            format!("{}/.system/test/paks/Emus/GB.pak/launch.sh", base),
            "#!/bin/sh\necho ok",
        ).unwrap();

        base
    }

    #[test]
    fn test_init_menu() {
        let base = setup_nav_test("init");
        let paks = format!("{}/.system/test/paks", base);

        let mut m = MinUi::new();
        m.init_menu(&base, "test", &paks);

        assert!(!m.stack.is_empty());
        assert!(m.total_entries() > 0);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_open_and_close_directory() {
        let base = setup_nav_test("openclose");
        let paks = format!("{}/.system/test/paks", base);

        let mut m = MinUi::new();
        m.open_directory(&base, false, &base, "test", &paks);
        assert_eq!(m.depth(), 1);

        if let Some(entry) = m.selected_entry_cloned() {
            if entry.entry_type == EntryType::Dir {
                m.open_directory(&entry.path, false, &base, "test", &paks);
                assert_eq!(m.depth(), 2);
            }
        }

        if m.depth() > 1 {
            m.close_directory();
            assert_eq!(m.depth(), 1);
        }

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_add_recent_direct() {
        let mut m = MinUi::new();
        m.max_recents = 3;
        m.add_recent_direct("/sdcard/Roms/GB/Zelda.gb", Some("Zelda"), "/sdcard");
        assert_eq!(m.recents.len(), 1);
        assert_eq!(m.recents[0].path, "/Roms/GB/Zelda.gb");
        assert_eq!(m.recents[0].alias.as_deref(), Some("Zelda"));
    }

    #[test]
    fn test_save_and_load_last() {
        let base = setup_nav_test("last");
        let paks = format!("{}/.system/test/paks", base);

        let last_rom = format!("{}/Roms/Game Boy (GB)/Zelda.gb", base);
        put_file(paths::LAST_PATH, &last_rom).unwrap();

        let mut m = MinUi::new();
        m.init_menu(&base, "test", &paks);

        let current_path = &m.current_dir().path;
        assert!(
            current_path.contains("Game Boy"),
            "Expected current dir to contain 'Game Boy', got: {}",
            current_path
        );

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_is_in_collection() {
        let mut m = MinUi::new();
        m.stack.push(Directory {
            path: "/sdcard/Collections/MyFavorites.txt".into(),
            name: "MyFavorites".into(),
            entries: vec![],
            alphas: vec![],
            selected: 0,
            start: 0,
            end: 0,
        });
        assert!(m.is_in_collection("/sdcard"));
    }

    #[test]
    fn test_handle_launcher_input_navigate_down() {
        let mut m = MinUi::new();
        m.stack.push(Directory {
            path: "/test".into(),
            name: "Test".into(),
            entries: vec![
                Entry { path: "/a".into(), name: "A".into(), unique: None, entry_type: EntryType::Rom, alpha: 0 },
                Entry { path: "/b".into(), name: "B".into(), unique: None, entry_type: EntryType::Rom, alpha: 0 },
                Entry { path: "/c".into(), name: "C".into(), unique: None, entry_type: EntryType::Rom, alpha: 0 },
            ],
            alphas: vec![0],
            selected: 0,
            start: 0,
            end: 3,
        });

        let mut pad = PadContext::default();
        pad.just_repeated = Button::DOWN;
        pad.just_pressed = Button::DOWN;

        m.handle_launcher_input(&pad, 0, 6, "/sdcard", "test", "/paks");
        assert_eq!(m.current_dir().selected, 1);

        m.handle_launcher_input(&pad, 0, 6, "/sdcard", "test", "/paks");
        assert_eq!(m.current_dir().selected, 2);

        m.handle_launcher_input(&pad, 0, 6, "/sdcard", "test", "/paks");
        assert_eq!(m.current_dir().selected, 0);
    }

    #[test]
    fn test_handle_launcher_input_empty_dir() {
        let mut m = MinUi::new();
        m.stack.push(Directory {
            path: "/empty".into(),
            name: "Empty".into(),
            entries: vec![],
            alphas: vec![],
            selected: 0,
            start: 0,
            end: 0,
        });
        let pad = PadContext::default();
        m.handle_launcher_input(&pad, 0, 6, "/sdcard", "test", "/paks");
    }
}
